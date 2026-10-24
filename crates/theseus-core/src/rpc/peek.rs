//! A running job's output on demand and live (theseus-e1hu4): `work.peek`,
//! `action.output` and `action.unfollow`, over the job's output file
//! (`results/<id>.out`, which its wrapper writes while it runs).
//!
//! - **`work.peek`** is one read on the blocking pool: a stat and at most 64
//!   KiB from the end of the file (`toolrun::newest`), scrubbed by the
//!   session's scrubber. Nothing is timed or kept: a surface asks when
//!   someone looks.
//! - **`action.output`** starts a follower for this connection: a task that
//!   holds the file open (so the end of a job that is settled and swept
//!   still reads) and, every `TICK`, sends what is new as
//!   `action.output.chunk { id, offset, text }` to this connection alone,
//!   whole lines at a time, then `action.output.end` once the job has
//!   settled and its last bytes are sent. It never runs more than
//!   `FOLLOW_BEHIND` (4 MiB) behind the file, skipping ahead (the next
//!   chunk's offset shows it), and queues nothing for a client whose queue
//!   holds `ROOM` messages: it counts the stream as lost, so the connection
//!   hears `events.lost` naming `output:<id>` once its queue drains, and
//!   skips to the end. That client re-peeks and follows `from` the peek's
//!   `bytes`.
//! - **The place rule**: a connection that reads no private text may read
//!   only a job of a shared place's session, and is told no job exists
//!   for any other (asking is no way to learn that one does).

use std::fs::File;
use std::os::unix::fs::FileExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use theseus_kernel::Action;
use theseus_memory::recall::Place;
use theseus_protocol::job_follow::{cut_line, FOLLOW_BEHIND, PEEK_LINES, PEEK_LINE_CHARS};
use theseus_protocol::{
    error_code, method, ActionOutputChunk, ActionOutputEnd, ActionOutputParams, ActionOutputResult,
    ActionUnfollowParams, ActionUnfollowResult, Event, JobStopParams, JobStopResult, Message,
    WorkPeekParams, WorkPeekResult,
};

use super::confirms::Act;
use super::server::{parse, reply, Conn, RpcFailure};
use super::Core;
use crate::approval::Refusal;
use crate::outbound::{Note, Outbound};

/// The methods this file serves besides `work.peek`, which `rpc_work` routes.
pub(super) const OWN: &[&str] = &[method::ACTION_OUTPUT, method::ACTION_UNFOLLOW];

/// How often a follower looks at the file: a timer a follower owns, only
/// while a client follows.
const TICK: Duration = Duration::from_millis(100);
/// The most one chunk holds.
const CHUNK: u64 = 32 * 1024;
/// The chunks a follower sends in one look before it waits for the next
/// tick, so a job printing faster than it can be sent does not hold it.
const PER_TICK: usize = 16;
/// The messages a connection's queue may hold before a follower stops
/// queueing for it and counts what it skipped as lost.
const ROOM: usize = 64;

/// A job a request names: its action, and its output file.
struct Job {
    action: Action,
    path: PathBuf,
}

impl Core {
    pub(super) fn rpc_peek(
        self: Arc<Self>,
        name: &str,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        match name {
            method::ACTION_OUTPUT => reply(self.action_output(parse(params)?, conn)?),
            _ => reply(self.action_unfollow(parse(params)?, conn)),
        }
    }

    /// The job a request names, if this connection may read it.
    fn job_for(&self, id: &str, conn: &Conn<'_>) -> Result<Job, RpcFailure> {
        let none = || RpcFailure::new(error_code::NOT_FOUND, format!("no job {id}"));
        let action = self
            .kernel
            .action(id)
            .ok()
            .flatten()
            .filter(|a| a.tool == "proc.run" && a.dispatched_at_ms.is_some())
            .ok_or_else(none)?;
        if !conn.surface.reads_private()
            && !matches!(self.runner.place_of(&action.session_id), Place::Shared(_))
        {
            return Err(none());
        }
        let path = self
            .tools
            .spool
            .as_ref()
            .map(|s| s.result_path(id))
            .ok_or_else(none)?;
        Ok(Job { action, path })
    }

    /// `job.stop`: the job stopped as the model's `job_stop` stops it
    /// (`toolrun::jobs_tools::stop_job`), but only from a trusted channel
    /// (`places::owner_anywhere`, judged first, so a refusal says nothing of
    /// which jobs exist), then under `job_for`'s place rule. A job
    /// that had ended already says so. Only the verdict's words go back.
    pub(super) async fn job_stop(
        self: &Arc<Self>,
        p: JobStopParams,
        conn: Conn<'_>,
    ) -> Result<JobStopResult, RpcFailure> {
        let who = conn.answerer(p.author.clone(), p.discord.clone());
        self.judge_act(&who, Act::Stop { job: &p.id })
            .map_err(|e| match e.downcast::<Refusal>() {
                Ok(r) => RpcFailure {
                    code: error_code::REFUSED,
                    message: format!(
                        "stopping job {} from {} does not count: {}. The job runs on.",
                        p.id, r.who, r.why
                    ),
                    data: json!({"who": r.who, "via": r.via, "why": r.why}),
                },
                Err(e) => RpcFailure::invalid(e),
            })?;
        let job = self.job_for(&p.id, &conn)?;
        let by = who.label.clone();
        let s = crate::toolrun::jobs_tools::stop_job(self, &p.id, &job.action, &by)
            .await
            .map_err(|e| RpcFailure::new(error_code::INTERNAL, format!("{e:#}")))?;
        let words = match (s.already, s.ended, s.verified) {
            (true, ..) => format!("Job {} had already ended.", p.id),
            (_, true, true) => format!("Stopped job {}: nothing of it is left.", p.id),
            (_, true, false) => format!(
                "Told job {} to stop; its end was not verified, so some of it may still run.",
                p.id
            ),
            _ => format!("Told job {} to stop; it has not ended yet.", p.id),
        };
        Ok(JobStopResult {
            id: p.id,
            ended: s.ended,
            verified: s.verified,
            words,
        })
    }

    /// `work.peek`: the output's size and its last lines, scrubbed.
    pub(super) async fn work_peek(
        self: &Arc<Self>,
        p: WorkPeekParams,
        conn: Conn<'_>,
    ) -> Result<WorkPeekResult, RpcFailure> {
        let job = self.job_for(&p.id, &conn)?;
        // The board's usual time of the job's program: seeded when it is not.
        self.push.ensure(self).await?;
        let p_id = p.id.clone();
        let path = job.path.clone();
        let newest =
            tokio::task::spawn_blocking(move || crate::toolrun::newest(&path, PEEK_LINES as u64))
                .await
                .map_err(|e| RpcFailure::new(error_code::INTERNAL, e.to_string()))?
                .unwrap_or_default();
        let text = self.tools.scrubber.scrub(&newest.text).0;
        Ok(WorkPeekResult {
            id: p.id,
            bytes: newest.bytes,
            lines: text.lines().map(|l| cut_line(l, PEEK_LINE_CHARS)).collect(),
            at_ms: theseus_protocol::now_unix_ms(),
            complete: job.action.state.is_settled(),
            started_at_ms: job.action.dispatched_at_ms,
            usual_ms: self.push.job_usual_ms(&p_id),
        })
    }

    /// `action.output`: begin following a job's output from a byte.
    fn action_output(
        self: &Arc<Self>,
        p: ActionOutputParams,
        conn: Conn<'_>,
    ) -> Result<ActionOutputResult, RpcFailure> {
        let job = self.job_for(&p.id, &conn)?;
        let file = File::open(&job.path).ok();
        let size = file
            .as_ref()
            .and_then(|f| f.metadata().ok())
            .map_or(0, |m| m.len());
        let from = p.from.unwrap_or(size).min(size);
        let offset = from.max(size.saturating_sub(FOLLOW_BEHIND));
        let follower = Follower {
            core: Arc::downgrade(self),
            tx: conn.tx.clone(),
            closed: conn.closed.clone(),
            client: conn.client.to_string(),
            id: p.id.clone(),
            path: job.path,
            file,
            offset,
        };
        let h = tokio::spawn(follower.run());
        self.bus.follow(conn.client, &p.id, h.abort_handle());
        Ok(ActionOutputResult {
            id: p.id,
            offset,
            bytes: size,
            complete: job.action.state.is_settled(),
        })
    }

    /// `action.unfollow`: end this connection's follower of a job.
    fn action_unfollow(&self, p: ActionUnfollowParams, conn: Conn<'_>) -> ActionUnfollowResult {
        ActionUnfollowResult {
            was_following: self.bus.unfollow(conn.client, &p.id),
        }
    }
}

/// One connection's follower of one job.
struct Follower {
    core: std::sync::Weak<Core>,
    tx: Outbound,
    closed: tokio::sync::watch::Receiver<()>,
    client: String,
    id: String,
    path: PathBuf,
    /// The output file, held open from the first look that finds it.
    file: Option<File>,
    /// The next byte to send.
    offset: u64,
}

impl Follower {
    async fn run(mut self) {
        let stream = format!("output:{}", self.id);
        'follow: loop {
            tokio::select! {
                () = tokio::time::sleep(TICK) => {}
                _ = self.closed.changed() => break,
            }
            if self.tx.is_closed() {
                break;
            }
            let Some(core) = self.core.upgrade() else {
                break;
            };
            if self.file.is_none() {
                self.file = File::open(&self.path).ok();
            }
            match self.look(&core, &stream, false).await {
                Look::Gone => break,
                // Chunks went out and more are waiting: no need to ask whether
                // the job has ended.
                Look::More => continue,
                Look::Quiet => {}
            }
            if self.settled(&core) {
                // Its last bytes, a line without its newline included.
                loop {
                    match self.look(&core, &stream, true).await {
                        Look::Gone => break 'follow,
                        Look::More => {}
                        Look::Quiet => break,
                    }
                }
                let outcome = core
                    .kernel
                    .action(&self.id)
                    .ok()
                    .flatten()
                    .map_or("outcome_unknown", |a| a.state.as_str());
                let end = Message::from(Event::ActionOutputEnd(ActionOutputEnd {
                    id: self.id.clone(),
                    outcome: outcome.into(),
                }));
                self.tx.notify(&Note::new(&end), &stream);
                break;
            }
        }
        if let Some(core) = self.core.upgrade() {
            core.bus.followed_ended(&self.client, &self.id);
        }
    }

    /// Whether the job has settled: its output is all written.
    fn settled(&self, core: &Core) -> bool {
        core.kernel
            .action(&self.id)
            .ok()
            .flatten()
            .is_none_or(|a| a.state.is_settled())
    }

    /// Send what is new, up to `PER_TICK` chunks. `flush` sends a last line
    /// that has no newline yet.
    async fn look(&mut self, core: &Arc<Core>, stream: &str, flush: bool) -> Look {
        for _ in 0..PER_TICK {
            let Some(file) = self.file.take() else {
                return Look::Quiet;
            };
            let at = self.offset;
            let Ok((step, file)) =
                tokio::task::spawn_blocking(move || (read_step(&file, at, flush), file)).await
            else {
                return Look::Gone;
            };
            self.file = Some(file);
            let Ok(Some(step)) = step else {
                return Look::Quiet;
            };
            self.offset = step.next;
            if self.tx.backlog() >= ROOM {
                // The client does not keep up: it is told, and it re-peeks.
                if !self.tx.lose(stream) {
                    return Look::Gone;
                }
                self.offset = self
                    .file
                    .as_ref()
                    .and_then(|f| f.metadata().ok())
                    .map_or(self.offset, |m| m.len());
                return Look::Quiet;
            }
            let text = core.tools.scrubber.scrub(&step.text).0;
            let msg = Message::from(Event::ActionOutputChunk(ActionOutputChunk {
                id: self.id.clone(),
                offset: step.start,
                text,
            }));
            if !self.tx.notify(&Note::new(&msg), stream) {
                return Look::Gone;
            }
        }
        Look::More
    }
}

#[derive(PartialEq, Eq, Debug)]
enum Look {
    /// Nothing more to read now.
    Quiet,
    /// Chunks went out, and the look's allowance ran out first.
    More,
    /// The connection is gone.
    Gone,
}

/// One chunk read from the output file.
struct Step {
    /// Where its text begins in the file (past `offset` when the follower
    /// had fallen `FOLLOW_BEHIND` behind).
    start: u64,
    text: String,
    /// Where the next chunk begins.
    next: u64,
}

/// The next chunk of `file` from `offset`: whole lines, unless `flush` or a
/// line longer than a chunk; never past the newest `FOLLOW_BEHIND` bytes.
fn read_step(file: &File, offset: u64, flush: bool) -> std::io::Result<Option<Step>> {
    let len = file.metadata()?.len();
    let mut start = offset;
    if len.saturating_sub(start) > FOLLOW_BEHIND {
        start = len - FOLLOW_BEHIND;
    }
    if start >= len {
        return Ok(None);
    }
    let want = (len - start).min(CHUNK) as usize;
    let mut buf = vec![0u8; want];
    let n = file.read_at(&mut buf, start)?;
    buf.truncate(n);
    // After a skip the first byte may be the middle of a character.
    let skip = if start != offset {
        buf.iter()
            .take(3)
            .take_while(|b| **b & 0xC0 == 0x80)
            .count()
    } else {
        0
    };
    let body = &buf[skip..];
    let mut cut = if flush {
        body.len()
    } else {
        match body.iter().rposition(|b| *b == b'\n' || *b == b'\r') {
            Some(i) => i + 1,
            None if body.len() as u64 >= CHUNK => body.len(),
            None => 0,
        }
    };
    // Not through the middle of a character at the end.
    if let Err(e) = std::str::from_utf8(&body[..cut]) {
        if e.error_len().is_none() {
            cut = e.valid_up_to();
        }
    }
    if cut == 0 {
        return Ok(None);
    }
    let start = start + skip as u64;
    Ok(Some(Step {
        start,
        text: String::from_utf8_lossy(&body[..cut]).into_owned(),
        next: start + cut as u64,
    }))
}
