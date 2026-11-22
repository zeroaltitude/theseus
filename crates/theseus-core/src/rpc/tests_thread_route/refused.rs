//! An address that cannot be honoured is refused, never run at home (issue
//! 44): `/t` with a word that names no thread, a word that names a paused or
//! done thread, and an addressed thread that does not open each end
//! `turn.submit` with one plain line, and no turn starts anywhere. A sealed
//! thread is refused as no thread at all.

use super::*;
use theseus_protocol::error_code;
use theseus_protocol::threads::{ThreadEditParams, ThreadSealParams};

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
    let opened = rows(c, "session.opened").len();
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
    // The session and its place's binding are one frame: neither is left.
    assert_eq!(rows(c, "session.opened").len(), opened, "an orphan session");
    assert_eq!(c.outbox.place_session("thread:security-fix").unwrap(), None);
    assert!(c
        .threads()
        .get("security-fix")
        .unwrap()
        .session_id
        .is_none());
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

/// What `turn.submit` did with `input`: its refusal's line, or where it ran.
async fn outcome(c: &Arc<Core>, dm: &str, input: &str, reply: Option<&str>) -> String {
    let mut w = Wire::open(c);
    let id = w.send(method::TURN_SUBMIT, params(dm, input, reply)).await;
    let r = w.response(id).await;
    match r["error"]["message"].as_str() {
        Some(line) => format!("refused: {line}"),
        None if r["result"]["session_id"] == json!(dm) => "ran at home".into(),
        None => format!("ran elsewhere: {r}"),
    }
}

/// Edits a thread's record as its writer does.
fn set(c: &Core, slug: &str, f: impl Fn(&mut theseus_protocol::threads::ThreadInfo)) {
    c.runner
        .threads
        .write(
            &c.store,
            |board| {
                let mut t = board.get(slug).unwrap().clone();
                f(&mut t);
                Ok(vec![t])
            },
            |_, _| Ok(vec![]),
        )
        .unwrap();
}

/// A thread the gate does not let this reader be told of (sealed, a seal
/// proposed, or one whose seal cannot be read), paused, is to every address
/// form exactly what no thread is: `/t` gets the same "no thread" line,
/// `#word` and "in the … thread:" run at home, and a reply to its turn runs
/// at home as a reply to an unknown turn does. Renamed before its seal, its
/// new title shows nowhere.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_thread_this_reader_may_not_be_told_of_is_no_thread_in_every_form() {
    let jev = FakeJev::start().unwrap();
    let r = rig(&jev, 24);
    let c = &r.core;
    two_threads(c);
    let ws0 = || ws(&[], &[], &[]);
    thread(c, "embargo", "ledger fix", ws0());
    thread(c, "pending", "pending work", ws0());
    thread(c, "lost", "lost work", ws0());
    let dm = home(c, Some(&format!("dm:{OWNER}")));
    thread_answer(&jev, "pr-round", 0.5);
    let earlier = submit(c, &dm, "#embargo what is left").await;
    assert_eq!(earlier.session_id, session_of(c, "embargo"));
    c.thread_edit(
        &ThreadEditParams {
            slug: "embargo".into(),
            title: Some("vault keys".into()),
            ..Default::default()
        },
        cli(),
    )
    .unwrap();
    c.thread_seal(
        &ThreadSealParams {
            slug: "embargo".into(),
            ..Default::default()
        },
        cli(),
    )
    .unwrap();
    for slug in ["embargo", "pending", "lost"] {
        c.thread_pause(slug, cli()).unwrap();
    }
    set(c, "pending", |t| t.seal_pending = Some("proposed".into()));
    set(c, "lost", |t| t.merged_into = Some("nowhere".into()));
    for (slug, title) in [
        ("embargo", "vault keys"),
        ("pending", "pending work"),
        ("lost", "lost work"),
    ] {
        for (form, absent) in [
            (format!("/t {slug} do it"), "/t nothere do it".to_string()),
            (format!("#{slug} do it"), "#nothere do it".to_string()),
            (
                format!("in the {title} thread: do it"),
                "in the nothing here thread: do it".to_string(),
            ),
        ] {
            let hidden = outcome(c, &dm, &form, None).await.replace(slug, "nothere");
            let none = outcome(c, &dm, &absent, None).await;
            assert_eq!(hidden, none, "{form}");
            assert!(
                !hidden.contains(title) && !hidden.contains("paused"),
                "{hidden}"
            );
        }
    }
    assert_eq!(
        outcome(c, &dm, "/t embargo do it", None).await,
        "refused: Not run: there is no thread \u{201c}embargo\u{201d}. Send it again with a \
         thread's name, or without one to talk here."
    );
    let reply = outcome(c, &dm, "and the vault?", Some(&earlier.turn_id)).await;
    let unknown = outcome(c, &dm, "and the vault?", Some("turn_unknown")).await;
    assert_eq!(
        (reply.as_str(), unknown.as_str()),
        ("ran at home", "ran at home")
    );
}

/// A failed explicit address is refused before the reply and the place are
/// read: `/t missing` or `/t <paused>` as a reply to a live thread's turn, or
/// in a place a live thread alone lists, runs nowhere, never in that thread.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_explicit_address_never_falls_through_to_the_reply_or_the_place() {
    let jev = FakeJev::start().unwrap();
    let r = rig(&jev, 4);
    let c = &r.core;
    two_threads(c);
    let dm = home(c, Some(&format!("dm:{OWNER}")));
    let a = submit(c, &dm, "#pr-round check the lint run").await;
    c.thread_pause("security-fix", cli()).unwrap();
    let before = rows(c, "turn.started").len();
    let ran_in_a = texts_of(c, &session_of(c, "pr-round")).len();
    for input in ["/t missing do X", "/t security-fix do X"] {
        let line = refusal(&{
            let mut w = Wire::open(c);
            let id = w
                .send(method::TURN_SUBMIT, params(&dm, input, Some(&a.turn_id)))
                .await;
            w.response(id).await
        });
        assert!(line.starts_with("Not run: "), "{input}: {line}");
    }
    set(c, "pr-round", |t| {
        t.surfaces = vec![format!("discord:dm:{OWNER2}")]
    });
    let bound = home(c, Some(&format!("dm:{OWNER2}")));
    for input in ["/t missing do X", "/t security-fix do X"] {
        let line = refusal(&answer(c, &bound, input).await);
        assert!(line.starts_with("Not run: "), "{input}: {line}");
    }
    nothing_ran(c, &dm, before);
    assert!(texts_of(c, &bound).is_empty());
    assert_eq!(texts_of(c, &session_of(c, "pr-round")).len(), ran_in_a);
}

/// An explicit address is read before the turn a message replies to, and
/// nothing of a reply's target is said before the gate: `/t <live>` on a
/// reply to another thread's turn runs in `<live>`; `/t missing` on a reply
/// to a sealed thread's turn gets the plain missing line, as on a reply to an
/// unknown turn; and in a place that may name no thread, a reply to the
/// sealed thread's turn runs exactly as a reply to an unknown turn does.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_explicit_address_comes_before_the_reply_and_a_hidden_reply_reads_as_unknown() {
    let jev = FakeJev::start().unwrap();
    let r = rig(&jev, 10);
    let c = &r.core;
    two_threads(c);
    thread(c, "embargo", "ledger fix", ws(&[], &[], &[]));
    let dm = home(c, Some(&format!("dm:{OWNER}")));
    thread_answer(&jev, "pr-round", 0.5);
    let in_pr = submit(c, &dm, "#pr-round check the lint run").await;
    let in_embargo = submit(c, &dm, "#embargo what is left").await;
    c.thread_seal(
        &ThreadSealParams {
            slug: "embargo".into(),
            ..Default::default()
        },
        cli(),
    )
    .unwrap();
    c.thread_pause("embargo", cli()).unwrap();
    let mut w = Wire::open(c);
    let id = w
        .send(
            method::TURN_SUBMIT,
            params(&dm, "/t security-fix rotate the key", Some(&in_pr.turn_id)),
        )
        .await;
    let res: TurnSubmitResult =
        serde_json::from_value(w.response(id).await["result"].clone()).expect("the turn ran");
    assert_eq!(res.session_id, session_of(c, "security-fix"));
    let hidden = outcome(c, &dm, "/t missing do X", Some(&in_embargo.turn_id)).await;
    let unknown = outcome(c, &dm, "/t missing do X", Some("turn_unknown")).await;
    assert_eq!(hidden, unknown);
    assert!(
        hidden.starts_with("refused: Not run: there is no thread \u{201c}missing\u{201d}"),
        "{hidden}"
    );
    let shared = home(c, Some("channel:777"));
    let hidden = outcome(c, &shared, "and then?", Some(&in_embargo.turn_id)).await;
    let unknown = outcome(c, &shared, "and then?", Some("turn_unknown")).await;
    assert_eq!(
        (hidden.as_str(), unknown.as_str()),
        ("ran at home", "ran at home")
    );
}
