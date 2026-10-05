//! The inbound point (M5 step 25a; design §2.4): `classify.v1` (CLASSIFY)
//! and `role.v1` (ROLE_GUESS) about every message a person sends, in
//! shadow. Both read the one `inbound` state, so they ride one request
//! (theseus-judge's batching), and the call's cost is split between their
//! two judgments by question count. Nothing acts on either in M5: routing a
//! message to a task is M7's, and the roles table and role switches are
//! 26c's. `route.v1` (25e) rides the same request, live: its verdict goes
//! back to the turn over a channel ([`RouteWait`]), and the call then waits
//! for an in-flight permit (`Urgency::Live`, under the client's whole-call
//! limit) instead of being shed. The turn waits for it beside its first
//! compile, never before (`turn::route_step`).
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
use theseus_judge::builders::{InboundInput, RoleInput, TaskInput};
use theseus_judge::{Ask, DecisionPoint, Input, Judge, Mode, Outcome, Pack, Urgency};
use theseus_kernel::Kernel;

use super::{sampled, spend, Built, JudgeService, ScrubWith};
use crate::config::PackMode;
use crate::node::{Body, Node, Origin};
use crate::places::PlaceClass;
use crate::trace::Trace;

/// CLASSIFY (§2.4), at `inbound`.
pub const CLASSIFY_PACK: &str = "classify.v1";
/// ROLE_GUESS (§2.4), at `inbound`, batched with `classify.v1`.
pub const ROLE_PACK: &str = "role.v1";
/// ROUTE (25e), at `inbound`, batched with `classify.v1`: live.
pub const ROUTE_PACK: &str = "route.v1";
/// The packs asked at `inbound`, in the order they are asked.
pub const PACKS: [&str; 3] = [CLASSIFY_PACK, ROLE_PACK, ROUTE_PACK];

/// What a turn waits on for `route.v1`'s verdict: `None` when Jev gave none
/// (a failure, a skip, model drift); a dropped sender (nothing sent, the
/// day's limit among it) reads the same, at once.
pub type RouteWait = tokio::sync::oneshot::Receiver<Option<crate::routing::Verdict>>;

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

/// What the turn hands the point once its input node is written.
pub struct Inbound {
    pub session_id: String,
    pub execution_id: String,
    pub turn_id: String,
    /// The input's node, which the state reads back with the session's.
    pub node_id: String,
    /// The input's text, read here only to tell a slash command.
    pub text: String,
    /// Where it came in (`cli`, `web`, `discord_dm`, …) and the place
    /// rule's answer for it, as [`place_kind`] says it.
    pub place_kind: String,
    /// `operator` in a private place; a shared place's person is not named.
    pub author: String,
    /// The live tasks are the kernel's.
    pub kernel: Arc<Kernel>,
    /// `route.v1`'s mode for this turn, as the turn decides it (`[routing]`,
    /// the judge's ladder, a pin): off, shadow, or live.
    pub route: PackMode,
    /// The profile the owner chose for this turn (a pin), for the record.
    pub chosen: Option<String>,
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
        for (pack, id, mode) in &packs {
            trace.mark(
                "judge",
                "mark",
                json!({"pack": pack.name(), "point": "inbound", "mode": mode.as_str(), "judgment": id}),
            );
        }
        let (tx, rx) = match packs.iter().any(|(p, _, _)| p.name() == ROUTE_PACK) {
            true => {
                let (tx, rx) = tokio::sync::oneshot::channel();
                (Some(tx), Some(rx))
            }
            false => (None, None),
        };
        rt.spawn(judge_inbound(self.me.clone(), packs, msg, tx));
        rx
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
        let input = self.inbound_input(msg)?;
        let built = self
            .built()
            .map_err(|e| tracing::warn!(error = %format!("{e:#}"), "judge: the Jev client was not built"))
            .ok()?;
        let scrub = ScrubWith(self.scrubber.clone());
        let input = Input::Inbound(input);
        let mut asks = Vec::with_capacity(packs.len());
        for (pack, id, mode) in packs {
            let state = theseus_judge::prepare(&pack, &input, &scrub).ok()?;
            // The packs share the state, so this is one blob, which the sink
            // writes before their rows (theseus-otny).
            let blob = self.stage_blob(state.state.json.as_bytes());
            let baseline = match pack.name().as_str() {
                CLASSIFY_PACK => "conversation",
                ROUTE_PACK => "session_profile",
                _ => "current_role",
            };
            let mut context = json!({
                "session": msg.session_id, "execution": msg.execution_id, "turn": msg.turn_id,
                "node": msg.node_id, "baseline": baseline, "place_kind": msg.place_kind,
                "blob": blob, "on_path_ms": 0,
            });
            if pack.name() == ROUTE_PACK {
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
        let need = built.judge.inner().reserve_micros(&asks).unwrap_or(0);
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
    let text = match message.map(|n| &n.body) {
        Some(Body::UserMessage { text, .. }) => text.clone(),
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
        current_role: None,
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

/// `route.v1`'s verdict from its judgment: its `mode` answer, when it was
/// answered by the model the pack pins.
pub fn verdict(j: &theseus_judge::Judgment, turn: &str) -> Option<crate::routing::Verdict> {
    if !j.actionable() {
        return None;
    }
    match &j.answer("mode")?.answer {
        theseus_judge::Answer::Choice {
            choice, confidence, ..
        } => Some(crate::routing::Verdict {
            mode: choice.clone(),
            confidence: *confidence,
            judgment: j.id.clone(),
            turn: turn.to_string(),
        }),
        _ => None,
    }
}

/// One inbound decision point, in its own task: every pack's ask in one
/// `judge` call, which batches them into one request. The service is held
/// only around the blocking half, never across the call. A live `route.v1`
/// waits for a permit rather than being shed, under the whole-call limit.
async fn judge_inbound(
    me: Weak<JudgeService>,
    packs: Vec<(Arc<Pack>, String, PackMode)>,
    msg: Inbound,
    route: Option<tokio::sync::oneshot::Sender<Option<crate::routing::Verdict>>>,
) {
    let today = spend::local_day(theseus_protocol::now_unix_ms());
    let Some(svc) = me.upgrade() else { return };
    let urgency = match packs.iter().any(|(_, _, m)| *m >= PackMode::Canary) {
        true => Urgency::live(Duration::from_secs(svc.cfg.total_secs)),
        false => Urgency::Shadow,
    };
    let turn = msg.turn_id.clone();
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
    let judgments = built.judge.judge(DecisionPoint { asks, urgency }).await;
    tracing::debug!(
        ms = t0.elapsed().as_millis() as u64,
        "judge: the inbound packs judged"
    );
    if let Some(tx) = route {
        let v = judgments
            .iter()
            .find(|j| j.pack == ROUTE_PACK)
            .and_then(|j| verdict(j, &turn));
        let _ = tx.send(v);
    }
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
