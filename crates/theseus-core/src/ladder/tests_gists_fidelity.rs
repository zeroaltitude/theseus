//! The reading sheet's material as the writer's request holds it (each value the writer substitutes
//! substituted), and its warning on an older writer's item and on sources no longer in the store. Every
//! name and value here is invented.

use theseus_protocol::exchange::ExchangeGistsParams;

use super::did::{self, DidLine};
use super::fold::{self, NoFlags};
use super::gists::sheet;
use super::read::{self, Step};
use super::tests::{exchange_at, store, Fake};
use super::writer::{request_of, Ask, WRITER_VERSION};
use super::{DayPurse, N};
use crate::exchange::{self, Exchange, Opener};
use crate::scrub::Scrubber;
use crate::store::Store;

const PLACE: &str = "discord:channel:4242";

fn asked(lines: u32, nodes: u32) -> ExchangeGistsParams {
    ExchangeGistsParams {
        place: Some(PLACE.into()),
        lines,
        nodes,
        seed: Some(7),
    }
}

fn none() -> Scrubber {
    Scrubber::default()
}

/// The text the writer's request puts before the model for `ask`.
fn sent(scrubber: &Scrubber, ask: &Ask) -> String {
    let req = request_of(scrubber, None, "fake-small-1", ask);
    req.messages[0]["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string()
}

/// A line for `turn`, written by hand by writer version `writer`.
fn line_for(s: &Store, turn: &str, writer: u32) {
    let d = DidLine {
        turn_id: turn.into(),
        session_id: "ses_a".into(),
        line: format!("Recalibrated the gauge of {turn}."),
        model: "fake-small-1".into(),
        writer,
        marks: super::Marks {
            place: Some(PLACE.into()),
            audience: crate::view::Audience::Private,
            author: crate::view::Author::Theseus,
            outside: false,
            sealed: false,
            veiled: false,
        },
        cost_usd: 0.0,
        at_ms: 9_000,
    };
    s.append(&[did::did_record(&d).unwrap()]).unwrap();
}

/// A first-level node over `turns` (each written) and `gone` (none written), by writer version `writer`.
fn node_over(s: &Store, id: &str, turns: &[String], gone: &[&str], writer: u32) {
    let exs: Vec<_> = turns
        .iter()
        .map(|t| exchange::exchange(s, t).unwrap().unwrap())
        .collect();
    let n = fold::FoldNode {
        id: id.into(),
        place: PLACE.into(),
        level: 1,
        exchanges: turns
            .iter()
            .cloned()
            .chain(gone.iter().map(|g| g.to_string()))
            .collect(),
        first_ms: exs[0].start_ms,
        last_ms: exs.last().unwrap().start_ms,
        his: exs
            .iter()
            .map(|e| fold::HisLine {
                node: e.ask.clone().unwrap(),
                correction: false,
                standing: false,
            })
            .collect(),
        counted: 0,
        children: vec![],
        title: "Tide gauges by the harbour".into(),
        account: "Recalibrated the harbour's gauges one by one.".into(),
        marks: super::marks_of(&exs[0]),
        model: Some("fake-small-1".into()),
        writer,
        at_ms: 9_500,
    };
    s.append(&fold::records(&n).unwrap()).unwrap();
}

/// An exchange whose message and reply are no longer in the store.
fn exchange_without_words(s: &Store, at: u64) -> String {
    let turn = crate::new_id("trn");
    let ex = Exchange {
        turn_id: turn.clone(),
        session_id: "ses_a".into(),
        place: PLACE.into(),
        private: true,
        opened_by: Opener::Message,
        person: Some(exchange::OWNER.into()),
        start_ms: at,
        end_ms: Some(at + 1),
        ask: Some("nod_gone_ask".into()),
        reply: Some("nod_gone_reply".into()),
        messages: vec!["nod_gone_ask".into()],
        stop: Some("no_tool_calls".into()),
        rebuilt: false,
    };
    s.append(&[exchange::exchange_record(&ex).unwrap()])
        .unwrap();
    turn
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

/// An older writer's line and node each carry a warning that the material may not be what it was given;
/// today's writer's line carries none.
#[test]
fn gists_warns_on_an_older_writers_items() {
    let (_d, s) = store();
    let old = exchange_at(&s, "ses_a", PLACE, 1_000, false);
    let new = exchange_at(&s, "ses_a", PLACE, 2_000, false);
    line_for(&s, &old, 1);
    line_for(&s, &new, WRITER_VERSION);
    node_over(&s, "fld_old", std::slice::from_ref(&old), &[], 1);
    let r = sheet(&s, &none(), &asked(50, 10)).unwrap();
    let by = |t: &str| r.lines.iter().find(|l| l.turn_id == t).unwrap();
    let warned = |notes: &[String]| {
        notes
            .iter()
            .any(|n| n.contains("older writer") && n.contains("may not match"))
    };
    assert!(warned(&by(&old).notes), "{:?}", by(&old).notes);
    assert!(by(&new).notes.is_empty(), "{:?}", by(&new).notes);
    assert!(warned(&r.nodes[0].notes), "{:?}", r.nodes[0].notes);
}

/// Sources no longer in the store are said on the item: a line's exchange, its message and its reply,
/// and a node's exchanges.
#[test]
fn gists_says_when_sources_are_missing() {
    let (_d, s) = store();
    let whole = exchange_at(&s, "ses_a", PLACE, 1_000, false);
    let wordless = exchange_without_words(&s, 2_000);
    line_for(&s, &whole, WRITER_VERSION);
    line_for(&s, &wordless, WRITER_VERSION);
    line_for(&s, "trn_gone", WRITER_VERSION);
    node_over(
        &s,
        "fld_gap",
        &[whole.clone(), wordless.clone()],
        &["trn_gone"],
        WRITER_VERSION,
    );

    let r = sheet(&s, &none(), &asked(50, 10)).unwrap();
    let by = |t: &str| r.lines.iter().find(|l| l.turn_id == t).unwrap();
    let says = |notes: &[String], what: &str| notes.iter().any(|n| n.contains(what));
    assert!(by(&whole).notes.is_empty(), "{:?}", by(&whole).notes);
    let w = &by(&wordless).notes;
    assert!(says(w, "Its message is no longer in the store"), "{w:?}");
    assert!(
        says(w, "Its final reply is no longer in the store"),
        "{w:?}"
    );
    let g = &by("trn_gone").notes;
    assert!(says(g, "Its exchange is no longer in the store"), "{g:?}");
    let g = &r.nodes[0];
    assert_eq!((g.exchanges, g.readable), (3, 2));
    assert!(
        says(&g.notes, "1 of its 3 exchanges is no longer in the store"),
        "{:?}",
        g.notes
    );
    assert!(
        says(
            &g.notes,
            "1 of the exchanges shown has lost its message or reply"
        ),
        "{:?}",
        g.notes
    );
}
