//! The inbound point (M5 step 25a; design §2.4): `classify.v1` (CLASSIFY)
//! and `role.v1` (ROLE_GUESS) about every message a person sends, in
//! shadow. Both read the one `inbound` state, so they ride one request
//! (theseus-judge's batching), and the call's cost is split between their
//! two judgments by question count. Nothing acts on either in M5: routing a
//! message to a task is M7's, and the roles table and role switches are
//! 26c's. `route.v1` (25e) reads the same state, live, in a request of its
//! own beside the batch (theseus-ddbi): one question answers sooner than
//! the batch's seven, and its verdict goes back to the turn over a channel
//! ([`RouteWait`]) the moment it lands, whatever the batch still does. Its
//! call waits for an in-flight permit (`Urgency::Live`, under the client's
//! whole-call limit) instead of being shed. The turn waits for it beside its
//! first compile, never before (`turn::route_step`). The state's input is
//! billed twice, once a request.
//!
//! - **Which turns.** A turn whose input is a person's message (the CLI, the
//!   web UI, a Discord message, a spoken one), once its input node is
//!   written. Never a continuation (a wake's turn, a report's, a task's
//!   first turn, whose brief its parent wrote: none has an input) or a slash
//!   command.
//! - **Off the turn's path.** The turn mints each judgment's id, marks its
//!   trace (a zero-length `judge` span naming the pack, the point, the mode,
//!   and the id its row will carry), spawns the point, and goes on. The
//!   state's build (the session's nodes, its live tasks), its blob, the
//!   reservation, the call, and the record are the spawned task's.
//! - **The roles.** Until 26c brings the versioned table, the state carries
//!   the spec's twelve seed rows (§3.4, [`SEED_ROLES`]) and no current role,
//!   so `role.v1` (asked only when there are roles) asks in shadow now.

use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use serde_json::json;
use theseus_judge::builders::{
    DataNames, InboundInput, PersonaInput, RoleInput, TaskInput, ThreadInput, ThreadNow, TurnInput,
};
use theseus_judge::{Ask, DecisionPoint, Input, Judge, Mode, Outcome, Pack, Urgency};
use theseus_kernel::Kernel;

use super::onto::ONTO_PACK;
use super::{sampled, spend, Built, JudgeService, ScrubWith};
use crate::config::PackMode;
use crate::node::{Body, Node, Origin};
use crate::places::PlaceClass;
use crate::trace::Trace;

/// CLASSIFY (§2.4), at `inbound`: `classify.v4` since theseus-98bz.9, v3's
/// questions and the `scope` question, which asks for the place digest; v3
/// added the `offer_task` question (theseus-bh3t.5), which finds work
/// described for later, and v2 the `thread` question (theseus-rha2).
/// `classify.v2` and `classify.v3` stay embedded for their history.
pub const CLASSIFY_PACK: &str = "classify.v4";
/// The question of `classify` that names a thread.
pub const THREAD_QUESTION: &str = "thread";
/// The question of `classify.v4` that asks whether the message spans every
/// thread of the place (theseus-98bz.9).
pub const SCOPE_QUESTION: &str = "scope";
/// The question of `classify.v3` that finds work to offer as a task.
pub const OFFER_QUESTION: &str = "offer_task";
/// ROLE_GUESS (§2.4), at `inbound`, batched with `classify.v3`.
pub const ROLE_PACK: &str = "role.v1";
/// ROUTE (25e), at `inbound`, in a request of its own beside the batch:
/// live.
/// The acting route pack: `route.v8` since theseus-f9qa8 (2026-10-23), v7's
/// questions with the `data` question's `open` and `unclear` options reworded
/// (v7 itself since theseus-a1wvq, 2026-10-19), live at once with route.v1's
/// rollback rules; `route.v7` to `route.v5` stay embedded for their history,
/// and the ladder never runs them again.
pub const ROUTE_PACK: &str = "route.v8";
/// DIRECTIVE (theseus-slvl.5), at `inbound`, batched with `classify.v1` and
/// `role.v1` (no request of its own), live: asked only for the owner's own
/// message in a place only the owner writes in (`Inbound.capture`).
pub const DIRECTIVE_PACK: &str = crate::capture::PACK;
/// SPLIT (theseus-z9mze.5), at `inbound`, batched with `classify` and `role`
/// (no request of its own), in shadow: whether the work a message asks for
/// divides by area. It decides nothing; the static fan-out cue
/// (`turn/route_step/fan_out_cue.rs`) does, and the two are read against each
/// other by the turn their rows share.
pub const SPLIT_PACK: &str = "split.v1";
/// The packs asked at `inbound`, in the order they are asked.
pub const PACKS: [&str; 6] = [
    CLASSIFY_PACK,
    ROLE_PACK,
    ROUTE_PACK,
    DIRECTIVE_PACK,
    ONTO_PACK,
    SPLIT_PACK,
];

/// Whether `pack` is `onto.v1` or a learned version standing in its place.
pub(super) fn is_onto(pack: &Pack) -> bool {
    pack.name().starts_with("onto.")
}

/// What a turn waits on for `route.v1`'s verdict: `None` when Jev gave none
/// (a failure, a skip, model drift); a dropped sender (nothing sent, the
/// day's limit among it) reads the same, at once.
pub type RouteWait = tokio::sync::oneshot::Receiver<Answered>;

/// What a turn waits on for `classify.v2`'s `thread` answer: dropped without
/// a value when Jev gave none.
pub type ThreadWait = tokio::sync::oneshot::Receiver<ThreadAnswered>;

/// What a turn waits on for `classify.v4`'s `scope` answer: dropped without a
/// value when Jev gave none.
pub type ScopeWait = tokio::sync::oneshot::Receiver<ScopeAnswered>;

/// `classify.v4`'s `scope` answer, come back with its batch.
#[derive(Debug, Clone)]
pub struct ScopeAnswered {
    /// `place`, at the question's confirm bar or above, in a judgment that
    /// acts: the turn gets the place digest.
    pub place: bool,
    /// The option Jev chose, and how sure.
    pub choice: String,
    pub confidence: f64,
    pub judgment: String,
}

/// `classify.v2`'s `thread` answer, come back with its batch.
#[derive(Debug, Clone)]
pub struct ThreadAnswered {
    /// A live thread's slug, `new_thread` or `home`.
    pub choice: String,
    pub confidence: f64,
    /// Whether the judgment acts (canary or live), or is only recorded.
    pub acts: bool,
    pub judgment: String,
    /// The options the question offered, in order (the live threads' slugs
    /// first): what a label of the answer may name.
    pub offered: Vec<String>,
    pub at: tokio::time::Instant,
}

/// A point's asks, begun: the marks its turn's trace takes, and what its
/// answers come back on.
pub struct Early {
    pub marks: Vec<serde_json::Value>,
    pub route: Option<RouteWait>,
    pub thread: Option<ThreadWait>,
    /// `classify.v4`'s `scope` answer, when it was asked (theseus-98bz.9).
    pub scope: Option<ScopeWait>,
    /// Where the owner's words were said, for `directive.v1` asked before
    /// the turn wrote its message (theseus-slvl.5 on thread-route's early
    /// judge): the turn's session and the message's node, sent at `adopt`.
    pub said: Option<tokio::sync::oneshot::Sender<(String, String)>>,
}

impl Early {
    /// The turn takes the point as its own: the marks go on its trace, it
    /// names the session and node its message was written as (`said`), and
    /// it waits on the route verdict as it would have.
    pub fn adopt(self, trace: &mut Trace, said: Option<(String, String)>) -> Option<RouteWait> {
        for m in self.marks {
            trace.mark("judge", "mark", m);
        }
        if let (Some(tx), Some(at)) = (self.said, said) {
            let _ = tx.send(at);
        }
        self.route
    }
}

/// `route.v1`'s request, come back: its verdict, if any, and when it came
/// (theseus-ddbi: `route.decided`'s `answered_ms`).
#[derive(Debug)]
pub struct Answered {
    pub verdict: Option<crate::routing::Verdict>,
    pub at: tokio::time::Instant,
    /// What the call settled at, in micro-dollars (a checkpoint's is charged
    /// to its task).
    pub cost_micros: u64,
}

/// What a point asks: every inbound pack, the route pack alone for a task's brief,
/// or the route pack alone for a running task's checkpoint, which is charged to the
/// task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Point {
    All,
    Brief,
    Checkpoint,
}

/// A checkpoint's settled cost, charged to its task's execution and its
/// parent's by the kernel's carry. It is the judge's day budget's spend too
/// (that record already holds it), so the day ceiling is not told again.
fn charge(kernel: &theseus_kernel::Kernel, execution: &str, micros: u64) {
    if micros == 0 {
        return;
    }
    if let Err(e) = kernel.charge_execution(execution, micros) {
        tracing::warn!(error = %format!("{e:#}"), "judge: a checkpoint's cost was not charged to its execution");
    }
}

/// The spec's twelve seed roles (§3.4), each with its stance and hints in
/// a sentence, as `role.v1`'s options: compiled-in data until step 26c's
/// versioned roles table replaces them.
pub const SEED_ROLES: [(&str, &str); 12] = [
    ("planner", "Goal-directed: tasks and decisions first, a long horizon; asks clarifying questions before acting."),
    ("coder", "Goal-directed: code, tool results, and the repository first; terse; tests as evidence."),
    ("reviewer", "Critical: diffs, prior decisions, and conventions first; never edits; findings with evidence."),
    ("operator", "Cautious, for infrastructure: resources, events, and runbooks first; aware of destructive actions; read tools first."),
    ("researcher", "Exploratory: external sources, topics, and citations first; longer answers; flags uncertainty."),
    ("thought_partner", "Exploratory: people, preferences, and earlier conversations first; asks back; no tools by default."),
    ("expresser", "Creative: persona and voice first; the quality of the prose; few tools."),
    ("triager", "Fast and decisive: events, tasks, and people first; short answers; routes rather than solves."),
    ("secretary", "Goal-directed: calendar, people, and events first; scheduling, briefings, and follow-ups."),
    ("security_analyst", "Sceptical: provenance, trust, and policy first; treats content as evidence, not instruction."),
    ("teacher", "Patient: deep knowledge and resources first; explains the structure; checks understanding."),
    ("archivist", "Curatorial: memory, contradictions, and supersessions first; proposes merges and forgetting."),
];

/// What route.v5's persona and seal questions read (theseus-c1uz): the
/// personas the place may be answered as, the one that answered last, and
/// the thread the message arrives in. Empty (the default) asks neither.
#[derive(Debug, Clone, Default)]
pub struct PersonaCtx {
    pub personas: Vec<PersonaInput>,
    pub speaker: Option<String>,
    pub thread: Option<ThreadHead>,
    /// The names of the categories the session is filed under, for the
    /// `persona` question's state (theseus-jrf4c).
    pub categories: Vec<String>,
}

/// A thread's head for the state: its title and goal, and which persona
/// answered the session's recent turns (turn id, key).
#[derive(Debug, Clone)]
pub struct ThreadHead {
    pub title: String,
    pub goal: String,
    pub answered: Vec<(String, Option<String>)>,
}

/// What the turn hands the point once its input node is written.
pub struct Inbound {
    pub session_id: String,
    pub execution_id: String,
    pub turn_id: String,
    /// The input's node, which the state reads back with the session's.
    pub node_id: String,
    /// The input's text: it tells a slash command, and is an offer's brief.
    pub text: String,
    /// Where it came in (`cli`, `web`, `discord_dm`, …) and the place
    /// rule's answer for it, as [`place_kind`] says it.
    pub place_kind: String,
    /// `operator` in a private place; a shared place's person is not named.
    pub author: String,
    /// The live tasks are the kernel's.
    pub kernel: Arc<Kernel>,
    /// The live threads the message may be about (theseus-rha2), most
    /// recent first; none: `classify.v2` asks no `thread` question.
    pub threads: Vec<ThreadInput>,
    /// The message arrives in a thread's own session and the place digest is
    /// on: `classify.v4` asks its `scope` question without the list of
    /// threads (theseus-98bz.9).
    pub in_thread: bool,
    /// `route.v1`'s mode for this turn, as the turn decides it (`[routing]`,
    /// the judge's ladder, a pin): off, shadow, or live.
    pub route: PackMode,
    /// The profile the owner chose for this turn (a pin), for the record.
    pub chosen: Option<String>,
    /// The turn is a task's.
    pub task: bool,
    /// route.v5's persona and seal inputs.
    pub persona: PersonaCtx,
    /// The message is the owner's own, in a place only the owner writes in,
    /// and `[standing] capture` is on: `directive.v1` is asked about it.
    pub capture: bool,
    /// The operator's names for open and private matter (`[routing.data]`),
    /// carried to the route pack's `data` question.
    pub data_names: DataNames,
    /// The message's attachments, as the node holds them: `onto.v1` is
    /// shown the pictures (their bytes read from the blobs) and the text
    /// ones' first characters (theseus-p6zgq). Only a turn that has its
    /// node carries them.
    pub attachments: Vec<crate::node::Attachment>,
}

/// The workload class (§2.2) an inbound judgment records. The turn has not
/// run, so its tools and its reply are not known: a task's message is
/// `task`, and any other is `unknown`, written on purpose, so the metrics
/// read a decision and not a gap (theseus-fi5n).
pub fn class(task: bool) -> &'static str {
    if task {
        "task"
    } else {
        "unknown"
    }
}

/// The state's `place_kind`: the surface (the session's place, or, with
/// none, the client that asked) and the place rule's class.
pub fn place_kind(place: Option<&str>, client: &str, class: PlaceClass) -> String {
    let surface = match place {
        Some(p) if p.starts_with("discord:dm:") => "discord_dm",
        Some(p) if p.starts_with("discord:channel:") => "discord_channel",
        Some(p) if p.starts_with("discord:") => "discord",
        Some(_) => "other",
        None if client.starts_with("web#") => "web",
        None => "cli",
    };
    let class = match class {
        PlaceClass::Private => "private",
        PlaceClass::Shared => "shared",
    };
    format!("{surface} ({class})")
}

/// The state's `author`: the operator in a private place (the place rule
/// gives a private place to the owner); a shared place's person goes
/// unnamed, so no name leaves for Jev.
pub fn author_of(class: PlaceClass) -> &'static str {
    match class {
        PlaceClass::Private => "operator",
        PlaceClass::Shared => "a person in a shared place",
    }
}

/// A slash command (`/stop`, `/tasks 2`): its first word is `/` and a
/// name. A path (`/etc/hosts is empty`) is not one.
pub fn slash_command(text: &str) -> bool {
    let Some(word) = text.split_whitespace().next() else {
        return false;
    };
    let Some(name) = word.strip_prefix('/') else {
        return false;
    };
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

impl JudgeService {
    /// A person's message, its node written: `classify.v1` and `role.v1`
    /// judge it in shadow, and `route.v1` in the mode the turn gives it, as
    /// one decision point in a task of its own. Each judgment's id is minted
    /// here and marked on the turn's trace. Returns at once, whatever Jev
    /// does, with the channel `route.v1`'s verdict comes back on when it is
    /// asked (in any mode): the turn decides whether to wait for it.
    pub fn at_inbound(&self, trace: &mut Trace, msg: Inbound) -> Option<RouteWait> {
        self.inbound_early(msg)?.adopt(trace, None)
    }

    /// `at_inbound`, begun before its turn has a trace: the same asks, in the
    /// same requests, and the marks the turn will take (theseus-rha2: the
    /// router waits on the thread answer before it knows which session's turn
    /// this is).
    pub fn inbound_early(&self, msg: Inbound) -> Option<Early> {
        self.inbound_early_of(msg, Point::All)
    }

    /// A task's brief, whose first turn has no message of its own: the route pack
    /// alone judges it (no `classify`, no `role`), so the verdict can pick
    /// the task's model (theseus-ypnxt.7, `turn/brief_step.rs`).
    pub fn at_brief(&self, trace: &mut Trace, msg: Inbound) -> Option<RouteWait> {
        self.inbound_early_of(msg, Point::Brief)?.adopt(trace, None)
    }

    /// A running task's checkpoint (theseus-ypnxt.9, `turn/checkpoint_step.rs`):
    /// the route pack alone judges `msg.text`, the task's recent state, as it
    /// judges a brief. What the call settles at is the task's, charged to its
    /// execution when it lands ([`charge`]); the verdict's cost rides
    /// `Answered`. A message with no node of its own: `msg.node_id` is empty.
    pub fn at_checkpoint(&self, trace: &mut Trace, msg: Inbound) -> Option<RouteWait> {
        self.inbound_early_of(msg, Point::Checkpoint)?
            .adopt(trace, None)
    }

    fn inbound_early_of(&self, msg: Inbound, point: Point) -> Option<Early> {
        let route_only = point != Point::All;
        if slash_command(&msg.text) {
            return None;
        }
        let mode_of = |p: &str| match p {
            ROUTE_PACK => msg.route,
            _ => self.mode_for(p, &msg.session_id).mode,
        };
        // Each root, or the learned version standing in its place (25f);
        // route.v1 is left alone.
        let packs: Vec<(Arc<Pack>, String, PackMode)> = PACKS
            .iter()
            .map(|root| match *root {
                ROUTE_PACK => ((*root).to_string(), *root),
                _ => (self.placed(root, &msg.session_id), *root),
            })
            .map(|(p, root)| (mode_of(&p), p, root))
            .filter(|(m, _, _)| *m != PackMode::Off)
            .filter(|(_, _, root)| *root != DIRECTIVE_PACK || msg.capture)
            .filter(|(_, _, root)| *root != ONTO_PACK || (!route_only && self.onto_wanted(&msg)))
            // A task's message is its parent's brief, never a person's ask to divide.
            .filter(|(_, _, root)| *root != SPLIT_PACK || !msg.task)
            .filter(|(_, _, root)| !route_only || *root == ROUTE_PACK)
            .filter_map(|(m, p, root)| Some((self.pack(&p)?, m, root)))
            .filter(|(p, _, root)| sampled(&msg.turn_id, self.cfg.sample_of(root, p.sample)))
            .map(|(p, m, _)| (p, m))
            .map(|(p, m)| (p, theseus_judge::judge::new_id(), m))
            .collect();
        if packs.is_empty() {
            return None;
        }
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            return None;
        };
        let marks: Vec<serde_json::Value> = packs
            .iter()
            .map(|(pack, id, mode)| {
                json!({"pack": pack.name(), "point": "inbound", "mode": mode.as_str(), "judgment": id})
            })
            .collect();
        let (tx, rx) = match packs.iter().any(|(p, _, _)| p.name() == ROUTE_PACK) {
            true => {
                let (tx, rx) = tokio::sync::oneshot::channel();
                (Some(tx), Some(rx))
            }
            false => (None, None),
        };
        let (ttx, trx) = match !msg.threads.is_empty()
            && packs.iter().any(|(p, _, _)| p.name() == CLASSIFY_PACK)
        {
            true => {
                let (tx, rx) = tokio::sync::oneshot::channel();
                (Some(tx), Some(rx))
            }
            false => (None, None),
        };
        let (stx, srx) = match (!msg.threads.is_empty() || msg.in_thread)
            && packs.iter().any(|(p, _, _)| p.name() == CLASSIFY_PACK)
        {
            true => {
                let (tx, rx) = tokio::sync::oneshot::channel();
                (Some(tx), Some(rx))
            }
            false => (None, None),
        };
        let heard = packs
            .iter()
            .any(|(p, _, _)| p.name().starts_with("directive."))
            .then(|| crate::capture::act::Message {
                session: msg.session_id.clone(),
                node: msg.node_id.clone(),
                text: msg.text.clone(),
                place_kind: msg.place_kind.clone(),
            });
        // Asked before its turn wrote the message (the router's early judge):
        // the turn names where it was said when it adopts the point.
        let (said, said_rx) = match heard.is_some() && msg.node_id.is_empty() {
            true => {
                let (tx, rx) = tokio::sync::oneshot::channel();
                (Some(tx), Some(rx))
            }
            false => (None, None),
        };
        rt.spawn(judge_inbound(
            self.me.clone(),
            packs,
            msg,
            tx,
            (ttx, stx),
            (heard, said_rx),
            point == Point::Checkpoint,
        ));
        Some(Early {
            marks,
            route: rx,
            thread: trx,
            scope: srx,
            said,
        })
    }

    /// A verdict that came after its turn's wait: it applies from the
    /// session's next message.
    pub fn set_late(&self, session: &str, v: crate::routing::Verdict) {
        self.late
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(session.to_string(), v);
    }

    /// The session's late verdict, taken.
    pub fn take_late(&self, session: &str) -> Option<crate::routing::Verdict> {
        self.late
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(session)
    }

    /// A turn route.v1 acts on (theseus-ddbi): its wait after the first
    /// compile, and whether the verdict missed it, as
    /// `theseus.route.wait`.
    pub fn record_route_wait(&self, wait: Duration, late: bool) {
        if let Some(t) = self.telemetry.get() {
            t.record_route_wait(wait, late);
        }
    }

    /// A turn the data rule kept off a restricted-origin model, as
    /// `theseus.route.data_gate` (theseus-a1wvq.11).
    pub fn record_data_gate(&self, what: &str, why: &str) {
        if let Some(t) = self.telemetry.get() {
            t.record_data_gate(what, why);
        }
    }

    /// The session holds a late verdict (a test's wait for one to land).
    #[cfg(test)]
    pub fn has_late(&self, session: &str) -> bool {
        let late = self.late.lock().unwrap_or_else(|e| e.into_inner());
        late.contains_key(session)
    }

    /// A routed turn of `session` ran on `profile` (route.v1 acting, no pin):
    /// the owner's pin of another profile within [`CHOSEN_MS`] after it
    /// counts on route.v1's ladder (26a). Older turns are dropped as each
    /// lands.
    ///
    /// [`CHOSEN_MS`]: crate::learning::system::CHOSEN_MS
    pub fn set_routed(&self, session: &str, profile: &str) {
        let now = theseus_protocol::now_unix_ms();
        let mut m = self.routed.lock().unwrap_or_else(|e| e.into_inner());
        m.retain(|_, (at, _)| now.saturating_sub(*at) <= crate::learning::system::CHOSEN_MS);
        m.insert(session.to_string(), (now, profile.to_string()));
    }

    /// A message of `session` the owner pinned to `profile`: after a routed
    /// turn of the session on another profile, within
    /// [`CHOSEN_MS`](crate::learning::system::CHOSEN_MS), it is route.v1's
    /// ladder event (26a: three in a local day roll it back), once per
    /// routed turn. Landed off the turn's path: `land` writes a frame.
    pub fn pinned(&self, session: &str, profile: &str) {
        let now = theseus_protocol::now_unix_ms();
        let after_routed = {
            let mut m = self.routed.lock().unwrap_or_else(|e| e.into_inner());
            match m.get(session) {
                Some((at, ran))
                    if now.saturating_sub(*at) <= crate::learning::system::CHOSEN_MS
                        && ran != profile =>
                {
                    m.remove(session);
                    true
                }
                _ => false,
            }
        };
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            return;
        };
        if after_routed {
            let me = self.me.clone();
            rt.spawn_blocking(move || {
                if let Some(svc) = me.upgrade() {
                    let day = svc.today();
                    svc.land(
                        ROUTE_PACK,
                        theseus_judge::learn::CanaryEvent::Pinned { day },
                    );
                }
            });
        }
    }

    /// The owner said a message was routed to the wrong thread: a "wrong
    /// thread" label on `classify.v2`'s count of the day, which its rollback
    /// rule reads. Landed off the turn's path: `land` writes a frame.
    pub fn wrong_thread(&self) {
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let me = self.me.clone();
        rt.spawn_blocking(move || {
            if let Some(svc) = me.upgrade() {
                let day = svc.today();
                svc.land(
                    CLASSIFY_PACK,
                    theseus_judge::learn::CanaryEvent::Label {
                        day,
                        label: "wrong thread".into(),
                    },
                );
            }
        });
    }

    /// Whether Jev's breaker is open now: a live verdict would not come, so
    /// the turn does not wait for one.
    pub fn breaker_open(&self) -> bool {
        self.built.get().is_some_and(|b| {
            matches!(
                b.judge.inner().breaker_status(),
                theseus_judge::breaker::Status::Open { .. }
            )
        })
    }

    /// Whether Jev can answer now: its key settled or settling, and its
    /// breaker not open. While it cannot, a session routing moved runs on
    /// its own profile again (theseus-9yyr).
    pub fn reachable(&self) -> bool {
        let key = matches!(
            self.secrets.states().get(&self.cfg.key_secret),
            Some(crate::secrets::SecretState::Ready(_) | crate::secrets::SecretState::Resolving)
        );
        key && !self.breaker_open()
    }

    /// The blocking half before the call: the state's build and blob, one
    /// ask per pack, and the reservation. `None`: nothing to send.
    fn prepare_inbound(
        &self,
        packs: Vec<(Arc<Pack>, String, PackMode)>,
        msg: &Inbound,
        today: &str,
    ) -> Option<Ready> {
        let mut input = self.inbound_input(msg)?;
        let built = self
            .built()
            .map_err(|e| tracing::warn!(error = %format!("{e:#}"), "judge: the Jev client was not built"))
            .ok()?;
        let scrub = ScrubWith(self.scrubber.clone());
        // onto.v1's fields ride the shared state: the batch's packs read them
        // (one state, one request), `route.v6` never does, and with
        // `request = "own"` only `onto.v1` does (theseus-xzxqv).
        let core = self.core();
        let onto = packs.iter().any(|(p, _, _)| is_onto(p))
            && core
                .as_ref()
                .is_some_and(|c| super::onto::fill(c, msg, &mut input));
        let onto_context = match (&core, onto) {
            (Some(c), true) => super::onto::context(c, &msg.session_id, &input),
            _ => serde_json::Value::Null,
        };
        let own = onto && self.onto_own();
        let plain = Input::Inbound(InboundInput {
            onto: None,
            ..input.clone()
        });
        let shared = Input::Inbound(input);
        let mut asks = Vec::with_capacity(packs.len());
        for (pack, id, mode) in packs {
            if is_onto(&pack) && !onto {
                continue;
            }
            let carries = match (is_onto(&pack), pack.name() == ROUTE_PACK) {
                (true, _) => &shared,
                (false, true) => &plain,
                (false, false) => {
                    if own {
                        &plain
                    } else {
                        &shared
                    }
                }
            };
            let state = theseus_judge::prepare(&pack, carries, &scrub).ok()?;
            // The packs share the state, so this is one blob, which the sink
            // writes before their rows (theseus-otny).
            let blob = self.stage_blob(state.state.json.as_bytes());
            let baseline = match pack.name().as_str() {
                CLASSIFY_PACK => "conversation",
                ROUTE_PACK => "session_profile",
                _ if is_onto(&pack) => "no_membership",
                _ => "current_role",
            };
            let mut context = json!({
                "session": msg.session_id, "execution": msg.execution_id, "turn": msg.turn_id,
                "node": msg.node_id, "baseline": baseline, "place_kind": msg.place_kind,
                "blob": blob, "on_path_ms": 0, "class": class(msg.task),
            });
            if is_onto(&pack) {
                context["onto"] = onto_context.clone();
            }
            if pack.name() == CLASSIFY_PACK && !msg.threads.is_empty() {
                // The slugs the `thread` question offered, so a label of its
                // answer names what was on offer (theseus-rha2).
                context["threads"] = json!(msg
                    .threads
                    .iter()
                    .map(|t| t.id.as_str())
                    .collect::<Vec<_>>());
            }
            if pack.name() == ROUTE_PACK {
                if msg.persona.personas.len() >= 2 {
                    // The ids the `persona` question offered, so a label of
                    // its answer names what was on offer (theseus-c1uz).
                    context["personas"] = json!(msg
                        .persona
                        .personas
                        .iter()
                        .map(|p| p.id.as_str())
                        .collect::<Vec<_>>());
                }
                context["pinned"] = json!(msg.chosen.is_some());
                if let Some(c) = &msg.chosen {
                    context["chosen"] = json!(c);
                }
            }
            // route.v1 asks in the mode its turn gave it: shadow for a pin or
            // `[routing]` in shadow, else the ladder's (live, or canary in a
            // canary's arm); the others in the ladder's. Each records its arm
            // (26a).
            let laddered = self.ask_mode(&pack.name(), &mut context);
            let jmode = match (pack.name() == ROUTE_PACK, mode) {
                (true, PackMode::Live | PackMode::Canary) => laddered,
                (true, _) => Mode::Shadow,
                (false, _) => laddered,
            };
            let mut ask = Ask::new(pack, &state, jmode, context);
            ask.id = Some(id);
            asks.push(ask);
        }
        // Two requests, each reserved as it goes out (theseus-ddbi), and a
        // third for onto.v1 when the owner moved it out of the batch.
        let (route, rest): (Vec<&Ask>, Vec<&Ask>) =
            asks.iter().partition(|a| a.pack.name() == ROUTE_PACK);
        let (alone, rest): (Vec<&Ask>, Vec<&Ask>) =
            rest.into_iter().partition(|a| own && is_onto(&a.pack));
        let reserve = |a: Vec<&Ask>| {
            let a: Vec<Ask> = a.into_iter().cloned().collect();
            built.judge.inner().reserve_micros(&a).unwrap_or(0)
        };
        let need = reserve(route) + reserve(alone) + reserve(rest);
        let beside = self.reserve_beside(today, need)?;
        Some(Ready {
            built,
            asks,
            need,
            beside,
        })
    }

    /// The `inbound` input from the session's nodes and its live tasks.
    fn inbound_input(&self, msg: &Inbound) -> Option<InboundInput> {
        let nodes: Vec<Node> = self
            .store
            .session_nodes(&msg.session_id)
            .map_err(|e| tracing::warn!(error = %format!("{e:#}"), "judge: the session's nodes were not read"))
            .ok()?
            .into_iter()
            .map(|(_, n)| n)
            .collect();
        let live_tasks = self.live_tasks(&msg.kernel, &msg.execution_id);
        Some(input(&nodes, msg, live_tasks))
    }

    /// The session's live tasks, oldest first: each one's id, its brief's
    /// first line, and its state. A task whose brief cannot be read shows
    /// its title.
    fn live_tasks(&self, kernel: &Kernel, execution_id: &str) -> Vec<TaskInput> {
        let tasks = match kernel.tasks(Some(execution_id)) {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "judge: the session's tasks were not read");
                return Vec::new();
            }
        };
        tasks
            .into_iter()
            .filter(|e| !e.state.is_terminal())
            .take(theseus_judge::builders::LIVE_TASKS)
            .map(|e| {
                let brief = self
                    .store
                    .session_nodes(&e.session_id)
                    .ok()
                    .and_then(|ns| ns.into_iter().find_map(|(_, n)| brief_of(&n)))
                    .unwrap_or_default();
                TaskInput {
                    id: crate::task::short(&e.session_id),
                    brief,
                    state: e.state.as_str().into(),
                }
            })
            .collect()
    }
}

/// A task's brief, from its first message: the text after the harness's
/// bracketed preamble.
fn brief_of(n: &Node) -> Option<String> {
    let Body::UserMessage { text, .. } = &n.body else {
        return None;
    };
    let brief = text.split_once("]\n\n").map_or(text.as_str(), |(_, b)| b);
    Some(brief.trim().to_string())
}

/// The input, from the session's nodes, oldest first: the message (its own
/// node), the previous message a person wrote and the last reply before it,
/// and the minutes since the message before it.
pub fn input(nodes: &[Node], msg: &Inbound, live_tasks: Vec<TaskInput>) -> InboundInput {
    let at = nodes.iter().position(|n| n.id == msg.node_id);
    let (before, message) = match at {
        Some(i) => (&nodes[..i], Some(&nodes[i])),
        None => (nodes, None),
    };
    // A task's brief is judged by its own words, not the preamble around it.
    let text = match message.map(|n| (n, &n.body)) {
        Some((n, Body::UserMessage { .. })) if msg.task && crate::task::is_brief(n) => {
            brief_of(n).unwrap_or_default()
        }
        Some((_, Body::UserMessage { text, .. })) => text.clone(),
        _ => msg.text.clone(),
    };
    let previous_human_message = before.iter().rev().find_map(|n| match &n.body {
        Body::UserMessage { text, .. } if n.origin == Origin::Operator => Some(text.clone()),
        _ => None,
    });
    let last_reply = crate::task::last_message(before.iter()).map(|(_, t)| t);
    let minutes_since_last_message = message.and_then(|m| {
        before
            .iter()
            .rev()
            .find(|n| {
                matches!(
                    n.body,
                    Body::UserMessage { .. } | Body::AssistantMessage { .. }
                )
            })
            .map(|p| m.created_at_ms.saturating_sub(p.created_at_ms) / 60_000)
    });
    InboundInput {
        message: text,
        author: msg.author.clone(),
        place_kind: msg.place_kind.clone(),
        previous_human_message,
        last_reply,
        minutes_since_last_message,
        live_tasks,
        live_threads: msg.threads.clone(),
        in_thread: msg.in_thread,
        personas: msg.persona.personas.clone(),
        speaker: msg.persona.speaker.clone(),
        categories: msg.persona.categories.clone(),
        thread_now: msg.persona.thread.as_ref().map(|h| ThreadNow {
            title: h.title.clone(),
            goal: h.goal.clone(),
            turns: before
                .iter()
                .filter(|n| n.origin == Origin::Operator)
                .filter_map(|n| match &n.body {
                    Body::UserMessage { text, .. } => Some(TurnInput {
                        said: text.clone(),
                        answered_by: n
                            .turn_id
                            .as_deref()
                            .and_then(|t| h.answered.iter().find(|(id, _)| id == t))
                            .and_then(|(_, k)| k.clone()),
                    }),
                    _ => None,
                })
                .collect(),
        }),
        current_role: None,
        data_names: msg.data_names.clone(),
        onto: None,
        roles: SEED_ROLES
            .iter()
            .map(|(id, stance)| RoleInput {
                id: (*id).into(),
                stance: (*stance).into(),
            })
            .collect(),
    }
}

/// The point's asks, ready to send, and the budget's frame to write beside
/// the call.
struct Ready {
    built: Arc<Built>,
    asks: Vec<Ask>,
    need: theseus_judge::price::Micros,
    beside: super::Beside,
}

/// The route pack's verdict from its judgment: its `mode` answer, when it
/// was answered by the model the pack pins, and route.v3's `reply_effort`
/// answer beside it, sure when its band reached the question's confirm bar
/// (theseus-qe3v).
///
/// The pack's `data` question is not read: whether a session may use a model
/// of restricted origin is decided by the labels on it (`crate::content`),
/// not by a verdict per message.
pub fn verdict(j: &theseus_judge::Judgment, turn: &str) -> Option<crate::routing::Verdict> {
    use crate::routing::effort::{EffortAnswer, QUESTION};
    if !j.actionable() {
        return None;
    }
    let choice = |q: &str| {
        let a = j.answer(q)?;
        match &a.answer {
            theseus_judge::Answer::Choice {
                choice, confidence, ..
            } => Some((choice.clone(), *confidence, a.band.band)),
            _ => None,
        }
    };
    let (mode, confidence, _) = choice("mode")?;
    let effort = choice(QUESTION).map(|(level, confidence, band)| EffortAnswer {
        level,
        confidence,
        sure: band != theseus_judge::band::Band::Escalate,
    });
    Some(crate::routing::Verdict {
        mode,
        confidence,
        effort,
        judgment: j.id.clone(),
        turn: turn.to_string(),
        persona: crate::engage::persona_answer(j).map(Box::new),
        seal: crate::engage::seal_answer(j).map(Box::new),
        personal: choice(crate::routing::personal::QUESTION).map(|(choice, confidence, band)| {
            Box::new(crate::routing::PersonalAnswer {
                choice,
                confidence,
                band,
            })
        }),
    })
}

/// The message names one of the operator's sensitive names
/// (`[privacy] sensitive_names`, the private names of Jev's state).
pub fn names_hit(names: &DataNames, text: &str) -> bool {
    !names.private.is_empty()
        && !theseus_judge::names::Names::new(&names.private)
            .hits(text)
            .is_empty()
}

/// `classify.v2`'s `thread` answer from its judgment, when it was answered
/// by the model the pack pins.
pub fn thread_answer(j: &theseus_judge::Judgment) -> Option<ThreadAnswered> {
    if j.pack != CLASSIFY_PACK || !j.actionable() {
        return None;
    }
    let a = j.answer(THREAD_QUESTION)?;
    let theseus_judge::Answer::Choice {
        choice, confidence, ..
    } = &a.answer
    else {
        return None;
    };
    Some(ThreadAnswered {
        choice: choice.clone(),
        confidence: *confidence,
        acts: j.mode != theseus_judge::Mode::Shadow,
        judgment: j.id.clone(),
        offered: j
            .context
            .get("threads")
            .and_then(|v| v.as_array())
            .map(|v| {
                v.iter()
                    .filter_map(|s| s.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        at: tokio::time::Instant::now(),
    })
}

/// `classify.v4`'s `scope` answer from its judgment, when it was answered by
/// the model the pack pins.
pub fn scope_answer(j: &theseus_judge::Judgment) -> Option<ScopeAnswered> {
    if j.pack != CLASSIFY_PACK || !j.actionable() {
        return None;
    }
    let a = j.answer(SCOPE_QUESTION)?;
    let theseus_judge::Answer::Choice {
        choice, confidence, ..
    } = &a.answer
    else {
        return None;
    };
    Some(ScopeAnswered {
        place: choice == "place"
            && a.band.band != theseus_judge::band::Band::Escalate
            && j.mode != theseus_judge::Mode::Shadow,
        choice: choice.clone(),
        confidence: *confidence,
        judgment: j.id.clone(),
    })
}

/// Whether `classify.v3` found the message to be work for later, at the act
/// bar, in a judgment that acts (a shadow one offers nothing).
pub fn offer_answer(j: &theseus_judge::Judgment) -> bool {
    j.pack == CLASSIFY_PACK
        && j.actionable()
        && j.mode != theseus_judge::Mode::Shadow
        && j.answer(OFFER_QUESTION).is_some_and(|a| a.band.acts_true())
}

/// The urgency of a request whose asks run in `modes`: live when one acts
/// (canary or live), so it waits for a permit rather than being shed.
fn urgency_of(modes: impl Iterator<Item = PackMode>, total_secs: u64) -> Urgency {
    let mut modes = modes;
    match modes.any(|m| m >= PackMode::Canary) {
        true => Urgency::live(Duration::from_secs(total_secs)),
        false => Urgency::Shadow,
    }
}

/// One request of the point's asks: none when it has none.
async fn lane(built: &Built, asks: Vec<Ask>, urgency: Urgency) -> Vec<theseus_judge::Judgment> {
    if asks.is_empty() {
        return Vec::new();
    }
    built.judge.judge(DecisionPoint { asks, urgency }).await
}

/// The thread answer goes to the router the moment the batch lands, and an
/// offer is noted for the reply.
async fn hand_thread_answer(
    batch: impl std::future::Future<Output = Vec<theseus_judge::Judgment>>,
    (thread, scope): (
        Option<tokio::sync::oneshot::Sender<ThreadAnswered>>,
        Option<tokio::sync::oneshot::Sender<ScopeAnswered>>,
    ),
    offer: (&Weak<JudgeService>, &str, &str),
) -> Vec<theseus_judge::Judgment> {
    let judgments = batch.await;
    if let (true, Some(svc)) = (judgments.iter().any(offer_answer), offer.0.upgrade()) {
        svc.note_offer(offer.1, offer.2);
    }
    if let Some(svc) = offer.0.upgrade() {
        for j in judgments.iter().filter(|j| j.pack.starts_with("onto.")) {
            svc.onto_landed(j, offer.1, offer.2);
        }
    }
    if let Some(tx) = thread {
        if let Some(a) = judgments.iter().find_map(thread_answer) {
            let _ = tx.send(a);
        }
    }
    if let Some(tx) = scope {
        if let Some(a) = judgments.iter().find_map(scope_answer) {
            let _ = tx.send(a);
        }
    }
    judgments
}

/// The route judgment's `seal` verdict, handed on (theseus-c1uz).
fn seal_of(me: &Weak<JudgeService>, session: &str, judgments: &[theseus_judge::Judgment]) {
    let found = judgments
        .iter()
        .find(|j| j.pack == ROUTE_PACK)
        .and_then(crate::engage::seal_answer);
    if let (Some(a), Some(svc)) = (found, me.upgrade()) {
        svc.seal_verdict(session, &a);
    }
}

/// The urgency of `route.v1`'s ask and of the batch beside it.
fn urgencies(packs: &[(Arc<Pack>, String, PackMode)], total: u64) -> (Urgency, Urgency) {
    let of = |route: bool| {
        let modes = packs
            .iter()
            .filter(|(p, _, _)| (p.name() == ROUTE_PACK) == route)
            .map(|(_, _, m)| *m);
        urgency_of(modes, total)
    };
    (of(true), of(false))
}

/// A route verdict has gone to its turn: a test waits for it (theseus-1b2zm).
fn route_sent(me: &Weak<JudgeService>) {
    #[cfg(test)]
    if let Some(svc) = me.upgrade() {
        svc.route_answers
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
    #[cfg(not(test))]
    let _ = me;
}

/// One inbound decision point, in its own task: `route.v1`'s ask in a
/// request of its own, and the other packs' asks batched into one, the two
/// at once (theseus-ddbi). The verdict goes to the turn as its request
/// answers, never after the batch's. The service is held only around the
/// blocking half, never across the calls. A live `route.v1` waits for a
/// permit rather than being shed, under the whole-call limit.
async fn judge_inbound(
    me: Weak<JudgeService>,
    packs: Vec<(Arc<Pack>, String, PackMode)>,
    msg: Inbound,
    route: Option<tokio::sync::oneshot::Sender<Answered>>,
    thread: (
        Option<tokio::sync::oneshot::Sender<ThreadAnswered>>,
        Option<tokio::sync::oneshot::Sender<ScopeAnswered>>,
    ),
    (heard, said): (
        Option<crate::capture::act::Message>,
        Option<tokio::sync::oneshot::Receiver<(String, String)>>,
    ),
    charged: bool,
) {
    let today = spend::local_day(theseus_protocol::now_unix_ms());
    let Some(svc) = me.upgrade() else { return };
    let own = svc.onto_own();
    let total = svc.cfg.total_secs;
    let (route_urgency, rest_urgency) = urgencies(&packs, total);
    let (turn, brief) = (msg.turn_id.clone(), msg.text.clone());
    let session = msg.session_id.clone();
    let to_task = charged.then(|| (msg.kernel.clone(), msg.execution_id.clone()));
    let day = today.clone();
    let ready = tokio::task::spawn_blocking(move || svc.prepare_inbound(packs, &msg, &day))
        .await
        .ok()
        .flatten();
    let Some(Ready {
        built,
        asks,
        need,
        beside,
    }) = ready
    else {
        return;
    };
    // The budget's frame goes beside the call, never before it
    // (theseus-otny).
    let wrote = beside.spawn(&me);
    let t0 = Instant::now();
    let (route_asks, rest_asks): (Vec<Ask>, Vec<Ask>) =
        asks.into_iter().partition(|a| a.pack.name() == ROUTE_PACK);
    let (own_asks, rest_asks): (Vec<Ask>, Vec<Ask>) =
        rest_asks.into_iter().partition(|a| own && is_onto(&a.pack));
    let routed = async {
        if route_asks.is_empty() {
            return Vec::new();
        }
        let point = DecisionPoint {
            asks: route_asks,
            urgency: route_urgency,
        };
        let judgments = built.judge.judge(point).await;
        tracing::debug!(
            ms = t0.elapsed().as_millis() as u64,
            "judge: route.v1 judged"
        );
        let cost_micros: u64 = judgments.iter().filter_map(|j| j.cost_micros).sum();
        if let Some((kernel, execution)) = &to_task {
            charge(kernel, execution, cost_micros);
        }
        if let Some(tx) = route {
            let _ = tx.send(Answered {
                verdict: judgments.first().and_then(|j| verdict(j, &turn)),
                at: tokio::time::Instant::now(),
                cost_micros,
            });
            route_sent(&me);
        }
        judgments
    };
    let rest = hand_thread_answer(
        lane(&built, rest_asks, rest_urgency),
        thread,
        (&me, &turn, &brief),
    );
    // onto.v1 in a request of its own: no turn waits on it, and it is
    // reserved and recorded as the batch's judgments are.
    let alone = hand_thread_answer(
        lane(&built, own_asks, rest_urgency),
        (None, None),
        (&me, &turn, &brief),
    );
    let (mut judgments, rest, alone) = futures_util::future::join3(routed, rest, alone).await;
    judgments.extend(rest);
    judgments.extend(alone);
    seal_of(&me, &session, &judgments);
    tracing::debug!(
        ms = t0.elapsed().as_millis() as u64,
        "judge: the inbound packs judged"
    );
    if let Some(w) = wrote {
        let _ = w.await;
    }
    let Some(svc) = me.upgrade() else { return };
    // The point settles as one: its reservation, what its judgments cost
    // together (a call whose usage is unknown at its reservation), and one
    // call, or one skip, whatever the packs that rode it.
    let (mut called, mut failed, mut unknown) = (false, false, false);
    let mut spent = 0;
    for j in &judgments {
        match &j.outcome {
            Outcome::Answered => called = true,
            Outcome::Failed { usage_unknown, .. } => {
                (called, failed) = (true, true);
                unknown |= *usage_unknown;
            }
            Outcome::Skipped { .. } => {}
        }
        spent += j.cost_micros.unwrap_or(0);
    }
    if unknown {
        spent = need;
    }
    svc.budget.settle(&today, need, spent, called, failed);
    // The owner's words, acted on once the batch has settled (theseus-slvl.5):
    // `directive.v1` rode this request, so no call was added for it.
    capture_after(&svc, (heard, said), &judgments).await;
}

/// How long an early `directive.v1` waits for its turn to write the message.
const SAID_WAIT: std::time::Duration = std::time::Duration::from_secs(60);

/// The owner's words, acted on (theseus-slvl.5): the batch's `directive.v1`
/// judgment, when there is one, goes to the core.
/// Asked early, it waits for the turn to say where the words were written,
/// and a turn that never does (it did not run) captures nothing.
async fn capture_after(
    svc: &JudgeService,
    (heard, said): (
        Option<crate::capture::act::Message>,
        Option<tokio::sync::oneshot::Receiver<(String, String)>>,
    ),
    judgments: &[theseus_judge::Judgment],
) {
    let heard = match (heard, said) {
        (Some(mut m), Some(rx)) => match tokio::time::timeout(SAID_WAIT, rx).await {
            Ok(Ok((session, node))) => {
                (m.session, m.node) = (session, node);
                Some(m)
            }
            _ => None,
        },
        (heard, _) => heard,
    };
    let found = judgments.iter().find(|j| j.pack.starts_with("directive."));
    if let (Some(m), Some(j), Some(core)) = (heard, found, svc.core()) {
        core.capture_directive(&m, j);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slash_command_is_a_slash_and_a_name_and_a_path_is_not() {
        for s in ["/stop", " /tasks 2", "/new", "/prompt-review now", "/x_y"] {
            assert!(slash_command(s), "{s}");
        }
        for s in [
            "stop",
            "/etc/hosts is empty",
            "/",
            "/ stop",
            "",
            "please /stop",
        ] {
            assert!(!slash_command(s), "{s}");
        }
    }

    #[test]
    fn a_message_is_a_tasks_or_its_class_is_unknown_on_purpose() {
        assert_eq!(class(true), "task");
        assert_eq!(class(false), "unknown");
    }

    #[test]
    fn the_place_kind_names_the_surface_and_the_class() {
        let k = |p: Option<&str>, c: &str, class| place_kind(p, c, class);
        assert_eq!(k(None, "sock#3", PlaceClass::Private), "cli (private)");
        assert_eq!(k(None, "web#1", PlaceClass::Private), "web (private)");
        assert_eq!(
            k(Some("discord:dm:42"), "Robin", PlaceClass::Private),
            "discord_dm (private)"
        );
        assert_eq!(
            k(Some("discord:channel:7"), "Robin", PlaceClass::Shared),
            "discord_channel (shared)"
        );
        assert_eq!(author_of(PlaceClass::Shared), "a person in a shared place");
    }

    #[test]
    fn the_seed_roles_are_the_specs_twelve_with_distinct_ids() {
        let ids: std::collections::BTreeSet<_> = SEED_ROLES.iter().map(|(id, _)| id).collect();
        assert_eq!(ids.len(), 12);
        assert!(
            !ids.contains(&"other"),
            "role.v1's no-match option is its own"
        );
    }
}
