//! No drift inside a turn (issue 20, "A turn on the newest model fails at its
//! second step"): what a turn sent at one step is what it sends at the next,
//! and anything new goes after it. A thinking block is bound to everything
//! before it, and `claude-fable-5-2` refuses a request that replays one after
//! a change there with a 400, where an older model reports it and goes on.
//! The stand-in provider carries that rule (`provider::binding`), so each test
//! runs a whole turn of two or more steps and reads what the provider would.

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
    // The date and the git line, which ride the last message of every request.
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
            recompile: None,
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

/// The turn's last request, and the same turn's earlier one, given the
/// answer between them: a second turn in the same conversation replays the
/// first's thinking, so its first request must carry the first's last one.
#[tokio::test]
async fn a_second_turn_replays_the_first_turns_thinking_as_it_was_sent() {
    let r = rig(
        Binding::Refuse,
        vec![
            thinks_then_reads("toolu_a"),
            Scripted::text("Four."),
            thinks_then_reads("toolu_b"),
            Scripted::text("Still four."),
        ],
    );
    let sid = private_session(&r.core);
    say(&r.core, &sid, "what does the tide table say?")
        .await
        .unwrap();
    let res = say(&r.core, &sid, "and again?").await.unwrap();
    assert_eq!(res.output, "Still four.", "{res:?}");
    let reqs = r.fake.requests();
    assert_eq!(reqs.len(), 4);
    // The second turn's first request is the first turn's last, extended.
    assert_same_prefix(&reqs[1], &reqs[2]);
    assert_same_prefix(&reqs[2], &reqs[3]);
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
            c.memory.mode = crate::config::MemoryMode::Canary;
            c.memory.canary_fraction = 1.0;
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

/// The kept tail costs what changed and no more within a turn: the second step's request holds
/// the date line once, in the place the first step put it, and not again at its own end. A
/// new turn's request carries its tail whole, and keeps the first turn's where it was.
#[tokio::test]
async fn a_tail_block_the_model_has_is_not_sent_again_within_its_turn() {
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
        (1, 1, 2),
        "{:?}",
        reqs[2].messages
    );
}

/// A keep-warm read between two turns (the landing's gate found it): the read is the prefix the
/// next turn sends, so a read made without the turns' kept tails warmed bytes the next turn did
/// not use, and its thinking was replayed under a changed prefix. A turn that thinks, the read,
/// then a turn with a tool step, all through the stand-in that enforces the binding: the read is
/// accepted, its messages are the next turn's first ones, and nothing drifts.
#[tokio::test]
async fn a_keep_warm_read_between_turns_sends_what_the_next_turn_sends() {
    use crate::keep_warm::{Outcome, MAX_TOKENS};
    use crate::tests_keep_warm::{build_with, one_hour, reads, session};
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::write(work.join("harbor.txt"), "the tide turns at four\n").unwrap();
    let mut c = one_hour(24);
    c.project.tail_facts = Some(true);
    c.tools.projects_dir = Some(work.canonicalize().unwrap().to_string_lossy().into_owned());
    let fake = Arc::new(FakeProvider {
        binding: Some(Binding::Refuse),
        ..FakeProvider::scripted(vec![
            Scripted::Blocks {
                blocks: vec![
                    json!({"type": "thinking", "thinking": "a tide question"}),
                    json!({"type": "text", "text": "Four."}),
                ],
                stop_reason: "end_turn".into(),
            },
            Scripted::text("."),
            thinks_then_reads("toolu_a"),
            Scripted::text("Still four."),
        ])
    });
    let core = build_with(dir.path(), c, fake.clone());
    let sid = session(&core, SessionKind::Conversation, None);
    core.runner.keep_warm.message(&sid, true);
    say(&core, &sid, "what does the tide table say?")
        .await
        .unwrap();
    let live = core.live_profile().0;
    let read = core.runner.keep_warm_once(&sid, &live).await;
    assert!(matches!(read, Outcome::Read { .. }), "{read:?}");
    let second = say(&core, &sid, "and again?").await.unwrap();
    assert_eq!(second.output, "Still four.", "{second:?}");
    let all = fake.requests();
    let read = reads(&fake).pop().expect("the read reached the provider");
    assert_eq!(read.max_tokens, MAX_TOKENS);
    let next = all
        .iter()
        .find(|q| q.max_tokens != MAX_TOKENS && q.messages.len() > read.messages.len() - 1)
        .expect("the next turn's first request");
    let n = read.messages.len() - 1;
    assert_eq!(
        unmarked(&json!(read.messages[..n])),
        unmarked(&json!(next.messages[..n])),
        "the read is the next turn's prefix"
    );
    let turn_two: Vec<_> = all
        .iter()
        .filter(|q| q.max_tokens != MAX_TOKENS && q.messages.len() > n)
        .cloned()
        .collect();
    assert!(turn_two.len() >= 2, "{} requests", turn_two.len());
    for w in turn_two.windows(2) {
        assert_same_prefix(&w[0], &w[1]);
    }
}
