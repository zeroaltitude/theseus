//! An ended terminal frees its slot (theseus-ggqf): a session holds at most
//! `PER_SESSION` terminals whose programs run, an ended one's last screen
//! stays readable, and an open that finds the session full reclaims the
//! oldest ended one and names it.

use serde_json::json;

use super::tests::{call, fails, id_of, terms};
use super::*;

/// Wait until terminal `id`'s program has ended.
async fn until_ended(terms: &Arc<Terms>, id: &str) {
    let t0 = std::time::Instant::now();
    while !terms.get(id).unwrap().ended() {
        assert!(t0.elapsed() < Duration::from_secs(10), "{id} never ended");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Four ended `bash -c` terminals do not block a fifth open: the open
/// reclaims the oldest, names it, and its close is a `term.closed` in the
/// result's meta; the others' last screens stay readable. A session with
/// four running is still refused, with why.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ended_terminals_free_their_slots_and_the_oldest_is_reclaimed() {
    let d = tempfile::tempdir().unwrap();
    let terms = terms(&[]);
    let mut ended = Vec::new();
    for i in 0..PER_SESSION {
        let (o, _) = call(
            &terms,
            "s1",
            OPEN,
            json!({"argv": ["bash", "-c", format!("echo done-{i}")], "quiet_ms": 0}),
            d.path(),
        )
        .await;
        let id = id_of(&o);
        until_ended(&terms, &id).await;
        ended.push(id);
    }
    // Health says each has ended, and that its slot is free.
    let info = terms.get(&ended[1]).unwrap().info();
    assert!(!info.running);
    assert!(
        theseus_protocol::term::health_line(&info, info.opened_at_unix_ms)
            .contains("its program has ended, its slot free"),
    );
    let (o, _) = call(
        &terms,
        "s1",
        OPEN,
        json!({"argv": ["cat"], "quiet_ms": 0}),
        d.path(),
    )
    .await;
    let fifth = id_of(&o);
    assert!(
        o.text.contains(&format!(
            "Opened in the slot of terminal {} (bash), whose program had ended (exit 0)",
            ended[0]
        )),
        "{}",
        o.text
    );
    assert_eq!(o.meta["reclaimed"]["terminal"], ended[0].as_str());
    assert_eq!(o.meta["reclaimed"]["by"], BY_RECLAIM);
    assert!(terms.get(&ended[0]).is_none(), "the oldest was reclaimed");
    let e = fails(&terms, "s1", READ, json!({"terminal": ended[0]}), d.path()).await;
    assert!(e.contains("no terminal"), "{e}");
    // An ended one's last screen is still read.
    let (o, _) = call(&terms, "s1", READ, json!({"terminal": ended[1]}), d.path()).await;
    assert!(o.text.contains("|done-1\n"), "{}", o.text);
    assert!(o.text.contains("its program exited (0)"), "{}", o.text);
    // Three more running ones fill the session: one ended is reclaimed at
    // each open, then a fifth running one is refused.
    for _ in 0..3 {
        let (o, _) = call(
            &terms,
            "s1",
            OPEN,
            json!({"argv": ["cat"], "quiet_ms": 0}),
            d.path(),
        )
        .await;
        assert!(o.meta.get("reclaimed").is_some(), "{}", o.meta);
    }
    assert_eq!(terms.of_session("s1").len(), PER_SESSION);
    let e = fails(&terms, "s1", OPEN, json!({"argv": ["cat"]}), d.path()).await;
    assert!(
        e.contains("this session has 4 terminals running, the most it may"),
        "{e}"
    );
    assert!(terms.get(&fifth).is_some());
    assert_eq!(terms.close_session("s1", BY_SESSION_END).len(), PER_SESSION);
}
