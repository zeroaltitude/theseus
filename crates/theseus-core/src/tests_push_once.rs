//! One serialization per notification (theseus-celu.36; Review 2's S6): the
//! requester and every watcher queue one line, serialized once, written as
//! it is, and byte for byte what the wire carried before; a notification no
//! connection queues is never serialized. And what a notification costs for
//! each connection that hears it, measured.
//!
//! The measurement is ignored by default; run it by name:
//! `cargo nextest run -p theseus-core --run-ignored all -E 'test(push_once::measure)' --no-capture`.

use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Instant;

use theseus_protocol::{Event, ModelDelta, NodeWritten, TurnSubmitResult};

use theseus_protocol::Message;

use crate::bus::{EventSink, SessionBus};
use crate::outbound::{self, counts, Drain, Item, Line};

const SESSION: &str = "ses_quillmoor";

/// A token's delta, as the turn streams it.
fn delta() -> Event {
    Event::ModelDelta(ModelDelta {
        turn_id: "trn_quillmoor01".into(),
        loop_index: 1,
        text: "the ".into(),
    })
}

/// A `node.written` made large: the notification names ids only, so its
/// size is its fields'.
fn large_node() -> Event {
    Event::NodeWritten(NodeWritten {
        session_id: SESSION.into(),
        node_id: format!("nod_{}", "q".repeat(48 << 10)),
        kind: "assistant_message".repeat(1 << 10),
    })
}

/// A turn's end with a long reply: the largest notification a turn sends.
fn large_end() -> Event {
    Event::TurnEnded(TurnSubmitResult {
        session_id: SESSION.into(),
        output: "a tide pool, a lantern, ".repeat(2 << 10),
        ..Default::default()
    })
}

/// Write everything a watcher's queue holds, as its connection's writer does,
/// to nowhere.
async fn write_out(rx: &mut Drain) -> usize {
    let mut sink = tokio::io::sink();
    let mut n = 0;
    while let Some(item) = rx.try_recv() {
        assert!(crate::rpc::write_item(&mut sink, &item).await);
        let _ = rx.written();
        n += 1;
    }
    n
}

/// Publishes `rounds` copies of each notification to 1, 10 and 100 watchers
/// on capped queues, writes every queue out, and prints the time per watcher
/// with the serializations and deep copies per notification.
#[tokio::test]
#[ignore = "a measurement: run it by name"]
async fn measure_serializations_per_notification() {
    const ROUNDS: usize = 200;
    println!("notification          watchers  ser/note  copies/note  ns/watcher");
    for (name, event) in [
        ("model.delta", delta()),
        ("node.written (64 KiB)", large_node()),
        ("turn.ended (48 KiB)", large_end()),
    ] {
        for watchers in [1usize, 10, 100] {
            let bus = Arc::new(SessionBus::default());
            let total = Arc::new(AtomicU64::new(0));
            let mut drains: Vec<Drain> = (0..watchers)
                .map(|i| {
                    let (tx, rx) = outbound::channel(total.clone());
                    bus.watch(SESSION, &format!("sock#{i}"), tx);
                    rx
                })
                .collect();
            let sink = EventSink::new(bus.clone(), SESSION, None);
            counts::take();
            let started = Instant::now();
            let mut written = 0;
            for _ in 0..ROUNDS {
                sink.send(event.clone());
                for rx in &mut drains {
                    written += write_out(rx).await;
                }
            }
            let elapsed = started.elapsed();
            let (serialized, copied) = counts::take();
            assert_eq!(written, ROUNDS * watchers, "each watcher heard each one");
            println!(
                "{name:<22}{watchers:>9}{:>10.1}{:>13.1}{:>12}",
                serialized as f64 / ROUNDS as f64,
                copied as f64 / ROUNDS as f64,
                elapsed.as_nanos() / (ROUNDS * watchers) as u128,
            );
        }
    }
}

/// `n` watchers on capped queues, and a sink whose requester is one more.
fn rig(n: usize) -> (EventSink, Drain, Vec<Drain>) {
    let bus = Arc::new(SessionBus::default());
    let total = Arc::new(AtomicU64::new(0));
    let watchers = (0..n)
        .map(|i| {
            let (tx, rx) = outbound::channel(total.clone());
            bus.watch(SESSION, &format!("sock#{i}"), tx);
            rx
        })
        .collect();
    let (tx, direct) = outbound::channel(total);
    bus.watch(SESSION, "web#1", tx.clone());
    let sink = EventSink::new(bus, SESSION, Some(("web#1".into(), tx)));
    (sink, direct, watchers)
}

/// Everything a queue holds, as lines.
fn lines(rx: &mut Drain) -> Vec<Line> {
    std::iter::from_fn(|| rx.try_recv())
        .map(|item| match item {
            Item::Line(l) => l,
            Item::Message(m) => panic!("a notification queued as a message: {m:?}"),
        })
        .collect()
}

/// The requester and two watchers each queue the notification once, as the
/// same line, serialized once; written, it is byte for byte the line the
/// writer wrote from the message before.
#[tokio::test]
async fn the_requester_and_two_watchers_write_one_line_byte_for_byte() {
    let (sink, mut direct, mut watchers) = rig(2);
    counts::take();
    sink.send(delta());
    assert_eq!(counts::take(), (1, 0), "one serialization, no copy");
    let mut heard = vec![lines(&mut direct)];
    heard.extend(watchers.iter_mut().map(lines));
    assert!(heard.iter().all(|h| h.len() == 1), "each once: {heard:?}");
    assert!(
        heard.iter().all(|h| Arc::ptr_eq(&h[0], &heard[0][0])),
        "one line"
    );
    let mut wire = Vec::new();
    for h in &heard {
        let item = Item::Line(h[0].clone());
        assert!(crate::rpc::write_item(&mut wire, &item).await);
    }
    let before = format!(
        "{}\n",
        serde_json::to_string(&Message::from(delta())).unwrap()
    );
    assert_eq!(String::from_utf8(wire).unwrap(), before.repeat(3));
}

/// A notification no connection queues is never serialized: none watches,
/// a raw channel takes its own message, and a queue past its cap drops it.
#[test]
fn a_notification_no_connection_queues_is_never_serialized() {
    let bus = Arc::new(SessionBus::default());
    counts::take();
    EventSink::new(bus.clone(), SESSION, None).send(large_node());
    assert_eq!(counts::take(), (0, 0), "no watcher");
    let (raw, mut taken) = tokio::sync::mpsc::unbounded_channel();
    bus.watch(SESSION, "test", raw);
    bus.publish(SESSION, &Message::from(large_node()), None);
    assert_eq!(counts::take(), (0, 1), "a raw channel's own copy");
    assert!(taken.try_recv().is_ok());
    bus.drop_conn("test");
    let (tx, _rx) = outbound::channel(Arc::default());
    let filler = Message::from(delta());
    for _ in 0..outbound::BACKLOG_CAP {
        assert!(tx.notify(&outbound::Note::new(&filler), "session:x"));
    }
    bus.watch(SESSION, "sock#9", tx);
    counts::take();
    bus.publish(SESSION, &Message::from(large_node()), None);
    assert_eq!(
        counts::take(),
        (0, 0),
        "past the cap: dropped, never serialized"
    );
}

/// One large notification queued to 100 capped watchers and its requester
/// is one copy: every queue holds the same line.
#[test]
fn a_large_notification_to_a_hundred_watchers_is_one_copy() {
    let (sink, mut direct, mut watchers) = rig(100);
    counts::take();
    sink.send(large_node());
    assert_eq!(counts::take(), (1, 0));
    let first = lines(&mut direct).pop().unwrap();
    assert!(first.len() > 64 << 10, "large: {}", first.len());
    assert_eq!(Arc::strong_count(&first), 101, "one copy, every queue's");
    for rx in &mut watchers {
        let heard = lines(rx);
        assert_eq!(heard.len(), 1);
        assert!(Arc::ptr_eq(&heard[0], &first));
    }
}
