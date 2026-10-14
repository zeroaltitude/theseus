//! The follow pane (theseus-e1hu4): `l` on a session row opens a running job's
//! output in the detail area, `esc` closes it. The state is pure: `App` sends
//! the calls (`action.list` to find the session's running job, `work.peek`
//! for its last lines and size, `action.output` from there) and feeds the
//! answers and the notifications (`action.output.chunk`, `.end`) in. A
//! stream that fell behind (`events.lost` naming the job) is read again by a
//! new peek; a daemon that restarts is too, from `connected`.

use serde_json::{json, Value};
use theseus_client::render::Tag;
use theseus_protocol::job_follow::job_words;
use theseus_protocol::{
    method, ActionListResult, ActionOutputChunk, Event, EventsLost, RpcError, WorkPeekResult,
};

use crate::app::{App, Call, Effect, Purpose};

/// The most lines the pane keeps; the oldest go first.
const KEPT: usize = 1_000;

#[derive(Debug, Default, PartialEq)]
pub struct Follow {
    /// The session whose job it is, and its execution.
    pub session_id: String,
    pub execution_id: String,
    /// The job (`act_…`), once found.
    pub job: Option<String>,
    lines: Vec<String>,
    /// The line being printed: it ends at the next newline.
    partial: String,
    /// The next byte of the output expected.
    next: u64,
    started_at_ms: Option<u64>,
    usual_ms: Option<u64>,
    /// How the job ended, once it has.
    pub ended: Option<String>,
    /// The TUI's own words above the output: it fell behind and read again.
    pub note: Option<String>,
}

impl Follow {
    pub fn new(session_id: &str, execution_id: &str) -> Self {
        Self {
            session_id: session_id.into(),
            execution_id: execution_id.into(),
            ..Self::default()
        }
    }

    /// The pane's rows: what the job printed, its open line last.
    pub fn lines(&self) -> Vec<&str> {
        self.lines
            .iter()
            .map(String::as_str)
            .chain((!self.partial.is_empty()).then_some(self.partial.as_str()))
            .collect()
    }

    /// A peek: its lines replace what the pane held, and the stream goes on
    /// from its end.
    pub fn peeked(&mut self, p: &WorkPeekResult) {
        self.lines = p.lines.clone();
        self.partial.clear();
        self.next = p.bytes;
        self.started_at_ms = p.started_at_ms;
        self.usual_ms = p.usual_ms;
        if p.complete && self.ended.is_none() {
            self.ended = Some("ended".into());
        }
    }

    /// A chunk of the job's output.
    pub fn chunk(&mut self, c: &ActionOutputChunk) {
        self.note = None;
        if c.offset > self.next {
            // What was being printed ended where the gap began.
            if !self.partial.is_empty() {
                let line = std::mem::take(&mut self.partial);
                self.push(line);
            }
            self.push(format!("-- {} bytes skipped --", c.offset - self.next));
        }
        self.next = c.offset + c.text.len() as u64;
        for ch in c.text.chars() {
            match ch {
                '\n' => {
                    let line = std::mem::take(&mut self.partial);
                    self.push(line);
                }
                // A progress bar rewrites its line.
                '\r' => self.partial.clear(),
                c if c.is_control() => {}
                c => self.partial.push(c),
            }
        }
    }

    fn push(&mut self, line: String) {
        self.lines.push(line);
        if self.lines.len() > KEPT {
            self.lines.remove(0);
        }
    }

    /// The pane's header: the job's time against its usual, or how it ended.
    pub fn header(&self, now_ms: u64, name: &str) -> String {
        let state = match (&self.ended, self.started_at_ms) {
            (Some(o), _) => format!("job {o}"),
            (None, Some(s)) => job_words(now_ms.saturating_sub(s), self.usual_ms),
            (None, None) => "job".to_string(),
        };
        match &self.note {
            Some(n) => format!("{state} · {n} · {name} · esc closes"),
            None => format!("{state} · {name} · esc closes"),
        }
    }
}

/// A peek of the job: its lines and size, then (while it runs) the stream
/// from there.
fn peek(id: &str) -> Effect {
    call(method::WORK_PEEK, json!({ "id": id }), Purpose::FollowPeek)
}

fn call(method: &'static str, params: Value, purpose: Purpose) -> Effect {
    Effect::Call(Call {
        method,
        params,
        purpose,
    })
}

impl App {
    /// `l`: follow the running job of the session under the cursor (else the
    /// one in focus): find it, peek, then follow from the peek's end.
    pub(crate) fn follow_key(&mut self) -> Vec<Effect> {
        let rows = self.rows();
        let sid = match self.cursor(&rows).and_then(|i| rows.get(i)) {
            Some(r) => Some(r.session_id.clone()),
            None => self.shown().map(str::to_string),
        };
        let Some(sid) = sid else {
            self.flash = Some((Tag::Dim, "no session under the cursor".to_string()));
            return Vec::new();
        };
        let Some(exe) = self.board.view(&sid).map(|v| v.execution_id.clone()) else {
            self.flash = Some((Tag::Dim, "that session has no execution".to_string()));
            return Vec::new();
        };
        let mut out = Vec::new();
        // One pane at a time: the job before it is let go.
        out.extend(self.follow_close());
        self.follow = Some(Follow::new(&sid, &exe));
        out.push(call(
            method::ACTION_LIST,
            json!({ "execution_id": exe, "unsettled": true }),
            Purpose::FollowFind,
        ));
        out
    }

    /// `esc` on the pane: close it, and tell the daemon to stop sending.
    pub(crate) fn follow_close(&mut self) -> Vec<Effect> {
        match self.follow.take().and_then(|f| f.job) {
            Some(id) => vec![call(
                method::ACTION_UNFOLLOW,
                json!({ "id": id }),
                Purpose::Unfollow,
            )],
            None => Vec::new(),
        }
    }

    /// An answer to the pane's requests.
    pub(crate) fn follow_answered(&mut self, purpose: &Purpose, v: Value) -> Vec<Effect> {
        match purpose {
            Purpose::FollowFind => {
                let Some(f) = self.follow.as_mut() else {
                    return Vec::new();
                };
                let found = serde_json::from_value::<ActionListResult>(v)
                    .ok()
                    .and_then(|r| {
                        r.actions
                            .into_iter()
                            .filter(|a| a.tool == "proc.run" && a.dispatched_at_ms.is_some())
                            .max_by_key(|a| a.dispatched_at_ms)
                    });
                match found {
                    Some(a) => {
                        f.job = Some(a.correlation_id.clone());
                        vec![peek(&a.correlation_id)]
                    }
                    None => {
                        let name = self.board.name(&f.session_id);
                        self.follow = None;
                        self.flash = Some((Tag::Dim, format!("no job is running in {name}")));
                        Vec::new()
                    }
                }
            }
            Purpose::FollowPeek => {
                let (Some(f), Ok(p)) = (
                    self.follow.as_mut(),
                    serde_json::from_value::<WorkPeekResult>(v),
                ) else {
                    return Vec::new();
                };
                f.peeked(&p);
                if p.complete {
                    return Vec::new();
                }
                vec![call(
                    method::ACTION_OUTPUT,
                    json!({ "id": p.id, "from": p.bytes }),
                    Purpose::FollowStart,
                )]
            }
            _ => Vec::new(),
        }
    }

    /// A request of the pane failed: say why, and close it.
    pub(crate) fn follow_failed(&mut self, what: &str, e: &RpcError) {
        self.follow = None;
        self.flash = Some((Tag::Bad, format!("{what}: {}", e.message)));
    }

    /// The pane's notifications: a followed job's chunks and end; a stream
    /// that fell behind is peeked again.
    pub(crate) fn follow_event(&mut self, e: &Event) -> Vec<Effect> {
        let Some(f) = self.follow.as_mut() else {
            return Vec::new();
        };
        match e {
            Event::ActionOutputChunk(c) if f.job.as_deref() == Some(c.id.as_str()) => f.chunk(c),
            Event::ActionOutputEnd(end) if f.job.as_deref() == Some(end.id.as_str()) => {
                f.ended = Some(end.outcome.clone());
            }
            _ => {}
        }
        Vec::new()
    }

    /// The connection fell behind: the pane's stream, if named (or all), is
    /// read again.
    pub(crate) fn follow_lost(&mut self, lost: &EventsLost) -> Vec<Effect> {
        let Some(f) = self.follow.as_mut() else {
            return Vec::new();
        };
        let Some(id) = f.job.clone() else {
            return Vec::new();
        };
        if !lost.streams.is_empty() && !lost.streams.contains(&format!("output:{id}")) {
            return Vec::new();
        }
        f.note = Some("fell behind; read again".into());
        vec![peek(&id)]
    }

    /// The daemon came back: its follow was the old connection's.
    pub(crate) fn follow_reconnected(&mut self) -> Vec<Effect> {
        match self.follow.as_ref().and_then(|f| f.job.clone()) {
            Some(id) => vec![peek(&id)],
            None => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peek(lines: &[&str], bytes: u64) -> WorkPeekResult {
        WorkPeekResult {
            id: "act_k5".into(),
            bytes,
            lines: lines.iter().map(|l| l.to_string()).collect(),
            at_ms: 6_000,
            complete: false,
            started_at_ms: Some(1_000),
            usual_ms: Some(31_000),
        }
    }

    fn chunk(offset: u64, text: &str) -> ActionOutputChunk {
        ActionOutputChunk {
            id: "act_k5".into(),
            offset,
            text: text.into(),
        }
    }

    /// The peek, then each chunk in order; a line arrives whole, a bar
    /// rewrites its line, a gap says what it skipped.
    #[test]
    fn the_pane_holds_the_peek_then_the_chunks_in_order() {
        let mut f = Follow::new("ses_a", "exe_a");
        f.peeked(&peek(&["leg 1", "leg 2"], 12));
        f.chunk(&chunk(12, "leg 3\nle"));
        f.chunk(&chunk(20, "g 4\n"));
        assert_eq!(f.lines(), ["leg 1", "leg 2", "leg 3", "leg 4"]);
        f.chunk(&chunk(24, "bar 10%\rbar 90%"));
        assert_eq!(f.lines().last(), Some(&"bar 90%"));
        f.chunk(&chunk(1_000, "x\n"));
        assert_eq!(
            f.lines()[f.lines().len() - 3..],
            ["bar 90%", "-- 961 bytes skipped --", "x"]
        );
        assert_eq!(
            f.header(6_000, "Read the buoys"),
            "job 0:05 of usual 0:31 · Read the buoys · esc closes"
        );
        f.ended = Some("succeeded".into());
        assert!(f.header(9_000, "x").starts_with("job succeeded"));
    }

    /// The pane keeps a bounded tail.
    #[test]
    fn the_pane_keeps_the_newest_lines() {
        let mut f = Follow::new("ses_a", "exe_a");
        let text: String = (0..KEPT + 50).map(|i| format!("l{i}\n")).collect();
        f.chunk(&chunk(0, &text));
        assert_eq!(f.lines().len(), KEPT);
        assert_eq!(f.lines().last().copied(), Some("l1049"));
    }
}
