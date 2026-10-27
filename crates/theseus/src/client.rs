//! A connection to `theseusd` (theseus-7yx, step 10a): over its Unix socket,
//! or over the pipes of a `theseusd --stdio` it spawns. It sends requests,
//! hands back their answers, and reads the notifications that come between.
//! It prints nothing and knows no command line, so the CLI and the TUI share
//! it.

use std::path::{Path, PathBuf};
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
    /// The socket this connection is to, when it is one: a watch that must
    /// outlive a daemon's restart connects to it again.
    socket: Option<PathBuf>,
}

impl Conn {
    /// Connect to a daemon's socket; a leading `~` is the home directory.
    pub async fn socket(path: &str) -> Result<Self> {
        Self::socket_within(path, None).await
    }

    /// `socket`, giving up after `within`: what a prompt's segment or a
    /// status line can afford (`theseus status`, theseus-lweh). The error
    /// reads as any failed connect does, so it exits 3.
    pub async fn socket_within(path: &str, within: Option<std::time::Duration>) -> Result<Self> {
        let path = PathBuf::from(tilde(path, std::env::var("HOME").ok()));
        let connect = tokio::net::UnixStream::connect(&path);
        let stream = match within {
            Some(d) => tokio::time::timeout(d, connect)
                .await
                .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "timed out"))
                .and_then(|r| r),
            None => connect.await,
        }
        .with_context(|| {
            format!(
                "connecting to theseusd at {} (is it running? try --spawn)",
                path.display()
            )
        })?;
        let (r, w) = stream.into_split();
        let mut conn = Self::over(r, w);
        conn.socket = Some(path);
        Ok(conn)
    }

    /// The socket this connection is to; none for a spawned daemon or a
    /// test's pipe, which there is nothing to come back to.
    pub fn socket_path(&self) -> Option<&Path> {
        self.socket.as_deref()
    }

    /// Spawn `BIN --stdio` and talk over its pipes. Its stderr is this
    /// process's. `close` stops it cleanly; a connection dropped without
    /// `close` kills it.
    pub fn spawn(bin: &str) -> Result<Self> {
        Self::spawn_with(bin, &[])
    }

    /// `spawn`, with more of the daemon's arguments after `--stdio`: an
    /// `ask`'s `--one-shot SECS` (theseus-mqxk).
    pub fn spawn_with(bin: &str, args: &[String]) -> Result<Self> {
        let mut child = tokio::process::Command::new(bin)
            .arg("--stdio")
            .args(args)
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
            socket: None,
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
/// trust, a publish, the AWS bootstrap, the alerts' confirmation, the
/// ontology's writes (theseus-8kk.1: guidance steers every session in its
/// category), the answers to Jev's proposals (28b), which write them, a
/// memory label (M6 30b: `wrong` keeps a node out of every session's
/// recall), a judgment's label (M5 25c: the learning ledger grades Jev by
/// it), an extension's revoke (M7 43b: it undoes the owner's ack), the
/// owner's runs over the learning ledger (M5 25d: a replay, an audit and a
/// backfill spend money, and a backfill sends his history to Jev), and a
/// move on the ladder (M5 26a: a promotion can make Jev act, and a rollback can
/// silence security's notices), and the import, its erase and its topics
/// (theseus-0lrr.6, theseus-anh3: an import can plant memories, an erase take
/// the owner's away, and the topics write tens of thousands of memberships),
/// a correction of routing (theseus-q31l: it labels the route judgment
/// and picks the session's model), and a session's retirement and its
/// reopening (theseus-emqx: a retired session leaves the default view), and the
/// Operations page's verbs (theseus-5iqf8: a session's model hold, a task's home and its start).
pub const OPERATORS: [(&str, &str); 111] = [
    (method::GOOGLE_AUTH_BEGIN, "theseus google auth"),
    (method::GOOGLE_AUTH_FINISH, "theseus google auth"),
    (method::GOOGLE_REVOKE, "theseus google revoke"),
    (method::VOICE_AUDITION, "theseus persona voice audition"),
    (method::INITIATIVE_RATE, "theseus persona rate"),
    (method::STANDING_KEEP, "theseus standing keep"),
    (method::STANDING_DISMISS, "theseus standing dismiss"),
    (method::PERSONA_IMPORT, "theseus persona import"),
    (method::PERSONA_ADD, "theseus persona add"),
    (method::PERSONA_SOUL, "theseus persona soul"),
    (method::PERSONA_SOUL_READ, "theseus persona soul"),
    (method::PERSONA_SET, "theseus persona set"),
    (method::PERSONA_RETIRE, "theseus persona retire"),
    (method::PERSONA_DEFAULT, "theseus persona default"),
    (method::FILES_RESTORE, "theseus rewind"),
    (method::ACTION_CONFIRM, "theseus answer"),
    (method::WORK_ANSWER, "theseus answer"),
    (method::THREAD_ANSWER, "theseus thread answer"),
    (method::THREAD_MOVE, "theseus thread move"),
    (method::PROJECT_CREATE, "theseus project new"),
    (method::PROJECT_ATTACH, "theseus project attach"),
    (method::POLICY_EARNED_ANSWER, "theseus policy earned accept"),
    (method::POLICY_UNTIGHTEN, "theseus policy untighten"),
    (method::POLICY_TRUST, "theseus policy trust"),
    (method::CONTENT_UNLABEL, "theseus content remove"),
    (method::POLICY_NOTICE_SEEN, "theseus policy dismiss"),
    (method::PLACE_PUBLISH, "theseus publish"),
    (method::AWS_BOOTSTRAP, "theseus aws bootstrap"),
    (method::AWS_CONFIRM_ALERTS, "theseus aws confirm-alerts"),
    (method::ONTOLOGY_CATEGORY_ADD, "theseus ontology topic add"),
    (method::ONTOLOGY_UNDO, "theseus ontology undo"),
    (method::ONTOLOGY_PROPOSAL_NEVER, "theseus ontology never"),
    (method::ONTOLOGY_NEVER_UNDO, "theseus ontology never --undo"),
    (method::ONTOLOGY_KIND_ADD, "theseus ontology kind add"),
    (method::ONTOLOGY_KIND_RETIRE, "theseus ontology kind retire"),
    (method::ONTOLOGY_GUIDANCE_SET, "theseus ontology guide"),
    (method::ONTOLOGY_MEMBERSHIP_SET, "theseus ontology member"),
    (method::ONTOLOGY_PROPOSAL_ACCEPT, "theseus ontology accept"),
    (method::ONTOLOGY_PROPOSAL_REJECT, "theseus ontology reject"),
    (
        method::ONTOLOGY_PROPOSAL_REJECT_ALL,
        "theseus ontology reject",
    ),
    (
        method::ONTOLOGY_PROPOSAL_ACCEPT_ALL,
        "theseus ontology accept",
    ),
    (
        method::ONTOLOGY_PERSON_MERGE,
        "theseus ontology person merge",
    ),
    (method::MEMORY_LABEL, "theseus memory label"),
    (method::FACT_RETRACT, "theseus retract"),
    (method::MEMORY_CONSOLIDATE, "theseus memory consolidate"),
    (method::INDEX_REBUILD, "theseus memory index rebuild"),
    (method::JUDGE_LABEL, "theseus judge label"),
    (method::ROUTE_CORRECT, "theseus judge correct"),
    (method::EXTENSION_REVOKE, "theseus extend revoke"),
    (method::JUDGE_REPLAY, "theseus judge replay"),
    (method::JUDGE_AUDIT, "theseus judge audit"),
    (method::JUDGE_BACKFILL, "theseus judge backfill"),
    (method::JUDGE_LEARN, "theseus judge learn"),
    (method::TURN_REPLAY, "theseus model replay"),
    (method::PACK_PROMOTE, "theseus packs promote"),
    (method::PACK_ROLLBACK, "theseus packs rollback"),
    (method::IMPORT_EPISODES, "theseus import"),
    (method::IMPORT_ERASE, "theseus import erase"),
    (method::IMPORT_TOPICS, "theseus import topics"),
    (method::IMPORT_PEOPLE, "theseus import people"),
    (method::IMPORT_SYNC_PLAN, "theseus import sync"),
    (method::IMPORT_SYNC_RECORD, "theseus import sync"),
    (method::SESSION_RETIRE, "theseus sessions retire"),
    (method::SESSION_REOPEN, "theseus sessions reopen"),
    (method::EXECUTION_DISMISS, "theseus dismiss"),
    (method::TASK_RETRY, "theseus retry"),
    (method::STANDING_ADD, "theseus standing add"),
    (method::CARD_PROPOSE, "theseus card propose"),
    (
        method::CARD_ANSWER,
        "theseus card keep|edit|decline|end|scope",
    ),
    (method::CARD_DONE, "theseus card done"),
    (method::STANDING_REVOKE, "theseus standing revoke"),
    (method::STANDING_ANSWER, "theseus standing answer"),
    (method::SELF_DROP, "theseus self drop"),
    (method::SELF_MOVE, "theseus self move"),
    (method::SELF_RETRY, "theseus self retry"),
    (method::SELF_PATTERNS, "theseus self patterns"),
    (method::WATCH_CREATE, "theseus watch-for add"),
    (method::SELF_NARROW, "theseus self narrow"),
    (method::THREAD_NEW, "theseus thread new"),
    (method::THREAD_EDIT, "theseus thread edit"),
    (method::THREAD_PAUSE, "theseus thread pause"),
    (method::THREAD_RESUME, "theseus thread resume"),
    (method::THREAD_DONE, "theseus thread done"),
    (method::THREAD_REOPEN, "theseus thread reopen"),
    (method::ROUTINE_CREATE, "theseus routine add"),
    (method::ROUTINE_PAUSE, "theseus routine pause"),
    (method::ROUTINE_RESUME, "theseus routine resume"),
    (method::ROUTINE_RUN_NOW, "theseus routine run"),
    (method::ROUTINE_TRY, "theseus routine try"),
    (method::ROUTINE_UPDATE, "theseus routine update"),
    (method::ROUTINE_DELETE, "theseus routine rm"),
    (method::THREAD_IMPORT, "theseus thread import"),
    (method::THREAD_SEAL, "theseus thread seal"),
    (method::THREAD_UNSEAL, "theseus thread unseal"),
    (
        method::THREAD_SEAL_DISMISS,
        "theseus thread seal --not-needed",
    ),
    (method::THREAD_MERGE, "theseus thread merge"),
    (method::THREAD_UNMERGE, "theseus thread unmerge"),
    (method::THREAD_SPLIT, "theseus thread split"),
    (method::THREAD_SPLIT_PLAN, "theseus thread split --dry-run"),
    (method::THREAD_UNSPLIT, "theseus thread unsplit"),
    (method::THREAD_LINK, "theseus thread link"),
    (method::THREAD_UNLINK, "theseus thread unlink"),
    (method::THREAD_UNCLAIM, "theseus thread unclaim"),
    (method::TASK_START, "theseus task start"),
    (method::TASK_PAUSE, "theseus task pause"),
    (method::TASK_NOT_THIS, "theseus task not-this"),
    (method::TASK_RESUME, "theseus task resume"),
    (method::SESSION_MODE, "theseus mode"),
    (method::SESSION_PROFILE, "theseus sessions model"),
    (method::TASK_REHOME, "theseus task rehome"),
    (method::TASK_RESCHEDULE, "theseus task reschedule"),
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

/// Refuse `stop --all` inside a Theseus job (theseus-cdcwr): stopping a conversation's tasks is the operator's, as
/// `refuse_in_a_job` says of the others, and the same speed bump. A plain `stop` is not refused.
pub fn refuse_stop_all_in_a_job(session: Option<&str>) -> Result<()> {
    let Some(session) = session else {
        return Ok(());
    };
    bail!(
        "theseus stop --all refused: it is the operator's to run, and this shell is a Theseus job's \
         (THESEUS_SESSION={session} is set), so it would cancel tasks for the operator. Run it from \
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

    /// `stop --all` is refused inside a job and goes outside one (theseus-cdcwr).
    #[test]
    fn stop_all_is_refused_inside_a_job() {
        let e = refuse_stop_all_in_a_job(Some("ses_0000aa1b2c3"))
            .unwrap_err()
            .to_string();
        assert!(e.starts_with("theseus stop --all refused:"), "{e}");
        assert!(
            e.contains("(THESEUS_SESSION=ses_0000aa1b2c3 is set)"),
            "{e}"
        );
        assert!(e.ends_with("Run it from your own shell."), "{e}");
        assert!(refuse_stop_all_in_a_job(None).is_ok());
        assert!(refuse_in_a_job(method::EXECUTION_STOP, Some("ses_0000aa1b2c3")).is_ok());
    }

    /// A job's process cannot write guidance (theseus-8kk.1): guidance
    /// steers every session in its category, so the ontology's writes are
    /// the operator's, refused inside a job; its reads are not.
    #[test]
    fn a_jobs_process_cannot_write_the_ontology() {
        for m in [
            method::ONTOLOGY_GUIDANCE_SET,
            method::ONTOLOGY_CATEGORY_ADD,
            method::ONTOLOGY_UNDO,
            method::ONTOLOGY_PROPOSAL_NEVER,
            method::ONTOLOGY_NEVER_UNDO,
            method::ONTOLOGY_KIND_ADD,
            method::ONTOLOGY_KIND_RETIRE,
            method::ONTOLOGY_MEMBERSHIP_SET,
            method::ONTOLOGY_PROPOSAL_ACCEPT,
            method::ONTOLOGY_PROPOSAL_REJECT,
            method::ONTOLOGY_PROPOSAL_ACCEPT_ALL,
            method::ONTOLOGY_PROPOSAL_REJECT_ALL,
            method::ONTOLOGY_PERSON_MERGE,
        ] {
            let e = refuse_in_a_job(m, Some("ses_0000aa1b2c3")).unwrap_err();
            assert!(e.to_string().contains("theseus ontology"), "{e}");
        }
        assert!(refuse_in_a_job(method::ONTOLOGY_LIST, Some("ses_0000aa1b2c3")).is_ok());
        assert!(refuse_in_a_job(method::ONTOLOGY_PROPOSALS, Some("ses_0000aa1b2c3")).is_ok());
    }

    /// A job's process cannot hold a session to a model, move a task's report or move its
    /// start (theseus-5iqf8): each refusal names the command that makes it.
    #[test]
    fn a_jobs_process_cannot_hold_a_model_or_move_a_task() {
        for (m, command) in [
            (method::SESSION_PROFILE, "theseus sessions model"),
            (method::TASK_REHOME, "theseus task rehome"),
            (method::TASK_RESCHEDULE, "theseus task reschedule"),
        ] {
            let e = refuse_in_a_job(m, Some("ses_0000aa1b2c3")).unwrap_err();
            assert!(e.to_string().contains(command), "{e}");
        }
    }

    /// A job's process cannot write the owner's standing records
    /// (theseus-a7vi): they ride every prompt of their scope. The list is a
    /// read, and goes.
    #[test]
    fn a_jobs_process_cannot_write_standing_records() {
        for m in [
            method::STANDING_ADD,
            method::STANDING_REVOKE,
            method::STANDING_ANSWER,
        ] {
            let e = refuse_in_a_job(m, Some("ses_0000aa1b2c3")).unwrap_err();
            assert!(e.to_string().contains("theseus standing"), "{e}");
        }
        assert!(refuse_in_a_job(method::STANDING_LIST, Some("ses_0000aa1b2c3")).is_ok());
    }

    /// A job's process cannot run the card's job, answer its questions or
    /// stop it (theseus-slvl.6): what the owner's card holds is the owner's
    /// to accept. The list is a read, and goes.
    #[test]
    fn a_jobs_process_cannot_write_the_card() {
        for m in [method::CARD_PROPOSE, method::CARD_ANSWER, method::CARD_DONE] {
            let e = refuse_in_a_job(m, Some("ses_0000aa1b2c3")).unwrap_err();
            assert!(e.to_string().contains("theseus card"), "{e}");
        }
        for read in [method::CARD_LIST, method::CARD_OBSERVED] {
            assert!(refuse_in_a_job(read, Some("ses_0000aa1b2c3")).is_ok());
        }
    }

    /// A job's process cannot make or change a thread (theseus-nhew): the
    /// threads are the owner's. The reads go.
    #[test]
    fn a_jobs_process_cannot_write_threads() {
        for m in [
            method::THREAD_NEW,
            method::THREAD_EDIT,
            method::THREAD_PAUSE,
            method::THREAD_RESUME,
            method::THREAD_DONE,
            method::THREAD_REOPEN,
            method::THREAD_IMPORT,
            method::THREAD_SEAL,
            method::THREAD_UNSEAL,
            method::THREAD_SEAL_DISMISS,
            method::THREAD_MERGE,
            method::THREAD_UNMERGE,
            method::THREAD_SPLIT,
            method::THREAD_SPLIT_PLAN,
            method::THREAD_UNSPLIT,
            method::THREAD_LINK,
            method::THREAD_UNLINK,
            method::THREAD_UNCLAIM,
        ] {
            let e = refuse_in_a_job(m, Some("ses_0000aa1b2c3")).unwrap_err();
            assert!(e.to_string().contains("theseus thread"), "{e}");
        }
        assert!(refuse_in_a_job(method::THREAD_LIST, Some("ses_0000aa1b2c3")).is_ok());
        assert!(refuse_in_a_job(method::THREAD_SHOW, Some("ses_0000aa1b2c3")).is_ok());
    }

    /// A job's process cannot label a judgment (M5 25c): the learning
    /// ledger grades the judge watching it by those labels. The report is a
    /// read, and goes.
    #[test]
    fn a_jobs_process_cannot_label_a_judgment() {
        let e = refuse_in_a_job(method::JUDGE_LABEL, Some("ses_0000aa1b2c3")).unwrap_err();
        assert!(
            e.to_string().starts_with("theseus judge label refused:"),
            "{e}"
        );
        assert!(refuse_in_a_job(method::LEARNING_REPORT, Some("ses_0000aa1b2c3")).is_ok());
        assert!(refuse_in_a_job(method::JUDGE_LIST, Some("ses_0000aa1b2c3")).is_ok());
    }

    /// A job's process cannot move a pack on the ladder (M5 26a): one that
    /// rolled back security's notices could silence them. The list is a
    /// read, and goes.
    #[test]
    fn a_jobs_process_cannot_promote_or_roll_back_a_pack() {
        for (m, cmd) in [
            (method::PACK_PROMOTE, "theseus packs promote"),
            (method::PACK_ROLLBACK, "theseus packs rollback"),
        ] {
            let e = refuse_in_a_job(m, Some("ses_0000aa1b2c3")).unwrap_err();
            assert!(e.to_string().starts_with(&format!("{cmd} refused:")), "{e}");
            assert!(refuse_in_a_job(m, None).is_ok());
        }
        assert!(refuse_in_a_job(method::PACK_LIST, Some("ses_0000aa1b2c3")).is_ok());
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
