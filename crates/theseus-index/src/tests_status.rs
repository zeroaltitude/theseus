//! The status socket never goes silent (theseus-uazd): `index.status`
//! answers while the embedding work holds the vector table, and while every
//! connection slot is taken.

use std::os::unix::net::UnixStream;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::client::Client;
use crate::proto::{method, IndexStatus, WarmResult};
use crate::server;
use crate::tender::Tender;
use crate::tests::user;
use crate::vtests::{settle_all, VRig, TEXTS};

fn status(sock: &std::path::Path, within: Duration) -> anyhow::Result<IndexStatus> {
    Client::connect(sock, within)?.call(method::STATUS, Value::Null)
}

/// The table held by a writer (as a compaction holds it across a file's
/// rewrite, or a batch across its append): the status answers at once with
/// the counts it last read, and says when it read them; once the writer
/// lets go, it reads them again. Before, it queued behind the writer, and
/// every health call left a connection waiting until the slots ran out.
#[test]
fn the_status_answers_while_the_table_is_held() {
    let v = VRig::new();
    let nodes: Vec<_> = TEXTS.iter().map(|t| user("ses_1", t)).collect();
    v.rig.put(&nodes);
    let mut t = v.open();
    settle_all(&mut t);
    let shared = t.shared();
    let sock = v.rig._tmp.path().join("s.sock");
    let _served = server::spawn(&sock, shared.clone()).unwrap();
    let before = status(&sock, Duration::from_secs(5)).unwrap();
    let vb = before.vectors.unwrap();
    assert!(vb.chunks > 0 && vb.counted_ms > 0, "{vb:?}");

    let (held_tx, held_rx) = mpsc::channel();
    let (let_go_tx, let_go_rx) = mpsc::channel::<()>();
    let holder = std::thread::spawn(move || {
        let _t = shared.vectors.hold_table();
        held_tx.send(()).unwrap();
        let _ = let_go_rx.recv_timeout(Duration::from_secs(30));
    });
    held_rx.recv().unwrap();
    std::thread::sleep(Duration::from_millis(20));
    let t0 = Instant::now();
    let during = status(&sock, Duration::from_secs(3));
    let took = t0.elapsed();
    let_go_tx.send(()).unwrap();
    holder.join().unwrap();
    let during = during.expect("the status did not answer while the table was held");
    let vd = during.vectors.unwrap();
    assert!(took < Duration::from_secs(1), "the status took {took:?}");
    assert_eq!(
        (vd.chunks, vd.vectors, vd.pending),
        (vb.chunks, vb.vectors, vb.pending)
    );
    assert_eq!(
        vd.counted_ms, vb.counted_ms,
        "the counts last read, and when"
    );
    std::thread::sleep(Duration::from_millis(5));
    let after = status(&sock, Duration::from_secs(5))
        .unwrap()
        .vectors
        .unwrap();
    assert!(after.counted_ms > vb.counted_ms, "read again once free");
    eprintln!("the status answered in {took:?} with the table held");
}

/// Every slot taken (connections that hold theirs, as queries waiting on
/// the model or an `index.embed` its caller left do): `index.status` is
/// still answered, on the lane past them, and anything else there is
/// refused with why. Before, a connection over the limit was closed unread.
#[test]
fn the_status_answers_past_a_full_socket() {
    let v = VRig::new();
    let mut t: Tender = v.open();
    settle_all(&mut t);
    let sock = v.rig._tmp.path().join("s.sock");
    let (_served, active) = server::spawn_counted(&sock, t.shared()).unwrap();
    let held: Vec<UnixStream> = (0..server::MAX_CONNECTIONS)
        .map(|_| UnixStream::connect(&sock).unwrap())
        .collect();
    let t0 = Instant::now();
    while active.load(Ordering::SeqCst) < server::MAX_CONNECTIONS {
        assert!(t0.elapsed() < Duration::from_secs(10), "not accepted");
        std::thread::sleep(Duration::from_millis(1));
    }
    let s = status(&sock, Duration::from_secs(3));
    assert!(
        s.is_ok(),
        "past {} held connections: {:?}",
        server::MAX_CONNECTIONS,
        s.err()
    );
    let other = Client::connect(&sock, Duration::from_secs(3))
        .unwrap()
        .call::<WarmResult>(method::WARM, Value::Null);
    let e = other.expect_err("past the limit, only the status answers");
    assert!(e.to_string().contains("index.status alone"), "{e}");
    assert_eq!(active.load(Ordering::SeqCst), server::MAX_CONNECTIONS);
    drop(held);
}

/// The measure the health line's 100 ms deadline is judged by
/// (theseus-la5oc): with every ordinary slot held and the status lane in
/// use, the status still answers, and how long it takes is printed. The
/// bound asserted is the deadline's own, so a tender whose status needs
/// more than health gives it fails here, and the number says by how much.
#[test]
fn the_status_answers_inside_health_s_deadline_with_every_slot_held() {
    const HEALTH_DEADLINE: Duration = Duration::from_millis(100);
    let v = VRig::new();
    let mut t: Tender = v.open();
    settle_all(&mut t);
    let sock = v.rig._tmp.path().join("s.sock");
    let (_served, active) = server::spawn_counted(&sock, t.shared()).unwrap();
    let held: Vec<UnixStream> = (0..server::MAX_CONNECTIONS)
        .map(|_| UnixStream::connect(&sock).unwrap())
        .collect();
    let t0 = Instant::now();
    while active.load(Ordering::SeqCst) < server::MAX_CONNECTIONS {
        assert!(t0.elapsed() < Duration::from_secs(10), "not accepted");
        std::thread::sleep(Duration::from_millis(1));
    }
    let mut took = Vec::new();
    for _ in 0..50 {
        let t0 = Instant::now();
        let s = status(&sock, HEALTH_DEADLINE).expect("a status inside health's deadline");
        took.push(t0.elapsed());
        assert!(s.vectors.is_some());
    }
    took.sort();
    assert_eq!(took.len(), 50, "fifty answers, not an empty set");
    let (p50, max) = (took[25], took[49]);
    eprintln!("status, 16 slots held: p50 {p50:?}, max {max:?}");
    assert!(max < HEALTH_DEADLINE, "the slowest answer took {max:?}");
    drop(held);
}

/// A connection's thread that dies mid-request (here: a poisoned lock under
/// the status) gives its slot back. Before, the count was let go only on a
/// clean return, so each such death held one of the sixteen slots for the
/// life of the tender.
#[test]
fn a_connection_that_panics_gives_back_its_slot() {
    let v = VRig::new();
    let mut t: Tender = v.open();
    settle_all(&mut t);
    let shared = t.shared();
    let sock = v.rig._tmp.path().join("s.sock");
    let (_served, active) = server::spawn_counted(&sock, shared.clone()).unwrap();
    status(&sock, Duration::from_secs(3)).expect("healthy before the lock is poisoned");
    shared.vectors.poison_stats();
    for _ in 0..3 {
        let e = status(&sock, Duration::from_secs(3)).expect_err("the status panics now");
        assert!(e.to_string().contains("closed"), "{e}");
    }
    let t0 = Instant::now();
    while active.load(Ordering::SeqCst) != 0 {
        assert!(
            t0.elapsed() < Duration::from_secs(5),
            "three dead connections still count {} slots",
            active.load(Ordering::SeqCst)
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A cursor holds the numbers its index was built under and no binary's
/// identity, so an install that moved none of them resumes, and one that
/// moved a number says which, in the words the tender logs (theseus-la5oc).
#[test]
fn a_rebuild_names_the_number_that_moved_and_a_plain_install_resumes() {
    use crate::engine::SCHEMA_VERSION;
    use crate::extract::EXTRACTOR_VERSION;
    use crate::state::{Saved, CURSOR_FORMAT};
    use crate::tender::rebuild_reason;
    let saved = |schema, extractor| Saved {
        format: CURSOR_FORMAT,
        schema,
        extractor,
        cursor: theseus_follow::Cursor::start(),
        saved_at_ms: 1,
    };
    // Another binary, the same numbers: the cursor is kept.
    assert_eq!(
        rebuild_reason(Some(&saved(SCHEMA_VERSION, EXTRACTOR_VERSION)), true, true),
        None
    );
    // A schema that moved is named, with the window the operator will see.
    let why = rebuild_reason(
        Some(&saved(SCHEMA_VERSION + 1, EXTRACTOR_VERSION)),
        true,
        true,
    )
    .expect("a moved schema rebuilds");
    assert!(
        why.contains(&format!(
            "index schema {} -> {SCHEMA_VERSION}",
            SCHEMA_VERSION + 1
        )) && why.contains("recall read-degraded"),
        "{why}"
    );
    assert!(!why.contains("extractor"), "{why}");
    // An unreadable cursor over an index is said so; no cursor at all over
    // one is "missing" (`a_rebuild_names_what_is_missing_and_not_what_is_there`).
    assert!(rebuild_reason(None, true, true)
        .unwrap()
        .contains("unreadable"));
    assert_eq!(rebuild_reason(None, false, false), None);
}

/// A rebuild's reason names what is missing, and never says missing what is
/// there (theseus-la5oc, the b2-tasks-and-tender review's F1): the words ride
/// `IndexBackfill.why` to the health line, where "its cursor is missing" over
/// an intact cursor read as a lost cursor. Every combination of a cursor that
/// reads, a cursor file that exists, and an index that exists, in the words
/// of what is not there.
#[test]
fn a_rebuild_names_what_is_missing_and_not_what_is_there() {
    use crate::engine::SCHEMA_VERSION;
    use crate::extract::EXTRACTOR_VERSION;
    use crate::state::{Saved, CURSOR_FORMAT};
    use crate::tender::rebuild_reason;
    let intact = Saved {
        format: CURSOR_FORMAT,
        schema: SCHEMA_VERSION,
        extractor: EXTRACTOR_VERSION,
        cursor: theseus_follow::Cursor::start(),
        saved_at_ms: 1,
    };
    // An intact cursor of this build over no index: the index is what is gone.
    let why = rebuild_reason(Some(&intact), false, true).expect("no index, so a rebuild");
    assert!(
        why.starts_with("its index is missing") && !why.contains("cursor is missing"),
        "{why}"
    );
    // No cursor file over an index: the cursor is missing, and the index is not.
    assert_eq!(
        rebuild_reason(None, true, false).as_deref(),
        Some("its cursor is missing")
    );
    // A cursor file that does not read, over an index: unreadable, not missing.
    let why = rebuild_reason(None, true, true).expect("unreadable");
    assert!(
        why.contains("unreadable") && !why.contains("missing"),
        "{why}"
    );
    // One that does not read and no index: both are said, and neither is
    // called missing when it is not.
    let why = rebuild_reason(None, false, true).expect("no index and no cursor to read");
    assert!(
        why.starts_with("its index is missing")
            && why.contains("cursor file is unreadable")
            && !why.contains("cursor is missing"),
        "{why}"
    );
    // Nothing at all is a first start: nothing is rebuilt, so nothing is said.
    assert_eq!(rebuild_reason(None, false, false), None);
    // And a cursor and an index the build can use are kept.
    assert_eq!(rebuild_reason(Some(&intact), true, true), None);
}
