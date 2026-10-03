//! A run's steps: what the operator, the people, and the binding do.

use std::collections::VecDeque;

use anyhow::{anyhow, Result};
use rand::seq::IndexedRandom;
use rand::Rng;
use theseus_core::approval::{Answerer, Surface};
use theseus_core::node::{Body, Node};
use theseus_core::peer::Peer;
use theseus_core::session::SessionRecord;
use theseus_core::turn::TurnRequest;
use theseus_protocol::SessionKind;

use super::atoms::{scan, Origin, R};
use super::world::{Act, OWNER};
use super::{push_all, HeldPost, PlaceKind, Sess, Sim};

/// A session runs this many operator turns before the place moves to a new
/// one (`/new`), which keeps every request short.
const TURNS_PER_SESSION: u32 = 10;
/// The binding reads a channel before a turn there when its last read is
/// this many steps old ("a minute").
const STALE_READ: u32 = 6;

/// One kind of step, with its weight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Turn,
    Viewers,
    Readable,
    Graduate,
    Answer,
    Trust,
    Foreign,
    New,
    Read,
}

const KINDS: [(Kind, u32); 9] = [
    (Kind::Turn, 45),
    (Kind::Viewers, 14),
    (Kind::Readable, 2),
    (Kind::Graduate, 8),
    (Kind::Answer, 8),
    (Kind::Trust, 5),
    (Kind::Foreign, 8),
    (Kind::New, 3),
    (Kind::Read, 7),
];

/// What the operator types, and through what.
pub(crate) struct Input {
    text: String,
    author: String,
    from_discord: bool,
    attachments: Vec<theseus_protocol::Attachment>,
}

/// The operator, answering through the CLI.
pub(crate) fn operator() -> Answerer {
    Answerer {
        label: "sim-operator".into(),
        surface: Surface::Cli,
        discord: None,
        peer: Peer::None,
    }
}

impl Sim {
    pub(crate) async fn one_step(&mut self) -> Result<()> {
        let kind = {
            let mut s = self.lock();
            let total: u32 = KINDS.iter().map(|(_, w)| w).sum();
            let mut roll = s.rng.random_range(0..total);
            KINDS
                .iter()
                .find(|(_, w)| {
                    if roll < *w {
                        true
                    } else {
                        roll -= w;
                        false
                    }
                })
                .map_or(Kind::Turn, |(k, _)| *k)
        };
        match kind {
            Kind::Turn => self.operator_turn().await,
            Kind::Viewers => {
                self.viewers();
                Ok(())
            }
            Kind::Readable => {
                self.readable();
                Ok(())
            }
            Kind::Graduate => self.graduate(),
            Kind::Answer => self.answer_held(false),
            Kind::Trust => self.trust(),
            Kind::Foreign => self.foreign(),
            Kind::New => self.new_session(),
            Kind::Read => {
                self.read_channel();
                Ok(())
            }
        }
    }

    pub(crate) fn open_session(&mut self, kind: PlaceKind) -> Result<usize> {
        let rec = SessionRecord::new(SessionKind::Conversation, None);
        self.core.store.put_session(&rec.session_id, &rec)?;
        if let Some(p) = kind.bind() {
            self.core.outbox.bind_place(&p, &rec.session_id)?;
        }
        let generation = self
            .sessions
            .iter()
            .filter(|s| s.place == Some(kind))
            .count()
            + 1;
        let name = match kind {
            PlaceKind::Cli => format!("cli/{generation}"),
            PlaceKind::Dm(u) => format!("dm:{}/{generation}", self.name_of(u)),
            PlaceKind::Channel(c) => format!("#{}/{generation}", self.channel_name(c)),
        };
        let target = self.core.outbox.target(&rec.session_id);
        Ok(self.register(Sess {
            id: rec.session_id,
            name,
            place: Some(kind),
            parent: None,
            short: None,
            target,
            live: true,
            turns: 0,
            latched: None,
            read_to: 0,
        }))
    }

    pub(crate) fn register(&mut self, s: Sess) -> usize {
        let i = self.sessions.len();
        self.by_id.insert(s.id.clone(), i);
        self.sessions.push(s);
        self.dirty.insert(i);
        i
    }

    pub(crate) fn name_of(&self, person: u64) -> String {
        self.lock()
            .names
            .get(&person)
            .cloned()
            .unwrap_or_else(|| person.to_string())
    }

    pub(crate) fn channel_name(&self, channel: u64) -> String {
        self.lock()
            .channel(channel)
            .map_or_else(|| channel.to_string(), |c| c.name.clone())
    }

    /// An operator's message in a place, and the turn it starts.
    async fn operator_turn(&mut self) -> Result<()> {
        let place = {
            let mut s = self.lock();
            let roll = s.rng.random_range(0..100u32);
            let want = |k: &PlaceKind| match k {
                PlaceKind::Channel(_) => roll < 50,
                PlaceKind::Dm(_) => (50..80).contains(&roll),
                PlaceKind::Cli => roll >= 80,
            };
            let fit: Vec<usize> = (0..self.places.len())
                .filter(|i| want(&self.places[*i].0))
                .collect();
            *fit.choose(&mut s.rng).unwrap_or(&0)
        };
        if self.sessions[self.places[place].1].turns >= TURNS_PER_SESSION {
            self.renew(place)?;
        }
        let (kind, sess) = self.places[place];
        if let PlaceKind::Channel(c) = kind {
            self.read_before_turn(c);
        }
        let input = self.input(kind);
        let plan = self.plan(false);
        self.note(&format!(
            "turn in {}: {:?}, {}",
            self.sessions[sess].name, plan, input.text
        ));
        self.sessions[sess].turns += 1;
        self.rep.turns += 1;
        self.lock()
            .plans
            .insert(self.sessions[sess].id.clone(), plan);
        self.run_turn(sess, Some(input)).await
    }

    /// The binding reads a channel before a turn there when its last read is
    /// a minute old.
    fn read_before_turn(&mut self, channel: u64) {
        let push = {
            let mut s = self.lock();
            let stale = s
                .channel(channel)
                .is_some_and(|c| s.step.saturating_sub(c.pushed_at) >= STALE_READ);
            if stale {
                s.read_one(channel)
            } else {
                None
            }
        };
        if let Some(p) = push {
            push_all(&self.core, &[p]);
        }
    }

    /// What the operator types: an atom read by whoever its surface says
    /// (§2.5: the CLI's words are the owner's; through Discord, a DM's
    /// person's or a channel's), and sometimes files with it, each an atom
    /// read as the message is.
    fn input(&mut self, kind: PlaceKind) -> Input {
        let mut s = self.lock();
        let cli = matches!(kind, PlaceKind::Cli) || s.rng.random_bool(0.2);
        let (readers, origin, author) = match kind {
            _ if cli => (R::Owner, Origin::Cli, "cli".to_string()),
            PlaceKind::Dm(u) => (R::People([u].into()), Origin::Dm(u), format!("discord:{u}")),
            PlaceKind::Channel(c) => (R::Place(c), Origin::Channel(c), format!("discord:{OWNER}")),
            PlaceKind::Cli => unreachable!("the CLI's surface is the CLI"),
        };
        let (_, m) = s.atoms.mint(readers.clone(), origin.clone());
        let mut attachments = Vec::new();
        if s.rng.random_bool(0.3) {
            for k in 0..s.rng.random_range(1..=2) {
                let (_, a) = s.atoms.mint(readers.clone(), origin.clone());
                let text = format!("Attached notes {k}: {a}\n");
                attachments.push(theseus_protocol::Attachment {
                    name: format!("notes-{k}.txt"),
                    media_type: "text/plain".into(),
                    size: text.len() as u64,
                    text: Some(text),
                    data: None,
                    not_read: None,
                });
            }
        }
        Input {
            text: format!("{m} Please look into this."),
            author,
            from_discord: !cli,
            attachments,
        }
    }

    /// What the model does this turn: up to three calls, then its answer. A
    /// task starts at most one task, and a task none.
    pub(crate) fn plan(&mut self, in_task: bool) -> VecDeque<Act> {
        let mut s = self.lock();
        let calls = [0, 0, 0, 1, 1, 1, 1, 2, 2, 3]
            .choose(&mut s.rng)
            .copied()
            .unwrap_or(0);
        let mut plan = VecDeque::new();
        let mut tasked = in_task;
        for _ in 0..calls {
            let roll = s.rng.random_range(0..100u32);
            let act = match roll {
                0..30 => Act::ReadPrivate(s.rng.random_range(0..4)),
                30..42 => Act::ReadOpen(s.rng.random_range(0..3)),
                42..60 => Act::Fetch,
                60..70 => Act::RunEgress,
                70..80 => Act::RunLocal,
                _ if !tasked => {
                    tasked = true;
                    Act::Task {
                        wake: s.rng.random_bool(0.5),
                    }
                }
                _ => Act::ReadPrivate(s.rng.random_range(0..4)),
            };
            plan.push_back(act);
        }
        plan
    }

    /// Run one turn of `sess`: the operator's message, or a continuation the
    /// driver takes; watch its events, and check what it compiled and
    /// streamed.
    pub(crate) async fn run_turn(&mut self, sess: usize, input: Option<Input>) -> Result<()> {
        let sid = self.sessions[sess].id.clone();
        let views = self.lock().pushed_views();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        self.core.bus.watch(&sid, "sim", tx);
        let channel = self.sessions[sess]
            .target
            .as_deref()
            .and_then(|t| t.strip_prefix("discord:channel:"))
            .and_then(|c| c.parse().ok());
        {
            let mut s = self.lock();
            s.current = Some(sid.clone());
            s.turn_channel = channel;
        }
        let t0 = std::time::Instant::now();
        let out = match input {
            Some(i) => self.submit(&sid, i).await,
            None => self.continue_turn(&sid).await,
        };
        self.spent("core turn", t0);
        {
            let mut s = self.lock();
            s.current = None;
            s.turn_channel = None;
            s.plans.remove(&sid);
        }
        self.core.bus.unwatch(&sid, "sim");
        let mut events = Vec::new();
        while let Ok(m) = rx.try_recv() {
            events.push(m);
        }
        out?;
        self.dirty.insert(sess);
        let t0 = std::time::Instant::now();
        let checked = self.after_turn(sess, &views, &events);
        self.spent("turn checks", t0);
        checked
    }

    async fn submit(&self, sid: &str, i: Input) -> Result<()> {
        let rec: SessionRecord = self
            .core
            .store
            .get_session(sid)?
            .ok_or_else(|| anyhow!("no session {sid}"))?;
        let (live, _) = self.core.live_profile();
        let target = self.core.runner.resolve_target(&live, None, None, None)?;
        let sink = theseus_core::bus::EventSink::new(self.core.bus.clone(), sid, None);
        self.core
            .runner
            .run(TurnRequest {
                session: rec,
                input: Some(i.text),
                target,
                sink,
                author: i.author,
                recompile: None,
                attachments: i.attachments,
                arrived: None,
                config_wait_us: 0,
                reply_to: None,
                from_discord: i.from_discord,
            })
            .await?;
        Ok(())
    }

    async fn continue_turn(&self, sid: &str) -> Result<()> {
        let rec: SessionRecord = self
            .core
            .store
            .get_session(sid)?
            .ok_or_else(|| anyhow!("no session {sid}"))?;
        let exec = rec
            .execution_id
            .ok_or_else(|| anyhow!("session {sid} has no execution to continue"))?;
        self.core.continue_execution(&exec).await?;
        Ok(())
    }

    /// The harness driver, one continuation at a time: each execution that is
    /// queued with a result or a resume waiting (a task's turn, a parent its
    /// task's report woke), in the order the sim learned of its session.
    pub(crate) async fn drive(&mut self) -> Result<()> {
        for _ in 0..64 {
            let Some(sess) = self.runnable()? else {
                return Ok(());
            };
            self.rep.continuations += 1;
            let in_task = self.sessions[sess].parent.is_some();
            let plan = if in_task && self.sessions[sess].turns == 0 {
                self.plan(true)
            } else {
                VecDeque::new()
            };
            self.sessions[sess].turns += 1;
            self.note(&format!("continue {}: {plan:?}", self.sessions[sess].name));
            self.lock()
                .plans
                .insert(self.sessions[sess].id.clone(), plan);
            self.run_turn(sess, None).await?;
        }
        Err(anyhow!(
            "the driver found work after 64 continuations in one step"
        ))
    }

    fn runnable(&mut self) -> Result<Option<usize>> {
        self.discover_tasks()?;
        // The queued executions, by the kernel's state term: a read that
        // stays small however long the run.
        let queued = theseus_kernel::terms::one(&theseus_kernel::terms::state(
            theseus_kernel::ExecState::Queued,
        ));
        let mut ready: Vec<usize> = self
            .core
            .kernel
            .executions_by(&[queued])?
            .into_iter()
            .filter(|e| {
                e.state == theseus_kernel::ExecState::Queued
                    && (e.resume_pending || !e.queued_results.is_empty())
                    && !self.core.kernel.is_held(&e.id)
            })
            .filter_map(|e| self.by_id.get(&e.session_id).copied())
            .collect();
        ready.sort_unstable();
        Ok(ready.first().copied())
    }

    /// Tasks the sessions written this step started: each is a session the
    /// sim learns of, in the order of its brief in the WAL.
    fn discover_tasks(&mut self) -> Result<()> {
        let parents: Vec<usize> = self
            .dirty
            .iter()
            .copied()
            .filter(|i| self.sessions[*i].parent.is_none())
            .collect();
        for parent in parents {
            let rec: Option<SessionRecord> =
                self.core.store.get_session(&self.sessions[parent].id)?;
            let Some(exec) = rec.and_then(|r| r.execution_id) else {
                continue;
            };
            let mut found: Vec<(u64, String, Option<String>)> = Vec::new();
            for t in self.core.kernel.tasks(Some(&exec))? {
                if self.by_id.contains_key(&t.session_id) {
                    continue;
                }
                let first = self
                    .core
                    .store
                    .session_nodes(&t.session_id)?
                    .first()
                    .map_or(0, |(pos, _)| *pos);
                let target = self
                    .core
                    .store
                    .get_session::<SessionRecord>(&t.session_id)?
                    .and_then(|r| r.task)
                    .and_then(|t| t.target);
                found.push((first, t.session_id.clone(), target));
            }
            found.sort();
            for (_, sid, target) in found {
                let n = self.rep.tasks + 1;
                self.rep.tasks = n;
                let name = format!("task {n} of {}", self.sessions[parent].name);
                self.register(Sess {
                    short: Some(theseus_core::task::short(&sid)),
                    id: sid,
                    name,
                    place: None,
                    parent: Some(parent),
                    target,
                    live: true,
                    turns: 0,
                    latched: None,
                    read_to: 0,
                });
            }
        }
        Ok(())
    }

    /// Someone joins or leaves a channel. The binding hears of it (a role or
    /// an overwrite: it reads every channel) half the time; a member's join
    /// or leave it hears of only at its next read.
    fn viewers(&mut self) {
        let (what, pushes) = {
            let mut s = self.lock();
            let n = s.channels.len();
            let i = s.rng.random_range(0..n);
            let (who, joined) = s.change_viewers(i);
            let heard = s.rng.random_bool(0.5);
            let what = format!(
                "{} {} #{} ({})",
                who,
                if joined { "joins" } else { "leaves" },
                s.channels[i].name,
                if heard { "heard" } else { "unheard" }
            );
            (what, heard.then(|| s.read_all()))
        };
        self.rep.viewer_changes += 1;
        match pushes {
            Some(p) => push_all(&self.core, &p),
            None => self.rep.unheard_changes += 1,
        }
        self.note(&what);
    }

    /// The bot's reach over a channel's members changes (the Server Members
    /// intent, a permission): from the next read on, who views it can or
    /// cannot be read.
    fn readable(&mut self) {
        let what = {
            let mut s = self.lock();
            let n = s.channels.len();
            let i = s.rng.random_range(0..n);
            s.channels[i].readable = !s.channels[i].readable;
            format!(
                "#{} readable: {}",
                s.channels[i].name, s.channels[i].readable
            )
        };
        self.note(&what);
    }

    /// The binding reads one channel (its minute-old rule, elsewhere).
    fn read_channel(&mut self) {
        let push = {
            let mut s = self.lock();
            let n = s.channels.len();
            let i = s.rng.random_range(0..n);
            let id = s.channels[i].id;
            s.read_one(id)
        };
        if let Some(p) = push {
            self.note(&format!("read #{}", p.name));
            push_all(&self.core, &[p]);
        }
    }

    /// `/new` in a place: its next turn runs in a new session.
    fn new_session(&mut self) -> Result<()> {
        let i = self.lock().rng.random_range(0..self.places.len());
        self.renew(i)
    }

    fn renew(&mut self, place: usize) -> Result<()> {
        let (kind, old) = self.places[place];
        self.sessions[old].live = false;
        let sess = self.open_session(kind)?;
        self.places[place].1 = sess;
        self.note(&format!("new {}", self.sessions[sess].name));
        Ok(())
    }

    /// The operator graduates a node its session's audience may not read: to
    /// the place, to the public, or to the audience's people. Each atom the
    /// node carries may then be read by the new readers too.
    fn graduate(&mut self) -> Result<()> {
        let live: Vec<usize> = self
            .places
            .iter()
            .map(|(_, s)| *s)
            .filter(|s| self.sessions[*s].turns > 0)
            .collect();
        let Some(sess) = live.choose(&mut self.lock().rng).copied() else {
            return Ok(());
        };
        let views = self.lock().pushed_views();
        let aud = self.audience(sess, &views);
        let mut candidates = Vec::new();
        for (_, n) in self.core.store.session_nodes(&self.sessions[sess].id)? {
            if matches!(n.body, Body::ToolCall { .. }) {
                continue;
            }
            let ids = scan(&serde_json::to_string(&n.body)?);
            let s = self.lock();
            if ids
                .iter()
                .any(|id| !s.atoms.allowed(*id, &aud, OWNER, &views))
            {
                candidates.push((n.id.clone(), ids));
            }
        }
        let (to, pick) = {
            let mut s = self.lock();
            let Some(pick) = candidates.choose(&mut s.rng).cloned() else {
                return Ok(());
            };
            let roll = s.rng.random_range(0..100u32);
            let to = match (self.sessions[sess].place, roll) {
                (Some(PlaceKind::Cli), 0..50) | (_, 0..20) => "public".to_string(),
                (Some(PlaceKind::Channel(c)), 20..40) => {
                    let ids: Vec<String> = views
                        .get(&c)
                        .cloned()
                        .flatten()
                        .unwrap_or_default()
                        .iter()
                        .map(u64::to_string)
                        .collect();
                    if ids.is_empty() {
                        "place".into()
                    } else {
                        format!("people:{}", ids.join(","))
                    }
                }
                (Some(PlaceKind::Cli), _) => format!("people:{OWNER}"),
                _ => "place".into(),
            };
            (to, pick)
        };
        self.note(&format!("graduate in {} to {to}", self.sessions[sess].name));
        match self
            .core
            .graduate(&pick.0, &to, "the simulated operator widens it", operator())
        {
            Ok(g) => {
                let r = super::check::readers_of(&g.readers);
                let mut s = self.lock();
                for id in pick.1 {
                    s.atoms.grant(id, r.clone());
                }
                drop(s);
                self.rep.graduated += 1;
                self.dirty.insert(sess);
            }
            Err(_) => self.rep.graduation_refused += 1,
        }
        Ok(())
    }

    /// The owner answers a held post: approve, and it goes as it is;
    /// decline, and its place gets the note.
    pub(crate) fn answer_held(&mut self, all: bool) -> Result<()> {
        if self.held.is_empty() {
            return Ok(());
        }
        let (i, approve) = {
            let mut s = self.lock();
            let i = if all {
                0
            } else {
                s.rng.random_range(0..self.held.len())
            };
            (i, s.rng.random_bool(0.6))
        };
        let h: HeldPost = self.held.remove(i);
        self.note(&format!("answer held post: approve {approve}"));
        self.core
            .confirm_action(&h.question, approve, None, operator())?;
        if approve {
            self.approved.insert(h.question);
        }
        Ok(())
    }

    /// The operator trusts a session that holds external text again.
    fn trust(&mut self) -> Result<()> {
        let latched: Vec<usize> = (0..self.sessions.len())
            .filter(|i| self.sessions[*i].latched.is_some() && self.sessions[*i].parent.is_none())
            .collect();
        let Some(sess) = latched.choose(&mut self.lock().rng).copied() else {
            return Ok(());
        };
        self.note(&format!("trust {}", self.sessions[sess].name));
        self.core
            .trust_session(&self.sessions[sess].id, operator())?;
        self.sessions[sess].latched = None;
        self.rep.trusts += 1;
        self.dirty.insert(sess);
        Ok(())
    }

    /// The test-only foreign node (M6's recall, before M6): a node of another
    /// session, with its own label, written into a live one between turns.
    fn foreign(&mut self) -> Result<()> {
        let (to, from) = {
            let mut s = self.lock();
            let to = self.places.choose(&mut s.rng).map(|(_, i)| *i);
            let from = s.rng.random_range(0..self.sessions.len());
            (to, from)
        };
        let Some(to) = to.filter(|t| *t != from) else {
            return Ok(());
        };
        let nodes = self.core.store.session_nodes(&self.sessions[from].id)?;
        let picks: Vec<Node> = nodes
            .into_iter()
            .map(|(_, n)| n)
            .filter(|n| n.label.is_some() && !matches!(n.body, Body::ToolCall { .. }))
            .collect();
        let Some(src) = picks.choose(&mut self.lock().rng).cloned() else {
            return Ok(());
        };
        let (text, files) = match &src.body {
            Body::UserMessage { text, attachments } => (text.clone(), attachments.clone()),
            Body::ToolResult { content, .. } => (content.clone(), Vec::new()),
            Body::AssistantMessage { blocks, .. } => {
                (theseus_core::provider::text_of(blocks), Vec::new())
            }
            Body::ToolCall { .. } => return Ok(()),
        };
        let label = src
            .label
            .ok_or_else(|| anyhow!("a picked node has a label"))?;
        let node = Node::user_with(
            &self.sessions[to].id,
            None,
            "recall",
            &format!("[Recalled from another session] {text}"),
            files,
        )
        .labeled(label);
        self.core.store.append(&[node.record()?])?;
        self.note(&format!(
            "foreign node from {} into {}",
            self.sessions[from].name, self.sessions[to].name
        ));
        self.rep.foreign += 1;
        self.dirty.insert(to);
        Ok(())
    }
}
