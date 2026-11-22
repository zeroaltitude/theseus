//! Five of the view's hand-backs, answered inside the view (issue 44): a wake answered where its reply is
//! delivered takes the view there, and a place that cannot be read fails the turn plainly (route); a turn
//! no message opened is assembled with one line saying what woke it (continuation); a failed read fails
//! the turn with its words (assembly error); a gate that moves a third time within one call fails the call
//! (moved). None of them sends a request it could not stand behind, and none writes a fallback row.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};

use serde_json::json;

use crate::config::assembly::{Assembly, AssemblySetting};
use crate::provider::{ProviderError, ProviderRequest, Scripted};
use crate::tests_places::{answers_a_call, last_user, rig_setup_scripted, rig_with, session, Rig, LAB};
use crate::tests_private_view::session_in;
use crate::tests_view_compile::{fallbacks, legal, submit, texts};

fn view(c: &mut crate::Config) {
    c.context.assembly = Some(AssemblySetting::All(Assembly::View));
}

/// The `context.compiled` rows of `sid`, oldest first: (turn, strategy).
fn compiled(c: &crate::Core, sid: &str) -> Vec<(String, String)> {
    (crate::test_support::rows(c, "context.compiled").into_iter())
        .filter(|r| r["session_id"] == sid)
        .map(|r| {
            let s = |k: &str| r[k].as_str().unwrap_or_default().to_string();
            (s("turn_id"), s("strategy"))
        })
        .collect()
}

/// The failure a turn ended with: its class and its words.
fn failure(e: &anyhow::Error) -> (String, String) {
    let te = e.downcast_ref::<crate::turn::TurnError>().expect("a turn's failure");
    (te.class.clone(), format!("{:#}", te.source))
}

/// A turn of `sid` with no mark: where it arrived is not said, so its place is where its reply goes.
async fn unmarked(c: &Arc<crate::Core>, sid: &str, input: &str) -> anyhow::Result<()> {
    use crate::session::SessionRecord;
    let rec = (c.store.get_session::<SessionRecord>(sid).unwrap()).unwrap();
    let (live, _) = c.live_profile();
    let target = c.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = crate::bus::EventSink::new(c.bus.clone(), sid, None);
    c.runner
        .run(crate::turn::TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "cli".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
        .map(|_| ())
}

/// (b) route: a session that posts nowhere now (the channel moved on to a new session) answers its wake
/// where the wake was set from. That one place is the wake turn's exchange's AND its gate's anchor, so the
/// turn takes the view there, with no hand-back.
#[tokio::test]
async fn a_wake_in_a_session_that_posts_nowhere_takes_the_view_where_its_answer_goes() {
    let r = rig_with(view);
    let c = &r.core;
    let old = session(c, Some(&format!("channel:{LAB}")));
    crate::tests_places::turn(c, &old, "WAKE in a second").await;
    let _new = session(c, Some(&format!("channel:{LAB}")));
    assert_eq!(c.outbox.target(&old), None, "the old session posts nowhere");
    r.until("the wake's turn", 15, |r| !r.asked("check again").is_empty())
        .await;
    r.until("the wake turn's compile row", 15, |_| compiled(c, &old).len() >= 2)
        .await;
    let (turn, strategy) = compiled(c, &old).pop().unwrap();
    assert_eq!(strategy, "view", "the wake's turn is the view's");
    let goes = c.outbox.try_wake_target(&old).unwrap().expect("a wake target");
    let ex = crate::exchange::exchange(&c.store, &turn).unwrap().unwrap();
    assert_eq!(ex.place, goes, "its exchange is where its answer goes");
    let rec = crate::view::record::of_turn(&c.store, &turn).unwrap();
    let anchor = &rec.first().expect("its view's record").pov.anchor;
    assert_eq!(
        *anchor,
        crate::view::Anchor::Place(goes),
        "its gate's anchor is the same place"
    );
    assert!(fallbacks(c).is_empty(), "{:?}", fallbacks(c));
}

/// (b) route: a turn whose place cannot be read (no mark, and its route unreadable) fails plainly, saying
/// so, and no request is sent.
#[tokio::test]
async fn a_place_that_cannot_be_read_fails_the_turn_plainly_and_sends_nothing() {
    let r = rig_with(view);
    let c = &r.core;
    let sid = session_in(c, None, &r.root);
    crate::turn::view_step::route_unreadable(&sid);
    let e = unmarked(c, &sid, "where does this go?").await.unwrap_err();
    let (class, words) = failure(&e);
    assert_eq!(class, "context_unreadable");
    assert!(
        words.contains("this conversation's place could not be read"),
        "{words}"
    );
    assert!(
        (r.requests().iter()).all(|q| !texts(q).contains("where does this go?")),
        "nothing is sent"
    );
    assert!(fallbacks(c).is_empty(), "{:?}", fallbacks(c));
}

/// (a) continuation: a background job ends after the turn that started it ended. The turn its late
/// result wakes (no message of its own) takes the view: an empty message, one line beside it saying a
/// job ended, the job's output as its tail, and it answers.
#[tokio::test]
async fn a_continuation_after_a_jobs_end_takes_the_view_and_answers() {
    const ENDED: &str = "a background job this conversation started has ended";
    let r = rig_setup_scripted(
        |c| {
            view(c);
            c.tools.proc_sync_secs = 1;
        },
        |_| {},
        |q| {
            if texts(q).contains(ENDED) {
                return Scripted::text("The tide script finished: TIDE-7731.");
            }
            if answers_a_call(q) {
                return Scripted::text("Started; it runs on.");
            }
            if last_user(q).contains("run the long tide script") {
                return Scripted::tools(
                    "",
                    &[(
                        "j1",
                        "proc_run",
                        json!({"argv": ["sh", "-c", "sleep 1.6; echo TIDE-7731"]}),
                    )],
                );
            }
            Scripted::text("Hello.")
        },
    );
    let c = &r.core;
    let sid = session_in(c, None, &r.root);
    crate::tests_places::turn(c, &sid, "run the long tide script").await;
    r.until("the job's turn", 30, |r| {
        r.requests().iter().any(|q| texts(q).contains(ENDED))
    })
    .await;
    let q = (r.requests().into_iter())
        .find(|q| texts(q).contains(ENDED))
        .unwrap();
    legal(&q);
    assert!(texts(&q).contains("TIDE-7731"), "the job's output: {}", texts(&q));
    r.until("its answer", 15, |_| {
        (c.store.session_nodes(&sid).unwrap().iter()).any(|(_, n)| {
            matches!(&n.body, crate::node::Body::AssistantMessage { .. })
                && serde_json::to_string(&n.body).unwrap().contains("finished: TIDE-7731")
        })
    })
    .await;
    let rows = compiled(c, &sid);
    assert!(rows.len() >= 2, "{rows:?}");
    assert!(rows.iter().all(|(_, s)| s == "view"), "{rows:?}");
    assert!(fallbacks(c).is_empty(), "{:?}", fallbacks(c));
}

/// (a) continuation: a turn whose call failed (the provider overloaded) is resumed by the driver
/// (`continue_execution`): no message of its own, so it takes the view with one line saying it resumes,
/// and the interrupted exchange (ended, failed) is in its window with the question it never answered.
#[tokio::test]
async fn a_turn_resumed_after_an_interruption_takes_the_view() {
    const RESUMES: &str = "it resumes work an earlier turn left unfinished";
    let calls = Arc::new(AtomicU32::new(0));
    let seen = calls.clone();
    let r = rig_setup_scripted(view, |_| {}, move |q| {
        if q.tools.is_empty() {
            return Scripted::text("aside");
        }
        match seen.fetch_add(1, Ordering::SeqCst) {
            0 => Scripted::Fail(ProviderError::Overloaded {
                message: "overloaded_error: Overloaded".into(),
            }),
            _ => Scripted::text("High tide is at four."),
        }
    });
    let c = &r.core;
    let sid = session_in(c, None, &r.root);
    let _ = submit(c, &sid, "when is high tide at the harbour?").await;
    let resumed = |r: &Rig| r.requests().iter().any(|q| texts(q).contains(RESUMES));
    // The driver resumes it on its own backoff; else it is resumed here, as the driver would.
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    if !resumed(&r) {
        let rec: crate::session::SessionRecord = c.store.get_session(&sid).unwrap().unwrap();
        let _ = c.continue_execution(&rec.execution_id.unwrap()).await;
    }
    r.until("the resumed turn", 30, |r| {
        r.requests().iter().any(|q| texts(q).contains(RESUMES))
    })
    .await;
    let q = (r.requests().into_iter())
        .find(|q| texts(q).contains(RESUMES))
        .unwrap();
    legal(&q);
    assert!(
        texts(&q).contains("when is high tide at the harbour?"),
        "the interrupted exchange is in the window: {}",
        texts(&q)
    );
    r.until("the resumed turn's compile row", 15, |_| !compiled(c, &sid).is_empty())
        .await;
    let rows = compiled(c, &sid);
    assert!(rows.iter().all(|(_, s)| s == "view"), "{rows:?}");
    assert!(fallbacks(c).is_empty(), "{:?}", fallbacks(c));
}

/// (e) assembly error: a read that fails during the view's assembly fails the turn with the error's
/// words; never a partial view, and nothing is sent.
#[tokio::test]
async fn a_read_failure_during_assembly_fails_the_turn_with_its_words() {
    let r = rig_with(view);
    let c = &r.core;
    let sid = session_in(c, None, &r.root);
    crate::turn::view_step::fail_next_assembly(&sid);
    let e = submit(c, &sid, "what is on the desk?").await.unwrap_err();
    let (class, words) = failure(&e);
    assert_eq!(class, "context_unreadable");
    assert!(
        words.contains("the conversation's context could not be read: ")
            && words.contains("did not decode"),
        "{words}"
    );
    assert!(
        (r.requests().iter()).all(|q| !texts(q).contains("what is on the desk?")),
        "nothing is sent"
    );
    assert!(fallbacks(c).is_empty(), "{:?}", fallbacks(c));
}

/// (f) moved: the gate's reading of the turn's presence moves after each of three assemblies of one call
/// (after the view's first, and after each reassembly). The call fails plainly, and nothing is sent.
#[tokio::test]
async fn a_third_move_in_one_call_fails_it_plainly_and_sends_nothing() {
    use theseus_memory::gate::Present;
    let r: Rig = rig_with(view);
    let c = &r.core;
    let sid = session_in(c, None, &r.root);
    let core: Arc<OnceLock<Arc<crate::Core>>> = Arc::new(OnceLock::new());
    let _ = core.set(c.clone());
    for present in [Present::PrivateChannel, Present::Shared, Present::Open] {
        let (held, s) = (core.clone(), sid.clone());
        crate::turn::view_step::env_tail_hook(
            &sid,
            Box::new(move || {
                let c = held.get().unwrap();
                let now = c.runner.presence.get(&s).unwrap();
                let (turn, place) = (now.turn_id().to_string(), now.place().to_string());
                crate::view_gate::seat_for_tests(&c.runner, &s, &turn, present, &place);
            }),
        );
    }
    let e = submit(c, &sid, "tidy the harbour notes").await.unwrap_err();
    let (class, words) = failure(&e);
    assert_eq!(class, "context_unreadable");
    assert!(
        words.contains("what this turn may see changed while it was being prepared; send it again"),
        "{words}"
    );
    let sent: Vec<ProviderRequest> = (r.requests().into_iter())
        .filter(|q| texts(q).contains("tidy the harbour notes"))
        .collect();
    assert!(sent.is_empty(), "nothing is sent: {}", sent.len());
    assert!(fallbacks(c).is_empty(), "{:?}", fallbacks(c));
}

/// (a) continuation, after a restart: the turn was cut before its exchange ended (as a crash leaves it: the
/// exchange opened, no end, no stop), and the next start's driver resumes its execution
/// (`continue_execution`). The resumed turn takes the view with its occasion line, and the cut exchange,
/// though it never ended, is in its window with the question it never answered.
#[tokio::test]
async fn a_turn_resumed_after_a_restart_shows_the_cut_exchange_in_its_window() {
    const RESUMES: &str = "it resumes work an earlier turn left unfinished";
    const ASKED: &str = "when does the ferry leave the harbour?";
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let mut cfg = crate::Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    view(&mut cfg);
    let fake = Arc::new(crate::provider::FakeProvider::scripted(vec![
        Scripted::Fail(ProviderError::Overloaded {
            message: "overloaded_error: Overloaded".into(),
        }),
        Scripted::text("The ferry leaves at six."),
    ]));
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    // No driver runs here: the resume below is the one a restart's driver makes.
    let c = crate::Core::build(crate::rpc::Parts::for_tests(cfg, fake.clone(), store)).unwrap();
    let sid = session_in(&c, None, &root);
    let _ = submit(&c, &sid, ASKED).await;
    let turn = (c.store.session_nodes(&sid).unwrap().into_iter())
        .find(|(_, n)| matches!(n.body, crate::node::Body::UserMessage { .. }))
        .and_then(|(_, n)| n.turn_id)
        .unwrap();
    let mut ex = crate::exchange::exchange(&c.store, &turn).unwrap().unwrap();
    (ex.end_ms, ex.stop) = (None, None);
    c.store
        .append(&[crate::exchange::exchange_record(&ex).unwrap()])
        .unwrap();
    let rec: crate::session::SessionRecord = c.store.get_session(&sid).unwrap().unwrap();
    let res = c
        .continue_execution(&rec.execution_id.unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(res.output, "The ferry leaves at six.");
    let q = (fake.requests().into_iter())
        .rev()
        .find(|q| !q.tools.is_empty())
        .unwrap();
    legal(&q);
    assert!(texts(&q).contains(RESUMES), "{}", texts(&q));
    assert!(
        texts(&q).contains(ASKED),
        "the cut exchange is in the window: {}",
        texts(&q)
    );
    let rows = compiled(&c, &sid);
    assert!(rows.iter().all(|(_, s)| s == "view"), "{rows:?}");
    assert!(fallbacks(&c).is_empty(), "{:?}", fallbacks(&c));
}
