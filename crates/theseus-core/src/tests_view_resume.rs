//! A turn no message opened carries the work it resumes, or fails plainly (issue 44, the review of the
//! hand-backs lane): a continuation of a continuation keeps the question and the work (its chain's presence
//! is kept beside each message-less exchange, `exchange::given`); a restart does not undo a steer's
//! narrowing (an unfinished exchange is read with its turn's durable messages); a turn whose work was asked
//! where its conversation does not answer now fails before anything is sent; and a conversation whose turn
//! failed plainly answers its next message.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use serde_json::json;
use theseus_memory::gate::Present;

use crate::config::assembly::{Assembly, AssemblySetting};
use crate::provider::{ProviderError, Scripted};
use crate::tests_places::{answers_a_call, last_user, rig_setup_scripted, session, LAB};
use crate::tests_private_view::session_in;
use crate::tests_view_compile::{fallbacks, legal, submit, texts};
use crate::tests_view_hand_backs::unmarked;

const ENDED: &str = "a background job this conversation started has ended";
const RESUMES: &str = "it resumes work an earlier turn left unfinished";

fn view(c: &mut crate::Config) {
    c.context.assembly = Some(AssemblySetting::All(Assembly::View));
}

fn cut() -> Scripted {
    Scripted::Fail(ProviderError::Api {
        status: 418,
        message: "api_error: the call was cut".into(),
    })
}

/// The presence the gate gave the message-less turn `turn` (`exchange::given`).
fn given(c: &crate::Core, turn: &str) -> Option<Present> {
    let g = crate::exchange::given::given(&c.store, turn).unwrap()?;
    Some(g.present())
}

/// The turns of `sid`, oldest first, each once: the turn of each of its nodes (a turn that extends the same
/// chain writes no compile row of its own, so its nodes are the record).
fn started(c: &crate::Core, sid: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (_, n) in c.store.session_nodes(sid).unwrap() {
        if let Some(turn) = n.turn_id.filter(|t| !out.contains(t)) {
            out.push(turn);
        }
    }
    out
}

fn answered(c: &crate::Core, sid: &str, words: &str) -> bool {
    (c.store.session_nodes(sid).unwrap().iter()).any(|(_, n)| {
        matches!(&n.body, crate::node::Body::AssistantMessage { .. })
            && serde_json::to_string(&n.body).unwrap().contains(words)
    })
}

/// P1: marked turn A starts a job; its late result opens continuation B, whose call fails; the driver's
/// resume C has no new result and continues B. C's request holds A's question (once) and the job's output,
/// its presence is A's (kept beside B's exchange), it is provider-legal, and it answers.
#[tokio::test]
async fn a_resume_after_a_failed_continuation_keeps_the_question_and_the_jobs_output() {
    const ASKED: &str = "run the long buoy script";
    let cuts = Arc::new(AtomicU32::new(0));
    let n = cuts.clone();
    let r = rig_setup_scripted(
        |c| {
            view(c);
            c.tools.proc_sync_secs = 1;
        },
        |_| {},
        move |q| {
            if texts(q).contains(RESUMES) {
                return Scripted::text("The buoy script printed BUOY-4410.");
            }
            if texts(q).contains(ENDED) && n.fetch_add(1, Ordering::SeqCst) == 0 {
                return cut();
            }
            if answers_a_call(q) {
                return Scripted::text("Started; it runs on.");
            }
            if last_user(q).contains(ASKED) {
                let argv = json!({"argv": ["sh", "-c", "sleep 1.6; echo BUOY-4410"]});
                return Scripted::tools("", &[("j1", "proc_run", argv)]);
            }
            Scripted::text("Hello.")
        },
    );
    let c = &r.core;
    let sid = session_in(c, None, &r.root);
    crate::tests_places::turn(c, &sid, ASKED).await;
    r.until("the continuation's failed call", 30, |r| {
        r.core.heartbeat("test");
        !crate::test_support::rows(&r.core, "turn.failed").is_empty()
    })
    .await;
    let rec: crate::session::SessionRecord = c.store.get_session(&sid).unwrap().unwrap();
    c.kernel.wake(&rec.execution_id.unwrap(), "test").unwrap();
    r.until("the resumed turn", 30, |r| {
        r.requests().iter().any(|q| texts(q).contains(RESUMES))
    })
    .await;
    let q = (r.requests().into_iter())
        .find(|q| texts(q).contains(RESUMES))
        .unwrap();
    legal(&q);
    let t = texts(&q);
    assert_eq!(t.matches(ASKED).count(), 1, "the question, once: {t}");
    assert!(t.contains("BUOY-4410"), "the job's output: {t}");
    r.until("its answer", 15, |_| answered(c, &sid, "printed BUOY-4410"))
        .await;
    let turns = started(c, &sid);
    assert!(turns.len() >= 3, "{turns:?}");
    let (b, last) = (&turns[1], &turns[turns.len() - 1]);
    assert_eq!(
        given(c, b),
        Some(Present::OwnerAlone),
        "B kept A's presence"
    );
    assert_eq!(
        given(c, last),
        Some(Present::OwnerAlone),
        "no wider than A's"
    );
    assert!(fallbacks(c).is_empty(), "{:?}", fallbacks(c));
}

/// P1: marked turn A starts a job; its late result opens B, which starts another job; that job's late
/// result opens C, which continues B through its call. C's request holds A's question and the second job's
/// output, its presence is A's, and it answers.
#[tokio::test]
async fn a_continuations_own_call_answered_later_keeps_the_question() {
    const ASKED: &str = "run both tide gauges";
    let ended = Arc::new(AtomicU32::new(0));
    let n = ended.clone();
    let r = rig_setup_scripted(
        |c| {
            view(c);
            c.tools.proc_sync_secs = 1;
        },
        |_| {},
        move |q| {
            if texts(q).contains(ENDED) {
                if n.fetch_add(1, Ordering::SeqCst) == 0 {
                    // Long enough that B has ended when it ends: its result opens a turn of its own, C.
                    let argv = json!({"argv": ["sh", "-c", "sleep 4; echo GAUGE-SECOND-88"]});
                    return Scripted::tools("", &[("j2", "proc_run", argv)]);
                }
                return Scripted::text("Both gauges read: GAUGE-SECOND-88.");
            }
            if answers_a_call(q) {
                return Scripted::text("Started; it runs on.");
            }
            if last_user(q).contains(ASKED) {
                let argv = json!({"argv": ["sh", "-c", "sleep 1.6; echo GAUGE-FIRST-11"]});
                return Scripted::tools("", &[("j1", "proc_run", argv)]);
            }
            Scripted::text("Hello.")
        },
    );
    let c = &r.core;
    let sid = session_in(c, None, &r.root);
    crate::tests_places::turn(c, &sid, ASKED).await;
    r.until("the second job's turn", 45, |r| {
        r.core.heartbeat("test");
        (r.requests().iter())
            .any(|q| texts(q).contains("GAUGE-SECOND-88") && texts(q).contains(ENDED))
    })
    .await;
    let q = (r.requests().into_iter())
        .find(|q| texts(q).contains("GAUGE-SECOND-88") && texts(q).contains(ENDED))
        .unwrap();
    legal(&q);
    let t = texts(&q);
    assert_eq!(t.matches(ASKED).count(), 1, "the question, once: {t}");
    r.until("its answer", 15, |_| answered(c, &sid, "GAUGE-SECOND-88"))
        .await;
    let turns = started(c, &sid);
    assert!(turns.len() >= 2, "{turns:?}");
    for t in &turns[1..] {
        assert_eq!(
            given(c, t),
            Some(Present::OwnerAlone),
            "{t}: no wider than A's"
        );
    }
    assert!(fallbacks(c).is_empty(), "{:?}", fallbacks(c));
}

/// P1, fail closed: a turn no message opened whose chain cannot be read (the turn it continues has no
/// exchange and nothing kept) fails plainly, and nothing is sent.
#[tokio::test]
async fn a_resume_whose_chain_cannot_be_read_fails_plainly() {
    let fake = Arc::new(crate::provider::FakeProvider::scripted(vec![
        cut(),
        Scripted::text("Should not be sent."),
    ]));
    let (c, sid, turn) = restart_rig(fake.clone(), None, "where is the pilot boat?").await;
    c.store
        .append(&[crate::exchange::exchange_record(&{
            let mut ex = crate::exchange::exchange(&c.store, &turn).unwrap().unwrap();
            // An exchange whose ask's mark is gone, and nothing kept: what it continues cannot be read.
            ex.ask = Some("msg-that-is-not-there".into());
            ex.messages.clear();
            (ex.end_ms, ex.stop) = (None, None);
            ex
        })
        .unwrap()])
        .unwrap();
    let e = resume(&c, &sid).await.unwrap_err();
    let te = e.downcast_ref::<crate::turn::TurnError>().unwrap();
    assert_eq!(te.class, "context_unreadable");
    assert!(
        format!("{:#}", te.source).contains("what it continues could not be read"),
        "{:#}",
        te.source
    );
    assert!(
        (fake.requests().iter()).all(|q| !texts(q).contains(RESUMES)),
        "nothing is sent"
    );
}

/// A Core with no driver (a restart's resume is made below), view set, its first call cut: a session
/// (`target`) whose first turn, `asked`, failed. (Core, session, that turn.)
async fn restart_rig(
    fake: Arc<crate::provider::FakeProvider>,
    target: Option<&str>,
    asked: &str,
) -> (Arc<crate::Core>, String, String) {
    restart_rig_with(fake, target, asked, |_| {}).await
}

/// `restart_rig`, its config changed by `tweak` first.
async fn restart_rig_with(
    fake: Arc<crate::provider::FakeProvider>,
    target: Option<&str>,
    asked: &str,
    tweak: impl FnOnce(&mut crate::Config),
) -> (Arc<crate::Core>, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let mut cfg = crate::Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    view(&mut cfg);
    tweak(&mut cfg);
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let c = crate::Core::build(crate::rpc::Parts::for_tests(cfg, fake, store)).unwrap();
    // The scratch directory lives as long as the test process: the store holds it open.
    std::mem::forget(dir);
    let sid = session(&c, target);
    let _ = submit(&c, &sid, asked).await;
    let turn = (c.store.session_nodes(&sid).unwrap().into_iter())
        .find(|(_, n)| matches!(n.body, crate::node::Body::UserMessage { .. }))
        .and_then(|(_, n)| n.turn_id)
        .unwrap();
    (c, sid, turn)
}

/// The next start's resume of `sid`'s interrupted execution (`continue_execution`).
async fn resume(
    c: &Arc<crate::Core>,
    sid: &str,
) -> anyhow::Result<theseus_protocol::TurnSubmitResult> {
    let rec: crate::session::SessionRecord = c.store.get_session(sid).unwrap().unwrap();
    let exec = rec.execution_id.unwrap();
    c.kernel.wake(&exec, "restart").unwrap();
    c.continue_execution(&exec).await.map(Option::unwrap)
}

/// P2: an owner-opened turn absorbs a steer with no owner's mark, then crashes before its exchange is
/// rewritten: the steer's node is durable (its mark is not: the boundary), the stored exchange lists only
/// the opening ask, and it never ended. Its presence is read with the turn's durable messages: no wider
/// than the steered turn's (`PrivateChannel`), not the owner alone again. A steer with another person's
/// mark narrows it the same way. Without the steer, the same exchange reads as the owner alone.
#[tokio::test]
async fn a_crash_after_a_steer_does_not_widen_the_interrupted_turns_presence() {
    let fake = Arc::new(crate::provider::FakeProvider::scripted(vec![
        cut(),
        Scripted::text("The board shows three ferries."),
    ]));
    let (c, sid, turn) = restart_rig(fake.clone(), None, "what is on the harbour board?").await;
    let class = c.runner.place_of(&sid);
    let mut ex = crate::exchange::exchange(&c.store, &turn).unwrap().unwrap();
    (ex.end_ms, ex.stop) = (None, None);
    ex.messages = ex.ask.iter().cloned().collect();
    c.store
        .append(&[crate::exchange::exchange_record(&ex).unwrap()])
        .unwrap();
    assert_eq!(
        crate::view_gate::continued(&c.store, &class, &turn),
        Some(Present::OwnerAlone),
        "the owner's ask alone"
    );
    let steer =
        crate::node::Node::user_with(&sid, Some(&turn), "agent", "also list the gulls", vec![]);
    c.store.append(&[steer.record().unwrap()]).unwrap();
    assert_eq!(
        crate::view_gate::continued(&c.store, &class, &turn),
        Some(Present::PrivateChannel),
        "a durable steer with no mark narrows the unfinished exchange"
    );
    let mut other = crate::view_gate::test_mark(&c.runner, &sid, false);
    other.node = steer.id.clone();
    c.store
        .append(&[crate::exchange::mark_record(&other).unwrap()])
        .unwrap();
    assert_eq!(
        crate::view_gate::continued(&c.store, &class, &turn),
        Some(Present::PrivateChannel),
        "an agent's steer, its mark durable too, narrows it the same"
    );
    // The resume reads the interrupted turn so: its presence is the steered turn's, and it answers.
    let res = resume(&c, &sid).await.unwrap();
    assert_eq!(res.output, "The board shows three ferries.");
    let last = started(&c, &sid).pop().unwrap();
    assert_eq!(
        given(&c, &last),
        Some(Present::PrivateChannel),
        "no wider than the steered turn"
    );
    let q = (fake.requests().into_iter())
        .rev()
        .find(|q| !q.tools.is_empty())
        .unwrap();
    legal(&q);
    assert_eq!(
        texts(&q).matches("what is on the harbour board?").count(),
        1,
        "{}",
        texts(&q)
    );
}

/// P3: the interrupted work was asked in a place other than where this conversation's reply goes now (a
/// reply target that changed): the resume fails plainly before any call, rather than lay that place's
/// conversation for another audience.
#[tokio::test]
async fn a_resume_whose_work_was_asked_elsewhere_fails_plainly_before_any_call() {
    let fake = Arc::new(crate::provider::FakeProvider::scripted(vec![
        cut(),
        Scripted::text("Should not be sent."),
    ]));
    let target = format!("channel:{LAB}");
    let (c, sid, turn) = restart_rig(fake.clone(), Some(&target), "which berth is free?").await;
    let mut ex = crate::exchange::exchange(&c.store, &turn).unwrap().unwrap();
    (ex.end_ms, ex.stop) = (None, None);
    ex.place = "local:/where-it-was-asked".into();
    c.store
        .append(&[crate::exchange::exchange_record(&ex).unwrap()])
        .unwrap();
    let e = resume(&c, &sid).await.unwrap_err();
    let te = e.downcast_ref::<crate::turn::TurnError>().unwrap();
    assert_eq!(te.class, "context_unreadable");
    assert!(
        format!("{:#}", te.source).contains("cannot be resumed here"),
        "{:#}",
        te.source
    );
    assert!(
        (fake.requests().iter()).all(|q| !texts(q).contains(RESUMES)),
        "nothing is sent"
    );
}

/// Item 4: after each plain failure (an assembly read failure; a third move in one call; a place that
/// cannot be read), the conversation's next message is answered.
#[tokio::test]
async fn a_conversation_answers_its_next_message_after_each_plain_failure() {
    let r = crate::tests_places::rig_with(view);
    let c = &r.core;
    let sid = session_in(c, None, &r.root);
    crate::turn::view_step::fail_next_assembly(&sid);
    assert!(submit(c, &sid, "what is on the desk?").await.is_err());
    let after = submit(c, &sid, "and now, what is on the desk?").await;
    assert!(after.is_ok(), "{after:?}");
    for present in [Present::PrivateChannel, Present::Shared, Present::Open] {
        let (held, s) = (r.core.clone(), sid.clone());
        crate::turn::view_step::env_tail_hook(
            &sid,
            Box::new(move || {
                let now = held.runner.presence.get(&s).unwrap();
                let (turn, place) = (now.turn_id().to_string(), now.place().to_string());
                crate::view_gate::seat_for_tests(&held.runner, &s, &turn, present, &place);
            }),
        );
    }
    assert!(submit(c, &sid, "tidy the harbour notes").await.is_err());
    let after = submit(c, &sid, "tidy the harbour notes, please").await;
    assert!(after.is_ok(), "{after:?}");
    let lone = session_in(c, None, &r.root);
    crate::turn::view_step::route_unreadable(&lone);
    assert!(unmarked(c, &lone, "where does this go?").await.is_err());
    crate::turn::view_step::route_readable(&lone);
    let after = unmarked(c, &lone, "where does this go now?").await;
    assert!(after.is_ok(), "{after:?}");
    assert!(fallbacks(c).is_empty(), "{:?}", fallbacks(c));
}

/// The review's P1 (composition with the own-history rule): a self-loop worker (its execution's principal is
/// `self`, not a task) opens with an unmarked harness message, so it has no mark and nothing kept; its first
/// call is cut, and the next start resumes it. It is not failed for the presence it cannot read: its view
/// lays what its own session said at its own place (`Permit::speaking`), the brief exactly once, nothing of
/// another place (the owner's DM fact is absent), and it answers.
#[tokio::test]
async fn a_self_workers_resume_reaches_its_own_history_and_nothing_else() {
    const BRIEF: &str = "SELF BRIEF: count the herons at the weir";
    let fake = Arc::new(crate::provider::FakeProvider::scripted(vec![
        Scripted::text("Noted."),
        cut(),
        Scripted::text("Herons: nine."),
    ]));
    // The self loop on: a self session's call is reserved on its budget, which spends only then.
    let (c, dm, _) = restart_rig_with(
        fake.clone(),
        Some(&format!("dm:{}", crate::tests_places::OWNER)),
        "dm: the vault word is plover",
        |cfg| cfg.self_improve.mode = theseus_protocol::rsi::SelfMode::Act,
    )
    .await;
    assert!(answered(&c, &dm, "Noted."));
    // The self loop's session: its execution's principal is `self`, it is no task.
    let mut rec =
        crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    let authority = theseus_kernel::Authority {
        principal: crate::rsi::SELF_PRINCIPAL.into(),
        ..Default::default()
    };
    let exec = (c.kernel)
        .open_execution(&rec.session_id, rec.kind, authority, None, None)
        .unwrap();
    rec.execution_id = Some(exec.id);
    c.store.put_session(&rec.session_id, &rec).unwrap();
    let sid = rec.session_id.clone();
    assert!(crate::view_gate::worker(&c.runner, &sid) && rec.task.is_none());
    // Its opening, as the self loop runs one: no arrival mark.
    let (live, _) = c.live_profile();
    let target = c.runner.resolve_target(&live, None, None, None).unwrap();
    let _ = c
        .runner
        .run(crate::turn::TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some(BRIEF.into()),
            target,
            sink: crate::bus::EventSink::new(c.bus.clone(), &sid, None),
            author: "self:mint".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await;
    let before = fake.requests().len();
    let done = resume(&c, &sid).await.expect("the worker's resume runs");
    assert_eq!(done.output, "Herons: nine.");
    let sent = &fake.requests()[before..];
    assert!(!sent.is_empty(), "the resume made its call");
    for q in sent {
        legal(q);
        let t = texts(q);
        assert_eq!(t.matches(BRIEF).count(), 1, "the brief exactly once: {t}");
        assert!(!t.contains("plover"), "nothing of another place: {t}");
    }
    assert!(answered(&c, &sid, "Herons: nine."));
}

/// The review's P2: a session `/new` displaced keeps only its saved wake target (no target of its own). Its
/// resume, whose chain cannot be read, fails plainly, and the failure is posted at that wake target, where its
/// reply would have gone; then a new message to the session is answered.
#[tokio::test]
async fn a_displaced_sessions_plain_failure_is_posted_where_its_wake_answers() {
    const AT: &str = "discord:dm:42";
    let fake = Arc::new(crate::provider::FakeProvider::scripted(vec![
        cut(),
        Scripted::text("Back again."),
    ]));
    let (c, sid, turn) = restart_rig(fake.clone(), None, "where is the pilot boat?").await;
    c.store
        .append(&[c.outbox.wake_target_record(&sid, AT).unwrap()])
        .unwrap();
    assert_eq!(
        c.outbox.target(&sid),
        None,
        "displaced: no target of its own"
    );
    // The interrupted turn's own failure is not this run's: the resume's failure is the first of its run.
    c.store
        .update_session(&sid, |r: &mut crate::session::SessionRecord| {
            r.failing = None;
            Ok(vec![])
        })
        .unwrap();
    c.store
        .append(&[crate::exchange::exchange_record(&{
            let mut ex = crate::exchange::exchange(&c.store, &turn).unwrap().unwrap();
            ex.ask = Some("msg-that-is-not-there".into());
            ex.messages.clear();
            (ex.end_ms, ex.stop) = (None, None);
            ex
        })
        .unwrap()])
        .unwrap();
    let e = resume(&c, &sid).await.unwrap_err();
    let te = e.downcast_ref::<crate::turn::TurnError>().unwrap();
    assert_eq!(te.class, "context_unreadable");
    let posted: Vec<serde_json::Value> = (c.outbox.open_for(AT).iter())
        .filter(|a| crate::outbox::kind_of(a) == "failed")
        .map(|a| crate::outbox::body_of(a).clone())
        .collect();
    assert!(
        (posted.iter()).any(|b| b["class"] == "context_unreadable"
            && b["error"]
                .as_str()
                .unwrap_or_default()
                .contains("could not be read")),
        "the failure is posted at the wake target: {posted:?}"
    );
    let next = submit(&c, &sid, "and now, the pilot boat?").await.unwrap();
    assert_eq!(next.output, "Back again.");
}

/// The review's §6, at the level of the request: an owner-opened turn is cut, the owner's words in another
/// private place of his are recent, and the turn's durable steer has no mark. Resumed after the crash, its
/// request holds none of that other place's words (the steer narrowed it: the owner is not alone). The
/// control, with no steer: the owner alone, his other place's words are laid (slot F), so their absence above
/// is the steer's doing.
#[tokio::test]
async fn a_crash_after_a_steer_keeps_another_places_words_out_of_the_resumed_request() {
    const ELSEWHERE: &str = "elsewhere: the spare key is under the blue heron statue";
    for steered in [true, false] {
        let fake = Arc::new(crate::provider::FakeProvider::scripted(vec![
            cut(),
            Scripted::text("Noted."),
            Scripted::text("The board shows three ferries."),
        ]));
        let (c, sid, turn) = restart_rig(fake.clone(), None, "what is on the harbour board?").await;
        let other = session_in(&c, None, std::path::Path::new("/srv/elsewhere-ship"));
        submit(&c, &other, ELSEWHERE).await.unwrap();
        let mut ex = crate::exchange::exchange(&c.store, &turn).unwrap().unwrap();
        (ex.end_ms, ex.stop) = (None, None);
        ex.messages = ex.ask.iter().cloned().collect();
        let mut records = vec![crate::exchange::exchange_record(&ex).unwrap()];
        if steered {
            let steer = crate::node::Node::user_with(
                &sid,
                Some(&turn),
                "agent",
                "also list the gulls",
                vec![],
            );
            records.push(steer.record().unwrap());
        }
        c.store.append(&records).unwrap();
        let before = fake.requests().len();
        let res = resume(&c, &sid).await.unwrap();
        assert_eq!(res.output, "The board shows three ferries.");
        let sent: Vec<String> = (fake.requests()[before..].iter()).map(texts).collect();
        assert!(!sent.is_empty());
        let at = |t: &String| t.find("spare key is under the blue heron");
        let laid = sent.iter().find_map(|t| {
            at(t).map(|i| {
                t[i.saturating_sub(700)..]
                    .chars()
                    .take(900)
                    .collect::<String>()
            })
        });
        assert_eq!(
            laid.is_some(),
            !steered,
            "steered {steered}: another private place's words are laid only where the owner is alone; where: {laid:?}"
        );
    }
}
