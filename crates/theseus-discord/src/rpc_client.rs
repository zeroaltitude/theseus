//! A protocol client over an in-process pipe. The binding speaks to the core
//! exactly as the web UI does (`web.rs`): NDJSON lines into `serve_connection`,
//! responses matched by id, notifications handed to the router.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use theseus_core::approval::Client;
use theseus_core::Core;
use theseus_protocol::{Id, Message, Notification, Request, Response};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot};

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Response>>>>;

pub struct RpcClient {
    out: mpsc::UnboundedSender<String>,
    pending: Pending,
    next: AtomicU64,
}

/// A failed call: the core's error, or the pipe itself.
#[derive(Debug, Clone)]
pub struct CallError {
    pub code: i64,
    pub message: String,
    pub data: Value,
}

impl CallError {
    fn local(message: impl std::fmt::Display) -> Self {
        Self {
            code: 0,
            message: message.to_string(),
            data: Value::Null,
        }
    }
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CallError {}

impl RpcClient {
    /// Open a connection as `client` (its label and surface) and return the
    /// client plus the stream of notifications (turn events for the sessions
    /// it asked about or watches).
    pub fn connect(
        core: Arc<Core>,
        client: Client,
    ) -> (Arc<Self>, mpsc::UnboundedReceiver<Notification>) {
        let (ours, theirs) = tokio::io::duplex(1 << 20);
        let (core_r, core_w) = tokio::io::split(theirs);
        tokio::spawn(core.serve_connection(core_r, core_w, client));
        let (from_core, mut to_core) = tokio::io::split(ours);

        let (out, mut out_rx) = mpsc::unbounded_channel::<String>();
        tokio::spawn(async move {
            while let Some(mut line) = out_rx.recv().await {
                line.push('\n');
                if to_core.write_all(line.as_bytes()).await.is_err() {
                    break;
                }
            }
        });

        let pending: Pending = Arc::default();
        let (ntx, nrx) = mpsc::unbounded_channel();
        let waiters = pending.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(from_core).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                match serde_json::from_str::<Message>(&line) {
                    Ok(Message::Response(r)) => {
                        let tx = match &r.id {
                            Id::Num(n) => waiters.lock().unwrap().remove(n),
                            _ => None,
                        };
                        if let Some(tx) = tx {
                            let _ = tx.send(r);
                        }
                    }
                    Ok(Message::Notification(n)) => {
                        let _ = ntx.send(n);
                    }
                    Ok(Message::Request(_)) => {}
                    Err(e) => tracing::warn!(error = %e, "discord: unreadable line from the core"),
                }
            }
            // The core went away: every waiter's sender drops, and each call fails.
            waiters.lock().unwrap().clear();
        });

        (
            Arc::new(Self {
                out,
                pending,
                next: AtomicU64::new(1),
            }),
            nrx,
        )
    }

    pub async fn call<P: Serialize, R: DeserializeOwned>(
        &self,
        method: &str,
        params: P,
    ) -> Result<R, CallError> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);
        let line =
            serde_json::to_string(&Message::Request(Request::new(Id::Num(id), method, params)))
                .map_err(CallError::local)?;
        if self.out.send(line).is_err() {
            self.pending.lock().unwrap().remove(&id);
            return Err(CallError::local("the core connection is closed"));
        }
        let resp = rx
            .await
            .map_err(|_| CallError::local("the core connection closed during the call"))?;
        if let Some(e) = resp.error {
            return Err(CallError {
                code: e.code,
                message: e.message,
                data: e.data,
            });
        }
        serde_json::from_value(resp.result.unwrap_or(Value::Null)).map_err(CallError::local)
    }
}
