//! The turn's floor on content flags (theseus-9ymz0, design §4.2b, §4.3 and
//! §11's tests 21 to 23): a turn bound for a model of restricted origin runs
//! on it when its session's labels allow it; it waits for the asks still out, at
//! most `[content_based_security] wait_ms`, then goes on the labels it reads; a turn bound for Claude never waits. The
//! rig is `tests_route`'s: `anthropic` (Claude) and `zai` (GLM, a China-origin
//! endpoint), each a fake that records its requests; `judging` stands a test's
//! judge behind the runner.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use theseus_judge::fake::FakeJev;
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::content::{ContentFlags, Flag};
use crate::content_wait::judging::{Judging, Silent};
use crate::provider::Scripted;
use crate::rpc::Core;
use crate::session::SessionRecord;
use crate::tests_dm_rig::{blocked, judging_with};
use crate::tests_route::{
    decided, mode, rig, rig_parts, turn, Rig,
};
use crate::turn::TurnRequest;

const PLAIN: &str = "Rename the call sites in the billing module.";

/// Test 22: a turn bound for Claude never waits, however long `wait_ms` is and
/// whatever the judge is doing: the notify is not asked, the turn is not
/// slowed, and Claude is sent it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_claude_turn_never_waits() {
    let r = rig(None, 2, |c| c.content_based_security.wait_ms = 60_000);
    let silent = Arc::new(Silent::default());
    r.core.runner.verdicts.set(silent.clone());
    let started = Instant::now();
    let res = turn(&r.core, None, PLAIN, None).await;
    assert!(started.elapsed() < Duration::from_secs(30), "it waited");
    assert_eq!(res.profile, "sonnet", "{:?}", res.route);
    assert!(
        silent.waits.lock().unwrap().is_empty(),
        "the notify was asked"
    );
    assert_eq!(r.claude.requests().len(), 1);
    assert!(r.zai.requests().is_empty());
}

/// A label the judge finds keeps the turn off the restricted model, with the
/// flag's word on the route row and the result: the same turn that a clean
/// judge would have let through.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_label_found_before_the_turn_moves_it_to_claude() {
    let r = rig(None, 2, |_| {});
    judging_with(
        &r,
        Judging::with(|_, text| {
            if text.contains("Examplecorp-roadmap") {
                vec![Flag::Confidential]
            } else {
                vec![]
            }
        }),
    );
    let res = turn(
        &r.core,
        None,
        "Summarise the Examplecorp-roadmap for the board.",
        Some("glm"),
    )
    .await;
    assert_eq!(res.profile, "default", "{:?}", res.route);
    let rows = decided(&r.core.store);
    assert_eq!(rows.last().unwrap()["reason"], "data_gate", "{rows:?}");
    assert_eq!(
        rows.last().unwrap()["data_gate"],
        "confidential",
        "{rows:?}"
    );
    let data = res.route.unwrap().data.unwrap();
    assert_eq!(
        (data.cn.as_str(), data.gate.as_deref()),
        ("confidential", Some("confidential"))
    );
    assert!(r.zai.requests().is_empty());
    assert_eq!(blocked(&r.core), 0, "it never met the chokepoint");
    let flags = crate::content_wait::flags_of(&r.core.store, &res.session_id).unwrap();
    assert!(flags.has(Flag::Confidential));
}

/// Through a whole turn: a session's stored labels are the same after a plain
/// turn, which writes nothing new of them (ported from the open view's
/// `a_plain_turn_keeps_the_sessions_data_state`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_plain_turn_keeps_the_sessions_labels() {
    use crate::content::{ItemKind, Label};
    let r = rig(None, 1, |_| {});
    let mut flags = ContentFlags::default();
    flags.raise(Label {
        flag: Flag::Confidential,
        node: Some("nod_earlier".into()),
        kind: ItemKind::ToolResult,
        source: "fs.read /srv/notes.txt".into(),
        confidence: 0.8,
        band: theseus_judge::band::Band::Confirm,
        at_ms: 1,
        judgment: String::new(),
    });
    let mut rec = SessionRecord::new(SessionKind::Conversation, None);
    rec.data = Some(Box::new(flags.clone()));
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    let res = turn(&r.core, Some(&rec.session_id), "Say done.", None).await;
    let after = crate::tests_route::session(&r.core, &res.session_id);
    assert_eq!(after.turns, 1, "the turn ran");
    assert_eq!(after.data.as_deref(), Some(&flags));
}

/// A turn on the live profile, run by the runner, whose failure the test
/// reads.
async fn try_turn(core: &Arc<Core>, input: &str) -> anyhow::Result<TurnSubmitResult> {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    try_turn_in(core, rec, input).await
}

/// A session record carrying a `confidential` label, as a names hit leaves it,
/// stored.
fn labelled(core: &Core) -> SessionRecord {
    let mut flags = ContentFlags::default();
    flags.raise(crate::content::deterministic::label(
        Flag::Confidential,
        "nod_earlier",
        crate::content::ItemKind::ToolResult,
        "name: Examplecorp".into(),
        1,
    ));
    let mut rec = SessionRecord::new(SessionKind::Conversation, None);
    rec.data = Some(Box::new(flags));
    core.store.put_session(&rec.session_id, &rec).unwrap();
    rec
}

/// `try_turn` on `rec`, which is already stored.
async fn try_turn_in(
    core: &Arc<Core>,
    rec: SessionRecord,
    input: &str,
) -> anyhow::Result<TurnSubmitResult> {
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = EventSink::new(core.bus.clone(), &rec.session_id, None);
    core.runner
        .run(TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
}

/// With the default itself restricted and a label on the session there is
/// nowhere to run: the turn fails before any model call, routed or not, and
/// both fakes record nothing. (Without a label it runs on the restricted
/// default: only a label keeps a session off a model, theseus-gyjyo.6.)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_usable_fallback_fails_the_turn_and_sends_nothing() {
    for routed in [true, false] {
        let jev = FakeJev::start().unwrap();
        mode(&jev, "chat", 0.95);
        // After the load's check, which refuses the owner-material keys'
        // profiles on the default's provider once it is restricted.
        let cn = |p: &mut crate::rpc::Parts| p.cfg.serve_default_from_cn();
        let r = rig_parts(routed.then_some(&jev), 1, |_| {}, cn);
        let rec = labelled(&r.core);
        let ended = try_turn_in(&r.core, rec, PLAIN).await;
        let said = format!("{ended:?}");
        assert!(
            said.contains("No model cleared for this request's data is usable"),
            "routed {routed}: {said}"
        );
        assert!(said.contains("data_gate"), "{said}");
        assert!(r.claude.requests().is_empty() && r.zai.requests().is_empty());
    }
}

/// A named message runs on a Claude served from a host vouched EU: the names'
/// floor is gone (theseus-gyjyo.4), and a model of EU origin is not restricted.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_named_request_runs_on_a_model_of_eu_origin() {
    let named = "Rename the call sites in Examplecorp's billing module.";
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    let r = rig(Some(&jev), 1, |c| {
        c.privacy.sensitive_names = vec!["Examplecorp".into()];
        c.serve_default_from_eu();
    });
    let origin = r
        .core
        .runner
        .cfg
        .profile_origin("default", &r.core.runner.catalog);
    assert_eq!(origin, crate::catalog::ModelOrigin::Eu, "unrestricted");
    let said = format!("{:?}", try_turn(&r.core, named).await);
    assert!(!said.contains("sensitive_names"), "{said}");
    assert_eq!(r.claude.requests().len(), 1, "{said}");
    assert!(r.zai.requests().is_empty());
}

/// A refusal is never retried into a send: the driver's rule parks a
/// `data_gate` failure on input at once, settled or not, so its one retry of a
/// lasting class (`internal` alone) never asks the model again.
#[test]
fn a_data_gate_failure_is_parked_never_retried() {
    use crate::session::{Failing, Then};
    for settled in [true, false] {
        let (_, then, _) = Failing::after(None, "data_gate", false, settled, 0, true);
        assert_eq!(then, Then::Park, "settled {settled}");
    }
}

// ----------------------------------------------- theseus-sh1n3: the two sets
//
// Every test above stands a judge behind `verdicts` that judges straight from
// the transcript, so it lands a verdict for a node **no hook site ever
// offered**. That is more generous than the running system, and it is why
// theseus-9ymz0 shipped with a gap between the set the floor blocked on and
// the set that is submitted. The floor blocks on nothing now (theseus-gyjyo.7:
// it waits on a counter and goes on the labels), so a node in the gap no
// longer wedges a session; it is **never asked about**, which is a label never
// raised, and the guard on it is [`every_item_a_turn_writes_is_offered_for_judgment`].
//
// These use `Judging::faithful`, which judges only what a site submitted.

/// The judge of `r`, faithful to what the hook sites actually submit.
fn faithful(r: &Rig) -> Arc<Judging> {
    let j = judging_with(r, Judging::clean());
    j.faithful(&r.core.runner.judge);
    j
}

/// A turn whose request carries a deadline, as `theseus ask --deadline` does.
async fn turn_with_deadline(
    core: &Arc<Core>,
    session: Option<&str>,
    input: &str,
    chosen: Option<&str>,
    secs: u64,
) -> TurnSubmitResult {
    let rec = match session {
        Some(id) => crate::tests_route::session(core, id),
        None => {
            let r = SessionRecord::new(SessionKind::Conversation, None);
            core.store.put_session(&r.session_id, &r).unwrap();
            r
        }
    };
    let arrived = Instant::now();
    crate::turn::clock_step::set_deadline(&rec.session_id, arrived, Some(secs));
    let (live, _) = core.live_profile();
    let mut target = core
        .runner
        .resolve_target(&live, chosen, None, None)
        .unwrap();
    target.chosen = chosen.map(|p| format!("profile {p}"));
    let sink = EventSink::new(core.bus.clone(), &rec.session_id, None);
    core.runner
        .run(TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "test".into(),
            recompile: None,
            attachments: Vec::new(),
            arrived: Some(arrived),
            reply_to: None,
            answer_at: None,
        })
        .await
        .unwrap()
}

/// **Repro 1, the turn clock** (theseus-sh1n3; found by the cell-8 bench
/// smoke, 2026-10-27). On a build carrying theseus-9ymz0 these two differed
/// only in one flag, and only the second was refused:
///
/// ```text
/// theseus ask --follow-for 0 -                 ->  the cn model answers
/// theseus ask --follow-for 0 --deadline 840 -  ->  data_gate, no model call
/// ```
///
/// A turn given a deadline appends `[The harness: N minutes left of this
/// turn's M.]`, author `harness:clock`, at its first loop. It was a user-role
/// node, so the floor counted it; nothing submits a harness-appended node, so
/// it was never judged, and the session never reached a model of restricted
/// origin again. The clock's words are the harness's own and are no item.
///
/// **The first assertion passes for a different reason now**: the floor no
/// longer counts items, so the session reaches the model whatever the clock
/// node is. What still bites is the last: the clock line is not an item, so
/// nothing offers it to the judge.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_deadline_does_not_take_the_session_off_the_restricted_model() {
    let r = rig(None, 2, |c| c.content_based_security.wait_ms = 300);
    faithful(&r);
    let with = turn_with_deadline(&r.core, None, PLAIN, Some("glm"), 840).await;
    assert_eq!(
        with.profile, "glm",
        "the same ask with --deadline must reach the same model: {:?}",
        with.route
    );
    assert_eq!(r.zai.requests().len(), 1, "GLM was sent it");
    assert_eq!(blocked(&r.core), 0, "nothing met the chokepoint");
    // And the clock's node really was written, so the test is not vacuous...
    let nodes = r.core.store.transcript(&with.session_id).unwrap();
    let clock: Vec<&str> = nodes
        .iter()
        .filter(|(_, n)| n.author.as_deref() == Some(crate::turn::clock_step::CLOCK_AUTHOR))
        .map(|(_, n)| n.id.as_str())
        .collect();
    assert!(
        !clock.is_empty(),
        "the turn wrote no clock line, so this proves nothing"
    );
    // ...and the harness's own words are never offered to the judge.
    let submitted = r.core.runner.judge.content_submitted();
    assert!(
        clock.iter().all(|id| !submitted.iter().any(|s| s == id)),
        "the clock line was offered for judgment: {submitted:?}"
    );
}

/// **Repro 2, a `task.create` result** (theseus-sh1n3). Kimi's bench trial
/// failed deterministically twice: its first act was three `task.create`
/// calls, one Jev call went out for the whole trial, and the next turn was
/// refused `unjudged` with `wait_ms: 0` — it did not even wait. A harness
/// tool's result node was written by a site that queued nothing, while the
/// floor counted it like any other result.
///
/// Here the node is planted as that site wrote it — written, never offered —
/// and the session still reaches the restricted model, on `wait_ms` 300 and on
/// zero. **It passes because nothing counts items now** (theseus-gyjyo.7): the
/// sweep that used to queue the node is gone, so the other side is asserted
/// too, and it is the price: the node is never asked about, and no row or
/// label comes of it. The site itself is held to its queueing by
/// [`every_item_a_turn_writes_is_offered_for_judgment`].
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_item_no_site_submitted_wedges_nothing_and_is_never_asked() {
    for wait_ms in [300, 0] {
        let r = rig(None, 2, |c| c.content_based_security.wait_ms = wait_ms);
        faithful(&r);
        let one = turn(&r.core, None, PLAIN, Some("glm")).await;
        assert_eq!(one.profile, "glm", "{:?}", one.route);
        let node = planted_result(&one.session_id);
        r.core.store.append(&[node.record().unwrap()]).unwrap();
        let two = turn(&r.core, Some(&one.session_id), "And again.", Some("glm")).await;
        assert_eq!(two.profile, "glm", "wait_ms {wait_ms}: {:?}", two.route);
        assert_eq!(r.zai.requests().len(), 2);
        assert!(
            !r.core.runner.judge.content_submitted().contains(&node.id),
            "something offered the planted result: the sweep is back"
        );
    }
}

/// The node a harness tool's completion writes, as `harness_done` writes one
/// and as nothing queued it before this row.
fn planted_result(session: &str) -> crate::node::Node {
    crate::node::Node::tool_result(
        session,
        None,
        None,
        crate::node::Body::ToolResult {
            tool_use_id: "tu_1".into(),
            tool: crate::task::CREATE.into(),
            status: crate::node::ResultStatus::Ok,
            is_error: false,
            content: "Started task ab12".into(),
            correlation_id: None,
            bytes_total: 0,
            truncated: false,
            full_ref: None,
            duration_ms: None,
            late: false,
            meta: serde_json::Value::Null,
            image: None,
            external: None,
        },
    )
}

/// **The invariant, over a real turn** (theseus-sh1n3): every node a turn
/// writes that is an item is a node some hook site offered for judgment.
/// Anything else is a node that is **never asked about** (theseus-gyjyo.7: no
/// sweep makes up for a site that forgets), and so a label never raised. This
/// is now the only guard on that gap.
///
/// The turn here calls `task.create`, a tool the **harness** runs itself
/// (`ToolRuntime::harness_done`) — the site that wrote a result node and
/// queued nothing, which is what failed Kimi's bench trial twice. It writes
/// the operator's message, a tool call, a result and the model's reply, so
/// the walk covers the ordinary kinds too.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_item_a_turn_writes_is_offered_for_judgment() {
    use crate::tests_tasks::{answers_a_call, arranged, parent_session, rig as task_rig, ASK};
    let r = task_rig(
        |req| {
            if answers_a_call(req) {
                return Scripted::text("Started it.");
            }
            Scripted::tools(
                "Starting it.",
                &[(
                    "t_task",
                    "task_create",
                    json!({"brief": "CHILD: count to three", "budget_usd": 1.5,
                           "arrangement": arranged()}),
                )],
            )
        },
        |_| {},
    );
    let sid = parent_session(&r.core);
    crate::test_support::turn(&r.core, &sid, ASK).await;
    let submitted = r.core.runner.judge.content_submitted();
    let nodes = r.core.store.transcript(&sid).unwrap();
    let missed: Vec<String> = nodes
        .iter()
        .filter(|(_, n)| crate::content::item_text(n).is_some())
        .filter(|(_, n)| !submitted.contains(&n.id))
        .map(|(_, n)| format!("{} {:?} {:?}", n.id, n.kind, n.author))
        .collect();
    assert!(
        missed.is_empty(),
        "these are items and no site submits them, so nothing is ever asked \
         about them and a label they earn is never raised: {missed:#?}"
    );
    // Not vacuous: the harness tool's result really is in the transcript.
    assert!(
        nodes.iter().any(|(_, n)| matches!(&n.body,
            crate::node::Body::ToolResult { tool, .. } if tool == crate::task::CREATE)),
        "no task.create result was written"
    );
}

/// A turn pinned to GLM whose session has an ask still out waits `wait_ms` for
/// it, and then **goes**: the label check at the end is the whole decision
/// (theseus-gyjyo.6). Nothing says the session is clean, and nothing says it
/// is not; no label is on it, so the request is sent. The route row says
/// `allowed`, never `unjudged`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cn_turn_waits_for_the_outstanding_ask_then_goes() {
    let r = rig(None, 2, |c| c.content_based_security.wait_ms = 300);
    let silent = Arc::new(Silent::default());
    r.core.runner.verdicts.set(silent.clone());
    let started = Instant::now();
    let res = turn(&r.core, None, PLAIN, Some("glm")).await;
    assert!(started.elapsed() >= Duration::from_millis(300), "it waited");
    assert_eq!(res.profile, "glm", "{:?}", res.route);
    assert_eq!(r.zai.requests().len(), 1, "GLM was sent it");
    assert_eq!(blocked(&r.core), 0);
    assert!(!silent.waits.lock().unwrap().is_empty());
}

/// `wait_ms = 0` never waits, and the turn goes on the labels it finds.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_zero_wait_goes_on_the_labels_at_once() {
    let r = rig(None, 2, |c| c.content_based_security.wait_ms = 0);
    let silent = Arc::new(Silent::default());
    r.core.runner.verdicts.set(silent.clone());
    let res = turn(&r.core, None, PLAIN, Some("glm")).await;
    assert_eq!(res.profile, "glm", "{:?}", res.route);
    assert!(silent.waits.lock().unwrap().is_empty(), "it never waited");
}

/// `cn_models = "never"`: the route skips every profile of restricted origin
/// whatever the session carries, so a pin of GLM runs on the `[model]` default
/// with `data_gate`, and the chokepoint is never asked.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cn_models_never_skips_the_restricted_profile_in_the_route() {
    use crate::config::content_based_security::CnModels;
    let r = rig(None, 2, |c| {
        c.content_based_security.cn_models = CnModels::Never;
    });
    judging_with(&r, Judging::clean());
    let res = turn(&r.core, None, PLAIN, Some("glm")).await;
    assert_eq!(res.profile, "default", "{:?}", res.route);
    let rows = decided(&r.core.store);
    assert_eq!(rows.last().unwrap()["reason"], "data_gate", "{rows:?}");
    assert_eq!(rows.last().unwrap()["data_gate"], "never", "{rows:?}");
    assert!(r.zai.requests().is_empty());
    assert_eq!(
        blocked(&r.core),
        0,
        "the route skipped it; nothing met the chokepoint"
    );
}

/// `cn_models = "always"`: a labelled session's turn runs on GLM. Synthetic
/// content only.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cn_models_always_runs_a_labelled_session_on_the_restricted_profile() {
    use crate::config::content_based_security::CnModels;
    let r = rig(None, 2, |c| {
        c.content_based_security.cn_models = CnModels::Always;
    });
    let mut flags = ContentFlags::default();
    flags.raise(crate::content::deterministic::label(
        Flag::Confidential,
        "nod_earlier",
        crate::content::ItemKind::ToolResult,
        "name: Examplecorp".into(),
        1,
    ));
    let mut rec = SessionRecord::new(SessionKind::Conversation, None);
    rec.data = Some(Box::new(flags));
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    let res = turn(&r.core, Some(&rec.session_id), PLAIN, Some("glm")).await;
    assert_eq!(res.profile, "glm", "{:?}", res.route);
    assert_eq!(r.zai.requests().len(), 1);
    assert_eq!(blocked(&r.core), 0);
    let flags = crate::content_wait::flags_of(&r.core.store, &res.session_id).unwrap();
    assert!(flags.has(Flag::Confidential), "the label stands");
}
