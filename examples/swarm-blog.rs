//! swarm-blog — markdown-driven blog with a single stable URL.
//!
//! Posts live as `posts/<slug>.md` files locally. On `publish`, each
//! post is wrapped in a tiny HTML shell, an `index.html` listing is
//! generated, and the whole site is uploaded as a Mantaray collection
//! whose root is published through a feed manifest. The feed manifest
//! URL stays the same forever; readers always see the latest version.
//!
//! ```text
//! swarm-blog init  <topic-name>
//! swarm-blog new   <slug> <title>
//! swarm-blog list
//! swarm-blog publish
//! ```
//!
//! Environment:
//! - `BEE_URL` — base URL (default: `http://localhost:1633`).
//! - `BEE_BATCH_ID` — usable postage batch (required).
//! - `BEE_SIGNER_HEX` — 32-byte hex private key (required).

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use bee::api::CollectionUploadOptions;
use bee::file::CollectionEntry;
use bee::swarm::{BatchId, PrivateKey, Topic};
use bee::{Client, Error};
use serde::{Deserialize, Serialize};

const BLOG_FILE: &str = "_blog.json";
const POSTS_DIR: &str = "posts";

#[derive(Serialize, Deserialize, Debug)]
struct BlogState {
    title: String,
    topic_hex: String,
    owner_hex: String,
    feed_manifest_ref: String,
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Error> {
    let url = env::var("BEE_URL").unwrap_or_else(|_| "http://localhost:1633".into());
    let mut args = env::args().skip(1);
    let cmd = args
        .next()
        .ok_or_else(|| Error::argument("usage: swarm-blog <init|new|list|publish> ..."))?;
    let client = Client::new(&url)?;

    match cmd.as_str() {
        "init" => {
            let title = args
                .next()
                .ok_or_else(|| Error::argument("usage: swarm-blog init <title>"))?;
            cmd_init(&client, &url, &title).await
        }
        "new" => {
            let slug = args
                .next()
                .ok_or_else(|| Error::argument("usage: swarm-blog new <slug> <title>"))?;
            let title = args
                .next()
                .ok_or_else(|| Error::argument("usage: swarm-blog new <slug> <title>"))?;
            cmd_new(&slug, &title)
        }
        "list" => cmd_list(),
        "publish" => cmd_publish(&client, &url).await,
        other => Err(Error::argument(format!("unknown command: {other}"))),
    }
}

async fn cmd_init(client: &Client, url: &str, title: &str) -> Result<(), Error> {
    if PathBuf::from(BLOG_FILE).exists() {
        return Err(Error::argument(format!("{BLOG_FILE} already exists")));
    }
    let batch_id = env_batch()?;
    let signer = env_signer()?;
    let owner = signer.public_key()?.address();
    let topic = Topic::from_string(&format!("swarm-blog:{title}"));

    let feed_manifest = client
        .file()
        .create_feed_manifest(&batch_id, &owner, &topic)
        .await?;

    fs::create_dir_all(POSTS_DIR).map_err(|e| Error::argument(format!("mkdir posts: {e}")))?;
    let st = BlogState {
        title: title.into(),
        topic_hex: topic.to_hex(),
        owner_hex: owner.to_hex(),
        feed_manifest_ref: feed_manifest.to_hex(),
    };
    save_state(&st)?;

    let trimmed = url.trim_end_matches('/');
    println!("Initialised blog {title:?}");
    println!("  feed manifest: {}", feed_manifest.to_hex());
    println!("  stable URL:    {trimmed}/bzz/{}/", feed_manifest.to_hex());
    println!("\nNext: `swarm-blog new <slug> <title>` then `swarm-blog publish`.");
    Ok(())
}

fn cmd_new(slug: &str, title: &str) -> Result<(), Error> {
    let _ = load_state()?;
    let path = PathBuf::from(POSTS_DIR).join(format!("{slug}.md"));
    if path.exists() {
        return Err(Error::argument(format!(
            "{} already exists",
            path.display()
        )));
    }
    let template = format!(
        "# {title}\n\nWrite your post here. Markdown is preserved\n\
         in a <pre> block on publish; this is a starter template.\n"
    );
    fs::write(&path, template).map_err(|e| Error::argument(format!("write: {e}")))?;
    println!("Created {}", path.display());
    Ok(())
}

fn cmd_list() -> Result<(), Error> {
    let posts = list_posts()?;
    if posts.is_empty() {
        println!("(no posts yet — `swarm-blog new <slug> <title>`)");
        return Ok(());
    }
    println!("posts/");
    for (slug, title) in posts {
        println!("  {slug:<24} {title}");
    }
    Ok(())
}

async fn cmd_publish(client: &Client, url: &str) -> Result<(), Error> {
    let st = load_state()?;
    let batch_id = env_batch()?;
    let signer = env_signer()?;
    let topic = Topic::from_hex(&st.topic_hex)?;

    let posts = list_posts()?;
    if posts.is_empty() {
        return Err(Error::argument("no posts in posts/ — nothing to publish"));
    }

    let mut entries: Vec<CollectionEntry> = Vec::new();
    let mut links = String::new();
    for (slug, title) in &posts {
        let md_path = PathBuf::from(POSTS_DIR).join(format!("{slug}.md"));
        let body = fs::read_to_string(&md_path)
            .map_err(|e| Error::argument(format!("read {md_path:?}: {e}")))?;
        let html = post_html(title, &body);
        entries.push(CollectionEntry {
            path: format!("posts/{slug}.html"),
            data: html.into_bytes(),
        });
        links.push_str(&format!(
            "<li><a href=\"posts/{slug}.html\">{title}</a></li>\n"
        ));
    }
    let index = index_html(&st.title, &links);
    entries.push(CollectionEntry {
        path: "index.html".into(),
        data: index.into_bytes(),
    });

    println!("Uploading {} entries...", entries.len());
    let opts = CollectionUploadOptions {
        index_document: Some("index.html".into()),
        ..Default::default()
    };
    let result = client
        .file()
        .upload_collection_entries(&batch_id, &entries, Some(&opts))
        .await?;
    println!("  site ref: {}", result.reference.to_hex());

    println!("Updating feed pointer...");
    client
        .file()
        .update_feed_with_reference(&batch_id, &signer, &topic, &result.reference, None)
        .await?;

    let trimmed = url.trim_end_matches('/');
    println!("\nPublished {} posts.", posts.len());
    println!("  stable URL: {trimmed}/bzz/{}/", st.feed_manifest_ref);
    Ok(())
}

fn list_posts() -> Result<Vec<(String, String)>, Error> {
    let _ = load_state()?;
    let dir = PathBuf::from(POSTS_DIR);
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut out: Vec<(String, String)> = vec![];
    for entry in fs::read_dir(&dir).map_err(|e| Error::argument(format!("read posts: {e}")))? {
        let entry = entry.map_err(|e| Error::argument(format!("dir entry: {e}")))?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let slug = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let body = fs::read_to_string(&path).unwrap_or_default();
        let title = body
            .lines()
            .find(|l| l.starts_with("# "))
            .map(|l| l.trim_start_matches('#').trim().to_string())
            .unwrap_or_else(|| slug.clone());
        out.push((slug, title));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

fn index_html(title: &str, links: &str) -> String {
    format!(
        "<!doctype html>\n<html><head><meta charset=\"utf-8\">\n\
         <title>{title}</title></head><body>\n\
         <h1>{title}</h1>\n<ul>\n{links}</ul>\n\
         <hr><p><small>powered by swarm-blog</small></p>\n\
         </body></html>\n"
    )
}

fn post_html(title: &str, body_md: &str) -> String {
    let escaped = body_md
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!(
        "<!doctype html>\n<html><head><meta charset=\"utf-8\">\n\
         <title>{title}</title></head><body>\n\
         <p><a href=\"../index.html\">&larr; back</a></p>\n\
         <pre>{escaped}</pre>\n\
         </body></html>\n"
    )
}

fn env_batch() -> Result<BatchId, Error> {
    let h = env::var("BEE_BATCH_ID").map_err(|_| Error::argument("BEE_BATCH_ID is required"))?;
    BatchId::from_hex(&h)
}

fn env_signer() -> Result<PrivateKey, Error> {
    let h =
        env::var("BEE_SIGNER_HEX").map_err(|_| Error::argument("BEE_SIGNER_HEX is required"))?;
    PrivateKey::from_hex(&h)
}

fn save_state(s: &BlogState) -> Result<(), Error> {
    let bytes = serde_json::to_vec_pretty(s)?;
    fs::write(BLOG_FILE, bytes).map_err(|e| Error::argument(format!("write state: {e}")))
}

fn load_state() -> Result<BlogState, Error> {
    let bytes = fs::read(BLOG_FILE)
        .map_err(|_| Error::argument(format!("{BLOG_FILE} not found — run `init` first")))?;
    Ok(serde_json::from_slice(&bytes)?)
}
