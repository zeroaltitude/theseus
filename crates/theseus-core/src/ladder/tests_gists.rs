//! The reading sheet (`gists`): each line beside what its writer read and as the view shows it, each
//! first-level node beside its exchanges as its writer read them, the draw repeated by its seed, and the
//! counts that say how many exist. It stores nothing. Every name here is invented.

use theseus_protocol::exchange::ExchangeGistsParams;

use super::did::{self, DidLine};
use super::fold::{self, NoFlags};
use super::gists::sheet;
use super::read::{self, Step};
use super::tests::{exchange_at, store, Fake};
use super::{DayPurse, N};
use crate::exchange;
use crate::store::Store;

const PLACE: &str = "discord:channel:4242";
const OTHER: &str = "discord:channel:5353";

fn asked(place: Option<&str>, lines: u32, nodes: u32, seed: u64) -> ExchangeGistsParams {
    ExchangeGistsParams {
        place: place.map(str::to_string),
        lines,
        nodes,
        seed: Some(seed),
    }
}

/// A line for `turn`, written by hand at `at_ms` by writer version `writer`.
fn line_for(s: &Store, turn: &str, writer: u32, at_ms: u64) -> DidLine {
    let ex = exchange::exchange(s, turn).unwrap().unwrap();
    let d = DidLine {
        turn_id: turn.into(),
        session_id: ex.session_id.clone(),
        line: format!("Recalibrated the gauge of {turn}."),
        model: "fake-small-1".into(),
        writer,
        marks: super::marks_of(&ex),
        cost_usd: 0.0,
        at_ms,
    };
    s.append(&[did::did_record(&d).unwrap()]).unwrap();
    d
}

/// A first-level node over `turns`, written by hand at `at_ms` by writer version `writer`.
fn node_over(s: &Store, id: &str, turns: &[String], writer: u32, at_ms: u64) -> fold::FoldNode {
    let exs: Vec<_> = turns
        .iter()
        .map(|t| exchange::exchange(s, t).unwrap().unwrap())
        .collect();
    let n = fold::FoldNode {
        id: id.into(),
        place: exs[0].place.clone(),
        level: 1,
        exchanges: turns.to_vec(),
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
        at_ms,
    };
    s.append(&fold::records(&n).unwrap()).unwrap();
    n
}

#[tokio::test]
async fn gists_shows_each_line_beside_what_its_writer_read() {
    let (_d, s) = store();
    let w = Fake::default();
    let purse = DayPurse::new(&s, None, false);
    let mut turns = Vec::new();
    for at in [1_000, 2_000, 3_000] {
        let t = exchange_at(&s, "ses_a", PLACE, at, at == 2_000);
        let out = did::write(&s, &w, &t, &purse).await.unwrap();
        assert!(matches!(out, did::Outcome::Written(_)), "{out:?}");
        turns.push(t);
    }
    let elsewhere = exchange_at(&s, "ses_b", OTHER, 4_000, false);
    did::write(&s, &w, &elsewhere, &purse).await.unwrap();
    let before = s.last_position();

    let r = sheet(&s, &asked(Some(PLACE), 50, 10, 7)).unwrap();
    assert_eq!((r.lines_total, r.lines.len()), (3, 3), "{r:?}");
    let asks = w.asks.lock().unwrap().clone();
    for (i, (item, t)) in r.lines.iter().zip(&turns).enumerate() {
        assert_eq!(&item.turn_id, t);
        assert_eq!(item.place, PLACE);
        // Exactly what the writer was given: its own cuts, the author it was told, the outside wrapper.
        assert_eq!(item.read.as_deref(), Some(asks[i].text.as_str()));
        let at = 1_000 * (i + 1);
        let read = item.read.as_deref().unwrap();
        assert!(read.contains(&format!("Recalibrate gauge {at}")), "{read}");
        assert!(
            read.contains(&format!("Gauge {at} reads 2 mm high")),
            "{read}"
        );
        assert!(
            !read.contains("SECRET-TOOL-OUTPUT"),
            "a tool result is never read"
        );
        assert_eq!(
            item.line,
            "Moved the tide table to the new gauge and checked it."
        );
        assert_eq!(
            (item.model.as_str(), item.at_ms),
            ("fake-small-1", at as u64)
        );
    }
    let all = sheet(&s, &asked(None, 50, 10, 7)).unwrap();
    assert_eq!(all.lines_total, 4, "every place when none is named");
    assert_eq!(s.last_position(), before, "the sheet stores nothing");
}

#[tokio::test]
async fn gists_shows_a_level_one_node_with_its_exchanges() {
    let (_d, s) = store();
    let w = Fake::default();
    let purse = DayPurse::new(&s, None, false);
    for i in 0..2 * N as u64 {
        let t = exchange_at(&s, "ses_a", PLACE, 1_000 + i, false);
        did::write(&s, &w, &t, &purse).await.unwrap();
    }
    let step = Step {
        writer: &w,
        flags: &NoFlags,
        purse: &purse,
    };
    let made = read::fold_step(&s, PLACE, None, &step).await.unwrap().made;
    assert_eq!(made.len(), 1);
    let fold_read = w.asks.lock().unwrap().last().unwrap().text.clone();
    let before = s.last_position();

    let r = sheet(&s, &asked(Some(PLACE), 0, 10, 7)).unwrap();
    assert_eq!((r.nodes_total, r.nodes.len()), (1, 1), "{r:?}");
    let g = &r.nodes[0];
    assert_eq!(
        (g.id.as_str(), g.exchanges, g.readable),
        (made[0].id.as_str(), N as u32, N as u32)
    );
    // Exactly what the fold's writer was given, every exchange in it.
    assert_eq!(g.read, fold_read);
    for i in 0..N as u64 {
        assert!(
            g.read.contains(&format!("Recalibrate gauge {}", 1_000 + i)),
            "{}",
            g.read
        );
    }
    assert!(
        g.read.contains("What Theseus did: Moved the tide table"),
        "{}",
        g.read
    );
    // As the view's fold block shows it: its title, the owner's words in it, its account, its handle.
    assert!(g.shown.contains("Tide gauges"), "{}", g.shown);
    assert!(
        g.shown
            .contains("Recalibrated gauge 4 and left gauge 5 open."),
        "{}",
        g.shown
    );
    assert!(g.shown.contains(&format!("(open {})", g.id)), "{}", g.shown);
    assert!(r.lines.is_empty(), "no line was asked");
    assert_eq!(s.last_position(), before, "the sheet stores nothing");
}

/// A line written after the node was not what its writer read: it reads as the writer saw it, none.
#[test]
fn gists_reads_a_node_without_a_line_written_after_it() {
    let (_d, s) = store();
    let a = exchange_at(&s, "ses_a", PLACE, 1_000, false);
    let b = exchange_at(&s, "ses_a", PLACE, 2_000, false);
    line_for(&s, &a, super::writer::WRITER_VERSION, 4_000);
    line_for(&s, &b, super::writer::WRITER_VERSION, 6_000);
    node_over(
        &s,
        "fld_two",
        &[a.clone(), b],
        super::writer::WRITER_VERSION,
        5_000,
    );
    let r = sheet(&s, &asked(Some(PLACE), 0, 10, 7)).unwrap();
    let g = &r.nodes[0];
    assert_eq!((g.exchanges, g.readable), (2, 2));
    assert!(
        g.read
            .contains(&format!("What Theseus did: Recalibrated the gauge of {a}.")),
        "{}",
        g.read
    );
    assert!(
        g.read
            .contains(&format!("What Theseus did: {}", did::NOT_SUMMARISED)),
        "{}",
        g.read
    );
    assert!(g.read.contains("Recalibrate gauge 1000") && g.read.contains("Recalibrate gauge 2000"));
}

/// The authorship rule as the view keeps it: an older line over messages not proven the owner's reads
/// as not shown, and an older node's title and account are withheld; what is stored is not changed.
#[test]
fn gists_shows_an_unauthored_line_as_the_view_does() {
    let (_d, s) = store();
    let t = exchange_at(&s, "ses_a", PLACE, 1_000, false);
    let old = line_for(&s, &t, 1, 1_500);
    node_over(&s, "fld_old", std::slice::from_ref(&t), 1, 1_600);
    let r = sheet(&s, &asked(Some(PLACE), 50, 10, 7)).unwrap();
    assert_eq!(r.lines[0].line, did::UNAUTHORED_LINE);
    assert_eq!(r.lines[0].line, did::shown_line(&s, &old));
    let g = &r.nodes[0];
    assert!(
        !g.shown.contains("Tide gauges by the harbour"),
        "{}",
        g.shown
    );
    assert!(
        !g.shown.contains("Recalibrated the harbour's gauges"),
        "{}",
        g.shown
    );
    assert!(
        g.shown
            .contains("(written before authorship was recorded; not shown)"),
        "{}",
        g.shown
    );
    assert_eq!(
        did::did(&s, &t).unwrap().unwrap().line,
        old.line,
        "nothing stored is rewritten"
    );
}

#[test]
fn gists_sample_repeats_by_seed() {
    let (_d, s) = store();
    for i in 0..30u64 {
        let t = exchange_at(&s, "ses_a", PLACE, 1_000 + i, false);
        line_for(&s, &t, super::writer::WRITER_VERSION, 2_000 + i);
    }
    let turns = |seed| -> Vec<String> {
        let r = sheet(&s, &asked(Some(PLACE), 10, 10, seed)).unwrap();
        assert_eq!((r.seed, r.lines_total, r.lines.len()), (seed, 30, 10));
        r.lines.into_iter().map(|l| l.turn_id).collect()
    };
    assert_eq!(turns(42), turns(42), "one seed, one draw");
    assert_ne!(turns(42), turns(43), "another seed, another draw");
    // A sheet asked with no seed says the one it drew with, and that seed draws it again.
    let chosen = sheet(
        &s,
        &ExchangeGistsParams {
            seed: None,
            lines: 10,
            ..asked(Some(PLACE), 10, 10, 0)
        },
    )
    .unwrap();
    let again = sheet(&s, &asked(Some(PLACE), 10, 10, chosen.seed)).unwrap();
    assert_eq!(chosen, again);
}

#[test]
fn gists_says_how_many_nodes_exist_when_fewer_than_asked() {
    let (_d, s) = store();
    let a = exchange_at(&s, "ses_a", PLACE, 1_000, false);
    let b = exchange_at(&s, "ses_a", PLACE, 2_000, false);
    let c = exchange_at(&s, "ses_b", OTHER, 3_000, false);
    line_for(&s, &a, super::writer::WRITER_VERSION, 4_000);
    node_over(
        &s,
        "fld_here",
        &[a, b],
        super::writer::WRITER_VERSION,
        5_000,
    );
    node_over(&s, "fld_there", &[c], super::writer::WRITER_VERSION, 5_000);
    let here = sheet(&s, &asked(Some(PLACE), 50, 10, 7)).unwrap();
    assert_eq!((here.nodes_total, here.nodes.len()), (1, 1), "{here:?}");
    assert_eq!((here.lines_total, here.lines.len()), (1, 1));
    let all = sheet(&s, &asked(None, 50, 10, 7)).unwrap();
    assert_eq!((all.nodes_total, all.nodes.len()), (2, 2), "{all:?}");
}
