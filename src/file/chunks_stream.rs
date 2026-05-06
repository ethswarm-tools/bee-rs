//! `WS /chunks/stream` — websocket-driven chunk upload session.
//!
//! Mirrors bee-go's `chunks/stream` server-side handler: the client
//! sends each chunk as a binary websocket message and the server
//! replies with a single-byte `0x00` ack. A non-zero or text frame is
//! treated as an error from the server.
//!
//! Tag-bound uploads (`swarm-tag` query) keep chunks local until the
//! socket closes; tag-less uploads forward each chunk to the network
//! as it arrives. See the OpenAPI summary for `/chunks/stream`.
//!
//! # Cancellation
//!
//! Drop the [`ChunkStream`] to release the socket; the in-flight TCP
//! connection is closed without a graceful websocket close frame.
//! Call [`ChunkStream::close`] for a graceful shutdown.

use bytes::Bytes;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

use crate::swarm::{BatchId, Error};

use super::FileApi;

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Active `/chunks/stream` upload session. Open one via
/// [`FileApi::chunks_stream`].
///
/// Each [`Self::send_chunk`] sends one binary frame and awaits the
/// server's `0x00` ack before returning. The session remains usable
/// until [`Self::close`] (or until dropped).
#[derive(Debug)]
pub struct ChunkStream {
    sink: SplitSink<Ws, Message>,
    stream: SplitStream<Ws>,
    closed: bool,
}

impl ChunkStream {
    /// Send one chunk (`span || payload` framing per Bee's content-
    /// addressed chunk format) and await the server ack.
    ///
    /// Returns [`Error::Argument`] if the websocket has already been
    /// closed or if the server replied with anything other than the
    /// single-byte `0` ack.
    pub async fn send_chunk(&mut self, chunk: impl Into<Bytes>) -> Result<(), Error> {
        if self.closed {
            return Err(Error::argument("chunk stream is closed"));
        }
        let bytes: Bytes = chunk.into();
        self.sink
            .send(Message::Binary(bytes.to_vec()))
            .await
            .map_err(|e| Error::argument(format!("websocket send: {e}")))?;

        // Wait for the next non-control frame and validate it as the
        // ack. Ping / Pong / Frame are forwarded transparently by
        // tokio-tungstenite, so they shouldn't surface here, but we
        // guard for them anyway.
        loop {
            let next = self
                .stream
                .next()
                .await
                .ok_or_else(|| Error::argument("chunk stream closed before ack"))?;
            match next {
                Ok(Message::Binary(b)) => {
                    if b.is_empty() || b == [0u8] {
                        return Ok(());
                    }
                    return Err(Error::argument(format!(
                        "chunk stream unexpected ack bytes: {b:?}"
                    )));
                }
                Ok(Message::Text(t)) => {
                    return Err(Error::argument(format!("chunk stream server error: {t}")));
                }
                Ok(Message::Close(_)) => {
                    self.closed = true;
                    return Err(Error::argument("chunk stream closed by server"));
                }
                Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_)) => continue,
                Err(e) => return Err(Error::argument(format!("websocket recv: {e}"))),
            }
        }
    }

    /// Close the websocket gracefully. Idempotent. Errors during
    /// close are swallowed — the socket is dropped regardless.
    pub async fn close(mut self) -> Result<(), Error> {
        if !self.closed {
            let _ = self.sink.send(Message::Close(None)).await;
            let _ = self.sink.close().await;
            self.closed = true;
        }
        Ok(())
    }
}

impl FileApi {
    /// Open a `/chunks/stream` websocket upload session against the
    /// configured Bee node.
    ///
    /// - `batch_id` is sent in the `Swarm-Postage-Batch-Id` header and
    ///   is used to stamp every chunk uploaded over this socket.
    /// - `tag` (optional) is the existing tag UID to associate with
    ///   the upload via the `swarm-tag` query parameter; without a
    ///   tag, chunks are forwarded to the network as they arrive.
    pub async fn chunks_stream(
        &self,
        batch_id: &BatchId,
        tag: Option<u64>,
    ) -> Result<ChunkStream, Error> {
        let mut url = self.inner.url("chunks/stream")?;
        let scheme = match url.scheme() {
            "http" => "ws",
            "https" => "wss",
            other => {
                return Err(Error::argument(format!(
                    "unsupported base URL scheme for websocket: {other}"
                )));
            }
        };
        url.set_scheme(scheme)
            .map_err(|_| Error::argument("failed to set websocket scheme"))?;
        if let Some(t) = tag {
            url.query_pairs_mut()
                .append_pair("swarm-tag", &t.to_string());
        }

        let mut req = url
            .as_str()
            .into_client_request()
            .map_err(|e| Error::argument(format!("websocket request: {e}")))?;
        let value = HeaderValue::from_str(&batch_id.to_hex())
            .map_err(|e| Error::argument(format!("invalid batch id header: {e}")))?;
        req.headers_mut().insert("swarm-postage-batch-id", value);

        let (ws, _resp) = connect_async(req)
            .await
            .map_err(|e| Error::argument(format!("websocket connect: {e}")))?;
        let (sink, stream) = ws.split();

        Ok(ChunkStream {
            sink,
            stream,
            closed: false,
        })
    }
}
