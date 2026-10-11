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
