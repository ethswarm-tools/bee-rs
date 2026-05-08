//! `/bzz` endpoints: file upload / download and tar-based collection
//! upload. Mirrors bee-go's `pkg/file/{file,collection}.go`.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use bytes::Bytes;
use reqwest::Method;
use serde::Deserialize;

use crate::api::{
    CollectionUploadOptions, DownloadOptions, FileHeaders, FileUploadOptions, UploadProgress,
    UploadResult, prepare_collection_upload_headers, prepare_download_headers,
    validate_collection_upload_options,
    prepare_file_upload_headers,
};
use crate::client::{Inner, MAX_JSON_RESPONSE_BYTES, request};
use crate::manifest::{MantarayNode, populate_self_addresses};
use crate::swarm::file_chunker::FileChunker;
use crate::swarm::{BatchId, Error, Reference};

use super::FileApi;

/// One entry of an in-memory collection: a tar path plus its raw bytes.
/// Mirrors bee-go's `CollectionEntry` and bee-js's `Collection` items.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollectionEntry {
    /// Relative path inside the collection (e.g. `"index.html"` or
    /// `"assets/logo.png"`).
    pub path: String,
    /// File contents.
    pub data: Vec<u8>,
}

impl CollectionEntry {
    /// Construct a new entry from its `(path, data)` parts.
    pub fn new(path: impl Into<String>, data: impl Into<Vec<u8>>) -> Self {
        Self {
            path: path.into(),
            data: data.into(),
        }
    }
}

/// Cumulative byte size of the entries' data — equivalent to bee-js
/// `getCollectionSize`.
pub fn collection_size(entries: &[CollectionEntry]) -> u64 {
    entries.iter().map(|e| e.data.len() as u64).sum()
}

#[derive(Deserialize)]
struct UploadBody {
    reference: String,
}

impl FileApi {
    /// Upload a single file via `POST /bzz`. `name` is sent as the
    /// `name=` query parameter (Bee uses it as the filename in
    /// `Content-Disposition` on download). When `content_type` is empty
    /// and `opts` does not specify one, `application/octet-stream` is
    /// used.
    pub async fn upload_file(
        &self,
        batch_id: &BatchId,
        data: impl Into<Bytes>,
        name: &str,
        content_type: &str,
        opts: Option<&FileUploadOptions>,
    ) -> Result<UploadResult, Error> {
        let mut builder = request(&self.inner, Method::POST, "bzz")?;
        if !name.is_empty() {
            builder = builder.query(&[("name", name)]);
        }
        let ct = match opts.and_then(|o| o.content_type.as_deref()) {
            Some(s) => s.to_string(),
            None if !content_type.is_empty() => content_type.to_string(),
            None => "application/octet-stream".to_string(),
        };
        let builder = builder.header("Content-Type", ct).body(data.into());
        let builder = Inner::apply_headers(builder, prepare_file_upload_headers(batch_id, opts));
        let resp = self.inner.send(builder).await?;
        let headers = resp.headers().clone();
        let body: UploadBody = serde_json::from_slice(&Inner::read_capped(resp, MAX_JSON_RESPONSE_BYTES).await?)?;
        UploadResult::from_response(&body.reference, &headers)
    }

    /// Download a file via `GET /bzz/{ref}`. Returns the body bytes plus
    /// the parsed [`FileHeaders`] (filename / content-type / tag UID).
    pub async fn download_file(
        &self,
        reference: &Reference,
        opts: Option<&DownloadOptions>,
    ) -> Result<(Bytes, FileHeaders), Error> {
        let resp = self.download_file_response(reference, opts).await?;
        let headers = FileHeaders::from_response(resp.headers());
        Ok((resp.bytes().await?, headers))
    }

    /// Same as [`FileApi::download_file`] but returns the raw
    /// [`reqwest::Response`] for streaming. The caller drives reading
    /// from `resp.bytes_stream()` or `resp.chunk()`.
    pub async fn download_file_response(
        &self,
        reference: &Reference,
        opts: Option<&DownloadOptions>,
    ) -> Result<reqwest::Response, Error> {
        let path = format!("bzz/{}", reference.to_hex());
        let builder = request(&self.inner, Method::GET, &path)?;
        let builder = Inner::apply_headers(builder, prepare_download_headers(opts));
        self.inner.send(builder).await
    }

    /// Download a path inside a collection via
    /// `GET /bzz/{ref}/{path}`. Useful for serving individual files of a
    /// previously uploaded site.
    pub async fn download_file_path(
        &self,
        reference: &Reference,
        path: &str,
        opts: Option<&DownloadOptions>,
    ) -> Result<(Bytes, FileHeaders), Error> {
        let p = format!(
            "bzz/{}/{}",
            reference.to_hex(),
            path.trim_start_matches('/')
        );
        let builder = request(&self.inner, Method::GET, &p)?;
        let builder = Inner::apply_headers(builder, prepare_download_headers(opts));
        let resp = self.inner.send(builder).await?;
        let headers = FileHeaders::from_response(resp.headers());
        Ok((resp.bytes().await?, headers))
    }

    /// Upload an in-memory collection (vec of [`CollectionEntry`]) as a
    /// tar stream via `POST /bzz`. Mirrors bee-go's
    /// `UploadCollectionEntries` and bee-js's
    /// `makeCollectionFromFileList` + `bzz.uploadCollection`.
    pub async fn upload_collection_entries(
        &self,
        batch_id: &BatchId,
        entries: &[CollectionEntry],
        opts: Option<&CollectionUploadOptions>,
    ) -> Result<UploadResult, Error> {
        if let Some(cb) = opts.and_then(|o| o.on_entry.as_ref()) {
            let total = entries.len();
            for (i, entry) in entries.iter().enumerate() {
                cb(UploadProgress {
                    path: &entry.path,
                    size: entry.data.len() as u64,
                    index: i,
                    total,
                });
            }
        }
        let tar_bytes = build_tar_archive(entries)?;
        validate_collection_upload_options(opts)?;
        let builder = request(&self.inner, Method::POST, "bzz")?
            .header("Content-Type", "application/x-tar")
            .header("Swarm-Collection", "true")
            .body(Bytes::from(tar_bytes));
        let builder =
            Inner::apply_headers(builder, prepare_collection_upload_headers(batch_id, opts));
        let resp = self.inner.send(builder).await?;
        let headers = resp.headers().clone();
        let body: UploadBody = serde_json::from_slice(&Inner::read_capped(resp, MAX_JSON_RESPONSE_BYTES).await?)?;
        UploadResult::from_response(&body.reference, &headers)
    }

    /// Walk the filesystem at `dir`, build a tar archive of every
    /// regular file (relative paths preserved), and upload it via
    /// `POST /bzz`. Symlinks and special files are skipped. Mirrors
    /// bee-go's `UploadCollection`.
    pub async fn upload_collection(
        &self,
        batch_id: &BatchId,
        dir: impl AsRef<Path>,
        opts: Option<&CollectionUploadOptions>,
    ) -> Result<UploadResult, Error> {
        let entries = read_directory_entries(dir.as_ref())?;
        self.upload_collection_entries(batch_id, &entries, opts)
            .await
    }
}

/// Offline content-address every entry in a collection and assemble
/// them into a Mantaray manifest. Returns the manifest's root chunk
/// address — the same value Bee would emit on
/// `upload_collection_entries`. No I/O.
///
/// Each file is hashed with [`FileChunker`]; the file path and
/// content are added to a Mantaray node, and the node's self-address
/// is calculated. Mirrors bee-js `hashCollectionEntries`.
pub fn hash_collection_entries(entries: &[CollectionEntry]) -> Result<Reference, Error> {
    let mut manifest = MantarayNode::new();
    for entry in entries {
        let mut chunker = FileChunker::new();
        chunker.write(&entry.data)?;
        let root = chunker.finalize()?;
        manifest.add_fork(entry.path.as_bytes(), Some(&root.address), None);
    }
    let addr = populate_self_addresses(&mut manifest)?;
    Reference::new(&addr)
}

/// Same as [`hash_collection_entries`] but reads entries off disk
/// from the directory tree at `dir`. Mirrors bee-js `hashDirectory`.
pub fn hash_directory(dir: impl AsRef<Path>) -> Result<Reference, Error> {
    let entries = read_directory_entries(dir.as_ref())?;
    hash_collection_entries(&entries)
}

/// Walk `dir` recursively, collecting every regular file as a
/// [`CollectionEntry`] with its filesystem-relative path. Mirrors
/// bee-go's filepath.Walk in `UploadCollection`.
pub fn read_directory_entries(dir: &Path) -> Result<Vec<CollectionEntry>, Error> {
    let canonical = dir
        .canonicalize()
        .map_err(|e| Error::argument(format!("invalid dir {dir:?}: {e}")))?;
    let mut out = Vec::new();
    walk(&canonical, &canonical, &mut out)?;
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

fn walk(root: &Path, here: &Path, out: &mut Vec<CollectionEntry>) -> Result<(), Error> {
    let read =
        std::fs::read_dir(here).map_err(|e| Error::argument(format!("read_dir {here:?}: {e}")))?;
    for entry in read {
        let entry = entry.map_err(|e| Error::argument(format!("dir entry: {e}")))?;
        let path: PathBuf = entry.path();
        let ty = entry
            .file_type()
            .map_err(|e| Error::argument(format!("file_type {path:?}: {e}")))?;
        if ty.is_dir() {
            walk(root, &path, out)?;
            continue;
        }
        if !ty.is_file() {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .map_err(|e| Error::argument(format!("strip_prefix {path:?}: {e}")))?;
        let rel_str = rel
            .to_str()
            .ok_or_else(|| Error::argument(format!("non-UTF-8 path {rel:?}")))?
            .replace(std::path::MAIN_SEPARATOR, "/");
        let mut data = Vec::new();
        std::fs::File::open(&path)
            .and_then(|mut f| f.read_to_end(&mut data))
            .map_err(|e| Error::argument(format!("open {path:?}: {e}")))?;
        out.push(CollectionEntry::new(rel_str, data));
    }
    Ok(())
}

/// Encode the entries as a USTAR archive in memory. Each entry is
/// written with mode `0o644`. Layout matches bee-go's
/// `UploadCollectionEntries`.
fn build_tar_archive(entries: &[CollectionEntry]) -> Result<Vec<u8>, Error> {
    let mut buf =
        Vec::with_capacity(entries.iter().map(|e| e.data.len() + 512).sum::<usize>() + 1024);
    {
        let mut tw = tar::Builder::new(&mut buf);
        for e in entries {
            let mut header = tar::Header::new_ustar();
            header
                .set_path(&e.path)
                .map_err(|err| Error::argument(format!("invalid tar path {:?}: {err}", e.path)))?;
            header.set_size(e.data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tw.append(&header, e.data.as_slice())
                .map_err(|err| Error::argument(format!("tar append failed: {err}")))?;
        }
        tw.finish()
            .map_err(|err| Error::argument(format!("tar finish failed: {err}")))?;
    }
    // Belt-and-braces: ensure the writer is fully flushed.
    let _ = std::io::sink().flush();
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collection_size_sums_entries() {
        let entries = vec![
            CollectionEntry::new("a.txt", b"abc".to_vec()),
            CollectionEntry::new("b.txt", b"defgh".to_vec()),
        ];
        assert_eq!(collection_size(&entries), 8);
    }

    #[test]
    fn hash_collection_entries_is_deterministic_and_path_sensitive() {
        let a = vec![CollectionEntry::new("index.html", b"x".to_vec())];
        let b = vec![CollectionEntry::new("index.html", b"x".to_vec())];
        let c = vec![CollectionEntry::new("other.html", b"x".to_vec())];

        let ra = hash_collection_entries(&a).unwrap();
        let rb = hash_collection_entries(&b).unwrap();
        let rc = hash_collection_entries(&c).unwrap();

        assert_eq!(ra, rb, "same entries → same address");
        assert_ne!(ra, rc, "different paths → different address");
    }

    #[test]
    fn read_directory_entries_walks_recursively() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("nested")).unwrap();
        std::fs::write(dir.path().join("a.txt"), b"alpha").unwrap();
        std::fs::write(dir.path().join("nested/b.bin"), [0u8, 1, 2]).unwrap();

        let entries = read_directory_entries(dir.path()).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, "a.txt");
        assert_eq!(&entries[0].data, b"alpha");
        assert_eq!(entries[1].path, "nested/b.bin");
        assert_eq!(&entries[1].data, &[0, 1, 2]);
    }

    #[test]
    fn build_tar_archive_round_trip() {
        let entries = vec![
            CollectionEntry::new("index.html", b"<html/>".to_vec()),
            CollectionEntry::new("nested/data.bin", b"\x00\x01\x02".to_vec()),
        ];
        let bytes = build_tar_archive(&entries).unwrap();

        let mut found: Vec<(String, Vec<u8>)> = Vec::new();
        let mut ar = tar::Archive::new(bytes.as_slice());
        for entry in ar.entries().unwrap() {
            let mut e = entry.unwrap();
            let path = e.path().unwrap().to_string_lossy().into_owned();
            let mut data = Vec::new();
            std::io::Read::read_to_end(&mut e, &mut data).unwrap();
            found.push((path, data));
        }
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].0, "index.html");
        assert_eq!(&found[0].1, b"<html/>");
        assert_eq!(found[1].0, "nested/data.bin");
        assert_eq!(&found[1].1, b"\x00\x01\x02");
    }
}
