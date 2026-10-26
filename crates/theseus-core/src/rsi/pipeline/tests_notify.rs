//! RSI tells the owner and never parks (theseus-o1h3f.6), through the
//! pipeline's rig: a tick that started builds stages one `self_digest` line
//! in the night's own words, a tick that started nothing stages none; an item
//! two reviews rejected is dropped with both findings and the way back, not
//! parked; and the keel still files `waiting_human` and never starts.

use serde_json::json;
use theseus_protocol::tasks::TaskState;

use super::tests::{rig_with, Rig};
use crate::ledger::LedgerRow;
use crate::places::BoundPlace;
use crate::rsi::backlog::start::{night_text, Item};
use crate::rsi::backlog::{Night, Started};

/// The owner's DM, bound: where the courier would post the lines.
fn bind_dm(r: &Rig) {
    r.core.bind_places(vec![BoundPlace {
        target: "discord:dm:100000000000000001".into(),
        name: "DM @owner".into(),
        private: true,
        ..Default::default()
    }]);
}

fn lines(r: &Rig) -> Vec<String> {
    crate::tests_day_ceiling::posts(&r.core, "self_digest")
        .iter()
        .map(|b| b["text"].as_str().unwrap_or_default().to_string())
        .collect()
}

fn title_of(r: &Rig, item: &str) -> String {
    crate::task_graph::get(&r.core.store, item)
        .unwrap()
        .unwrap()
        .title
}

/// The night's own words for these items, as `night_text` words them.
fn nights_words(r: &Rig, items: &[String]) -> String {
    let night = Night {
        started: items
            .iter()
            .map(|i| Started {
                item: i.clone(),
                title: title_of(r, i),
                session_id: String::new(),
                objective: String::new(),
                mint: None,
            })
            .collect(),
        ..Night::default()
    };
    night_text(&night, |id| Item {
        title: title_of(r, id),
        count: 0,
        since_ms: 0,
    })
}

/// A tick that starts one build stages one line in `night_text`'s words,
/// naming the item; the next tick, which starts nothing, stages none.
#[tokio::test]
async fn a_tick_that_started_a_build_tells_the_owner_once() {
    let r = rig_with(1, 30.0);
    bind_dm(&r);
    let t = r.tick().await;
    assert_eq!(t.started.len(), 1, "{t:?}");
    let said = lines(&r);
    assert_eq!(said.len(), 1, "{said:?}");
    assert_eq!(said[0], nights_words(&r, &t.started), "the night's wording");
    assert!(said[0].contains(&t.started[0]), "{}", said[0]);
    let t = r.tick().await;
    assert!(t.started.is_empty(), "the one slot is taken: {t:?}");
    assert_eq!(
        lines(&r).len(),
        1,
        "a tick that started nothing says nothing"
    );
}

/// One line a tick, not one per item: two builds started together are both
/// named in the one line.
#[tokio::test]
async fn a_tick_that_started_two_builds_names_both_in_one_line() {
    let r = rig_with(2, 30.0);
    bind_dm(&r);
    let t = r.tick().await;
    assert_eq!(t.started.len(), 2, "{t:?}");
    let said = lines(&r);
    assert_eq!(said.len(), 1, "{said:?}");
    assert_eq!(said[0], nights_words(&r, &t.started));
    assert!(t.started.iter().all(|i| said[0].contains(i.as_str())));
}

/// Two rejections drop the item: failed, not `waiting_human`, no `parked:`
/// evidence, both findings and the real command in the notice, and nothing
/// waits. The owner's retry puts it back to ready with a clean count.
#[tokio::test]
async fn an_item_two_reviews_rejected_is_dropped_with_both_findings() {
    let r = rig_with(1, 30.0);
    bind_dm(&r);
    r.tick().await;
    let item = r.fake.starts.lock().unwrap()[0].0.clone();
    let (b1, h1) = r.built(&item, 1);
    r.reject_with(&item, &b1, &h1, "the test passes without the fix");
    r.tick().await;
    let (b2, h2) = r.built(&item, 2);
    r.reject_with(&item, &b2, &h2, "the diff widens a public type");
    r.tick().await;
    assert_eq!(r.state(&item), TaskState::Failed);
    let t = crate::task_graph::get(&r.core.store, &item)
        .unwrap()
        .unwrap();
    assert!(
        !t.evidence.iter().any(|e| e.identity.starts_with("parked:")),
        "{:?}",
        t.evidence
    );
    assert!(t
        .evidence
        .iter()
        .any(|e| e.identity == "dropped:rejected-twice"));
    let said = lines(&r);
    let dropped: Vec<_> = said
        .iter()
        .filter(|l| l.contains("dropped after two rejections"))
        .collect();
    assert_eq!(dropped.len(), 1, "{said:?}");
    let note = dropped[0];
    assert!(note.contains("the test passes without the fix"), "{note}");
    assert!(note.contains("the diff widens a public type"), "{note}");
    assert!(
        note.contains(&format!(
            "`{}`",
            theseus_protocol::rsi::retry_command(&item)
        )),
        "{note}"
    );
    // The drop is final until the owner's word: it starts nothing.
    let again = r.tick().await;
    assert!(!again.started.contains(&item), "{again:?}");
    // `theseus self retry` brings it back, and the old rejections no longer count.
    r.core
        .self_retry(
            &theseus_protocol::rsi::SelfRetryParams {
                item: item.clone(),
                ..Default::default()
            },
            &crate::approval::Answerer::from("the CLI"),
        )
        .unwrap();
    assert_eq!(r.state(&item), TaskState::Proposed);
    r.tick().await;
    let v = r.view();
    assert!(
        v.ready.iter().any(|e| e.item == item),
        "ready again: {:?}",
        v.ready
    );
    assert_eq!(r.rows("self.pipeline.parked").len(), 1, "not dropped again");
}

/// The keel holds on the paths this row loosened: a cause in a keel class
/// is still filed `waiting_human` and the pipeline never starts it.
#[tokio::test]
async fn a_keel_class_cause_still_waits_for_the_owner_and_never_starts() {
    let r = rig_with(3, 30.0);
    let t0 = theseus_protocol::now_unix_ms();
    for i in 0..2u64 {
        let mut row = LedgerRow::named(
            "approval.refused",
            None,
            None,
            json!({"act": "self.resume", "who": "discord:1", "why": "not the owner"}),
        );
        row.at_unix_ms = t0 + (1 + i) * 3_600_000;
        r.core.store.append_ledger(&row).unwrap();
    }
    let night = r.core.backlog_night(t0 + 4 * 3_600_000).unwrap();
    assert_eq!(night.waiting.len(), 1, "{night:?}");
    let marked = night.waiting[0].clone();
    assert_eq!(r.state(&marked), TaskState::WaitingHuman);
    for _ in 0..3 {
        let t = r.tick().await;
        assert!(!t.started.contains(&marked), "{t:?}");
    }
    assert_eq!(r.state(&marked), TaskState::WaitingHuman);
}

/// A head a join returned for a conflict with main joins nothing and holds nobody (the item is
/// built again off the new main): it is in no queue, so no join pass is due and the tender sleeps
/// its interval instead of spinning on a head no pass can advance (theseus-o1h3f.6).
#[tokio::test]
async fn a_conflicted_head_is_in_no_queue_and_no_join_is_due() {
    let r = rig_with(1, 30.0);
    r.tick().await;
    let item = r.fake.starts.lock().unwrap()[0].0.clone();
    let (b, h) = r.built(&item, 1);
    r.core
        .rec(None)
        .record(&crate::fact::rsi_join::SelfReviewVerdict {
            item: item.clone(),
            branch: b.clone(),
            head: h.clone(),
            reviewer: "fable (claude-fable-5-1)".into(),
            model: "claude-fable-5-1".into(),
            verdict: "accept".into(),
            planted_revert: "failed".into(),
            reasons: vec![],
            cost_usd: 0.5,
        });
    r.core
        .rec(None)
        .record(&crate::fact::rsi_join::SelfJoinStopped {
            branch: b,
            head: h,
            phase: "merge".into(),
            why: "it conflicts with main in same.txt".into(),
            numbers: json!({"held": true, "conflict": true, "dropped": false}),
        });
    let t = r.tick().await;
    assert!(!t.join_due, "{t:?}");
    let v = r.view();
    assert!(!v.joining.iter().any(|e| e.item == item), "{:?}", v.joining);
    assert!(!v.held.iter().any(|e| e.item == item), "{:?}", v.held);
}

/// The notice for an item two reviews rejected carries both findings, each bounded: a reviewer's
/// note is up to 1,000 characters, and two whole would pass Discord's 2,000-character message.
#[test]
fn a_twice_rejected_notice_bounds_each_finding() {
    let e = super::queues::Entry {
        item: "tsk_1".into(),
        title: "Trim the sails".into(),
        findings: vec!["a".repeat(1_000), "b".repeat(1_000)],
        ..Default::default()
    };
    let text = super::rejected_twice_text(&e);
    assert!(text.contains(&"a".repeat(300)), "{text}");
    assert!(!text.contains(&"a".repeat(301)), "{text}");
    assert!(text.contains(&"b".repeat(300)), "{text}");
    assert!(!text.contains(&"b".repeat(301)), "{text}");
    assert!(text.chars().count() < 1_500, "{}", text.chars().count());
}
