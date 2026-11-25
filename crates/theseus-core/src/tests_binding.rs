//! No drift inside a turn (issue 20, "A turn on the newest model fails at its
//! second step"): what a turn sent at one step is what it sends at the next,
//! and anything new goes after it. A thinking block is bound to everything
//! before it, and `claude-fable-5-2` refuses a request that replays one after
//! a change there with a 400, where an older model reports it and goes on.
//! The stand-in provider carries that rule (`provider::binding`), so each test
//! runs a whole turn of two or more steps and reads what the provider would.
//!
//! What the harness says beside the person's message (the date and the git
//! line, the task graph, the wakes, the surface's records: `turn::now_step`)
//! is one block, composed at the turn's first call, laid before the person's
//! message and frozen for the turn (issue 46): no request appends anything to
//! a message an earlier answer followed, so nothing is kept or put back.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::provider::binding::unmarked;
use crate::provider::{Binding, FakeProvider, ProviderRequest, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

struct Rig {
    core: Arc<Core>,
    fake: Arc<FakeProvider>,
    _dir: tempfile::TempDir,
}

fn rig(mode: Binding, script: Vec<Scripted>) -> Rig {
    rig_with(mode, script, |_| {})
}

fn rig_with(mode: Binding, script: Vec<Scripted>, tweak: impl FnOnce(&mut Config)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("harbor.txt"), "the tide turns at four\n").unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    // The date and the git line, the first part of the harness's block beside the message.
    cfg.project.tail_facts = Some(true);
    tweak(&mut cfg);
    cfg.validate().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider {
        binding: Some(mode),
        ..FakeProvider::scripted(script)
    });
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake.clone(), store)).unwrap();
    Rig {
        core,
        fake,
        _dir: dir,
    }
}

/// An answer that thinks, then reads the harbor file.
fn thinks_then_reads(id: &str) -> Scripted {
    Scripted::Blocks {
        blocks: vec![
            json!({"type": "thinking", "thinking": format!("read it ({id})")}),
            json!({"type": "tool_use", "id": id, "name": "fs_read", "input": {"path": "harbor.txt"}}),
        ],
        stop_reason: "tool_use".into(),
    }
}

/// A conversation bound to a place, as a person's private message is.
fn private_session(core: &Core) -> String {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    core.outbox.bind_place("dm:7", &rec.session_id).unwrap();
    core.runner.place_rule.bind_one(crate::places::BoundPlace {
        target: "discord:dm:7".into(),
        name: "DM".into(),
        private: true,
        ..Default::default()
    });
    rec.session_id
}

async fn say(core: &Arc<Core>, sid: &str, input: &str) -> anyhow::Result<TurnSubmitResult> {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    core.runner
        .run(TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink: EventSink::new(core.bus.clone(), sid, None),
            author: "test".into(),
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
}

/// What `later` sends before the answer that `earlier` got must be what
/// `earlier` sent: the system, the tools, and every message of it, block for
/// block (a cache mark moves by design and is not content). Names the first
/// place they differ.
pub(crate) fn assert_same_prefix(earlier: &ProviderRequest, later: &ProviderRequest) {
    let (a, b) = (
        unmarked(&json!(earlier.system)),
        unmarked(&json!(later.system)),
    );
    assert_eq!(a, b, "the system changed between steps");
    let (a, b) = (
        unmarked(&json!(earlier.tools)),
        unmarked(&json!(later.tools)),
    );
    assert_eq!(a, b, "the tools changed between steps");
    assert!(
        later.messages.len() >= earlier.messages.len(),
        "the later request is shorter"
    );
    for (i, m) in earlier.messages.iter().enumerate() {
        let (a, b): (Value, Value) = (unmarked(m), unmarked(&later.messages[i]));
        assert_eq!(a, b, "messages.{i} changed between steps");
    }
}

/// The steps of the turn: each request after the first repeats the one
/// before it, and the turn ended on the provider's word that it was whole.
fn assert_stable(r: &Rig) {
    let reqs = r.fake.requests();
    assert!(reqs.len() >= 2, "{} requests", reqs.len());
    for w in reqs.windows(2) {
        assert_same_prefix(&w[0], &w[1]);
    }
}

/// An ordinary turn: a person's message in a private place, one tool step,
/// then the answer.
#[tokio::test]
async fn a_persons_turn_with_a_tool_step_sends_its_first_step_again() {
    let r = rig(
        Binding::Refuse,
        vec![thinks_then_reads("toolu_a"), Scripted::text("Four.")],
    );
    let sid = private_session(&r.core);
    let res = say(&r.core, &sid, "what does the tide table say?")
        .await
        .unwrap();
    assert_eq!(res.output, "Four.", "{res:?}");
    assert_stable(&r);
}

/// The self join's review: one check turn in a session of the self principal,
/// the turn that failed on `claude-fable-5-2` at its second step.
#[tokio::test(flavor = "multi_thread")]
async fn a_review_turn_with_a_tool_step_sends_its_first_step_again() {
    use crate::rsi::join::review::{Ask, CheckTurn, Reviewer};
    let r = rig_with(
        Binding::Refuse,
        vec![
            thinks_then_reads("toolu_a"),
            Scripted::text("It does what it says.\nVERDICT: accept\n- the test fails without it"),
        ],
        |c| {
            let repo = std::path::Path::new(&c.server.state_dir).join("repo");
            std::fs::create_dir_all(repo.join(".git")).unwrap();
            c.self_improve.mode = theseus_protocol::rsi::SelfMode::Act;
            c.self_improve.repo = Some(repo.to_string_lossy().into_owned());
        },
    );
    let (live, _) = r.core.live_profile();
    let target = r
        .core
        .runner
        .resolve_target(&live, None, None, None)
        .unwrap();
    let scratch = r._dir.path().join("scratch");
    std::fs::create_dir_all(&scratch).unwrap();
    let ask = Ask {
        branch: "br-one".into(),
        head: "0123abc".into(),
        item: "sails".into(),
        diff: String::new(),
        report: "a report".into(),
        evidence: "the evidence".into(),
        proof: String::new(),
        target,
        built_ms: 0,
        scratch: scratch.display().to_string(),
        repo: r._dir.path().join("work").display().to_string(),
        offer_fix: None,
        fixed: None,
    };
    let review = CheckTurn.review(&r.core, &ask).await.unwrap();
    assert!(review.accept, "{:?}", review.reasons);
    assert_stable(&r);
}

/// A task's turn: a session of the task kind, which a job or a builder runs.
#[tokio::test]
async fn a_task_turn_with_a_tool_step_sends_its_first_step_again() {
    let r = rig(
        Binding::Refuse,
        vec![thinks_then_reads("toolu_a"), Scripted::text("Four.")],
    );
    let rec = SessionRecord::new(SessionKind::Task, None);
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    let res = say(&r.core, &rec.session_id, "read the tide table")
        .await
        .unwrap();
    assert_eq!(res.output, "Four.", "{res:?}");
    assert_stable(&r);
}

/// A step the provider answers late, and the same request sent again: the
/// retry repeats the request it repeats, byte for byte.
#[tokio::test]
async fn a_retry_after_a_transient_failure_sends_the_same_request() {
    let net = crate::provider::ProviderError::Network {
        message: "connection reset".into(),
    };
    let r = rig(
        Binding::Refuse,
        vec![
            thinks_then_reads("toolu_a"),
            Scripted::Fail(net),
            Scripted::text("Four."),
        ],
    );
    let sid = private_session(&r.core);
    let res = say(&r.core, &sid, "what does the tide table say?")
        .await
        .unwrap();
    assert_eq!(res.output, "Four.", "{res:?}");
    let reqs = r.fake.requests();
    assert_eq!(reqs.len(), 3, "the step, its failure, its retry");
    assert_eq!(
        reqs[1].messages, reqs[2].messages,
        "the retry is the request it repeats"
    );
    assert_same_prefix(&reqs[0], &reqs[2]);
}

/// An answer whose thinking is bound to a conversation this request is not:
/// what a drift looks like, planted.
fn planted_drift(id: &str) -> Scripted {
    Scripted::Blocks {
        blocks: vec![
            json!({"type": "thinking", "thinking": "read it", "signature": "bound:0000"}),
            json!({"type": "tool_use", "id": id, "name": "fs_read", "input": {"path": "harbor.txt"}}),
        ],
        stop_reason: "tool_use".into(),
    }
}

fn call_rows(r: &Rig, sid: &str) -> Vec<Value> {
    crate::test_support::session_rows_1k(&r.core, sid, "provider.call")
}

/// The net: a drifted block the provider refuses is answered by one repeat
/// of the call with the remedy it names, the turn goes on, and the call's
/// row says a block was dropped. The first calls never carry the remedy.
#[tokio::test]
async fn a_refused_prefix_is_repeated_once_with_the_providers_remedy() {
    let r = rig(
        Binding::Refuse,
        vec![
            planted_drift("toolu_a"),
            thinks_then_reads("toolu_b"),
            Scripted::text("Four."),
        ],
    );
    let sid = private_session(&r.core);
    let res = say(&r.core, &sid, "what does the tide table say?")
        .await
        .unwrap();
    assert_eq!(res.output, "Four.", "{res:?}");
    let reqs = r.fake.requests();
    assert_eq!(
        reqs.len(),
        4,
        "the step, its refusal, its repeat, the step after"
    );
    // The history held the drifted block still, so the next compile let the old thinking go and
    // the step after it is not made twice.
    assert!(
        !serde_json::to_string(&reqs[3].messages)
            .unwrap()
            .contains("\"thinking\""),
        "{:?}",
        reqs[3].messages
    );
    for q in &reqs[..2] {
        assert!(q.thinking.as_ref().unwrap().get("block_binding").is_none());
    }
    assert_eq!(
        reqs[2].thinking.as_ref().unwrap()["block_binding"],
        json!({"prefix_mismatch_behavior": "drop_block"})
    );
    assert_eq!(
        reqs[1].messages, reqs[2].messages,
        "the repeat sends what it repeats"
    );
    let rows = call_rows(&r, &sid);
    assert_eq!(
        rows.len(),
        3,
        "the refused call is no row: it never answered"
    );
    assert!(rows[0].get("prefix_repeated").is_none(), "{}", rows[0]);
    let repeated = &rows[1];
    assert_eq!(
        (
            repeated["thinking_dropped"].as_u64(),
            repeated["prefix_repeated"].as_bool()
        ),
        (Some(1), Some(true)),
        "{repeated}"
    );
    assert!(
        rows[2].get("prefix_repeated").is_none(),
        "the step after is made once: {}",
        rows[2]
    );
}

/// Where a mismatch is allowed, the model keeps its thinking: nothing is
/// repeated, nothing is dropped, and the row counts the mismatch.
#[tokio::test]
async fn where_a_mismatch_is_allowed_nothing_is_dropped_and_the_row_counts_it() {
    let r = rig(
        Binding::Report,
        vec![planted_drift("toolu_a"), Scripted::text("Four.")],
    );
    let sid = private_session(&r.core);
    let res = say(&r.core, &sid, "what does the tide table say?")
        .await
        .unwrap();
    assert_eq!(res.output, "Four.", "{res:?}");
    let reqs = r.fake.requests();
    assert_eq!(reqs.len(), 2);
    assert!(reqs
        .iter()
        .all(|q| q.thinking.as_ref().unwrap().get("block_binding").is_none()));
    let rows = call_rows(&r, &sid);
    let last = rows.last().unwrap();
    assert_eq!(
        last["thinking_mismatch_allowed"].as_u64(),
        Some(1),
        "{last}"
    );
    assert!(
        last.get("thinking_dropped").is_none() && last.get("prefix_repeated").is_none(),
        "{last}"
    );
}

/// Narrow: another 400 is not answered by a repeat, and the repeat is made
/// once, not again when it is refused the same way.
#[tokio::test]
async fn only_the_prefix_refusal_is_repeated_and_only_once() {
    use crate::provider::{binding, ProviderError};
    let other = ProviderError::InvalidRequest {
        status: 400,
        message: "messages.0.content.0: text content blocks must be non-empty".into(),
    };
    let r = rig(Binding::Refuse, vec![Scripted::Fail(other)]);
    let sid = private_session(&r.core);
    assert!(say(&r.core, &sid, "what does the tide table say?")
        .await
        .is_err());
    assert_eq!(
        r.fake.requests().len(),
        1,
        "a different 400 is not repeated"
    );

    let again = binding::refusal(&[(1, 0)]);
    let r = rig(
        Binding::Refuse,
        vec![Scripted::Fail(again.clone()), Scripted::Fail(again)],
    );
    let sid = private_session(&r.core);
    assert!(say(&r.core, &sid, "what does the tide table say?")
        .await
        .is_err());
    assert_eq!(r.fake.requests().len(), 2, "one repeat, and no more");
}

/// A recall that reaches the model after the session's first compile: the
/// turn's note is written at its first call and is in the transcript at its
/// second, and the first user message reads the same at both.
#[tokio::test]
async fn a_recall_note_reads_the_same_at_every_step_of_its_turn() {
    use crate::tests_recall::{index_of, session, OWNER};
    let r = rig_with(
        Binding::Refuse,
        vec![thinks_then_reads("toolu_a"), Scripted::text("Four.")],
        |c| {
            c.memory.mode = crate::config::MemoryMode::Live;
            c.places.owner = Some(vec![format!("discord:{OWNER}")]);
        },
    );
    let source = session(
        &r.core,
        None,
        &["Remember: the grey heron nests by the old weir."],
    );
    r.core
        .runner
        .memory
        .set_ask(index_of(&r.core, vec![source]));
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    let res = say(&r.core, &rec.session_id, "where does the heron nest?")
        .await
        .unwrap();
    assert_eq!(res.output, "Four.", "{res:?}");
    let reqs = r.fake.requests();
    assert!(
        serde_json::to_string(&reqs[0].messages)
            .unwrap()
            .contains("grey heron"),
        "the note reached the first call"
    );
    assert_stable(&r);
}

/// A refusal's fallback answers the refused request, and the next one extends
/// it: the thinking of both models goes back with its prefix as it was.
#[tokio::test]
async fn a_fallback_and_the_steps_after_it_send_what_was_sent() {
    let refused = Scripted::Refused {
        blocks: vec![
            json!({"type": "text", "text": "Let me look at the archive."}),
            json!({"type": "tool_use", "id": "toolu_refused", "name": "fs_read", "input": {"path": "harbor.txt"}}),
        ],
        category: Some("cyber".into()),
    };
    let r = rig(
        Binding::Refuse,
        vec![
            thinks_then_reads("toolu_a"),
            refused,
            thinks_then_reads("toolu_b"),
            Scripted::text("Four."),
        ],
    );
    let sid = private_session(&r.core);
    let res = say(&r.core, &sid, "what does the tide table say?")
        .await
        .unwrap();
    assert_eq!(res.output, "Four.", "{res:?}");
    assert_stable(&r);
}

/// The harness's block is sent once and never twice within a turn: the second step's request holds
/// the date line once, in the place the first step put it, and not again at its own end. A new
/// turn's request carries its own block whole, and none of the first turn's: the block lives in its
/// turn alone (issue 46; once the kept tails of `sent_tail`, issue 44).
#[tokio::test]
async fn the_harness_block_is_not_sent_again_within_its_turn_nor_into_the_next() {
    let r = rig(
        Binding::Refuse,
        vec![
            thinks_then_reads("toolu_a"),
            Scripted::text("Four."),
            Scripted::text("Still four."),
        ],
    );
    let sid = private_session(&r.core);
    say(&r.core, &sid, "what does the tide table say?")
        .await
        .unwrap();
    say(&r.core, &sid, "and again?").await.unwrap();
    let reqs = r.fake.requests();
    let dates = |q: &ProviderRequest| {
        serde_json::to_string(&q.messages)
            .unwrap()
            .matches("Date: ")
            .count()
    };
    assert_eq!(
        (dates(&reqs[0]), dates(&reqs[1]), dates(&reqs[2])),
        (1, 1, 1),
        "{:?}",
        reqs[2].messages
    );
    let block = |q: &ProviderRequest| crate::turn::now_step::carried(q);
    assert!(
        block(&reqs[0]).is_some(),
        "the first step carries the block"
    );
    assert_eq!(
        block(&reqs[0]),
        block(&reqs[1]),
        "the same block at the second step"
    );
    assert!(block(&reqs[2]).is_some(), "the next turn carries its own");
}

/// The text blocks of a request's messages, each with its message's index.
fn text_blocks(q: &ProviderRequest) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (i, m) in q.messages.iter().enumerate() {
        match &m["content"] {
            Value::String(s) => out.push((i, s.clone())),
            c => out.extend(
                (c.as_array().into_iter().flatten())
                    .filter_map(|b| b["text"].as_str())
                    .map(|t| (i, t.to_string())),
            ),
        }
    }
    out
}

/// The harness's part of the block beside the message (issue 46): composed once at the turn's first call,
/// counted in slot I of the view, and carried byte for byte at every call of the turn, in the message the
/// first call put it in.
#[tokio::test]
async fn the_harness_block_is_the_same_at_every_call_of_its_turn() {
    let r = rig(
        Binding::Refuse,
        vec![
            thinks_then_reads("toolu_a"),
            thinks_then_reads("toolu_b"),
            Scripted::text("Four."),
        ],
    );
    let sid = private_session(&r.core);
    let res = say(&r.core, &sid, "what does the tide table say?")
        .await
        .unwrap();
    let reqs = r.fake.requests();
    assert_eq!(reqs.len(), 3, "three calls");
    let harness = |q: &ProviderRequest| -> (usize, String) {
        let found: Vec<(usize, String)> = (text_blocks(q).into_iter())
            .filter(|(_, t)| t.starts_with(crate::turn::now_step::HEAD))
            .collect();
        assert_eq!(found.len(), 1, "one harness block: {found:?}");
        found.into_iter().next().unwrap()
    };
    let first = harness(&reqs[0]);
    assert!(first.1.contains("Date: "), "{}", first.1);
    for q in &reqs[1..] {
        assert_eq!(
            harness(q),
            first,
            "the block moved or changed within its turn"
        );
    }
    let view = crate::view::record::newest(&r.core.store, &res.turn_id)
        .unwrap()
        .expect("the turn's view is kept");
    let counted = (view.shown.iter())
        .find(|s| {
            matches!(&s.piece.range.space, crate::view::Space::Code { part } if part == "harness")
        })
        .expect("slot I counts the harness's part");
    assert_eq!(counted.piece.slot, crate::view::Slot::Now);
    let beside = format!("\n\n{}", crate::view::wire_text::BESIDE_HEAD);
    let own = first.1.split(beside.as_str()).next().unwrap();
    let est = (own.len() as u64).div_ceil(crate::task_graph::view::BYTES_PER_TOKEN);
    assert!(
        counted.piece.sizes.whole >= est,
        "counted {} for a part of {est}",
        counted.piece.sizes.whole
    );
}

/// A turn in `sid` whose input came through `surface`.
async fn say_on(
    core: &Arc<Core>,
    sid: &str,
    input: &str,
    surface: Option<&'static str>,
) -> anyhow::Result<TurnSubmitResult> {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    core.runner
        .run(TurnRequest {
            prompt: None,
            surface,
            session: rec,
            input: Some(input.into()),
            target,
            sink: EventSink::new(core.bus.clone(), sid, None),
            author: "test".into(),
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
}

/// The compilations a turn's calls were compiled into, in order (its `context.compiled` rows).
fn chains(r: &Rig, turn: &str) -> Vec<String> {
    (r.core
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(10_000)
        .unwrap()
        .into_iter())
    .map(|(_, row)| row)
    .filter(|row| row.kind == "context.compiled" && row.turn_id.as_deref() == Some(turn))
    .map(|row| {
        row.data["compilation_id"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    })
    .collect()
}

/// Run `f` after the second compile of `sid` has assembled its view: between the turn's first call and
/// its second.
fn between_calls(sid: &str, f: impl FnOnce() + Send + 'static) {
    crate::turn::view_step::assembled_hook(sid, Box::new(|| {}));
    crate::turn::view_step::assembled_hook(sid, Box::new(f));
}

/// An observation that stops being today's open one between two calls of one turn is not sent again in
/// the harness's block: the part is rendered again from its producer and the view re-frozen on a new chain
/// (issue 46, round 2).
#[tokio::test]
async fn an_observation_dropped_between_two_calls_is_not_sent_again() {
    const SAID: &str = "likes the tide table read aloud";
    let r = rig(
        Binding::Refuse,
        vec![thinks_then_reads("toolu_a"), Scripted::text("Four.")],
    );
    let sid = private_session(&r.core);
    let mut obs = theseus_protocol::CardObservation {
        id: "cob_tide".into(),
        session: sid.clone(),
        nodes: vec!["msg_seed".into()],
        kind: theseus_protocol::ObservationKind::Preference,
        text: SAID.into(),
        at_ms: theseus_protocol::now_unix_ms(),
        contradicts: None,
        state: theseus_protocol::ObservationState::Open,
        proposal: None,
    };
    let seed = crate::card::state::obs_record(&obs).unwrap();
    r.core.store.append(&[seed]).unwrap();
    obs.state = theseus_protocol::ObservationState::Dropped;
    let core = r.core.clone();
    between_calls(&sid, move || {
        let dropped = crate::card::state::obs_record(&obs).unwrap();
        core.store.append(&[dropped]).unwrap();
    });
    let res = say(&r.core, &sid, "what does the tide table say?")
        .await
        .unwrap();
    let reqs = r.fake.requests();
    assert_eq!(reqs.len(), 2, "two calls");
    let said = |q: &ProviderRequest| serde_json::to_string(&q.messages).unwrap();
    assert!(said(&reqs[0]).contains(SAID), "the first call carries it");
    assert!(!said(&reqs[1]).contains(SAID), "{}", said(&reqs[1]));
    let chains = chains(&r, &res.turn_id);
    assert_ne!(chains.first(), chains.last(), "a new chain: {chains:?}");
}

/// A standing record of the turn's surface revoked between two calls of one turn is not sent again, and
/// the turn's check holds the block to the records live now (issue 46, round 2).
#[tokio::test]
async fn a_standing_record_revoked_between_two_calls_is_not_sent_again() {
    const ORDER: &str = "read the tide table in metres";
    let r = rig(
        Binding::Refuse,
        vec![thinks_then_reads("toolu_a"), Scripted::text("Four.")],
    );
    let sid = private_session(&r.core);
    let id = r
        .core
        .standing_add(
            &theseus_protocol::StandingAddParams {
                kind: theseus_protocol::StandingKind::Order,
                text: ORDER.into(),
                scope: theseus_protocol::StandingScope::Surface("terminal".into()),
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap()
        .record
        .id;
    let core = r.core.clone();
    between_calls(&sid, move || {
        core.standing_revoke(
            &theseus_protocol::StandingRevokeParams {
                id,
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    });
    let res = say_on(
        &r.core,
        &sid,
        "what does the tide table say?",
        Some("terminal"),
    )
    .await
    .unwrap();
    let reqs = r.fake.requests();
    assert_eq!(reqs.len(), 2, "two calls");
    let said = |q: &ProviderRequest| serde_json::to_string(&q.messages).unwrap();
    assert!(said(&reqs[0]).contains(ORDER), "the first call carries it");
    assert!(!said(&reqs[1]).contains(ORDER), "{}", said(&reqs[1]));
    let chains = chains(&r, &res.turn_id);
    assert_ne!(chains.first(), chains.last(), "a new chain: {chains:?}");
}

/// What a look back kept by reference quotes (issue 49): a message of another session.
const QUOTED: &str = "the grey heron nests by the old weir";

/// A look back of the turn `turn` of `sid`, kept by reference, added after its answer as a later step
/// would be: the call, its result quoting a message of another session as a page line, and the answer after
/// it. The quoted message's id, and the result's rendering.
fn looked_back(core: &Core, sid: &str, turn: &str) -> (String, String) {
    use crate::compiler::tests_view::{result, said};
    use crate::node::{Body, Node};
    use crate::quotes::{Field, Form, Part, Quote};
    let put = |n: &Node| core.store.append(&[n.record().unwrap()]).unwrap()[0];
    let source = Node::user("ses_weir", Some("t_weir"), "cli", QUOTED);
    let at = put(&source);
    let ours = |mut n: Node| {
        n.session_id = sid.to_string();
        n.loop_index = Some(1);
        n
    };
    let input = json!({"words": "heron"});
    let asked = ours(said(
        turn,
        vec![
            json!({"type": "tool_use", "id": "toolu_h", "name": "context_history", "input": input}),
        ],
    ));
    let call = ours(Node::tool_call(
        sid,
        Some(turn),
        Some(1),
        Body::ToolCall {
            tool_use_id: "toolu_h".into(),
            tool: "context.history".into(),
            wire_name: "context_history".into(),
            input,
            assistant_node: asked.id.clone(),
            correlation_id: None,
            gate: None,
        },
    ));
    let mut page = ours(result(turn, "toolu_h", ""));
    if let Body::ToolResult { tool, parts, .. } = &mut page.body {
        *tool = "context.history".into();
        *parts = vec![
            Part::Code("- ".into()),
            Part::Quote(Quote {
                node: source.id.clone(),
                position: at,
                field: Field::Text,
                from: 0,
                to: QUOTED.len() as u32,
                form: Form::Flat1,
            }),
            Part::Code(" \u{b7} t_weir".into()),
        ];
    }
    let done = ours(said(
        turn,
        vec![json!({"type": "text", "text": "It nests by the weir."})],
    ));
    for n in [&asked, &call, &page, &done] {
        put(n);
    }
    (source.id, format!("- {QUOTED} \u{b7} t_weir"))
}

/// A look back kept by reference in the window, its quoted message erased between the turn's two calls
/// (issue 49): the first call lays the rendering, the second lays the erased line in place of the words on a
/// new chain (its answers so far without their thinking), and the provider that refuses a changed prefix
/// under a thinking block takes it: no 400.
#[tokio::test]
async fn an_erase_between_steps_starts_a_new_chain_not_a_400() {
    let r = rig(
        Binding::Refuse,
        vec![
            Scripted::text("Noted."),
            thinks_then_reads("toolu_a"),
            Scripted::text("Four."),
        ],
    );
    let sid = private_session(&r.core);
    let first = say(&r.core, &sid, "where does the heron nest?")
        .await
        .unwrap();
    let (source, shown) = looked_back(&r.core, &sid, &first.turn_id);
    let core = r.core.clone();
    between_calls(&sid, move || {
        let (_, n) = core.store.get_node(&source).unwrap().unwrap();
        let gone = n.erased(theseus_protocol::now_unix_ms(), "a test");
        core.store.append(&[gone.record().unwrap()]).unwrap();
    });
    let res = say(&r.core, &sid, "what does the tide table say?")
        .await
        .unwrap();
    assert_eq!(res.output, "Four.", "{res:?}");
    let reqs = r.fake.requests();
    assert_eq!(reqs.len(), 3, "one call, then two");
    let said = |q: &ProviderRequest| serde_json::to_string(&q.messages).unwrap();
    assert!(said(&reqs[1]).contains(&shown), "{}", said(&reqs[1]));
    let after = said(&reqs[2]);
    assert!(!after.contains(QUOTED), "{after}");
    assert!(after.contains(crate::quotes::ERASED_QUOTE), "{after}");
    let chains = chains(&r, &res.turn_id);
    assert_ne!(chains.first(), chains.last(), "a new chain: {chains:?}");
}

/// `turn.inspect` shows a result kept by reference as its rendering, as the model was shown it (issue 49).
#[tokio::test]
async fn inspect_shows_the_rendering() {
    let r = rig(Binding::Refuse, vec![Scripted::text("Noted.")]);
    let sid = private_session(&r.core);
    let first = say(&r.core, &sid, "where does the heron nest?")
        .await
        .unwrap();
    let (_, shown) = looked_back(&r.core, &sid, &first.turn_id);
    let out = r
        .core
        .turn_inspect(
            &theseus_protocol::inspect::TurnInspectParams {
                session_id: sid.clone(),
                turn_id: first.turn_id,
            },
            false,
        )
        .unwrap();
    let call = (out.loops.iter())
        .flat_map(|l| &l.calls)
        .find(|c| c.tool_use_id == "toolu_h")
        .expect("the look back's call");
    assert_eq!(call.content, shown);
}
