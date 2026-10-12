//! The number and the grants' storage (theseus-uttu): interrupts per active
//! hour over four weeks, what the grants would have saved, and a grant that
//! survives a store's reopen with its ledger row in the same frame.

use serde_json::json;
use theseus_protocol::earned::{EarnedGrant, EarnedGroup};
use theseus_protocol::LedgerKind;

use super::measure::{measure, WEEK_MS};
use super::{covers, Earned};
use crate::ledger::LedgerRow;
use crate::store::Store;

const NOW: u64 = 40 * 86_400_000;
const HOUR: u64 = 3_600_000;
const POSTURE: &str = "x: proc.run — approve (enforcement = approve)";

fn ask(corr: &str, argv: &[&str], at: u64) -> LedgerRow {
    let mut r = LedgerRow::named(
        "tool.confirm_requested",
        None,
        None,
        json!({"correlation_id": corr, "session_id": "s", "tool": "proc.run",
               "input": {"argv": argv}, "reason": POSTURE, "floor": false}),
    );
    r.at_unix_ms = at;
    r
}

fn grant(prefix: &[&str]) -> EarnedGrant {
    EarnedGrant {
        group: EarnedGroup {
            id: "ern_00000009".into(),
            tool: "proc.run".into(),
            prefix: prefix.iter().map(|s| s.to_string()).collect(),
            label: prefix.join(" "),
            ..Default::default()
        },
        by: "the CLI".into(),
        ..Default::default()
    }
}

#[test]
fn the_number_is_questions_per_active_hour_and_the_grants_save_what_they_cover() {
    let mut rows = Vec::new();
    // Week 4 (the newest): 6 questions in 3 hours, 4 of them `cargo fmt`.
    let base = NOW - 2 * 86_400_000;
    for i in 0..6u64 {
        let argv: &[&str] = if i < 4 {
            &["cargo", "fmt", "--all"]
        } else {
            &["ls", "-la"]
        };
        rows.push(ask(&format!("n{i}"), argv, base + (i / 2) * HOUR));
    }
    // Week 1 (the oldest): 2 questions, one hour; a budget question is no interrupt.
    rows.push(ask("o1", &["cargo", "fmt"], NOW - 4 * WEEK_MS + 1));
    rows.push(ask("o2", &["cargo", "fmt"], NOW - 4 * WEEK_MS + 2));
    let mut b = LedgerRow::named(
        "tool.confirm_requested",
        None,
        None,
        json!({"correlation_id": "bq", "tool": "budget.reset", "input": {}, "reason": "spend"}),
    );
    b.at_unix_ms = NOW - 1000;
    rows.push(b);
    // An hour with a turn and no question still counts as active.
    let turns = [base + 10 * HOUR, base + 10 * HOUR + 5];
    let m = measure(&rows, &turns, &[grant(&["cargo", "fmt"])], NOW);
    assert_eq!(m.weeks.len(), 4);
    assert_eq!((m.weeks[0].interrupts, m.weeks[0].active_hours), (2, 1));
    assert_eq!((m.weeks[3].interrupts, m.weeks[3].active_hours), (6, 4));
    assert_eq!((m.interrupts, m.active_hours), (8, 5));
    assert!((m.per_hour - 1.6).abs() < 1e-9, "{}", m.per_hour);
    assert_eq!(m.saved, 6, "the six `cargo fmt` questions");
    assert!(
        (m.per_hour_with_grants - 0.4).abs() < 1e-9,
        "{}",
        m.per_hour_with_grants
    );
    let none = measure(&rows, &turns, &[], NOW);
    assert_eq!((none.saved, none.per_hour_with_grants), (0, none.per_hour));
    assert_eq!(measure(&[], &[], &[], NOW).per_hour, 0.0);
}

#[test]
fn a_prefix_covers_what_follows_it_and_a_stored_shell_covers_nothing() {
    let g = grant(&["cargo", "nextest", "run"]);
    let w = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    assert!(covers(
        &g.group,
        "proc.run",
        &w(&["cargo", "nextest", "run", "-p", "x"]),
        &[]
    ));
    assert!(!covers(
        &g.group,
        "proc.run",
        &w(&["cargo", "nextest", "list"]),
        &[]
    ));
    assert!(!covers(
        &g.group,
        "proc.run",
        &w(&["cargo", "nextest"]),
        &[]
    ));
    assert!(!covers(
        &g.group,
        "fs.write",
        &w(&["cargo", "nextest", "run"]),
        &[]
    ));
    let curl = grant(&["curl", "-s"]);
    assert!(covers(
        &curl.group,
        "proc.run",
        &w(&["curl", "-s", "-o", "out.txt"]),
        &[]
    ));
    assert!(
        !covers(
            &curl.group,
            "proc.run",
            &w(&["curl", "-s", "https://example.org/x"]),
            &[]
        ),
        "a URL after the prefix is no grant's"
    );
    let sh = grant(&["bash", "-c"]);
    assert!(!covers(
        &sh.group,
        "proc.run",
        &w(&["bash", "-c", "echo hi"]),
        &[]
    ));
    let mut f = grant(&[]);
    f.group.tool = "fs.write".into();
    f.group.root = Some("/w".into());
    let p = |s: &str| vec![std::path::PathBuf::from(s)];
    assert!(covers(&f.group, "fs.write", &[], &p("/w/a.rs")));
    assert!(!covers(&f.group, "fs.write", &[], &p("/elsewhere/a.rs")));
    assert!(!covers(&f.group, "fs.write", &[], &[]));
}

#[test]
fn a_grant_is_stored_with_its_row_in_one_frame_and_read_back_after_a_reopen() {
    let d = tempfile::tempdir().unwrap();
    let store = Store::open(&d.path().join("store")).unwrap();
    let e = Earned::default();
    e.load(&store).unwrap();
    assert!(!e.any());
    let frames = store.stats().unwrap().frames_appended;
    let row = LedgerRow::new(
        LedgerKind::PolicyEarnedGranted,
        None,
        None,
        json!({"id": "ern_00000009"}),
    );
    let g = grant(&["cargo", "fmt"]);
    let put = g.clone();
    assert!(e
        .update(&store, &row, vec![], move |s| {
            s.grants.push(put);
            true
        })
        .unwrap());
    assert_eq!(
        store.stats().unwrap().frames_appended - frames,
        1,
        "the state and its row are one frame"
    );
    assert!(
        !e.update(&store, &row, vec![], |_| false).unwrap(),
        "no change writes nothing"
    );
    assert_eq!(store.stats().unwrap().frames_appended - frames, 1);
    drop(store);

    let store = Store::open(&d.path().join("store")).unwrap();
    let again = Earned::default();
    again.load(&store).unwrap();
    assert_eq!(again.grants(), vec![g]);
    let kinds: Vec<String> = store
        .ledger_tail::<LedgerRow>(5)
        .unwrap()
        .into_iter()
        .map(|(_, r)| r.kind)
        .collect();
    assert_eq!(kinds, ["ledger.chain_started", "policy.earned_granted"]);
    assert!(
        again.skips("ern_00000009", NOW, WEEK_MS),
        "a grant is not proposed again"
    );
}

#[test]
fn an_open_card_holds_its_candidates_for_a_week_then_lets_them_ride_the_next() {
    let d = tempfile::tempdir().unwrap();
    let store = Store::open(&d.path().join("store")).unwrap();
    let e = Earned::default();
    let row = LedgerRow::new(LedgerKind::PolicyEarnedProposed, None, None, json!({}));
    let card = theseus_protocol::earned::EarnedCard {
        proposed_ms: NOW,
        candidates: vec![grant(&["cargo", "fmt"]).group],
        answered: vec![],
    };
    e.update(&store, &row, vec![], move |s| {
        s.card = Some(card);
        true
    })
    .unwrap();
    assert!(e.skips("ern_00000009", NOW + 1, WEEK_MS), "open, and fresh");
    assert!(
        !e.skips("ern_00000009", NOW + WEEK_MS + 1, WEEK_MS),
        "a stale card holds nothing"
    );
    e.update(&store, &row, vec![], |s| {
        s.card
            .as_mut()
            .unwrap()
            .answered
            .push("ern_00000009".into());
        true
    })
    .unwrap();
    assert!(
        !e.skips("ern_00000009", NOW + 1, WEEK_MS),
        "an answered candidate is not open"
    );
}
