//! The ladder over a store with a fake writer: a `did` line from the message
//! and the reply alone, once, with its exchange's marks; none for a sealed
//! source or past the day's line; the reader's words where there is none;
//! the fold by count, its words by reference and its marks; the levels above
//! built by code; and `/new`. Every name here is invented.

use std::sync::Mutex;

use serde_json::json;

use super::did::{self, Outcome, Said, NOT_SUMMARISED};
use super::fold::{self, NoFlags};
use super::read::{self, Step};
use super::writer::{Ask, Work, Writer, Written};
use super::{Marks, N};
use crate::exchange::{self, Exchange, Opener, Root};
use crate::node::{Body, Node};
use crate::store::Store;

const PLACE: &str = "discord:channel:4242";

/// A writer that answers each call with its script, and keeps what it read.
#[derive(Default)]
pub(super) struct Fake {
    pub asks: Mutex<Vec<Ask>>,
    pub cost: f64,
}

impl Writer for Fake {
    fn write(
        &self,
        ask: Ask,
        left_usd: f64,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Written> + Send + '_>> {
        let work = ask.work;
        self.asks.lock().unwrap().push(ask);
        let cost = self.cost;
        Box::pin(async move {
            if cost > left_usd {
                return Written::not("the day's line");
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
    let recs = vec![
        ask.record().unwrap(),
        result.record().unwrap(),
        reply.record().unwrap(),
        exchange::exchange_record(&ex).unwrap(),
        exchange::touch(Root::Place(place), &turn, sid, at).unwrap(),
    ];
    store.append(&recs).unwrap();
    turn
}

fn shown() -> impl Fn(&str) -> bool + Sync {
    |_: &str| false
}

#[tokio::test]
async fn a_did_line_reads_the_message_and_the_reply_never_a_tool_result() {
    let (_d, s) = store();
    let turn = exchange_at(&s, "ses_a", PLACE, 1_000, true);
    let w = Fake::default();
    let out = did::write(&s, &w, &turn, &shown(), None).await.unwrap();
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
    assert_eq!(
        d.marks,
        Marks { outside_text: true },
        "it carries its exchange's marks"
    );
    let asked = &w.asks.lock().unwrap()[0];
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
        did::write(&s, &w, &turn, &shown(), None).await.unwrap(),
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
async fn a_sealed_source_is_not_summarised_and_nothing_is_asked() {
    let (_d, s) = store();
    let turn = exchange_at(&s, "ses_sealed", PLACE, 1_000, false);
    let w = Fake::default();
    let sealed = |sid: &str| sid == "ses_sealed";
    let out = did::write(&s, &w, &turn, &sealed, None).await.unwrap();
    assert_eq!(out, Outcome::Hidden);
    assert!(w.asks.lock().unwrap().is_empty());
    assert_eq!(did::did(&s, &turn).unwrap(), None);
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
        if let Outcome::Written(_) = did::write(&s, &w, &turn, &shown(), None).await.unwrap() {
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
        did::write(&s, &w, &turn, &shown(), None).await.unwrap(),
        Outcome::NotEnded
    );
}

fn step<'a>(w: &'a Fake, hidden: &'a (dyn Fn(&str) -> bool + Sync)) -> Step<'a> {
    Step {
        writer: w,
        flags: &NoFlags,
        hidden,
        cap_usd: None,
    }
}

#[tokio::test]
async fn the_window_folds_its_oldest_n_at_2n_and_keeps_n() {
    let (_d, s) = store();
    let turns: Vec<String> = (0..2 * N as u64 - 1)
        .map(|i| exchange_at(&s, "ses_a", PLACE, 1_000 + i, i == 3))
        .collect();
    let w = Fake::default();
    let hidden = shown();
    let s1 = read::fold_step(&s, PLACE, 0, &step(&w, &hidden))
        .await
        .unwrap();
    assert!(s1.made.is_empty(), "2N - 1 exchanges: no fold");
    assert_eq!(read::ladder(&s, PLACE, 0).unwrap().window.len(), 2 * N - 1);
    let last = exchange_at(&s, "ses_a", PLACE, 1_000 + 2 * N as u64, false);
    let s2 = read::fold_step(&s, PLACE, 0, &step(&w, &hidden))
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
        (node.title.as_str(), node.marks.outside_text),
        ("Tide gauges", true)
    );
    assert_eq!(node.account, "Recalibrated gauge 4 and left gauge 5 open.");
    // His words by reference: the message nodes, never their text.
    let first = exchange::exchange(&s, &turns[0]).unwrap().unwrap();
    assert_eq!(node.his[0].node, first.ask.unwrap());
    let l = read::ladder(&s, PLACE, 0).unwrap();
    assert_eq!(l.window.len(), N, "the window is N again");
    assert_eq!(l.window.last().unwrap().turn_id, last);
    assert_eq!(l.rungs[0].nodes, vec![node.clone()]);
    // The handle opens its exchanges; a node never changes once written.
    let opened = fold::open(&s, &fold::node(&s, &node.id).unwrap().unwrap()).unwrap();
    assert_eq!(opened.len(), N);
    let asked = w.asks.lock().unwrap();
    let fold_ask = asked.iter().find(|a| a.work == Work::Fold).unwrap();
    assert!(!fold_ask.text.contains("SECRET-TOOL-OUTPUT"));
    assert!(
        fold_ask.text.contains(NOT_SUMMARISED),
        "no line: the reader's words"
    );
}

#[tokio::test]
async fn a_sealed_window_folds_by_code_and_no_model_reads_it() {
    let (_d, s) = store();
    for i in 0..2 * N as u64 {
        exchange_at(&s, "ses_sealed", PLACE, 1_000 + i, false);
    }
    let w = Fake::default();
    let sealed = |sid: &str| sid == "ses_sealed";
    let st = read::fold_step(&s, PLACE, 0, &step(&w, &sealed))
        .await
        .unwrap();
    assert_eq!(st.made.len(), 1);
    assert_eq!(st.made[0].account, fold::HIDDEN);
    assert_eq!(st.made[0].model, None);
    assert!(w.asks.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_level_of_2n_nodes_folds_into_one_above_built_by_code() {
    let (_d, s) = store();
    let w = Fake::default();
    let hidden = shown();
    let flags = Flagged;
    let st = Step {
        writer: &w,
        flags: &flags,
        hidden: &hidden,
        cap_usd: None,
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
            made.extend(read::fold_step(&s, PLACE, 0, &st).await.unwrap().made);
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
    let l = read::ladder(&s, PLACE, 0).unwrap();
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
    fn correction(&self, _: &str) -> bool {
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
    let l = read::ladder(&s, PLACE, 9_000).unwrap();
    assert_eq!(l.window.len(), 3);
    assert!(l.window.iter().all(|e| e.session_id == "ses_new"));
    let w = Fake::default();
    let hidden = shown();
    let st = read::fold_step(&s, PLACE, 9_000, &step(&w, &hidden))
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
