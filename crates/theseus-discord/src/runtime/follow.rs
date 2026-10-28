//! Follow (theseus-a5uds): one silent message in a place that mirrors a task's
//! tool lines and its stream, edited in place and sealed with its report line.
//! A child of `runtime`.
//!
//! - **`session.watch`** on the task's session, from a connection of the
//!   follow's own, so it ends with the follow and no place's events change.
//!   Private places only: a task's tool lines and words are private text.
//! - **Quiet by construction.** The message is created silent and never posted
//!   again; its edits are coalesced to the latest state, at most one in
//!   [`PLACE_EVERY`] for all of a place's follows together (so none edits more than once in two seconds), so two busy follows cost a place 25 edits a minute
//!   beside the board's 30.
//! - **At most [`MAX_FOLLOWS`] a place.** Follow is the DM's stand-in for a
//!   thread.
//! - **Sealed** when the task ends: its report line, no more edits.
//! - **A reply to the mirror joins the task** (theseus-esxyb): the mirror's
//!   message id is kept (`Follows::note_mirror`, in memory, bounded), and a
//!   message that replies to it is the owner's word to the task, under the
//!   rule of a message in a task's thread (`Place::join_task`). The mirror's
//!   footer says so; an ended task's reply goes to the conversation with the
//!   shared redirect line. A message that is not a reply is the conversation's.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use theseus_core::approval::{Client, Surface};
use theseus_core::exits::Exits;
use theseus_protocol::work::WorkView;
use theseus_protocol::{Event, SessionRef};
use tokio::time::Instant;
use twilight_model::channel::message::{AllowedMentions, MessageFlags};
use twilight_model::id::marker::ChannelMarker;
use twilight_model::id::Id;

use super::inbound::Inbound;
use super::threads::Joined;
use super::Shared;
use crate::render::board_v2::short;
use crate::render::clip;
use crate::rpc_client::RpcClient;

/// The most follows a place runs at once.
pub(super) const MAX_FOLLOWS: usize = 3;
/// All of a place's follows together edit at most this often (so one follow
/// edits no more than once in two seconds).
pub(super) const PLACE_EVERY: Duration = Duration::from_millis(2400);
/// The mirrors a reply can find.
const MIRRORS: usize = 64;
/// The tool lines kept, and the stream's tail.
const LINES: usize = 8;
const STREAM_CHARS: usize = 700;

/// The follows running, by place.
#[derive(Default)]
pub(crate) struct Follows {
    by_place: Mutex<HashMap<String, Vec<String>>>,
    last_edit: Mutex<HashMap<String, Instant>>,
    /// The mirrors made, by message, with their tasks: the newest [`MIRRORS`].
    mirrors: Mutex<VecDeque<(u64, String)>>,
}

impl Follows {
    /// Begin following `task` from `target`'s place: refused, with the
    /// reason, past [`MAX_FOLLOWS`] or for a task followed already.
    pub(super) fn begin(&self, target: &str, task: &str) -> Result<(), String> {
        let mut m = self.by_place.lock().unwrap();
        let list = m.entry(target.to_string()).or_default();
        if list.iter().any(|t| t == task) {
            return Err("You are following that task here already.".into());
        }
        if list.len() >= MAX_FOLLOWS {
            return Err(format!(
                "This place follows {MAX_FOLLOWS} tasks already: it keeps no more. Wait for one \
                 to end."
            ));
        }
        list.push(task.to_string());
        Ok(())
    }

    pub(super) fn end(&self, target: &str, task: &str) {
        let mut m = self.by_place.lock().unwrap();
        if let Some(l) = m.get_mut(target) {
            l.retain(|t| t != task);
            if l.is_empty() {
                m.remove(target);
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn count(&self, target: &str) -> usize {
        self.by_place
            .lock()
            .unwrap()
            .get(target)
            .map_or(0, Vec::len)
    }

    /// `message` is the mirror of `task`.
    pub(super) fn note_mirror(&self, message: u64, task: &str) {
        let mut m = self.mirrors.lock().unwrap_or_else(|e| e.into_inner());
        m.push_back((message, task.to_string()));
        while m.len() > MIRRORS {
            m.pop_front();
        }
    }

    /// The task whose mirror `message` is.
    pub(super) fn task_of(&self, message: u64) -> Option<String> {
        let m = self.mirrors.lock().unwrap_or_else(|e| e.into_inner());
        m.iter()
            .find(|(x, _)| *x == message)
            .map(|(_, t)| t.clone())
    }

    /// Whether a follow of `target` may edit now, taking the turn if so.
    pub(super) fn may_edit(&self, target: &str, now: Instant) -> bool {
        let mut m = self.last_edit.lock().unwrap();
        match m.get(target) {
            Some(at) if now < *at + PLACE_EVERY => false,
            _ => {
                m.insert(target.to_string(), now);
                true
            }
        }
    }
}

/// What a follow shows: the task's latest tool lines and the stream's tail.
#[derive(Default)]
pub(super) struct Mirror {
    pub(super) lines: VecDeque<String>,
    pub(super) stream: String,
    /// The task's channel, when it has one (theseus-bh3t.1).
    pub(super) channel: Option<u64>,
}

impl Mirror {
    pub(super) fn tool(&mut self, line: String) {
        self.lines.push_back(line);
        while self.lines.len() > LINES {
            self.lines.pop_front();
        }
    }

    pub(super) fn said(&mut self, text: &str) {
        self.stream.push_str(text);
        let n = self.stream.chars().count();
        if n > STREAM_CHARS {
            self.stream = self.stream.chars().skip(n - STREAM_CHARS).collect();
        }
    }

    /// The message: the head, the lines, the stream's tail; `end` seals it.
    /// The text as it is posted: the vault's values replaced. The mirror
    /// posts with `create_message` and `update_message` directly, not through
    /// the courier, and what it holds is a tool's input and the model's words.
    pub(super) fn sendable(&self, _exits: &Exits, v: &WorkView, end: Option<&str>) -> String {
        self.text(v, end)
    }

    pub(super) fn text(&self, v: &WorkView, end: Option<&str>) -> String {
        let mut t = format!(
            "{} **{}** `…{}`",
            if end.is_some() { "🏁" } else { "👁" },
            clip(v.title.trim(), 80),
            short(&v.id)
        );
        if let Some(c) = self.channel {
            t.push_str(&format!("\n-# in <#{c}>"));
        }
        for l in &self.lines {
            t.push_str(&format!("\n`{}`", clip(&l.replace('`', "'"), 100)));
        }
        let said = self.stream.trim();
        if !said.is_empty() {
            t.push_str(&format!(
                "\n> {}",
                clip(&said.replace('\n', "\n> "), STREAM_CHARS)
            ));
        }
        match end {
            Some(r) => t.push_str(&format!("\n{r}")),
            None => t.push_str("\n-# following · reply here to talk to the task"),
        }
        clip(&t, 1900)
    }
}

/// How a task ended, in a line: its state and its report.
pub(super) fn report_line(v: &WorkView) -> String {
    let word = match v.state {
        theseus_protocol::work::WorkState::Done => "✅ done",
        theseus_protocol::work::WorkState::Failed => "❌ failed",
        theseus_protocol::work::WorkState::Cancelled => "⛔ cancelled",
        _ => "⌛ ended",
    };
    let said = v
        .report
        .as_ref()
        .and_then(|r| r.first_line.as_deref().or(r.reason.as_deref()))
        .unwrap_or("");
    if said.is_empty() {
        word.to_string()
    } else {
        format!("{word}: {}", clip(said, 300))
    }
}

impl Shared {
    /// The task whose mirror a message replies to.
    pub(super) fn reply_task_of(&self, m: &twilight_model::channel::Message) -> Option<String> {
        let to = m.reference.as_ref().and_then(|r| r.message_id);
        let to = to.or(m.referenced_message.as_ref().map(|r| r.id))?;
        self.cmds.follows.task_of(to.get())
    }

    /// Follow a task from the place `target`, in `channel`: a message that
    /// mirrors it. Why not, when it cannot be.
    pub(super) fn follow(
        self: &Arc<Self>,
        target: &str,
        channel: u64,
        view: &WorkView,
    ) -> Result<(), String> {
        let class = self
            .core
            .runner
            .place_rule
            .class(&self.core.cfg, Some(target));
        if class == theseus_core::places::PlaceClass::Shared {
            return Err(
                "Follow shows a task's tool lines and words, which are private: it runs in a DM \
                 with you or a private channel."
                    .into(),
            );
        }
        let Some(session) = view.session_id.clone() else {
            return Err("That task has no session to follow.".into());
        };
        self.cmds.follows.begin(target, &view.id)?;
        let (s, target, task) = (self.clone(), target.to_string(), view.id.clone());
        tokio::spawn(async move {
            s.run_follow(&target, channel, &task, &session).await;
            s.cmds.follows.end(&target, &task);
        });
        Ok(())
    }

    async fn run_follow(self: &Arc<Self>, target: &str, channel: u64, task: &str, session: &str) {
        let (rpc, mut notes) = RpcClient::connect(
            self.core.clone(),
            Client::new("discord-follow", Surface::Discord),
        );
        let watched = rpc
            .call::<_, Value>(
                theseus_protocol::method::SESSION_WATCH,
                SessionRef {
                    session_id: session.to_string(),
                },
            )
            .await;
        if let Err(e) = watched {
            self.board.error("follow session.watch", Some(session), e);
            return;
        }
        let channel: Id<ChannelMarker> = Id::new(channel);
        let none = AllowedMentions::default();
        let mut mirror = Mirror::default();
        let mut message = None;
        let mut sent = String::new();
        let mut dirty = true;
        let mut tick = tokio::time::interval(Duration::from_millis(500));
        let stopped = self.core.outbox.stopped();
        tokio::pin!(stopped);
        loop {
            tokio::select! {
                n = notes.recv() => {
                    let Some(n) = n else { break };
                    if let Ok(Some(e)) = Event::from_notification(&n.method, &n.params) {
                        dirty |= absorb(&mut mirror, &e);
                    }
                }
                _ = tick.tick() => {}
                () = &mut stopped => return,
            }
            let Some(view) = self.tasks.view(task) else {
                break;
            };
            let ended = view.state.ended();
            mirror.channel = self.task_threads.channel_of(task);
            let text = mirror.sendable(
                &self.core.exits,
                &view,
                ended.then(|| report_line(&view)).as_deref(),
            );
            let due = ended
                || (dirty
                    && (message.is_none() || self.cmds.follows.may_edit(target, Instant::now())));
            if due && text != sent {
                let r = match message {
                    None => {
                        self.http
                            .create_message(channel)
                            .content(&text)
                            .flags(MessageFlags::SUPPRESS_NOTIFICATIONS)
                            .allowed_mentions(Some(&none))
                            .await
                    }
                    Some(m) => {
                        self.http
                            .update_message(channel, m)
                            .content(Some(&text))
                            .allowed_mentions(Some(&none))
                            .await
                    }
                };
                match r {
                    Ok(resp) => {
                        if message.is_none() {
                            message = resp.model().await.ok().map(|m| m.id);
                            if let Some(m) = message {
                                self.cmds.follows.note_mirror(m.get(), task);
                                self.add_controls(channel.get(), m.get(), task.to_string());
                            }
                            self.cmds.follows.may_edit(target, Instant::now());
                        }
                        sent = text;
                        dirty = false;
                    }
                    Err(e) => {
                        self.board.error("follow message", None, e);
                        dirty = false;
                    }
                }
            }
            if ended {
                break;
            }
        }
    }
}

impl super::Place {
    /// Whether `m` replies to the mirror of a task that has a session.
    pub(super) fn mirrors_a_task(&self, m: &Inbound) -> bool {
        m.task
            .as_deref()
            .and_then(|t| self.shared.tasks.view(t))
            .is_some_and(|v| v.session_id.is_some())
    }

    /// A reply to a task's mirror: the owner's word to the task, which must
    /// still have a session (the arm that calls this checks).
    pub(super) fn task_reply(&mut self, m: Inbound) {
        let Some(v) = m.task.as_deref().and_then(|t| self.shared.tasks.view(t)) else {
            return;
        };
        let to = Joined {
            session: v.session_id.clone().unwrap_or_default(),
            title: clip(v.title.trim(), 80),
            task: v.id,
            parent_session: self.session_id.clone(),
        };
        self.join_task(m, to, "task_reply");
    }
}

/// An event's mark on the mirror; whether it changed what is shown.
pub(super) fn absorb(m: &mut Mirror, e: &Event) -> bool {
    match e {
        Event::ToolProposed(t) => {
            m.tool(format!(
                "{} {}",
                t.tool,
                theseus_protocol::term::words::argument(&t.tool, &t.input)
            ));
            true
        }
        Event::ModelDelta(d) => {
            m.said(&d.text);
            true
        }
        Event::ModelAnswered(d) => {
            m.stream.clear();
            m.said(&d.text);
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_place_follows_three_and_no_more_and_none_twice() {
        let f = Follows::default();
        for t in ["a", "b", "c"] {
            assert!(f.begin("discord:dm:1", t).is_ok());
        }
        assert!(f
            .begin("discord:dm:1", "d")
            .unwrap_err()
            .contains("3 tasks"));
        assert!(f
            .begin("discord:dm:1", "a")
            .unwrap_err()
            .contains("already"));
        assert!(
            f.begin("discord:dm:2", "d").is_ok(),
            "another place is its own"
        );
        f.end("discord:dm:1", "b");
        assert_eq!(f.count("discord:dm:1"), 2);
        assert!(f.begin("discord:dm:1", "d").is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn two_busy_follows_cost_a_place_25_edits_a_minute() {
        let f = Follows::default();
        let mut edits = 0;
        let end = Instant::now() + Duration::from_secs(60);
        while Instant::now() < end {
            // Both follows always have something new, and ask every half second.
            for _ in 0..2 {
                if f.may_edit("discord:dm:1", Instant::now()) {
                    edits += 1;
                }
            }
            tokio::time::advance(Duration::from_millis(500)).await;
        }
        assert!(edits <= 25, "{edits} edits a minute");
        assert!(edits >= 20, "{edits}: the gate still lets them through");
    }

    #[test]
    fn the_mirror_keeps_the_last_lines_and_the_streams_tail() {
        let mut m = Mirror::default();
        for n in 0..12 {
            m.tool(format!("fs.read file{n}"));
        }
        assert_eq!(m.lines.len(), LINES);
        assert_eq!(m.lines.front().unwrap(), "fs.read file4");
        m.said(&"x".repeat(2000));
        assert_eq!(m.stream.chars().count(), STREAM_CHARS);
    }

    /// The mirror posts what the model said and the tools it called, outside
    /// the courier: a vault value in either leaves as its name.
    #[test]
    fn the_mirror_posts_the_vaults_values_replaced() {
        use theseus_core::secrets::{Secret, SecretBoard};
        const VALUE: &str = "vault-token-4c8e1f7a92b3";
        let board = SecretBoard::new(["courier_token".to_string()], std::time::Instant::now());
        board.publish(
            [("courier_token".to_string(), Ok(Secret::new(VALUE.into())))].into(),
            "fake",
        );
        let exits = Exits::new(board, true);
        let mut m = Mirror::default();
        m.tool(format!("proc.run echo {VALUE}"));
        m.said(&format!("the token is {VALUE}"));
        let v =
            crate::runtime::task_channels::work_view("exe_01J0000000000000000a1b2c3", "running");
        let text = m.sendable(&exits, &v, None);
        assert!(!text.contains(VALUE), "{text}");
        assert!(
            text.contains("proc.run echo [redacted:courier_token]"),
            "{text}"
        );
        assert!(
            text.contains("the token is [redacted:courier_token]"),
            "{text}"
        );
    }

    #[test]
    fn a_task_with_a_channel_says_so_under_its_head() {
        let mut m = Mirror::default();
        let v =
            crate::runtime::task_channels::work_view("exe_01J0000000000000000a1b2c3", "running");
        assert!(!m.text(&v, None).contains("-# in <#"));
        m.channel = Some(900_000_000_000_500_002);
        let text = m.text(&v, None);
        let (head, rest) = text.split_once('\n').unwrap();
        assert!(head.contains("a1b2c3"), "{head}");
        assert!(rest.starts_with("-# in <#900000000000500002>"), "{text}");
    }
}
