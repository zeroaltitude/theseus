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
    line_cap: usize,
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

/// A test seam: calls of a method whose line holds the text, each refused once
/// (`refuse_once`), so a bind can be made to fail without the core's help.
#[cfg(test)]
static REFUSALS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

/// Every method called through any client of this process, in order: a test
/// reads it to prove a command asked the daemon nothing (theseus-a5uds).
#[cfg(test)]
static CALLS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// The request line of every `vault.keep.answer` sent through any client of this process: a test
/// reads what a form's submit carried (issue 29).
#[cfg(test)]
static KEEP_LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());

#[cfg(test)]
pub(crate) fn keep_lines() -> Vec<String> {
    KEEP_LINES.lock().unwrap().clone()
}

/// The methods called so far.
#[cfg(test)]
pub(crate) fn calls() -> Vec<String> {
    CALLS.lock().unwrap().clone()
}

/// The next call of `method` whose request holds `text` fails, once.
#[cfg(test)]
pub(crate) fn refuse_once(method: &str, text: &str) {
    REFUSALS
        .lock()
        .unwrap()
        .push((method.to_string(), text.to_string()));
}

/// A call a test holds open (`slow_once`): it says it was made (`reached`) and goes no further until the test
/// lets it (`release`), with no sleep.
#[cfg(test)]
pub(crate) struct Hold {
    pub(crate) reached: tokio::sync::Notify,
    pub(crate) release: tokio::sync::Notify,
}

#[cfg(test)]
type Slow = (String, String, Value, Arc<Hold>);

#[cfg(test)]
static SLOW: Mutex<Vec<Slow>> = Mutex::new(Vec::new());

/// The next call of `method` whose request line holds `text` is held until the test releases it, and then
/// answers `answer` (no request is sent): a daemon that takes its time.
#[cfg(test)]
pub(crate) fn slow_once(method: &str, text: &str, answer: Value) -> Arc<Hold> {
    let hold = Arc::new(Hold {
        reached: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
    });
    let slow = (method.to_string(), text.to_string(), answer, hold.clone());
    SLOW.lock().unwrap().push(slow);
    hold
}

impl RpcClient {
    /// Open a connection as `client` (its label and surface) and return the
    /// client plus the stream of notifications (turn events for the sessions
    /// it asked about or watches).
    pub fn connect(
        core: Arc<Core>,
        client: Client,
    ) -> (Arc<Self>, mpsc::UnboundedReceiver<Notification>) {
        let line_cap = core.request_line_cap();
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
                    Ok(Message::Response(r)) if connection_limit(&r) => {
                        tracing::warn!(
                            limit_bytes = line_cap,
                            "discord: core rejected the connection's request size"
                        );
                    }
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
                line_cap,
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
        #[cfg(test)]
        CALLS.lock().unwrap().push(method.to_string());
        let line =
            serde_json::to_string(&Message::Request(Request::new(Id::Num(id), method, params)))
                .map_err(CallError::local)?;
        // Count the encoded request and the newline the writer adds, not the
        // decoded files: text escaping and coalesced messages count too.
        if line.len() >= self.line_cap {
            return Err(CallError {
                code: theseus_protocol::error_code::LIMIT,
                message: "The attachments together are too large; please send fewer at once."
                    .into(),
                data: Value::Null,
            });
        }
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);
        #[cfg(test)]
        if method == theseus_protocol::method::VAULT_KEEP_ANSWER {
            KEEP_LINES.lock().unwrap().push(line.clone());
        }
        #[cfg(test)]
        {
            let mut refusals = REFUSALS.lock().unwrap();
            if let Some(i) = refusals
                .iter()
                .position(|(m, t)| m == method && line.contains(t.as_str()))
            {
                refusals.remove(i);
                self.pending.lock().unwrap().remove(&id);
                return Err(CallError::local(format!("{method} refused by the test")));
            }
        }
        #[cfg(test)]
        {
            let slow = {
                let mut slow = SLOW.lock().unwrap();
                let at = slow
                    .iter()
                    .position(|(m, t, ..)| m == method && line.contains(t.as_str()));
                at.map(|i| slow.remove(i))
            };
            if let Some((_, _, answer, hold)) = slow {
                self.pending.lock().unwrap().remove(&id);
                hold.reached.notify_one();
                hold.release.notified().await;
                return serde_json::from_value(answer).map_err(CallError::local);
            }
        }
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

fn connection_limit(response: &Response) -> bool {
    response.id == Id::Num(0)
        && response
            .error
            .as_ref()
            .is_some_and(|e| e.code == theseus_protocol::error_code::LIMIT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_limit_is_not_a_call_reply() {
        let limit = theseus_protocol::error_code::LIMIT;
        assert!(connection_limit(&Response::err(
            Id::Num(0),
            limit,
            "too large"
        )));
        assert!(!connection_limit(&Response::err(
            Id::Num(1),
            limit,
            "too large"
        )));
        assert!(!connection_limit(&Response::err(
            Id::Num(0),
            theseus_protocol::error_code::PARSE,
            "bad json"
        )));
    }
}
