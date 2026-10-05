//! One serialization per notification (theseus-celu.36; Review 2's S6): what
//! a notification costs for each connection that hears it, measured.
//!
//! The measurement is ignored by default; run it by name:
//! `cargo nextest run -p theseus-core --run-ignored all -E 'test(push_once::measure)' --no-capture`.

use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Instant;

use theseus_protocol::{Event, ModelDelta, NodeWritten, TurnSubmitResult};

use crate::bus::{EventSink, SessionBus};
use crate::outbound::{self, counts, Drain};

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
    while let Some(m) = rx.try_recv() {
        assert!(crate::rpc::write_line(&mut sink, &m).await);
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
