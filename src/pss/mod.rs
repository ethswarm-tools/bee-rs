//! PSS send / subscribe / receive. Mirrors `pkg/pss` in bee-go and
//! `Bee.pssSend` / `Bee.pssSubscribe` / `Bee.pssReceive` in bee-js.
//!
//! Get a [`PssApi`] handle via [`crate::Client::pss`].

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use reqwest::Method;
use tokio::sync::mpsc;
use tokio::time;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::protocol::Message;

use crate::client::{Inner, request};
use crate::swarm::{BatchId, Error, PublicKey, Topic};

/// Handle exposing the PSS endpoints. Cheap to clone.
#[derive(Clone, Debug)]
pub struct PssApi {
    pub(crate) inner: Arc<Inner>,
}

impl PssApi {
    pub(crate) fn new(inner: Arc<Inner>) -> Self {
        Self { inner }
    }

    /// `POST /pss/send/{topic}/{target}` — send a PSS message.
    ///
    /// - `batch_id` is mandatory (Bee 2.7+ rejects requests without
    ///   `Swarm-Postage-Batch-Id`).
    /// - `target` is a routing prefix as hex (e.g. `"1234"`), not a
    ///   full overlay address. Bee uses it as a partial XOR target so
    ///   gossip can reach the recipient without revealing the full
    ///   address.
    /// - `recipient` is optional; when provided, the message is
    ///   end-to-end encrypted with the recipient's public key.
    pub async fn send(
        &self,
        batch_id: &BatchId,
        topic: &Topic,
        target: &str,
        data: impl Into<Bytes>,
        recipient: Option<&PublicKey>,
    ) -> Result<(), Error> {
        let path = format!("pss/send/{}/{target}", topic.to_hex());
        let mut builder = request(&self.inner, Method::POST, &path)?
            .header("Swarm-Postage-Batch-Id", batch_id.to_hex())
            .body(data.into());
        if let Some(pk) = recipient {
            let compressed = pk.compressed_hex()?;
            builder = builder.query(&[("recipient", compressed)]);
        }
        self.inner.send(builder).await?;
        Ok(())
    }

    /// `GET /pss/subscribe/{topic}` over a WebSocket. Returns a
    /// [`Subscription`] yielding decoded message bytes via
    /// [`Subscription::recv`].
    pub async fn subscribe(&self, topic: &Topic) -> Result<Subscription, Error> {
        let path = format!("pss/subscribe/{}", topic.to_hex());
        Subscription::open(&self.inner, &path).await
    }

    /// One-shot helper: subscribe, wait for the next message (with
    /// `timeout`), close the subscription, return the bytes.
    pub async fn receive(&self, topic: &Topic, timeout: Duration) -> Result<Bytes, Error> {
        let mut sub = self.subscribe(topic).await?;
        let result = match time::timeout(timeout, sub.recv()).await {
            Ok(Some(b)) => Ok(b),
            Ok(None) => Err(Error::argument("pss subscription closed before message")),
            Err(_) => Err(Error::argument("pss receive timed out")),
        };
        sub.cancel();
        result
    }
}

/// Active PSS or GSOC websocket subscription. Drop it (or call
/// [`Subscription::cancel`]) to close the underlying socket.
#[derive(Debug)]
pub struct Subscription {
    rx: mpsc::Receiver<Bytes>,
    cancel: Option<mpsc::Sender<()>>,
}

impl Subscription {
    /// Open a websocket subscription against an arbitrary `path` on
    /// the configured base URL. Used internally by PSS and GSOC.
    pub(crate) async fn open(inner: &Arc<Inner>, path: &str) -> Result<Self, Error> {
        let mut url = inner.url(path)?;
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

        let (ws, _resp) = connect_async(url.as_str())
            .await
            .map_err(|e| Error::argument(format!("websocket connect: {e}")))?;
        let (mut sink, mut stream) = ws.split();

        let (msg_tx, msg_rx) = mpsc::channel::<Bytes>(64);
        let (cancel_tx, mut cancel_rx) = mpsc::channel::<()>(1);

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel_rx.recv() => {
                        let _ = sink.send(Message::Close(None)).await;
                        let _ = sink.close().await;
                        break;
                    }
                    msg = stream.next() => {
                        match msg {
                            Some(Ok(Message::Binary(b))) => {
                                if msg_tx.send(Bytes::from(b)).await.is_err() {
                                    break;
                                }
                            }
                            Some(Ok(Message::Text(t))) => {
                                if msg_tx
                                    .send(Bytes::from(t.into_bytes()))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }
                            Some(Ok(Message::Close(_))) | None => break,
                            Some(Ok(_)) => continue, // Ping/Pong/Frame ignored
                            Some(Err(_)) => break,
                        }
                    }
                }
            }
        });

        Ok(Self {
            rx: msg_rx,
            cancel: Some(cancel_tx),
        })
    }

    /// Wait for the next message. Returns `None` when the
    /// subscription has been closed.
    pub async fn recv(&mut self) -> Option<Bytes> {
        self.rx.recv().await
    }

    /// Close the subscription. Idempotent.
    pub fn cancel(&mut self) {
        if let Some(tx) = self.cancel.take() {
            let _ = tx.try_send(());
        }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.cancel();
    }
}
