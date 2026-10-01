//! A blocking JSON-RPC client for a daemon's socket (NDJSON, one request at a
//! time per connection). Notifications that arrive while a call waits are
//! handed to the caller, and can be read on their own after it.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};

/// The daemon answered with an error.
#[derive(Debug, Clone)]
pub struct RpcError {
    pub method: String,
    pub code: i64,
    pub message: String,
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} failed ({}): {}",
            self.method, self.code, self.message
        )
    }
}

impl std::error::Error for RpcError {}

pub struct Client {
    writer: UnixStream,
    reader: BufReader<UnixStream>,
    /// A line read in part when a wait ran out: the rest follows.
    pending: String,
    next: u64,
}

impl Client {
    pub fn connect(sock: &Path) -> Result<Client> {
        let s = UnixStream::connect(sock)
            .with_context(|| format!("connecting to {}", sock.display()))?;
        let reader = BufReader::new(s.try_clone()?);
        Ok(Client {
            writer: s,
            reader,
            pending: String::new(),
            next: 1,
        })
    }

    /// One line from the daemon, or None at `deadline`.
    fn line(&mut self, deadline: Instant) -> Result<Option<Value>> {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Ok(None);
        }
        // The clone shares the socket, so its receive timeout is the reader's.
        self.writer
            .set_read_timeout(Some(left.max(Duration::from_millis(1))))?;
        match self.reader.read_line(&mut self.pending) {
            Ok(_) if !self.pending.ends_with('\n') => bail!("the daemon closed the connection"),
            Ok(_) => {
                let line = std::mem::take(&mut self.pending);
                Ok(Some(
                    serde_json::from_str(line.trim_end()).context("a line that is not JSON")?,
                ))
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                Ok(None)
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Call `method`; notifications on the way go to `on_note`.
    pub fn call_with(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
        mut on_note: impl FnMut(&str, &Value),
    ) -> Result<Value> {
        let id = self.next;
        self.next += 1;
        let req = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        self.writer.write_all(format!("{req}\n").as_bytes())?;
        let deadline = Instant::now() + timeout;
        loop {
            let Some(v) = self.line(deadline)? else {
                bail!("{method}: no answer in {} s", timeout.as_secs());
            };
            if v.get("id").and_then(Value::as_u64) == Some(id) {
                if let Some(e) = v.get("error").filter(|e| !e.is_null()) {
                    return Err(anyhow!(RpcError {
                        method: method.into(),
                        code: e["code"].as_i64().unwrap_or(0),
                        message: e["message"].as_str().unwrap_or("").into(),
                    }));
                }
                return Ok(v.get("result").cloned().unwrap_or(Value::Null));
            }
            if let Some(m) = v.get("method").and_then(Value::as_str) {
                on_note(m, v.get("params").unwrap_or(&Value::Null));
            }
        }
    }

    pub fn call(&mut self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        self.call_with(method, params, timeout, |_, _| {})
    }

    /// The next notification, or None at `deadline`.
    pub fn notification(&mut self, deadline: Instant) -> Result<Option<(String, Value)>> {
        loop {
            let Some(v) = self.line(deadline)? else {
                return Ok(None);
            };
            if let Some(m) = v.get("method").and_then(Value::as_str) {
                return Ok(Some((
                    m.to_string(),
                    v.get("params").cloned().unwrap_or(Value::Null),
                )));
            }
        }
    }
}
