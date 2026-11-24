//! The reading sheet's material as the writer's request holds it: each value the writer substitutes
//! substituted. Every name and value here is invented.

use theseus_protocol::exchange::ExchangeGistsParams;

use super::did;
use super::fold::NoFlags;
use super::gists::sheet;
use super::read::{self, Step};
use super::tests::{exchange_at, store, Fake};
use super::writer::{request_of, Ask};
use super::{DayPurse, N};
use crate::scrub::Scrubber;

const PLACE: &str = "discord:channel:4242";

fn asked(lines: u32, nodes: u32) -> ExchangeGistsParams {
    ExchangeGistsParams {
        place: Some(PLACE.into()),
        lines,
        nodes,
        seed: Some(7),
    }
}

/// The text the writer's request puts before the model for `ask`.
fn sent(scrubber: &Scrubber, ask: &Ask) -> String {
    let req = request_of(scrubber, None, "fake-small-1", ask);
    req.messages[0]["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string()
}

/// A value the writer substitutes reads on the sheet as the writer's request holds it: substituted, for
/// a line and for a node alike.
#[tokio::test]
async fn gists_shows_the_material_the_writers_request_holds() {
    let (_d, s) = store();
    let w = Fake::default();
    let purse = DayPurse::new(&s, None, false);
    let mut turns = Vec::new();
    for i in 0..2 * N as u64 {
        let t = exchange_at(&s, "ses_a", PLACE, 1_000 + i, i == 3);
        did::write(&s, &w, &t, &purse).await.unwrap();
        turns.push(t);
    }
    let step = Step {
        writer: &w,
        flags: &NoFlags,
        purse: &purse,
    };
    assert_eq!(
        read::fold_step(&s, PLACE, None, &step)
            .await
            .unwrap()
            .made
            .len(),
        1
    );
    let asks = w.asks.lock().unwrap().clone();
    // An invented value, as if the vault had given it: every reply holds it.
    let scrubber = Scrubber::with_values(vec![("reads 2 mm high".into(), "TIDE_KEY".into())]);
    assert!(
        asks[0].text.contains("reads 2 mm high"),
        "the writer's material holds the value"
    );

    let r = sheet(&s, &scrubber, &asked(50, 10)).unwrap();
    assert_eq!(r.lines.len(), 2 * N);
    for (item, (t, ask)) in r.lines.iter().zip(turns.iter().zip(&asks)) {
        assert_eq!(&item.turn_id, t);
        let read = item.read.as_deref().unwrap();
        assert_eq!(
            read,
            sent(&scrubber, ask),
            "the line's material is the request's"
        );
        assert!(!read.contains("reads 2 mm high"), "{read}");
        assert!(read.contains("[redacted:TIDE_KEY]"), "{read}");
    }
    let node = &r.nodes[0];
    assert_eq!(node.read, sent(&scrubber, asks.last().unwrap()));
    assert!(!node.read.contains("reads 2 mm high"), "{}", node.read);
    assert!(node.read.contains("[redacted:TIDE_KEY]"), "{}", node.read);
}
