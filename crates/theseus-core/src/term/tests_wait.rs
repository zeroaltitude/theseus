//! A read's wait never loses the end it is waiting for (issue 20: a terminal test failed by chance
//! with "quiet for 5000 ms after 5001 ms" beside a header that said the program was ended by
//! signal 2).
//!
//! The wait looks (did the program hang up, does the screen show the text) and then waits for the
//! screen's next change. A change that lands between the two, when the look had not yet seen it,
//! was erased by the mark that began the wait, and the wait ran out its whole quiet on a program
//! that had ended. The hook of [`Wait::looked`] runs in exactly that gap, so the test makes the
//! change land there, every time.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde_json::json;

use super::tests::{call, id_of, terms};
use super::tools::{wait, Wait};
use super::*;

/// Once, at the gap: send `bytes` to the program and wait until the reader has seen what follows
/// (`done`) and published it.
fn at_the_gap<'a>(
    t: &'a Terminal,
    bytes: &'a [u8],
    done: impl Fn() -> bool + Sync + 'a,
) -> impl Fn() + Sync + 'a {
    let first = AtomicBool::new(true);
    move || {
        if !first.swap(false, Ordering::SeqCst) {
            return;
        }
        let seen = *t.pty.shared.version.borrow();
        t.pty.send(bytes).unwrap();
        let t0 = Instant::now();
        while !(done() && *t.pty.shared.version.borrow() != seen) {
            assert!(
                t0.elapsed() < Duration::from_secs(10),
                "the change never came"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

/// KEY: the program ends and hangs up between the wait's look and its wait: the wait says so, at
/// once, and does not run out its quiet.
#[tokio::test]
async fn a_program_that_ends_between_the_look_and_the_wait_is_not_waited_out() {
    let d = tempfile::tempdir().unwrap();
    let terms = terms();
    let (o, _) = call(&terms, "s1", OPEN, json!({"argv": ["/bin/cat"]}), d.path()).await;
    let t = terms.get(&id_of(&o)).unwrap();
    let hook = at_the_gap(&t, b"\x03", || t.pty.shared.hung_up.load(Ordering::Relaxed));
    let w = Wait {
        quiet: Some(Duration::from_millis(300)),
        looked: Some(&hook),
        ..Wait::default()
    };
    let steer = terms.steers.watch("s1");
    let t0 = Instant::now();
    let (said, _) = wait(&t, w, Duration::from_secs(10), &steer).await;
    assert!(said.starts_with("its program ended"), "{said}");
    assert!(t0.elapsed() < Duration::from_millis(250), "{said}");
    terms.close_session("s1", BY_SESSION_END);
}

/// KEY: the text a wait is for lands between the look and the wait: it is seen, not waited out to
/// the bound.
#[tokio::test]
async fn a_text_that_appears_between_the_look_and_the_wait_is_not_waited_out() {
    let d = tempfile::tempdir().unwrap();
    let terms = terms();
    let (o, _) = call(&terms, "s1", OPEN, json!({"argv": ["/bin/cat"]}), d.path()).await;
    let t = terms.get(&id_of(&o)).unwrap();
    let shows = || {
        t.pty
            .shared
            .screen
            .lock()
            .unwrap()
            .rows()
            .join("\n")
            .contains("hello")
    };
    let hook = at_the_gap(&t, b"hello\n", shows);
    let w = Wait {
        until: Some("hello"),
        looked: Some(&hook),
        ..Wait::default()
    };
    let steer = terms.steers.watch("s1");
    let t0 = Instant::now();
    let (said, _) = wait(&t, w, Duration::from_secs(10), &steer).await;
    assert!(said.contains("\"hello\" appeared"), "{said}");
    assert!(t0.elapsed() < Duration::from_secs(5), "{said}");
    terms.close_session("s1", BY_SESSION_END);
}
