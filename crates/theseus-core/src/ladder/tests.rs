//! The ladder over a store with a fake writer: a `did` line from the message
//! and the reply alone, once, with its exchange's marks; none past the day's
//! line; the reader's words where there is none;
//! the fold by count, its words by reference and its marks; the levels above
//! built by code; and `/new`. Every name here is invented.

use std::sync::Mutex;

use serde_json::json;

use super::did::{self, Outcome, Said, NOT_SUMMARISED};
use super::fold::{self, NoFlags};
use super::read::{self, Step};
use super::writer::{Ask, Work, Writer, Written};
use super::{DayPurse, Purse, N};
use crate::exchange::{self, Exchange, Opener, Root};
use crate::node::{Body, Node};
use crate::store::Store;
use crate::view::Audience;
use std::sync::atomic::Ordering::SeqCst;

const PLACE: &str = "discord:channel:4242";

/// A writer that answers each call with its script, and keeps what it read:
/// its call reserved and settled on the purse at `cost`, the first `fail`
/// calls charged and failed.
#[derive(Default)]
pub(super) struct Fake {
    pub asks: Mutex<Vec<Ask>>,
    pub cost: f64,
    pub fail: std::sync::atomic::AtomicUsize,
}

impl Writer for Fake {
    fn write<'a>(
        &'a self,
        ask: Ask,
        purse: &'a dyn Purse,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Written> + Send + 'a>> {
        let work = ask.work;
        self.asks.lock().unwrap().push(ask);
        let cost = self.cost;
        Box::pin(async move {
            let id = match purse.reserve(cost) {
                Ok(id) => id,
                Err(why) => return Written::not(why),
            };
            purse.settle(&id, cost);
            let failing = self
                .fail
                .fetch_update(SeqCst, SeqCst, |n| n.checked_sub(1))
                .is_ok();
            if failing {
                return Written {
                    cost_usd: cost,
                    failed: Some("the provider failed".into()),
                    ..Written::default()
                };
            }
            let text = match work {
                Work::Did => {
                    "Moved the tide table to the new gauge and checked it.\nignored".into()
                }
                Work::Fold => "Tide gauges\n\nRecalibrated gauge 4 and left gauge 5 open.".into(),
            };
            Written {
                text: Some(text),
                model: "fake-small-1".into(),
                cost_usd: cost,
                failed: None,
            }
        })
    }

    fn estimate(&self, _: &Ask) -> Option<f64> {
        Some(0.01)
    }
}

pub(super) fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    (dir, store)
}

/// One ended exchange of `place` in `sid` at `at`: its message, a tool
/// result (from outside when `outside`), and its reply.
pub(super) fn exchange_at(store: &Store, sid: &str, place: &str, at: u64, outside: bool) -> String {
    let turn = crate::new_id("trn");
    let ask = Node::user(
        sid,
        Some(&turn),
        "op",
        &format!("Recalibrate gauge {at}, please."),
    );
    let mut result = Node::user(sid, Some(&turn), "op", "");
    result.body = serde_json::from_value::<Body>(json!({
        "kind": "tool_result", "tool_use_id": "tu_1", "tool": "http.fetch", "status": "ok",
        "is_error": false, "content": "SECRET-TOOL-OUTPUT",
        "external": if outside { json!({"url": "https://tides.invalid/4"}) } else { json!(null) },
    }))
    .unwrap();
    let mut reply = Node::user(sid, Some(&turn), "op", "");
    reply.body = serde_json::from_value::<Body>(json!({
        "kind": "assistant_message", "model": "big", "provider": "fake",
        "blocks": [{"type": "text", "text": format!("Gauge {at} reads 2 mm high; corrected.")}],
    }))
    .unwrap();
    let ex = Exchange {
        turn_id: turn.clone(),
        session_id: sid.into(),
        place: place.into(),
        private: true,
        opened_by: Opener::Message,
        person: Some(exchange::OWNER.into()),
        start_ms: at,
        end_ms: Some(at + 1),
        ask: Some(ask.id.clone()),
        reply: Some(reply.id.clone()),
        messages: vec![ask.id.clone()],
        stop: Some("no_tool_calls".into()),
        rebuilt: false,
    };
    let mut recs = vec![
        ask.record().unwrap(),
        result.record().unwrap(),
        reply.record().unwrap(),
        exchange::exchange_record(&ex).unwrap(),
    ];
    // A place whose name would break a key gets no touch from the ledger
    // (it refuses the root), and so no ladder: the test of that place holds
    // with the exchange alone.
    recs.extend(exchange::touch(Root::Place(place), &turn, sid, at));
    store.append(&recs).unwrap();
    turn
}

/// A `/new` at `at_ms`, after no exchange of that millisecond.
pub(super) fn new_at(at_ms: u64) -> crate::view::Pos {
    crate::view::Pos {
        at_ms,
        id: String::new(),
    }
}

#[tokio::test]
async fn a_did_line_reads_the_message_and_the_reply_never_a_tool_result() {
    let (_d, s) = store();
    let turn = exchange_at(&s, "ses_a", PLACE, 1_000, true);
    let w = Fake::default();
    let out = did::write(&s, &w, &turn, &DayPurse::new(&s, None, false))
        .await
        .unwrap();
    let Outcome::Written(d) = out else {
        panic!("{out:?}")
    };
    assert_eq!(
        d.line,
        "Moved the tide table to the new gauge and checked it."
    );
    assert_eq!(
        (d.model.as_str(), d.writer),
        ("fake-small-1", super::writer::WRITER_VERSION)
    );
    assert!(d.marks.outside, "it carries its exchange's marks");
    assert_eq!(
        (d.marks.place.as_deref(), d.marks.audience),
        (Some(PLACE), Audience::Private)
    );
    let asked = w.asks.lock().unwrap()[0].clone();
    assert!(
        asked.text.contains("Recalibrate gauge 1000"),
        "{}",
        asked.text
    );
    assert!(
        asked.text.contains("Gauge 1000 reads 2 mm high"),
        "{}",
        asked.text
    );
    assert!(
        !asked.text.contains("SECRET-TOOL-OUTPUT"),
        "a tool result is never read"
    );
    assert_eq!(did::said(&s, &turn).unwrap(), Said::Line(d));
    // Written once.
    assert_eq!(
        did::write(&s, &w, &turn, &DayPurse::new(&s, None, false))
            .await
            .unwrap(),
        Outcome::Had
    );
    assert_eq!(w.asks.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn a_line_is_cut_by_code_and_none_is_shown_as_not_summarised() {
    assert_eq!(
        did::line_of(&"x".repeat(500)).unwrap().chars().count(),
        did::MAX_CHARS
    );
    assert_eq!(did::line_of("\n\n  "), None);
    let (_d, s) = store();
    let turn = exchange_at(&s, "ses_a", PLACE, 1_000, false);
    let said = did::said(&s, &turn).unwrap();
    assert_eq!(
        (said.text(), said.handle()),
        (NOT_SUMMARISED, turn.as_str())
    );
}

#[tokio::test]
async fn the_days_line_holds_and_what_was_spent_is_kept() {
    let (_d, s) = store();
    let w = Fake {
        cost: 0.75,
        ..Fake::default()
    };
    let mut written = 0;
    for i in 0..4 {
        let turn = exchange_at(&s, "ses_a", PLACE, 1_000 + i, false);
        if let Outcome::Written(_) = did::write(&s, &w, &turn, &DayPurse::new(&s, None, false))
            .await
            .unwrap()
        {
            written += 1;
        }
    }
    // $2.00 a day: two calls at $0.75, the third's worst case would pass it.
    assert_eq!(written, 2);
    let day: super::Day = s.get_meta(super::DAY_KEY).unwrap().unwrap();
    assert!((day.spent_usd - 1.5).abs() < 1e-9, "{day:?}");
}

#[tokio::test]
async fn an_exchange_still_being_answered_has_no_line_yet() {
    let (_d, s) = store();
    let turn = exchange_at(&s, "ses_a", PLACE, 1_000, false);
    let mut ex = exchange::exchange(&s, &turn).unwrap().unwrap();
    ex.end_ms = None;
    s.append(&[exchange::exchange_record(&ex).unwrap()])
        .unwrap();
    let w = Fake::default();
    assert_eq!(
        did::write(&s, &w, &turn, &DayPurse::new(&s, None, false))
            .await
            .unwrap(),
        Outcome::NotEnded
    );
}

fn step<'a>(w: &'a Fake, purse: &'a DayPurse<'a>) -> Step<'a> {
    Step {
        writer: w,
        flags: &NoFlags,
        purse,
    }
}

#[tokio::test]
async fn the_window_folds_its_oldest_n_at_2n_and_keeps_n() {
    let (_d, s) = store();
    let turns: Vec<String> = (0..2 * N as u64 - 1)
        .map(|i| exchange_at(&s, "ses_a", PLACE, 1_000 + i, i == 3))
        .collect();
    let w = Fake::default();
    let s1 = read::fold_step(&s, PLACE, None, &step(&w, &DayPurse::new(&s, None, false)))
        .await
        .unwrap();
    assert!(s1.made.is_empty(), "2N - 1 exchanges: no fold");
    assert_eq!(
        read::ladder(&s, PLACE, None).unwrap().window.len(),
        2 * N - 1
    );
    let last = exchange_at(&s, "ses_a", PLACE, 1_000 + 2 * N as u64, false);
    let s2 = read::fold_step(&s, PLACE, None, &step(&w, &DayPurse::new(&s, None, false)))
        .await
        .unwrap();
    assert_eq!(s2.made.len(), 1);
    let node = &s2.made[0];
    assert_eq!((node.level, node.exchanges.len()), (1, N));
    assert_eq!(
        node.exchanges,
        turns[..N].to_vec(),
        "the oldest N, in order"
    );
    assert_eq!(
        (node.title.as_str(), node.marks.outside),
        ("Tide gauges", true)
    );
    assert_eq!(node.account, "Recalibrated gauge 4 and left gauge 5 open.");
    // His words by reference: the message nodes, never their text.
    let first = exchange::exchange(&s, &turns[0]).unwrap().unwrap();
    assert_eq!(node.his[0].node, first.ask.unwrap());
    let l = read::ladder(&s, PLACE, None).unwrap();
    assert_eq!(l.window.len(), N, "the window is N again");
    assert_eq!(l.window.last().unwrap().turn_id, last);
    assert_eq!(l.rungs[0].nodes, vec![node.clone()]);
    // The handle opens its exchanges; a node never changes once written.
    let opened = fold::open(&s, &fold::node(&s, &node.id).unwrap().unwrap()).unwrap();
    assert_eq!(opened.len(), N);
    let asked = w.asks.lock().unwrap().clone();
    let fold_ask = asked.iter().find(|a| a.work == Work::Fold).unwrap();
    assert!(!fold_ask.text.contains("SECRET-TOOL-OUTPUT"));
    assert!(
        fold_ask.text.contains(NOT_SUMMARISED),
        "no line: the reader's words"
    );
}

/// What a first-level node over a sealed or veiled source said in place of an account, before anything goes
/// (2026-11-22): no node is made so any more, and one already written is read and shown as it stands.
const OLD_HIDDEN: &str = "Not summarised: a source of these exchanges is sealed or veiled.";

/// A first-level node written over a sealed or veiled source before anything goes (2026-11-22), as the
/// store holds it: it still decodes, the ladder reads it, and it is shown as it stands: a node is written
/// once for its range, so the fold step takes it and asks no model for its exchanges again.
#[tokio::test]
async fn an_old_node_over_a_sealed_source_still_reads_and_is_shown_as_it_stands() {
    let (_d, s) = store();
    let turns: Vec<String> = (0..2 * N as u64)
        .map(|i| exchange_at(&s, "ses_sealed", PLACE, 1_000 + i, false))
        .collect();
    let last_ms = 1_000 + N as u64 - 1;
    let old = json!({
        "id": "fld_old", "place": PLACE, "level": 1, "exchanges": &turns[..N],
        "first_ms": 1_000, "last_ms": last_ms, "his": [], "counted": 0, "children": [],
        "title": format!("{N} exchanges, not summarised"), "account": OLD_HIDDEN,
        "marks": {"place": PLACE, "audience": "private", "author": "theseus", "outside": false,
                  "sealed": true, "veiled": true},
        "model": null, "writer": 1, "at_ms": 5,
    });
    let old: fold::FoldNode = serde_json::from_value(old).unwrap();
    s.append(&fold::records(&old).unwrap()).unwrap();
    let l = read::ladder(&s, PLACE, None).unwrap();
    assert_eq!(
        l.rungs[0].nodes,
        vec![old.clone()],
        "it reads as it was written"
    );
    assert_eq!(
        l.window.len(),
        N,
        "the exchanges it stands for are out of the window"
    );
    let w = Fake::default();
    let purse = DayPurse::new(&s, None, false);
    let oldest: Vec<Exchange> = turns[..N]
        .iter()
        .map(|t| exchange::exchange(&s, t).unwrap().unwrap())
        .collect();
    let (again, spent) = read::first(&s, &oldest, &step(&w, &purse))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (again, spent),
        (old, 0.0),
        "shown as it stands, never written again"
    );
    assert!(w.asks.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_level_of_2n_nodes_folds_into_one_above_built_by_code() {
    let (_d, s) = store();
    let w = Fake::default();
    let flags = Flagged;
    let purse = DayPurse::new(&s, None, false);
    let st = Step {
        writer: &w,
        flags: &flags,
        purse: &purse,
    };
    let mut made = Vec::new();
    for i in 0..(2 * N * N + N) as u64 {
        exchange_at(
            &s,
            &format!("ses_{}", i / N as u64),
            PLACE,
            1_000 + i,
            false,
        );
        if (i + 1) % N as u64 == 0 {
            made.extend(read::fold_step(&s, PLACE, None, &st).await.unwrap().made);
        }
    }
    let above: Vec<_> = made.iter().filter(|n| n.level == 2).collect();
    assert_eq!(
        above.len(),
        1,
        "{:?}",
        made.iter().map(|n| n.level).collect::<Vec<_>>()
    );
    let two = above[0];
    assert_eq!(
        (two.children.len(), two.exchanges.len(), two.model.clone()),
        (N, N * N, None)
    );
    let firsts = w
        .asks
        .lock()
        .unwrap()
        .iter()
        .filter(|a| a.work == Work::Fold)
        .count();
    assert_eq!(
        firsts,
        made.len() - 1,
        "no model writes a level above the first"
    );
    // His lines above the first level: the flagged kept, the rest counted.
    assert!(!two.his.is_empty() && two.his.iter().all(|h| h.standing));
    assert_eq!(two.his.len() as u32 + two.counted, (N * N) as u32);
    let l = read::ladder(&s, PLACE, None).unwrap();
    assert_eq!(
        l.rungs
            .iter()
            .map(|r| (r.level, r.nodes.len()))
            .collect::<Vec<_>>(),
        vec![(2, 1), (1, N)]
    );
    assert_eq!(l.window.len(), N);
}

/// Every tenth of the person's lines made a standing record.
struct Flagged;

impl fold::Flags for Flagged {
    fn correction(&self, _: &str, _: &str) -> bool {
        false
    }
    fn standing(&self, node: &str) -> bool {
        node.ends_with('0')
    }
}

#[tokio::test]
async fn only_what_came_after_the_latest_new_is_counted() {
    let (_d, s) = store();
    for i in 0..2 * N as u64 {
        exchange_at(&s, "ses_old", PLACE, 1_000 + i, false);
    }
    for i in 0..3 {
        exchange_at(&s, "ses_new", PLACE, 9_000 + i, false);
    }
    let l = read::ladder(&s, PLACE, Some(&new_at(9_000))).unwrap();
    assert_eq!(l.window.len(), 3);
    assert!(l.window.iter().all(|e| e.session_id == "ses_new"));
    let w = Fake::default();
    let st = read::fold_step(
        &s,
        PLACE,
        Some(&new_at(9_000)),
        &step(&w, &DayPurse::new(&s, None, false)),
    )
    .await
    .unwrap();
    assert!(
        st.made.is_empty(),
        "the window before /new does not fold into this ladder"
    );
}

#[test]
fn a_fold_answer_is_a_title_and_an_account_cut_by_code() {
    let (t, a) = fold::parse(&format!("{}\n\n{}", "t".repeat(300), "a".repeat(5_000))).unwrap();
    assert_eq!(
        (t.chars().count(), a.chars().count()),
        (fold::TITLE_CHARS, fold::ACCOUNT_CHARS)
    );
    assert_eq!(fold::parse("a title alone"), None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_views_traits_read_the_ladder_and_write_its_nodes() {
    use crate::view::fold::{FoldWriter, Ladder};
    let (_d, s) = store();
    for i in 0..2 * N as u64 {
        exchange_at(&s, "ses_a", PLACE, 1_000 + i, false);
    }
    let w = Fake::default();
    let folds = super::view::StoreFolds {
        store: &s,
        writer: &w,
        flags: &NoFlags,
    };
    let ladder = super::view::StoreLadder(&s);
    let before = ladder.read(PLACE, N, None).unwrap();
    assert!(before.nodes.is_empty());
    assert_eq!(before.window.len(), 2 * N);
    assert!(crate::view::fold::fold_due(before.window.len(), N));
    let node = folds.fold(PLACE, &before.window[..N]).unwrap();
    assert_eq!(
        (node.level, node.exchanges, node.title.as_str()),
        (1, N as u32, "Tide gauges")
    );
    assert_eq!(node.first.id, before.window[0].turn_id);
    assert_eq!(node.writer.by, "fake-small-1");
    let after = ladder.read(PLACE, N, None).unwrap();
    assert_eq!(after.nodes, vec![node.clone()]);
    assert_eq!(after.window, before.window[N..].to_vec());
    assert!(ladder.read(PLACE, N + 1, None).is_err(), "one grain");
    let lifted = folds.lift(2, std::slice::from_ref(&node)).unwrap();
    assert_eq!((lifted.level, lifted.writer.by.as_str()), (2, "code"));
    assert!(
        folds.lift(3, &[node]).is_err(),
        "a level is built from the level below"
    );
}

/// Exchange `turn` heard in a place whose class became shared.
fn made_shared(s: &Store, turn: &str) {
    let mut ex = exchange::exchange(s, turn).unwrap().unwrap();
    ex.private = false;
    s.append(&[exchange::exchange_record(&ex).unwrap()])
        .unwrap();
}

#[tokio::test]
async fn a_fold_over_a_change_of_audience_carries_the_narrowest_and_so_does_a_lift() {
    let (_d, s) = store();
    for i in 0..2 * N as u64 {
        let t = exchange_at(&s, "ses_a", PLACE, 1_000 + i, false);
        if i >= 10 {
            made_shared(&s, &t);
        }
    }
    let w = Fake::default();
    let st = read::fold_step(&s, PLACE, None, &step(&w, &DayPurse::new(&s, None, false)))
        .await
        .unwrap();
    assert_eq!(st.made[0].marks.audience, Audience::Private, "private wins");
    // A lift over a private node and a shared one, and over an unknown one.
    let mut shared = st.made[0].clone();
    shared.marks.audience = Audience::Shared;
    let up = fold::above(&[shared.clone(), st.made[0].clone()]).unwrap();
    assert_eq!(up.marks.audience, Audience::Private);
    let mut unknown = shared.clone();
    unknown.marks.audience = Audience::Unknown;
    assert_eq!(
        fold::above(&[st.made[0].clone(), unknown])
            .unwrap()
            .marks
            .audience,
        Audience::Unknown
    );
    let mut open = shared.clone();
    open.marks.audience = Audience::Open;
    assert_eq!(
        fold::above(&[open, shared]).unwrap().marks.audience,
        Audience::Shared
    );
}

#[tokio::test]
async fn a_ladder_reads_only_its_own_place_and_a_place_that_breaks_a_key_has_none() {
    let (_d, s) = store();
    let other = format!("{PLACE}{}01{}x", super::SEP, super::SEP);
    for i in 0..5 {
        exchange_at(&s, "ses_a", PLACE, 1_000 + i, false);
        exchange_at(&s, "ses_b", &other, 1_000 + i, false);
    }
    let l = read::ladder(&s, PLACE, None).unwrap();
    assert_eq!(l.window.len(), 5);
    assert!(
        l.window.iter().all(|e| e.place == PLACE),
        "the prefix is not the boundary"
    );
    assert!(read::ladder(&s, &other, None).unwrap().window.is_empty());
    let turn = exchange_at(&s, "ses_b", &other, 2_000, false);
    let w = Fake::default();
    let out = did::write(&s, &w, &turn, &DayPurse::new(&s, None, false))
        .await
        .unwrap();
    assert_eq!(out, Outcome::NoLadder);
    assert!(w.asks.lock().unwrap().is_empty());
}

/// Every exchange of `turns` is held by one node or stands in the window, in order.
fn nothing_lost(s: &Store, turns: &[String]) {
    let l = read::ladder(s, PLACE, None).unwrap();
    let mut held: Vec<String> = fold::newest(s, PLACE, 1, 1_000)
        .unwrap()
        .into_iter()
        .rev()
        .flat_map(|n| n.exchanges)
        .collect();
    held.extend(l.window.iter().map(|e| e.turn_id.clone()));
    assert_eq!(held, turns.to_vec());
}

#[tokio::test]
async fn a_history_past_eighty_folds_from_its_oldest_and_loses_nothing() {
    for total in [100u64, 250] {
        let (_d, s) = store();
        let turns: Vec<String> = (0..total)
            .map(|i| exchange_at(&s, "ses_a", PLACE, 1_000 + i, false))
            .collect();
        assert_eq!(
            read::ladder(&s, PLACE, None).unwrap().window.len(),
            total as usize
        );
        let w = Fake::default();
        let st = read::fold_step(&s, PLACE, None, &step(&w, &DayPurse::new(&s, None, false)))
            .await
            .unwrap();
        let folds = (total as usize - N) / N;
        assert_eq!(st.made.len(), folds, "{total}");
        assert_eq!(
            st.made[0].exchanges,
            turns[..N].to_vec(),
            "the oldest first"
        );
        assert_eq!(
            read::ladder(&s, PLACE, None).unwrap().window.len(),
            total as usize - folds * N
        );
        nothing_lost(&s, &turns);
    }
}

#[tokio::test]
async fn a_long_failure_of_the_folds_model_grows_the_window_and_recovery_folds_it_all() {
    let (_d, s) = store();
    let w = Fake {
        fail: 30.into(),
        ..Fake::default()
    };
    let mut turns = Vec::new();
    for i in 0..120u64 {
        turns.push(exchange_at(&s, "ses_a", PLACE, 1_000 + i, false));
        let st = read::fold_step(&s, PLACE, None, &step(&w, &DayPurse::new(&s, None, false)))
            .await
            .unwrap();
        if i < 2 * N as u64 + 20 {
            assert!(st.made.is_empty());
        }
    }
    assert_eq!(read::ladder(&s, PLACE, None).unwrap().window.len(), N);
    nothing_lost(&s, &turns);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_fold_writer_returns_a_written_range_and_never_writes_it_again() {
    use crate::view::fold::{FoldWriter, Ladder};
    let (_d, s) = store();
    for i in 0..2 * N as u64 {
        exchange_at(&s, "ses_a", PLACE, 1_000 + i, false);
    }
    let w = Fake::default();
    let folds = super::view::StoreFolds {
        store: &s,
        writer: &w,
        flags: &NoFlags,
    };
    let window = super::view::StoreLadder(&s)
        .read(PLACE, N, None)
        .unwrap()
        .window;
    let a = folds.fold(PLACE, &window[..N]).unwrap();
    let b = folds.fold(PLACE, &window[..N]).unwrap();
    assert_eq!(a, b, "the same node, the same handle");
    assert_eq!(fold::newest(&s, PLACE, 1, 10).unwrap().len(), 1, "one rung");
    let asked = w
        .asks
        .lock()
        .unwrap()
        .iter()
        .filter(|a| a.work == Work::Fold)
        .count();
    assert_eq!(asked, 1, "paid for once");
    let up = folds.lift(2, std::slice::from_ref(&a)).unwrap();
    assert_eq!(folds.lift(2, std::slice::from_ref(&a)).unwrap(), up);
    assert_eq!(fold::newest(&s, PLACE, 2, 10).unwrap().len(), 1);
}

#[tokio::test]
async fn an_exchange_that_read_outside_text_is_sent_inside_the_wrapper() {
    let (_d, s) = store();
    let outside = exchange_at(&s, "ses_a", PLACE, 1_000, true);
    let inside = exchange_at(&s, "ses_a", PLACE, 1_001, false);
    let w = Fake::default();
    for t in [&outside, &inside] {
        did::write(&s, &w, t, &DayPurse::new(&s, None, false))
            .await
            .unwrap();
    }
    let asked = w.asks.lock().unwrap().clone();
    assert!(
        asked[0].text.starts_with("[outside text from "),
        "{}",
        asked[0].text
    );
    assert!(asked[0].text.contains("Gauge 1000 reads 2 mm high"));
    assert!(asked[0].text.trim_end().ends_with(']'));
    assert!(!asked[1].text.contains("[outside text from "));
}

#[test]
fn a_ladder_keeps_every_exchange_when_one_millisecond_spans_pages() {
    let (_d, s) = store();
    let mut turns: Vec<_> = (0..401)
        .map(|_| exchange_at(&s, "ses_a", PLACE, 1_000, false))
        .collect();
    turns.sort();
    let l = read::ladder(&s, PLACE, None).unwrap();
    assert_eq!(
        l.window
            .iter()
            .map(|e| e.turn_id.clone())
            .collect::<Vec<_>>(),
        turns
    );
}
