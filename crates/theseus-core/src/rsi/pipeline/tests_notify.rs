//! RSI tells the owner and never parks (theseus-o1h3f.6), through the
//! pipeline's rig: a tick that started builds says nothing at once, and the
//! next note that goes out (the night's, or the morning's) lists them, once,
//! in the night's own words (the owner, 2026-11-09: five "1 started" lines in
//! two and a half hours); an item two reviews rejected is dropped with both
//! findings and the way back, not parked; and the keel still files
//! `waiting_human` and never starts.

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

fn task_of(r: &Rig, item: &str) -> theseus_protocol::tasks::TaskRecord {
    crate::task_graph::get(&r.core.store, item)
        .unwrap()
        .unwrap()
}

fn title_of(r: &Rig, item: &str) -> String {
    task_of(r, item).title
}

/// A night's note, an hour on: what it posts, and when.
fn night_now() -> u64 {
    theseus_protocol::now_unix_ms() + 3_600_000
}

/// The night's own words for these items, as `night_text` words them for
/// items filed by one sighting each (the rig's three causes).
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
        count: 1,
        since_ms: task_of(r, id).created_at_ms,
    })
}

/// A tick that starts one build says nothing at once, and neither does the
/// next, which starts nothing; the night's note lists the item once, in the
/// night's words, and the note after it has nothing to add.
#[tokio::test]
async fn a_tick_that_started_a_build_tells_the_owner_once_in_the_next_note() {
    let r = rig_with(1, 30.0);
    bind_dm(&r);
    let t = r.tick().await;
    assert_eq!(t.started.len(), 1, "{t:?}");
    assert!(lines(&r).is_empty(), "no line of its own: {:?}", lines(&r));
    let t2 = r.tick().await;
    assert!(t2.started.is_empty(), "the one slot is taken: {t2:?}");
    assert!(lines(&r).is_empty(), "{:?}", lines(&r));
    r.core.backlog_night(night_now()).unwrap();
    let said = lines(&r);
    assert_eq!(said.len(), 1, "{said:?}");
    assert_eq!(said[0], nights_words(&r, &t.started), "the night's wording");
    assert!(said[0].contains(&t.started[0]), "{}", said[0]);
    r.core.backlog_night(night_now() + 1).unwrap();
    assert_eq!(lines(&r).len(), 1, "told once: nothing is left to tell");
}

/// One line, not one per item: two builds started together are both named in
/// the one note that tells them.
#[tokio::test]
async fn a_tick_that_started_two_builds_names_both_in_one_line() {
    let r = rig_with(2, 30.0);
    bind_dm(&r);
    let t = r.tick().await;
    assert_eq!(t.started.len(), 2, "{t:?}");
    assert!(lines(&r).is_empty(), "{:?}", lines(&r));
    r.core.backlog_night(night_now()).unwrap();
    let said = lines(&r);
    assert_eq!(said.len(), 1, "{said:?}");
    assert_eq!(said[0], nights_words(&r, &t.started));
    assert!(t.started.iter().all(|i| said[0].contains(i.as_str())));
}

/// Two starts on two later ticks (the first build's turn ended, its slot
/// freed) post nothing: the day's five "1 started" lines are one line in the
/// next note, naming both, with the one count in its title.
#[tokio::test]
async fn two_later_starts_post_nothing_and_are_one_line_in_the_next_note() {
    let mut r = rig_with(1, 30.0);
    bind_dm(&r);
    let first = r.tick().await.started;
    r.grace = std::time::Duration::ZERO;
    let second = r.tick().await.started;
    assert_eq!((first.len(), second.len()), (1, 1), "{first:?} {second:?}");
    assert_ne!(first, second);
    assert!(lines(&r).is_empty(), "no FYI per start: {:?}", lines(&r));
    r.core.backlog_night(night_now()).unwrap();
    let said = lines(&r);
    assert_eq!(said.len(), 1, "{said:?}");
    let both = [first, second].concat();
    assert_eq!(said[0], nights_words(&r, &both));
    assert!(said[0].contains("2 started"), "{}", said[0]);
}

/// The morning note's facts count the starts waiting for a note, its lines
/// name each in the night's words, and a note that listed them takes only
/// those off (a start that came after is still waiting).
#[tokio::test]
async fn the_morning_note_lists_the_waiting_starts_and_takes_only_those() {
    let r = rig_with(1, 30.0);
    bind_dm(&r);
    assert!(r.core.morning_facts(night_now(), None, 0, 0).is_none());
    let t = r.tick().await;
    let folded = r.core.folded_read();
    assert_eq!(folded.count(), 1, "{folded:?}");
    let (brief, n) = r
        .core
        .morning_facts(night_now(), None, 0, folded.count() as u64)
        .expect("a start");
    assert_eq!(n, 1);
    assert!(
        brief.contains("- 1 self-improvement builds started since the last note"),
        "{brief}"
    );
    assert_eq!(
        folded.lines(),
        [format!(
            "- started work on: {} (item {})",
            title_of(&r, &t.started[0]),
            t.started[0]
        )]
    );
    r.core
        .folded_add(&[("tsk_late".into(), "A late start".into())]);
    r.core.folded_told(&folded);
    let left = r.core.folded_read();
    assert_eq!(left.started.len(), 1, "{left:?}");
    assert_eq!(left.started[0].item, "tsk_late");
}

/// The note reads the folded record once: the count it speaks is the count it
/// lists and takes off, so a start that lands after the read is in none of
/// the three and waits for the next note (folded.rs's own promise).
#[tokio::test]
async fn the_morning_facts_count_what_the_one_read_holds_not_a_second_read() {
    let r = rig_with(1, 30.0);
    bind_dm(&r);
    r.tick().await;
    let folded = r.core.folded_read();
    assert_eq!(folded.count(), 1, "{folded:?}");
    r.core
        .folded_add(&[("tsk_late".into(), "A late start".into())]);
    let (brief, n) = r
        .core
        .morning_facts(night_now(), None, 0, folded.count() as u64)
        .expect("a start");
    assert_eq!(n, 1);
    assert!(
        brief.contains("- 1 self-improvement builds started since the last note"),
        "{brief}"
    );
    assert_eq!(r.core.folded_read().count(), 2, "the late start is waiting");
}

/// A note takes off exactly what it counted, by identity: a start that lands
/// between its read and its post (even with the list full) stays alone, and a
/// start added again counts once, in the list or past it.
#[tokio::test]
async fn a_note_takes_off_what_it_counted_and_a_re_added_start_counts_once() {
    let r = rig_with(1, 30.0);
    bind_dm(&r);
    let add = |i: &str| {
        r.core
            .folded_add(&[(format!("tsk_{i}"), format!("Fix cause {i}"))]);
    };
    for i in 0..10 {
        add(&format!("{i:02}"));
    }
    let snapshot = r.core.folded_read();
    add("late");
    r.core.folded_told(&snapshot);
    let left = r.core.folded_read();
    assert_eq!(left.count(), 1, "{left:?}");
    assert_eq!(left.started[0].item, "tsk_late", "{left:?}");
    r.core.folded_told(&left);
    assert!(r.core.folded_read().is_empty());
    // The list is past its cap when the note reads: its "and 2 more" names
    // two starts, and a start that lands after must not take their place.
    for i in 0..12 {
        add(&format!("{i:02}"));
    }
    let snapshot = r.core.folded_read();
    assert_eq!((snapshot.started.len(), snapshot.more), (10, 2));
    add("late");
    r.core.folded_told(&snapshot);
    let left = r.core.folded_read();
    assert_eq!(left.count(), 1, "{left:?}");
    assert_eq!(left.started[0].item, "tsk_late", "{left:?}");
    r.core.folded_told(&left);
    for i in 0..11 {
        add(&format!("{i:02}"));
    }
    add("00");
    let again = r.core.folded_read();
    assert_eq!(again.count(), 11, "{again:?}");
}

/// A post that failed leaves every start where it was: the night's note does
/// not reach the store, so the record still names the item, and the next
/// night's note lists it once and leaves the record empty. (The morning note's
/// twin, `morning.rs`, runs only from a spawned task, so it is left to its own
/// tests.)
#[tokio::test]
async fn a_post_that_failed_leaves_every_start_where_it_was() {
    let _clock = crate::test_support::pin_mid_period();
    let r = rig_with(1, 30.0);
    bind_dm(&r);
    let t = r.tick().await;
    assert_eq!(t.started.len(), 1, "{t:?}");
    r.core
        .store
        .fail_plain_frame(|rs| rs.iter().any(|x| contains(&x.payload, b"self_digest")));
    let first = night_now();
    r.core.backlog_night(first).unwrap();
    assert!(lines(&r).is_empty(), "the post failed: {:?}", lines(&r));
    let waiting = r.core.folded_read();
    assert_eq!(waiting.count(), 1, "{waiting:?}");
    assert_eq!(waiting.started[0].item, t.started[0], "{waiting:?}");
    r.core.backlog_night(first + 24 * 3_600_000).unwrap();
    let said = lines(&r);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].contains("1 started"), "{}", said[0]);
    assert!(
        r.core.folded_read().is_empty(),
        "told once: {:?}",
        r.core.folded_read()
    );
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// Starts past what a note lists are counted, not dropped silently: twelve
/// are ten lines and "and 2 more started", and the title counts all twelve.
#[tokio::test]
async fn starts_past_the_cap_are_counted_in_the_note() {
    let r = rig_with(1, 30.0);
    bind_dm(&r);
    for i in 0..12 {
        r.core
            .folded_add(&[(format!("tsk_{i:02}"), format!("Fix cause {i:02}"))]);
    }
    let folded = r.core.folded_read();
    assert_eq!((folded.started.len(), folded.more), (10, 2), "{folded:?}");
    r.core.backlog_night(night_now()).unwrap();
    let said = lines(&r);
    assert_eq!(said.len(), 1, "{said:?}");
    assert_eq!(
        said[0].matches("- started work on: ").count(),
        10,
        "{}",
        said[0]
    );
    assert!(said[0].contains("- and 2 more started"), "{}", said[0]);
    assert!(said[0].contains("12 started"), "{}", said[0]);
    assert!(r.core.folded_read().is_empty(), "all told");
}

/// A note that waits for the owner's word is never folded: the night's keel
/// card posts at once as an action, and carries the waiting starts with it.
#[tokio::test]
async fn a_note_that_waits_for_the_owner_posts_at_once_with_the_waiting_starts() {
    let r = rig_with(1, 30.0);
    bind_dm(&r);
    let t = r.tick().await;
    assert!(lines(&r).is_empty(), "{:?}", lines(&r));
    let t0 = theseus_protocol::now_unix_ms();
    for i in 0..2u64 {
        let mut row = LedgerRow::named(
            "approval.refused",
            None,
            None,
            json!({"act": "self.resume", "who": "stranger", "why": "not the owner"}),
        );
        row.at_unix_ms = t0 + (1 + i) * 60_000;
        r.core.store.append_ledger(&row).unwrap();
    }
    r.core.backlog_night(t0 + 4 * 3_600_000).unwrap();
    let said = lines(&r);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].contains("waiting for your word"), "{}", said[0]);
    assert!(said[0].contains(&t.started[0]), "{}", said[0]);
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
