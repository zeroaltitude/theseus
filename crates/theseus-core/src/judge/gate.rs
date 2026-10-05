//! `security.v1` and `security.v3` at `gate` (M5 step 24; design §2.4's
//! `security.v1` row, §2.8b, §5's Q2 and Q12): the operator's "this is
//! risky: 0-100%", asked of every call that acts, after the gate decides.
//!
//! - **Which calls** ([`judged`]): every call whose class is not `read`, and
//!   every `http.fetch` and `web.search` in a session that holds external
//!   text, the exfiltration path the hold leaves open by design (Q12).
//! - **Off the call's path.** `ToolRuntime::start` plans the call and sends
//!   its notice, then hands it here: on its path are the choice to judge and
//!   the marks, nothing else. The state's build (the session's nodes, the
//!   hold, the last calls, v3's recent reads), its scrub and cap, the blobs,
//!   the reservation, the call, and the record are the spawned task's. The
//!   gate never waits on it, so a judgment changes no call's treatment: in
//!   shadow it is recorded and shown, and nothing acts on it (Q2).
//! - **One decision point**, `security.v1` (the incumbent) and `security.v3`
//!   (the candidate: v2's questions with `steered` deciding beside `risky`).
//!   v2 stays unwired: its questions are v3's, and would be asked twice.
//! - **Ids minted here.** A judgment's id is the call's correlation id and
//!   its pack, hashed ([`judgment_id`]), so the trace's mark names the row's
//!   key before the row exists, and a "should have asked" press names the
//!   call's judgments from its correlation id alone ([`JudgeService::of_call`]),
//!   inside the sink's window as after it.
//! - **The record** names the call (its correlation id), its posture and
//!   why, and whether the hold made it wait, so the hold's asks can be set
//!   beside the scores (theseus-hnc8).
//! - **The score** of a notified call follows its notice: `security.v1`'s
//!   `risky`, as `judge.scored`, to the turn's clients (best effort, as live
//!   progress is; the `judge.call` row is the record).
//! - **A notice** follows an open call `security.v3`, live, answered
//!   `risky` in its act band (`notice`): posted once the judgments land,
//!   beside the running call, never on its path.

use std::sync::{Arc, Weak};
use std::time::Instant;

use serde_json::{json, Value};
use sha2::Digest;
use theseus_judge::builders::{CallOutcome, HoldInput, ReadInput, SecurityInput, ToolCallInput};
use theseus_judge::{Ask, DecisionPoint, Input, Judge, Outcome, Pack, Urgency};
use theseus_protocol::{ExternalText, Plan, ToolClass};
use theseus_store::Store as _;

use super::{spend, JudgeService, ScrubWith};
use crate::bus::EventSink;
use crate::config::PackMode;
use crate::node::{Body, Node, Origin, ResultStatus};

/// The incumbent: Eddie's `risky`, whose score the notice shows.
pub const SECURITY_PACK: &str = "security.v1";
/// The candidate: v2's state and questions, with `steered` deciding.
pub const SECURITY_CANDIDATE: &str = "security.v3";
/// The packs asked at `gate`, in the decision point's order.
pub const GATE_PACKS: [&str; 2] = [SECURITY_PACK, SECURITY_CANDIDATE];

/// The headline question whose probability a notice shows.
pub const RISKY: &str = "risky";

/// The session's calls before this one a state is built from: v1 shows the
/// newest five, v3 computes its facts over the tail.
const LAST_CALLS: usize = 20;
/// The reads v3's state may show (it keeps the newest three).
const RECENT_READS: usize = 6;
/// The excerpt of a read the builder is given (it clips further).
const READ_EXCERPT: usize = 2000;

/// Whether the gate's call is judged (Q12): any call whose class is not
/// `read`, and a fetch or a search in a session that holds external text.
/// `holds` is read only for a fetch or a search.
pub fn judged(class: ToolClass, tool: &str, holds: impl FnOnce() -> bool) -> bool {
    class != ToolClass::Read || (crate::web::NAMES.contains(&tool) && holds())
}

/// A judgment's id at `gate`: the pack and the call's correlation id,
/// hashed, so a press finds it from the call alone.
pub fn judgment_id(pack: &str, correlation_id: &str) -> String {
    let mut h = sha2::Sha256::new();
    h.update(pack.as_bytes());
    h.update([0]);
    h.update(correlation_id.as_bytes());
    let d = h.finalize();
    let hex: String = d[..16].iter().map(|b| format!("{b:02x}")).collect();
    format!("jdg_{hex}")
}

/// What the gate decided about one call, as the judgment's state and record
/// need it. Built on the call's path from what the gate already holds.
#[derive(Debug, Clone)]
pub struct GateCall {
    pub session_id: String,
    pub execution_id: String,
    pub turn_id: String,
    pub loop_index: Option<u32>,
    pub task: bool,
    pub correlation_id: String,
    pub tool_use_id: String,
    pub tool: String,
    pub class: ToolClass,
    /// The gate's posture (`open`, `notify`, `approve`) and why.
    pub posture: String,
    pub reason: String,
    pub floor: bool,
    /// A notice was sent for it: its score follows on `judge.scored`.
    pub notified: bool,
    /// The hold raised its posture (the decision names the hold).
    pub hold_raised: bool,
    pub input: Value,
    pub plan: Plan,
}

impl GateCall {
    /// The hold made it wait: the hold raised its posture, to `approve`.
    pub fn waited_on_hold(&self) -> bool {
        self.hold_raised && self.posture == "approve"
    }
}

/// One dispatch's mark on the turn's trace: a zero-length `judge` span.
#[derive(Debug, Clone)]
pub struct Mark {
    pub at: Instant,
    pub attrs: Value,
}

/// The marks as zero-length `judge` spans of kind `mark`, under the call's
/// span in the turn's trace; `at` places an instant on the trace's clock.
pub fn marks(judged: &[Mark], at: impl Fn(Instant) -> u64) -> Vec<theseus_protocol::Span> {
    judged
        .iter()
        .map(|m| {
            let t = at(m.at);
            theseus_protocol::Span {
                name: "judge".into(),
                kind: "mark".into(),
                start_us: t,
                end_us: Some(t),
                attrs: m.attrs.clone(),
                children: Vec::new(),
            }
        })
        .collect()
}

impl JudgeService {
    /// Any pack at `gate` is on: false with the judge off, so the call's
    /// path reads nothing more.
    pub fn gate_on(&self) -> bool {
        GATE_PACKS.iter().any(|p| self.pack_on(p))
    }

    /// `security.v3` is live as notices in `session`, as the ladder gives it
    /// there (26a): its judgments are marked and recorded `live`, and the
    /// gate hands it the turn's clients.
    pub fn notices_live(&self, session: &str) -> bool {
        let name = self.placed(SECURITY_CANDIDATE, session);
        self.mode_for(&name, session).mode == PackMode::Live
    }

    /// A gated call, planned and its notice sent: each gate pack that is on
    /// and samples it gets a judgment, minted and marked here and made in a
    /// task of its own. Returns at once with the marks, whatever Jev does.
    pub fn at_gate(&self, call: GateCall, sink: Option<EventSink>) -> Vec<Mark> {
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            return Vec::new();
        };
        let mut asks = Vec::new();
        let mut marks = Vec::new();
        for name in GATE_PACKS {
            // The version standing in the root's place (25f); the
            // judgment's id stays the root's, so a press finds it.
            let placed = self.placed(name, &call.session_id);
            let given = self.mode_for(&placed, &call.session_id);
            if !given.on() {
                continue;
            }
            let Some(pack) = self.pack(&placed) else {
                continue;
            };
            if !super::sampled(&call.correlation_id, self.cfg.sample_of(name, pack.sample)) {
                continue;
            }
            let id = judgment_id(name, &call.correlation_id);
            marks.push(Mark {
                at: Instant::now(),
                attrs: json!({"pack": placed, "point": "gate", "mode": super::mark::mode_str(given.judge_mode()),
                    "judgment": id, "call": call.correlation_id}),
            });
            asks.push((pack, id, given));
        }
        if asks.is_empty() {
            return marks;
        }
        self.pending_insert(asks.iter().map(|(_, id, _)| id.clone()));
        rt.spawn(judge_gate(self.me.clone(), call, asks, sink));
        marks
    }

    /// The judgments made of a call at `gate`, by pack: those whose rows are
    /// written, and those still in the sink's window.
    pub fn of_call(&self, correlation_id: &str) -> Vec<(&'static str, String)> {
        GATE_PACKS
            .iter()
            .map(|p| (*p, judgment_id(p, correlation_id)))
            .filter(|(_, id)| {
                self.pending_has(id)
                    || self
                        .store
                        .inner()
                        .latest_by_key(theseus_store::kinds::LEDGER, id)
                        .is_ok_and(|r| r.is_some())
            })
            .collect()
    }

    /// "Should have asked" on a call (`theseus policy tighten TOOL --call
    /// ID`, and the web UI's and Discord's press): a `judge.label` row for
    /// each of the call's judgments, `risky` true at the operator's weight
    /// (1.0), keyed `lbl_<id>` and scoped `judge:security`, for the press's
    /// frame. None when the call was not judged.
    pub fn press_labels(
        &self,
        correlation_id: &str,
        session_id: Option<&str>,
        who: &str,
        via: &str,
    ) -> anyhow::Result<Vec<theseus_store::NewRecord>> {
        let mut out = Vec::new();
        for (pack, judgment) in self.of_call(correlation_id) {
            let id = crate::new_id("lbl");
            let label = crate::fact::judge::JudgeLabel {
                id: &id,
                judgment: &judgment,
                pack,
                question: Some(RISKY),
                about: None,
                label: json!(true),
                source: "operator",
                who,
                via,
                weight: 1.0,
                note: "should have asked",
                correlation_id: Some(correlation_id),
                rule: None,
            };
            let mut r = crate::fact::row(&label, session_id, None)?;
            r.key = Some(id);
            let scope = format!("judge:{}", pack.split('.').next().unwrap_or(pack));
            out.push(r.scoped(&scope));
        }
        Ok(out)
    }

    /// The blocking half before the call: the session's nodes, each pack's
    /// state and blob, and one reservation for the point.
    fn prepare_gate(
        &self,
        call: &GateCall,
        asks: Vec<(Arc<Pack>, String, super::ladder::Given)>,
        today: &str,
    ) -> Option<GatePrepared> {
        let nodes: Vec<Node> = self
            .store
            .session_nodes(&call.session_id)
            .map_err(|e| tracing::warn!(error = %format!("{e:#}"), "judge: the session's nodes were not read"))
            .ok()?
            .into_iter()
            .map(|(_, n)| n)
            .collect();
        let hold = crate::external::held(&self.store, &call.session_id)
            .ok()
            .flatten();
        let now = theseus_protocol::now_unix_ms();
        let v1 = input(call, &nodes, hold.as_ref(), now);
        let scrub = ScrubWith(self.scrubber.clone());
        let built = self
            .built()
            .map_err(|e| tracing::warn!(error = %format!("{e:#}"), "judge: the Jev client was not built"))
            .ok()?;
        let mut out = Vec::new();
        for (pack, id, given) in asks {
            let i = match pack.builder {
                theseus_judge::pack::Builder::Security => Input::Security(v1.clone()),
                _ => Input::Security2(SecurityInput {
                    recent_reads: recent_reads(&nodes, &call.tool_use_id),
                    ..v1.clone()
                }),
            };
            let Ok(state) = theseus_judge::prepare(&pack, &i, &scrub) else {
                continue;
            };
            let Ok(blob) = self.store.blobs().put(state.state.json.as_bytes()) else {
                tracing::warn!("judge: the state's blob was not written; not judged");
                continue;
            };
            let mut context = json!({
                "session": call.session_id, "execution": call.execution_id, "turn": call.turn_id,
                "loop": call.loop_index, "call": call.correlation_id, "tool_use_id": call.tool_use_id,
                "tool": call.tool, "tool_class": call.class.as_str(), "posture": call.posture,
                "posture_reason": call.reason, "floor": call.floor, "notified": call.notified,
                "hold": hold.is_some(), "hold_raised": call.hold_raised,
                "waited_on_hold": call.waited_on_hold(),
                "baseline": "posture", "decision": call.posture,
                "class": if call.task { "task" } else { "tools" },
                "blob": blob, "on_path_ms": 0, "root": self.root_of(&pack.name()),
            });
            // The ladder's arm in the context, and the mode `at_gate` gave the
            // call (26a): live, canary in a canary's arm, else shadow.
            context["pack_arm"] = json!(given.arm.as_str());
            let mut ask = Ask::new(pack, &state, given.judge_mode(), context);
            ask.id = Some(id);
            out.push(ask);
        }
        if out.is_empty() {
            return None;
        }
        let need = built.judge.inner().reserve_micros(&out).unwrap_or(0);
        self.reserve(today, need).then_some(GatePrepared {
            built,
            asks: out,
            need,
        })
    }
}

struct GatePrepared {
    built: Arc<super::Built>,
    asks: Vec<Ask>,
    need: theseus_judge::price::Micros,
}

/// One call's judgments, in their own task. The service is held only around
/// the blocking halves, never across the call.
async fn judge_gate(
    me: Weak<JudgeService>,
    call: GateCall,
    asks: Vec<(Arc<Pack>, String, super::ladder::Given)>,
    sink: Option<EventSink>,
) {
    let today = spend::local_day(theseus_protocol::now_unix_ms());
    let Some(svc) = me.upgrade() else { return };
    let ids: Vec<String> = asks.iter().map(|(_, id, _)| id.clone()).collect();
    let (day, c) = (today.clone(), call.clone());
    let s = svc.clone();
    let prepared = tokio::task::spawn_blocking(move || s.prepare_gate(&c, asks, &day))
        .await
        .ok()
        .flatten();
    let Some(GatePrepared { built, asks, need }) = prepared else {
        // Nothing went out, so no row will come.
        svc.pending_remove(&ids);
        return;
    };
    let sent: Vec<String> = asks.iter().filter_map(|a| a.id.clone()).collect();
    svc.pending_remove(
        &ids.into_iter()
            .filter(|i| !sent.contains(i))
            .collect::<Vec<_>>(),
    );
    drop(svc);
    let t0 = Instant::now();
    let judgments = built
        .judge
        .judge(DecisionPoint {
            asks,
            urgency: Urgency::Shadow,
        })
        .await;
    tracing::debug!(
        ms = t0.elapsed().as_millis() as u64,
        tool = %call.tool,
        "judge: a gated call judged"
    );
    let Some(svc) = me.upgrade() else { return };
    // Each judgment settles its share of the point's reservation (the last
    // takes what is left); a call whose usage is unknown is booked at its
    // share, conservatively.
    let mut left = need;
    let n = judgments.len();
    for (k, j) in judgments.iter().enumerate() {
        let reserved = match k + 1 == n {
            true => left,
            false => j.reserve_micros.unwrap_or(0).min(left),
        };
        left -= reserved;
        let (called, failed, unknown) = match &j.outcome {
            Outcome::Answered => (true, false, false),
            Outcome::Failed { usage_unknown, .. } => (true, true, *usage_unknown),
            Outcome::Skipped { .. } => (false, false, false),
        };
        let spent = j.cost_micros.unwrap_or(if unknown { reserved } else { 0 });
        svc.budget.settle(&today, reserved, spent, called, failed);
    }
    if let Some(n) = super::notice::flagged(&call, &judgments) {
        // Its frame and post are the blocking half; the call ran already.
        let (s, c, k) = (svc.clone(), call.clone(), sink.clone());
        let _ = tokio::task::spawn_blocking(move || s.post_notice(&c, &n, k.as_ref())).await;
    }
    if !call.notified {
        return;
    }
    let Some(sink) = sink else { return };
    let scored = judgments
        .iter()
        .filter(|j| j.pack == SECURITY_PACK && j.outcome == Outcome::Answered)
        .find_map(|j| Some((j, j.answer(RISKY)?.band.value)));
    if let Some((j, risky)) = scored {
        crate::fact::To::Sink(&sink).tell(&crate::fact::judge::JudgeScored {
            scored: &theseus_protocol::judge::JudgeScored {
                session_id: call.session_id.clone(),
                turn_id: call.turn_id.clone(),
                tool_use_id: call.tool_use_id.clone(),
                correlation_id: call.correlation_id.clone(),
                tool: call.tool.clone(),
                pack: j.pack.clone(),
                judgment: j.id.clone(),
                mode: "shadow".into(),
                risky,
                percent: theseus_protocol::judge::percent(risky),
            },
        });
    }
}

/// `security.v1`'s input for the call, from its session's nodes: the call
/// (tool, class, posture and why, argv, paths, URL, other arguments), the
/// operator's last ask, the hold, and the calls before it.
pub fn input(
    call: &GateCall,
    nodes: &[Node],
    hold: Option<&ExternalText>,
    now_ms: u64,
) -> SecurityInput {
    let other = match &call.input {
        Value::Object(m) => {
            let rest: serde_json::Map<String, Value> = m
                .iter()
                .filter(|(k, _)| !matches!(k.as_str(), "argv" | "path" | "paths" | "url"))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            (!rest.is_empty()).then_some(Value::Object(rest))
        }
        Value::Null => None,
        v => Some(v.clone()),
    };
    SecurityInput {
        tool: call.tool.clone(),
        class: call.class.as_str().into(),
        posture: call.posture.clone(),
        posture_reason: (!call.reason.is_empty()).then(|| call.reason.clone()),
        argv: call.plan.argv.clone().unwrap_or_default(),
        paths: call
            .plan
            .resources
            .iter()
            .map(|r| r.path.display().to_string())
            .collect(),
        url: call.plan.url.clone(),
        other_args: other,
        operator_last_ask: last_ask(nodes, call.task),
        hold: hold.map(|h| HoldInput {
            tool: h.tool.clone(),
            host: reqwest::Url::parse(&h.url)
                .ok()
                .and_then(|u| u.host_str().map(str::to_string)),
            minutes_ago: now_ms.saturating_sub(h.since_ms) / 60_000,
        }),
        last_calls: last_calls(nodes, &call.tool_use_id),
        recent_reads: Vec::new(),
        // File provenance waits for v3's shadow data (Item 95).
        tainted_paths: Vec::new(),
    }
}

/// The operator's newest message (a task's brief, its first).
fn last_ask(nodes: &[Node], task: bool) -> Option<String> {
    let user = |n: &&Node| matches!(n.body, Body::UserMessage { .. });
    let ask = if task {
        nodes.iter().find(user)
    } else {
        nodes
            .iter()
            .rev()
            .filter(user)
            .find(|n| n.origin == Origin::Operator)
            .or_else(|| nodes.iter().rev().find(user))
    };
    match ask.map(|n| &n.body) {
        Some(Body::UserMessage { text, .. }) => Some(text.clone()),
        _ => None,
    }
}

/// The session's calls before this one, oldest first, the newest
/// [`LAST_CALLS`], each with its result's outcome.
fn last_calls(nodes: &[Node], this: &str) -> Vec<ToolCallInput> {
    let calls: Vec<(&String, &String, &Value)> = nodes
        .iter()
        .filter_map(|n| match &n.body {
            Body::ToolCall {
                tool_use_id,
                tool,
                input,
                ..
            } if tool_use_id != this => Some((tool_use_id, tool, input)),
            _ => None,
        })
        .collect();
    let from = calls.len().saturating_sub(LAST_CALLS);
    calls[from..]
        .iter()
        .map(|(id, tool, input)| {
            let status = nodes.iter().rev().find_map(|r| match &r.body {
                Body::ToolResult {
                    tool_use_id,
                    status,
                    ..
                } if tool_use_id == *id => Some(*status),
                _ => None,
            });
            let (outcome, error_class) = match status {
                Some(ResultStatus::Ok) => (CallOutcome::Ok, None),
                Some(ResultStatus::Error) => (CallOutcome::Error, Some("error".into())),
                Some(ResultStatus::Unknown) => (CallOutcome::Error, Some("unknown".into())),
                Some(ResultStatus::Declined) => (CallOutcome::Held, None),
                Some(ResultStatus::Cancelled) => (CallOutcome::Error, Some("cancelled".into())),
                Some(ResultStatus::Background) | None => (CallOutcome::Running, None),
            };
            ToolCallInput {
                tool: (*tool).clone(),
                args: (*input).clone(),
                outcome,
                error_class,
            }
        })
        .collect()
}

/// What the session read lately, for v3: each file or page whose text its
/// model saw (a read's result), by its path or URL, oldest first.
fn recent_reads(nodes: &[Node], this: &str) -> Vec<ReadInput> {
    let mut reads: Vec<ReadInput> = Vec::new();
    for n in nodes {
        let Body::ToolResult {
            tool_use_id,
            tool,
            status: ResultStatus::Ok,
            content,
            ..
        } = &n.body
        else {
            continue;
        };
        if tool_use_id == this || !matches!(tool.as_str(), "fs.read" | "http.fetch" | "web.search")
        {
            continue;
        }
        let input = nodes.iter().find_map(|c| match &c.body {
            Body::ToolCall {
                tool_use_id: id,
                input,
                ..
            } if id == tool_use_id => Some(input),
            _ => None,
        });
        let source = input
            .and_then(|i| {
                ["path", "url", "query"]
                    .iter()
                    .find_map(|k| i.get(*k)?.as_str())
            })
            .unwrap_or(tool.as_str())
            .to_string();
        let excerpt: String = content.chars().take(READ_EXCERPT).collect();
        reads.push(ReadInput { source, excerpt });
    }
    let from = reads.len().saturating_sub(RECENT_READS);
    reads.split_off(from)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Q12: an acting call is judged whatever the hold; a fetch or a search
    /// only in a holding session, and the hold is read for nothing else.
    #[test]
    fn the_gate_judges_acting_calls_and_held_fetches() {
        let never = || -> bool { panic!("the hold is read only for a fetch or a search") };
        assert!(judged(ToolClass::Run, "proc.run", never));
        assert!(judged(ToolClass::Write, "fs.write", never));
        assert!(!judged(ToolClass::Read, "fs.read", never));
        assert!(judged(ToolClass::Read, "web.search", || true));
        assert!(!judged(ToolClass::Read, "web.search", || false));
        assert!(judged(ToolClass::Read, "http.fetch", || true));
    }

    #[test]
    fn a_judgments_id_is_its_pack_and_call() {
        let a = judgment_id(SECURITY_PACK, "act_1");
        assert_eq!(a, judgment_id(SECURITY_PACK, "act_1"));
        assert_ne!(a, judgment_id(SECURITY_CANDIDATE, "act_1"));
        assert_ne!(a, judgment_id(SECURITY_PACK, "act_2"));
        assert!(a.starts_with("jdg_") && a.len() == 36, "{a}");
    }
}
