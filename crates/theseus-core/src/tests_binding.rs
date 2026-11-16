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
fn assert_same_prefix(earlier: &ProviderRequest, later: &ProviderRequest) {
    let (a, b) = (unmarked(&json!(earlier.system)), unmarked(&json!(later.system)));
    assert_eq!(a, b, "the system changed between steps");
    let (a, b) = (unmarked(&json!(earlier.tools)), unmarked(&json!(later.tools)));
    assert_eq!(a, b, "the tools changed between steps");
    assert!(later.messages.len() >= earlier.messages.len(), "the later request is shorter");
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
    let res = say(&r.core, &sid, "what does the tide table say?").await.unwrap();
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
    say(&r.core, &sid, "what does the tide table say?").await.unwrap();
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
    let target = r.core.runner.resolve_target(&live, None, None, None).unwrap();
    let scratch = r._dir.path().join("scratch");
    std::fs::create_dir_all(&scratch).unwrap();
    let ask = Ask {
        branch: "br-one".into(),
        head: "0123abc".into(),
        item: "sails".into(),
        diff: String::new(),
        report: "a report".into(),
        evidence: "the evidence".into(),
        target,
        built_ms: 0,
        scratch: scratch.display().to_string(),
        repo: r._dir.path().join("work").display().to_string(),
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
    let res = say(&r.core, &rec.session_id, "read the tide table").await.unwrap();
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
    let res = say(&r.core, &sid, "what does the tide table say?").await.unwrap();
    assert_eq!(res.output, "Four.", "{res:?}");
    let reqs = r.fake.requests();
    assert_eq!(reqs.len(), 3, "the step, its failure, its retry");
    assert_eq!(reqs[1].messages, reqs[2].messages, "the retry is the request it repeats");
    assert_same_prefix(&reqs[0], &reqs[2]);
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
    let source = session(&r.core, None, &["Remember: the grey heron nests by the old weir."]);
    r.core.runner.memory.set_ask(index_of(&r.core, vec![source]));
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    let res = say(&r.core, &rec.session_id, "where does the heron nest?").await.unwrap();
    assert_eq!(res.output, "Four.", "{res:?}");
    let reqs = r.fake.requests();
    assert!(
        serde_json::to_string(&reqs[0].messages).unwrap().contains("grey heron"),
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
    let res = say(&r.core, &sid, "what does the tide table say?").await.unwrap();
    assert_eq!(res.output, "Four.", "{res:?}");
    assert_stable(&r);
}

/// The kept tail costs what changed and no more: the second turn's request
/// holds the date line once, in the place the first turn put it, and
/// not again at its own end.
#[tokio::test]
async fn a_tail_block_the_model_already_has_is_not_sent_again() {
    let r = rig(
        Binding::Refuse,
        vec![Scripted::text("Four."), Scripted::text("Still four.")],
    );
    let sid = private_session(&r.core);
    say(&r.core, &sid, "what does the tide table say?").await.unwrap();
    say(&r.core, &sid, "and again?").await.unwrap();
    let reqs = r.fake.requests();
    let dates = |q: &ProviderRequest| {
        serde_json::to_string(&q.messages).unwrap().matches("Date: ").count()
    };
    assert_eq!((dates(&reqs[0]), dates(&reqs[1])), (1, 1), "{:?}", reqs[1].messages);
}
