//! A connection to `theseusd` (theseus-7yx, step 10a): over its Unix socket,
//! or over the pipes of a `theseusd --stdio` it spawns. It sends requests,
//! hands back their answers, and reads the notifications that come between.
//! It prints nothing and knows no command line, so the CLI and the TUI share
//! it.

use std::path::PathBuf;
use std::process::Stdio;

use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;
use serde_json::Value;
use theseus_protocol::{method, Id, Message, Request, Response};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

/// A JSON-RPC error response, kept structured so callers can read `data`.
#[derive(Debug)]
pub struct CallError {
    pub code: i64,
    pub message: String,
    pub data: Value,
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let class = self.data.get("class").and_then(Value::as_str);
        let transient = self.data.get("transient").and_then(Value::as_bool);
        match (class, transient) {
            (Some(c), Some(t)) => write!(
                f,
                "{} [class={c}, transient={t}, code {}]",
                self.message, self.code
            ),
            // `config_unconfirmed` (theseus-2fo) names its class alone.
            (Some(c), None) => write!(f, "{} [class={c}, code {}]", self.message, self.code),
            _ => write!(f, "{} (code {})", self.message, self.code),
        }
    }
}

impl std::error::Error for CallError {}

/// One connection to a daemon: newline-delimited JSON-RPC, its requests
/// numbered from 1.
pub struct Conn {
    reader: BufReader<Box<dyn AsyncRead + Unpin + Send>>,
    writer: Box<dyn AsyncWrite + Unpin + Send>,
    /// What `next` has read of the line it is reading. It is kept here, not
    /// in `next`, so a `next` dropped mid-line (a `select!` another branch
    /// won) loses nothing: the next call goes on where it stopped.
    line: Vec<u8>,
    _child: Option<tokio::process::Child>,
    next_id: u64,
}

impl Conn {
    /// Connect to a daemon's socket; a leading `~` is the home directory.
    pub async fn socket(path: &str) -> Result<Self> {
        let path = PathBuf::from(tilde(path, std::env::var("HOME").ok()));
        let stream = tokio::net::UnixStream::connect(&path)
            .await
            .with_context(|| {
                format!(
                    "connecting to theseusd at {} (is it running? try --spawn)",
                    path.display()
                )
            })?;
        let (r, w) = stream.into_split();
        Ok(Self::over(r, w))
    }

    /// Spawn `BIN --stdio` and talk over its pipes. Its stderr is this
    /// process's. `close` stops it cleanly; a connection dropped without
    /// `close` kills it.
    pub fn spawn(bin: &str) -> Result<Self> {
        let mut child = tokio::process::Command::new(bin)
            .arg("--stdio")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("spawning {bin}"))?;
        let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;
        let mut conn = Self::over(stdout, stdin);
        conn._child = Some(child);
        Ok(conn)
    }

    /// A connection over any pair of streams: a test's scripted daemon over
    /// `tokio::io::duplex`, for one.
    pub fn over(
        reader: impl AsyncRead + Unpin + Send + 'static,
        writer: impl AsyncWrite + Unpin + Send + 'static,
    ) -> Self {
        Self {
            reader: BufReader::new(Box::new(reader)),
            writer: Box::new(writer),
            line: Vec::new(),
            _child: None,
            next_id: 1,
        }
    }

    /// Send a request and return its id at once: its answer comes from
    /// `next`, with the same id. An operator's method is refused inside a
    /// Theseus job before anything is sent (`refuse_in_a_job`).
    pub async fn send(&mut self, method: &str, params: Value) -> Result<Id> {
        refuse_in_a_job(method, job_session().as_deref())?;
        let id = Id::Num(self.next_id);
        self.next_id += 1;
        let mut line = serde_json::to_string(&Request::new(id.clone(), method, params))?;
        line.push('\n');
        self.writer.write_all(line.as_bytes()).await?;
        self.writer.flush().await?;
        Ok(id)
    }

    /// The next message, an answer or a notification; `None` once the daemon
    /// closes the connection. It is cancel-safe: a call dropped before it
    /// returns keeps the part of a line it read for the next call.
    pub async fn next(&mut self) -> Result<Option<Message>> {
        loop {
            let n = self.reader.read_until(b'\n', &mut self.line).await?;
            if n == 0 && self.line.is_empty() {
                return Ok(None);
            }
            let bytes = std::mem::take(&mut self.line);
            // What `read_line` said of a line that is not UTF-8.
            let line = String::from_utf8(bytes).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "stream did not contain valid UTF-8",
                )
            })?;
            if line.trim().is_empty() {
                continue;
            }
            return Ok(Some(
                serde_json::from_str(line.trim()).context("bad line from server")?,
            ));
        }
    }

    /// Send a request and wait for its answer, handing the notifications
    /// that come first to `on_notify`.
    pub async fn call(
        &mut self,
        method: &str,
        params: Value,
        mut on_notify: impl FnMut(&str, &Value),
    ) -> Result<Value> {
        let id = self.send(method, params).await?;
        while let Some(msg) = self.next().await? {
            match msg {
                Message::Notification(n) => on_notify(&n.method, &n.params),
                Message::Response(r) if r.id == id => return answer(r),
                _ => {}
            }
        }
        Err(anyhow!("connection closed before response"))
    }

    /// A request whose answer is all it waits for: its notifications, if
    /// any, are dropped.
    pub async fn request(&mut self, method: &str, params: impl Serialize) -> Result<Value> {
        self.call(method, serde_json::to_value(params)?, |_, _| {})
            .await
    }

    /// End the connection. A daemon this connection spawned gets the clean
    /// stop every daemon has, `shutdown`: its stopping row and its
    /// checkpoint, so the next open of its store replays nothing
    /// (theseus-n88g.2; before, the drop killed it, and the next open
    /// replayed the run's tail). Then its stdin closes, which ends its read,
    /// and it is waited for, at most `STOP_WAIT`, and killed only past that:
    /// `Err` says so. A socket's connection just closes.
    ///
    /// A daemon that has already exited (`theseus --spawn shutdown`, or one
    /// that failed) is not written to. One that exits as the stop is written
    /// breaks the pipe: the caller ignores SIGPIPE around this call, or the
    /// write's signal ends the process (the CLI restores its default).
    pub async fn close(mut self) -> Result<()> {
        let Some(mut child) = self._child.take() else {
            return Ok(());
        };
        if matches!(child.try_wait(), Ok(None)) {
            // Its answer is not needed: its exit is what counts.
            let _ = tokio::time::timeout(
                STOP_WAIT,
                self.request(theseus_protocol::method::SHUTDOWN, Value::Null),
            )
            .await;
        }
        drop(self);
        match tokio::time::timeout(STOP_WAIT, child.wait()).await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => Err(anyhow!("waiting for the spawned theseusd: {e}")),
            Err(_) => {
                let _ = child.kill().await;
                Err(anyhow!(
                    "the spawned theseusd did not stop within {} s of its shutdown; it was killed",
                    STOP_WAIT.as_secs()
                ))
            }
        }
    }
}

/// How long a spawned daemon has to answer its `shutdown` and to exit after
/// it (`Conn::close`). A clean stop takes tens of milliseconds.
const STOP_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

/// An answer's result, or its error as a `CallError`: what `Conn::call`
/// returns, for a caller that reads the messages itself (`Conn::next`).
pub fn answer(r: Response) -> Result<Value> {
    if let Some(e) = r.error {
        return Err(CallError {
            code: e.code,
            message: e.message,
            data: e.data,
        }
        .into());
    }
    Ok(r.result.unwrap_or(Value::Null))
}

/// `path` with a leading `~` (alone, or before `/`) as `home`, as a shell
/// reads it, so a quoted `--socket` or `THESEUS_SOCKET` works too. Without a
/// home, it is left as written (review 2's consideration 5: no
/// `shellexpand`). `theseus herdr sync` reads its `--socket` with it too.
pub fn tilde(path: &str, home: Option<String>) -> String {
    match (path.strip_prefix('~'), home) {
        (Some(rest), Some(h)) if !h.is_empty() && (rest.is_empty() || rest.starts_with('/')) => {
            format!("{h}{rest}")
        }
        _ => path.to_string(),
    }
}

/// The session whose job this client runs in (theseus-b5cl): the
/// `THESEUS_SESSION` every job's environment carries. A session this client
/// opens, or a turn it sends, names it as `opened_from`, so it takes that
/// session's hold of external text. None outside a job.
pub fn job_session() -> Option<String> {
    job_session_in(std::env::var(theseus_protocol::JOB_SESSION_ENV).ok())
}

/// The methods only the operator makes, each with the command that makes it
/// (theseus-zmgb): an answer to a waiting call, the undo of a tightening, a
/// trust, a publish, the AWS bootstrap, the alerts' confirmation, and the
/// ontology's writes (theseus-8kk.1: guidance steers every session in its
/// category), and the answers to Jev's proposals (28b), which write them.
pub const OPERATORS: [(&str, &str); 11] = [
    (method::ACTION_CONFIRM, "theseus confirm"),
    (method::POLICY_UNTIGHTEN, "theseus policy untighten"),
    (method::POLICY_TRUST, "theseus policy trust"),
    (method::PLACE_PUBLISH, "theseus publish"),
    (method::AWS_BOOTSTRAP, "theseus aws bootstrap"),
    (method::AWS_CONFIRM_ALERTS, "theseus aws confirm-alerts"),
    (method::ONTOLOGY_CATEGORY_ADD, "theseus ontology topic add"),
    (method::ONTOLOGY_GUIDANCE_SET, "theseus ontology guide"),
    (method::ONTOLOGY_MEMBERSHIP_SET, "theseus ontology member"),
    (method::ONTOLOGY_PROPOSAL_ACCEPT, "theseus ontology accept"),
    (method::ONTOLOGY_PROPOSAL_REJECT, "theseus ontology reject"),
];

/// Refuse an operator's method from inside a Theseus job (theseus-zmgb):
/// `session` is the job's marker, the `THESEUS_SESSION` every job's
/// environment carries. A speed bump, not a boundary: a job can strip its
/// environment. L1, whose view has no route to the daemon, is the boundary.
pub fn refuse_in_a_job(method: &str, session: Option<&str>) -> Result<()> {
    let (Some(session), Some((_, command))) =
        (session, OPERATORS.iter().find(|(m, _)| *m == method))
    else {
        return Ok(());
    };
    bail!(
        "{command} refused: it is the operator's to run, and this shell is a Theseus job's \
         (THESEUS_SESSION={session} is set), so it would answer for the operator. Run it from \
         your own shell."
    )
}

/// `job_session` of the variable's value: an empty one names none.
fn job_session_in(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// The operator's methods, refused inside a job with the reason and the
    /// way out, and only there; every other method goes (theseus-zmgb).
    #[test]
    fn the_operators_methods_are_refused_inside_a_job() {
        for (m, command) in OPERATORS {
            let e = refuse_in_a_job(m, Some("ses_0000aa1b2c3"))
                .unwrap_err()
                .to_string();
            assert!(e.starts_with(&format!("{command} refused:")), "{e}");
            assert!(
                e.contains("(THESEUS_SESSION=ses_0000aa1b2c3 is set)"),
                "{e}"
            );
            assert!(e.ends_with("Run it from your own shell."), "{e}");
            assert!(refuse_in_a_job(m, None).is_ok(), "{m}");
        }
        for m in [
            method::POLICY_TIGHTEN,
            method::CONFIRM_LIST,
            method::TURN_SUBMIT,
            method::HEALTH,
        ] {
            assert!(refuse_in_a_job(m, Some("ses_0000aa1b2c3")).is_ok(), "{m}");
        }
    }

    /// A job's process cannot write guidance (theseus-8kk.1): guidance
    /// steers every session in its category, so the ontology's writes are
    /// the operator's, refused inside a job; its reads are not.
    #[test]
    fn a_jobs_process_cannot_write_the_ontology() {
        for m in [
            method::ONTOLOGY_GUIDANCE_SET,
            method::ONTOLOGY_CATEGORY_ADD,
            method::ONTOLOGY_MEMBERSHIP_SET,
            method::ONTOLOGY_PROPOSAL_ACCEPT,
            method::ONTOLOGY_PROPOSAL_REJECT,
        ] {
            let e = refuse_in_a_job(m, Some("ses_0000aa1b2c3")).unwrap_err();
            assert!(e.to_string().contains("theseus ontology"), "{e}");
        }
        assert!(refuse_in_a_job(method::ONTOLOGY_LIST, Some("ses_0000aa1b2c3")).is_ok());
        assert!(refuse_in_a_job(method::ONTOLOGY_PROPOSALS, Some("ses_0000aa1b2c3")).is_ok());
    }

    #[test]
    fn a_jobs_session_is_its_variable_and_an_empty_one_is_none() {
        assert_eq!(
            job_session_in(Some("ses_0000aa1b2c3".into())).as_deref(),
            Some("ses_0000aa1b2c3")
        );
        assert_eq!(job_session_in(Some(" ".into())), None);
        assert_eq!(job_session_in(None), None);
    }

    #[test]
    fn a_leading_tilde_is_the_home_directory() {
        let home = || Some("/home/invented".to_string());
        assert_eq!(
            tilde("~/.theseus/theseus.sock", home()),
            "/home/invented/.theseus/theseus.sock"
        );
        assert_eq!(tilde("~", home()), "/home/invented");
        assert_eq!(tilde("~other/s.sock", home()), "~other/s.sock");
        assert_eq!(tilde("/run/x.sock", home()), "/run/x.sock");
        assert_eq!(tilde("~/x.sock", None), "~/x.sock");
    }

    /// A connection to a scripted daemon: its other end.
    fn pair() -> (Conn, tokio::io::DuplexStream) {
        let (ours, theirs) = tokio::io::duplex(4096);
        let (r, w) = tokio::io::split(ours);
        (Conn::over(r, w), theirs)
    }

    /// `next` is cancel-safe (theseus-7yx): a call dropped mid-line keeps what
    /// it read, so a `select!` that races it with a key press loses nothing.
    /// `read_line`, which it used before, lost the part it had read.
    #[tokio::test]
    async fn a_next_dropped_mid_line_loses_nothing() {
        let (mut conn, mut daemon) = pair();
        daemon
            .write_all(br#"{"jsonrpc":"2.0","method":"model.delta","#)
            .await
            .unwrap();
        let cut = tokio::time::timeout(Duration::from_millis(50), conn.next()).await;
        assert!(cut.is_err(), "half a line is no message: {cut:?}");
        daemon
            .write_all(b"\"params\":{\"turn_id\":\"turn_a\",\"loop_index\":0,\"text\":\"tide\"}}\n")
            .await
            .unwrap();
        match conn.next().await.unwrap() {
            Some(Message::Notification(n)) => {
                assert_eq!(n.method, "model.delta");
                assert_eq!(n.params["text"], "tide");
            }
            other => panic!("{other:?}"),
        }
        drop(daemon);
        assert!(conn.next().await.unwrap().is_none(), "the daemon closed it");
    }

    /// `call` hands the notifications that come before its answer to its
    /// callback, skips blank lines, and returns the answer. An error answer
    /// is a `CallError` that names its class.
    #[tokio::test]
    async fn a_call_hands_over_its_notifications_and_returns_its_answer() {
        let (mut conn, daemon) = pair();
        let (dr, mut dw) = tokio::io::split(daemon);
        let script = tokio::spawn(async move {
            let mut lines = BufReader::new(dr).lines();
            let line = lines.next_line().await.unwrap().unwrap();
            let first: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(
                (first["method"].as_str(), first["id"].as_u64()),
                (Some("health"), Some(1))
            );
            dw.write_all(
                b"{\"jsonrpc\":\"2.0\",\"method\":\"narrative.line\",\"params\":{\"seq\":1}}\n\n\
                  {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"name\":\"theseusd\"}}\n",
            )
            .await
            .unwrap();
            let line = lines.next_line().await.unwrap().unwrap();
            let second: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(second["params"]["input"], "low water?");
            dw.write_all(
                b"{\"jsonrpc\":\"2.0\",\"id\":2,\"error\":{\"code\":-32003,\"message\":\"the \
                  provider is overloaded\",\"data\":{\"class\":\"overloaded\",\"transient\":true}}}\n",
            )
            .await
            .unwrap();
        });
        let mut seen = Vec::new();
        let health = conn
            .call(theseus_protocol::method::HEALTH, Value::Null, |m, _| {
                seen.push(m.to_string())
            })
            .await
            .unwrap();
        assert_eq!(health["name"], "theseusd");
        assert_eq!(seen, ["narrative.line"]);
        let err = conn
            .request("turn.submit", serde_json::json!({"input": "low water?"}))
            .await
            .unwrap_err();
        assert_eq!(
            err.downcast_ref::<CallError>().map(|c| c.code),
            Some(-32003)
        );
        assert_eq!(
            err.to_string(),
            "the provider is overloaded [class=overloaded, transient=true, code -32003]"
        );
        script.await.unwrap();
    }
}
