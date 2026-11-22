//! A reply to a thread's earlier response finds its thread after a restart
//! (issue 44): the router's memory of turns is a cache, and a miss reads the
//! turn's `thread.routed` row. A reply to an old home response runs at home;
//! a reply to a thread that is now done is refused, as an address to it is
//! (`refused.rs`); one now sealed and paused runs at home as a reply to an
//! unknown turn does.

use super::*;
use crate::provider::FakeProvider;
use crate::rpc::Parts;
use theseus_protocol::error_code;
use theseus_protocol::threads::ThreadSealParams;

/// A second core over `c`'s store, as a restarted daemon's: no memory of
/// the turns the first ran.
fn restarted(c: &Core, n: usize) -> Arc<Core> {
    let fake = Arc::new(FakeProvider::scripted(crate::tests_judge::texts(n)));
    let mut p = Parts::for_tests((*c.cfg).clone(), fake, c.store.clone());
    p.secrets = crate::tests_judge::board();
    let core = Core::build(p).unwrap();
    crate::tests_judge::warm(&core);
    core
}

/// A reply's `turn.submit` answer, whatever it is.
async fn reply(c: &Arc<Core>, session: &str, input: &str, to: &str) -> Value {
    let mut w = Wire::open(c);
    let id = w
        .send(method::TURN_SUBMIT, params(session, input, Some(to)))
        .await;
    w.response(id).await
}

fn ran(r: &Value) -> TurnSubmitResult {
    serde_json::from_value(r["result"].clone()).unwrap_or_else(|_| panic!("a turn result in {r}"))
}

fn refused(r: &Value) -> String {
    assert_eq!(r["error"]["code"], json!(error_code::REFUSED), "{r}");
    r["error"]["message"].as_str().unwrap().to_string()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reply_to_an_old_response_finds_its_thread_after_a_restart() {
    let jev = FakeJev::start().unwrap();
    let r = rig(&jev, 4);
    let c = &r.core;
    two_threads(c);
    let dm = home(c, Some(&format!("dm:{OWNER}")));
    let in_thread = submit(c, &dm, "#pr-round check the lint run").await;
    let sealed_later = submit(c, &dm, "#security-fix rotate the key").await;
    thread_answer(&jev, "pr-round", 0.5);
    let at_home = submit(c, &dm, "what is the weather like").await;
    assert_eq!(at_home.session_id, dm);

    let c = &restarted(c, 4);
    assert_eq!(
        c.runner.threads.route.thread_of_turn(&in_thread.turn_id),
        None,
        "the restarted core holds no turn"
    );
    // A reply to the thread's old response runs in that thread.
    let again = ran(&reply(c, &dm, "why is the build red", &in_thread.turn_id).await);
    assert_eq!(again.session_id, session_of(c, "pr-round"));
    assert_eq!(tag(&again).how, RouteHow::Reply);
    // A reply to an old home response runs at home, as before.
    thread_answer(&jev, "pr-round", 0.5);
    let home_again = ran(&reply(c, &dm, "and tomorrow?", &at_home.turn_id).await);
    assert_eq!(home_again.session_id, dm);
    assert_eq!(tag(&home_again).how, RouteHow::Home);

    // A reply to a thread now done is refused, and no turn starts.
    let before = rows(c, "turn.started").len();
    c.thread_done("pr-round", cli()).unwrap();
    let line = refused(&reply(c, &dm, "one more thing", &in_thread.turn_id).await);
    assert!(
        line.starts_with("Not run: the thread \u{201c}pr round\u{201d} is done"),
        "{line}"
    );
    assert_eq!(rows(c, "turn.started").len(), before, "a turn started");
    // One sealed and paused is no thread to this reader: the reply runs at
    // home exactly as a reply to a turn no record names does, and its name
    // shows nowhere.
    c.thread_seal(
        &ThreadSealParams {
            slug: "security-fix".into(),
            ..Default::default()
        },
        cli(),
    )
    .unwrap();
    c.thread_pause("security-fix", cli()).unwrap();
    let sealed = ran(&reply(c, &dm, "and the vault?", &sealed_later.turn_id).await);
    let unknown = ran(&reply(c, &dm, "and the vault?", "turn_unknown").await);
    assert_eq!(
        (sealed.session_id.as_str(), unknown.session_id.as_str()),
        (dm.as_str(), dm.as_str())
    );
    assert_eq!(sealed.thread, unknown.thread);
}

/// A thread whose slug is `home` is told apart from the place's own
/// conversation after a restart: by the row's `thread` field from now on,
/// and by its `session` in a row an older build wrote.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_thread_named_home_is_found_after_a_restart_beside_a_home_reply() {
    let jev = FakeJev::start().unwrap();
    let r = rig(&jev, 4);
    let c = &r.core;
    two_threads(c);
    thread(c, "home", "home base", ws(&[], &[], &[]));
    let dm = home(c, Some(&format!("dm:{OWNER}")));
    let in_home_thread = submit(c, &dm, "#home check the lint run").await;
    assert_eq!(in_home_thread.session_id, session_of(c, "home"));
    thread_answer(&jev, "pr-round", 0.5);
    let at_home = submit(c, &dm, "what is the weather like").await;
    assert_eq!(at_home.session_id, dm);
    let c = &restarted(c, 4);
    let a = ran(&reply(c, &dm, "and then?", &in_home_thread.turn_id).await);
    assert_eq!(a.session_id, session_of(c, "home"), "the thread named home");
    let b = ran(&reply(c, &dm, "and tomorrow?", &at_home.turn_id).await);
    assert_eq!(b.session_id, dm, "the place's own conversation");
}

/// An older row (no `thread` field) says "home" for both: its session tells.
#[test]
fn an_older_rows_home_is_read_by_its_session() {
    use crate::fact::thread_route::routed_dest;
    let at_home = json!({"home": "ses_a", "to": "home", "session": "ses_a"});
    let named_home = json!({"home": "ses_a", "to": "home", "session": "ses_b"});
    let other = json!({"home": "ses_a", "to": "pr-round", "session": "ses_c"});
    assert_eq!(routed_dest(&at_home), Some(None));
    assert_eq!(routed_dest(&named_home), Some(Some("home".into())));
    assert_eq!(routed_dest(&other), Some(Some("pr-round".into())));
    let new_home = json!({"home": "ses_a", "to": "home", "thread": "home", "session": "ses_b"});
    let new_at_home = json!({"home": "ses_a", "to": "home", "thread": null, "session": "ses_a"});
    assert_eq!(routed_dest(&new_home), Some(Some("home".into())));
    assert_eq!(routed_dest(&new_at_home), Some(None));
}
