//! An address that cannot be honoured is refused, never run at home (issue
//! 44): `/t` with a word that names no thread, a word that names a paused or
//! done thread, and an addressed thread that does not open each end
//! `turn.submit` with one plain line, and no turn starts anywhere. A sealed
//! thread is refused as no thread at all.

use super::*;
use theseus_protocol::error_code;
use theseus_protocol::threads::ThreadSealParams;

/// `turn.submit`'s answer, whatever it is.
async fn answer(c: &Arc<Core>, session: &str, input: &str) -> Value {
    let mut w = Wire::open(c);
    let id = w
        .send(method::TURN_SUBMIT, params(session, input, None))
        .await;
    w.response(id).await
}

/// The refusal's line: the answer is a refusal, with no result.
fn refusal(r: &Value) -> String {
    assert_eq!(r["error"]["code"], json!(error_code::REFUSED), "{r}");
    assert!(r["result"].is_null(), "{r}");
    r["error"]["message"].as_str().unwrap().to_string()
}

/// No turn started anywhere, and nothing was written in the place's session.
fn nothing_ran(c: &Core, home: &str, turns_before: usize) {
    assert_eq!(
        rows(c, "turn.started").len(),
        turns_before,
        "a turn started"
    );
    assert!(texts_of(c, home).is_empty(), "the message ran at home");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_address_that_names_no_thread_is_refused_and_runs_nowhere() {
    let jev = FakeJev::start().unwrap();
    let r = rig(&jev, 4);
    let c = &r.core;
    two_threads(c);
    let dm = home(c, Some(&format!("dm:{OWNER}")));
    let before = rows(c, "turn.started").len();
    let line = refusal(&answer(c, &dm, "/t nosuch check the lint run").await);
    assert_eq!(
        line,
        "Not run: there is no thread \u{201c}nosuch\u{201d}. Send it again with a thread's \
         name, or without one to talk here."
    );
    nothing_ran(c, &dm, before);
    // A `#word` that names no thread is no address: it runs at home, as before.
    thread_answer(&jev, "pr-round", 0.5);
    let tagged = submit(c, &dm, "#nosuch is a tag, not a thread").await;
    assert_eq!(tagged.session_id, dm);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_address_to_a_paused_or_done_thread_is_refused_and_runs_nowhere() {
    let jev = FakeJev::start().unwrap();
    let r = rig(&jev, 4);
    let c = &r.core;
    two_threads(c);
    let dm = home(c, Some(&format!("dm:{OWNER}")));
    let before = rows(c, "turn.started").len();
    c.thread_pause("pr-round", cli()).unwrap();
    for input in [
        "/t pr-round check the lint run",
        "#pr-round check the lint run",
    ] {
        let line = refusal(&answer(c, &dm, input).await);
        assert_eq!(
            line,
            "Not run: the thread \u{201c}pr-round\u{201d} is paused: `theseus thread resume \
             pr-round` lets it take messages. Send it again then, or without the thread's \
             name to talk here.",
            "{input}"
        );
    }
    // With no thread live in the place, the address is still refused.
    c.thread_done("security-fix", cli()).unwrap();
    let line = refusal(&answer(c, &dm, "In the security fix thread: and the vault?").await);
    assert!(
        line.starts_with("Not run: the thread \u{201c}security fix\u{201d} is done: `theseus thread reopen security-fix`"),
        "{line}"
    );
    nothing_ran(c, &dm, before);
    assert!(c.threads().get("pr-round").unwrap().session_id.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_addressed_thread_that_does_not_open_is_refused_and_runs_nowhere() {
    let jev = FakeJev::start().unwrap();
    let r = rig(&jev, 4);
    let c = &r.core;
    two_threads(c);
    let dm = home(c, Some(&format!("dm:{OWNER}")));
    let before = rows(c, "turn.started").len();
    // The thread's first message opens its session and binds the thread's
    // place to it: that binding's write fails, as a full disk would.
    c.store.fail_plain_frame(|recs| {
        recs.iter().any(|r| {
            r.kind == theseus_store::kinds::META
                && r.key.as_deref().is_some_and(|k| {
                    k.starts_with(crate::outbox::PLACE_META_PREFIX) && k.ends_with("security-fix")
                })
        })
    });
    let line = refusal(&answer(c, &dm, "#security-fix rotate the key").await);
    assert!(
        line.starts_with("Not run: the thread \u{201c}security-fix\u{201d} could not be opened ("),
        "{line}"
    );
    nothing_ran(c, &dm, before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sealed_thread_is_refused_as_no_thread_at_all() {
    let jev = FakeJev::start().unwrap();
    let r = rig(&jev, 4);
    let c = &r.core;
    two_threads(c);
    let dm = home(c, Some(&format!("dm:{OWNER}")));
    let before = rows(c, "turn.started").len();
    c.thread_seal(
        &ThreadSealParams {
            slug: "security-fix".into(),
            ..Default::default()
        },
        cli(),
    )
    .unwrap();
    c.thread_pause("security-fix", cli()).unwrap();
    let sealed = refusal(&answer(c, &dm, "/t security-fix rotate the key").await);
    let missing = refusal(&answer(c, &dm, "/t security-fiz rotate the key").await);
    assert_eq!(
        sealed,
        missing.replace("security-fiz", "security-fix"),
        "the refusal does not tell that a sealed thread exists"
    );
    nothing_ran(c, &dm, before);
}
