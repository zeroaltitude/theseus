//! `theseus thread import`'s words (theseus-71ib): the plan file a dry run
//! writes and the ticks read back, the table's numbers, and the apply's
//! lines. The whole command against a daemon is theseusd's
//! `tests/thread_import.rs`.

use std::collections::BTreeMap;

use theseus_protocol::threads::WorkingSet;
use theseus_protocol::threads_import::{
    ThreadApplied, ThreadCandidate, ThreadImportResult, ThreadSkipped,
};

use super::*;

fn cand(n: u32, slug: &str, title: &str) -> ThreadCandidate {
    ThreadCandidate {
        n,
        slug: slug.into(),
        title: title.into(),
        kind: "channel".into(),
        persona: (n == 1).then(|| "wren".to_string()),
        topics: vec!["projects/tides".into()],
        working_set: WorkingSet {
            repos: vec!["reef-lab/gauge-log".into()],
            refs: vec!["#12".into()],
            ..Default::default()
        },
        owner_messages: 5 * n,
        episodes: 2,
        last_active_ms: 1_791_633_600_000,
        sensitivity: BTreeMap::from([
            ("personal".to_string(), 3),
            ("partner-confidential".to_string(), 2),
        ]),
        exists: n == 3,
        channel: None,
        notes: vec![],
    }
}

fn result() -> ThreadImportResult {
    ThreadImportResult {
        from: "openclaw".into(),
        since_days: 30,
        min_cluster: 8,
        candidates: vec![
            cand(1, "tide-gauges", "Tide gauges"),
            cand(2, "boat-ops", "Boat ops"),
            cand(3, "harbor", "Harbor"),
        ],
        applied: vec![],
        skipped: vec![],
        spent_usd: 0.0,
        spent_today_usd: 0.0,
        cap_usd: 2.0,
        notes: vec!["2 streams with no owner message in the window".into()],
    }
}

#[test]
fn the_plan_file_ticks_back_into_slugs() {
    let text = plan_file(&result());
    assert!(text.starts_with("# theseus thread import plan"), "{text}");
    assert_eq!(text.lines().filter(|l| l.starts_with("[ ] ")).count(), 3);
    assert!(
        text.contains(
            "[ ] 3 harbor Harbor  (channel, 15 owner messages)  (exists: will be skipped)"
        ),
        "{text}"
    );
    assert!(ticked(&text).is_empty(), "nothing ticked yet");
    let edited = text
        .replace("[ ] 2 boat-ops", "[x] 2 boat-ops")
        .replace("[ ] 1 tide-gauges", "[X] 1 tide-gauges");
    assert_eq!(ticked(&edited), ["tide-gauges", "boat-ops"]);
    // A comment that mentions [x], a line without a number, and a repeat are no ticks.
    let noisy = "# put [x] here\n[x] tide-gauges\n[x] 4 reef\n[x] 4 reef\n[ ] 5 other\n";
    assert_eq!(ticked(noisy), ["reef"]);
}

#[test]
fn table_numbers_name_the_candidates() {
    assert_eq!(numbers("1,3, 5,3").unwrap(), [1, 3, 5]);
    assert!(numbers("").is_err() && numbers("a").is_err() && numbers("1,x").is_err());
    let r = result();
    assert_eq!(
        slugs_of(&r.candidates, &[3, 1]).unwrap(),
        ["harbor", "tide-gauges"]
    );
    let e = slugs_of(&r.candidates, &[9]).unwrap_err().to_string();
    assert!(e.contains("no candidate 9") && e.contains("has 3"), "{e}");
}

#[test]
fn the_table_has_a_row_a_candidate_and_what_was_left_out() {
    let lines = table_lines(&result());
    let text = lines.join("\n");
    assert!(
        lines[0].contains("slug") && lines[0].contains("persona") && lines[0].contains("msgs"),
        "{text}"
    );
    assert!(lines[1].starts_with("  1  tide-gauges"), "{text}");
    assert!(
        lines[1].contains("wren") && lines[1].contains("projects/tides"),
        "{text}"
    );
    assert!(
        text.contains("working set: repos reef-lab/gauge-log; refs #12"),
        "{text}"
    );
    assert!(
        text.contains("sensitivity: partner-confidential 2, personal 3 (2 episodes)"),
        "{text}"
    );
    assert!(text.contains("exists"), "{text}");
    assert!(
        text.ends_with("left out: 2 streams with no owner message in the window"),
        "{text}"
    );
    let mut empty = result();
    empty.candidates.clear();
    assert!(table_lines(&empty)[0].starts_with("No candidates from openclaw in the last 30 days."));
}

#[test]
fn the_apply_says_what_was_made_skipped_and_spent() {
    let mut r = result();
    r.applied = vec![
        ThreadApplied {
            slug: "tide-gauges".into(),
            card: vec!["Gauge 4 drifts. [m:ab12cd]".into()],
            cited: vec!["m:ab12cd".into()],
            cost_usd: 0.0004,
            card_failed: None,
        },
        ThreadApplied {
            slug: "boat-ops".into(),
            card: vec![],
            cited: vec![],
            cost_usd: 0.0,
            card_failed: Some("the day's import budget ($2.00) leaves $0.00".into()),
        },
    ];
    r.skipped = vec![ThreadSkipped {
        slug: "harbor".into(),
        why: "a thread of this slug exists".into(),
    }];
    r.spent_usd = 0.0004;
    r.spent_today_usd = 0.0004;
    let text = applied_lines(&r).join("\n");
    assert!(
        text.contains("Made the thread tide-gauges: 1 card lines citing 1 episodes ($0.0004)."),
        "{text}"
    );
    assert!(text.contains("  Gauge 4 drifts. [m:ab12cd]"), "{text}");
    assert!(text.contains("no card: the day's import budget"), "{text}");
    assert!(
        text.contains("Skipped harbor: a thread of this slug exists"),
        "{text}"
    );
    assert!(
        text.ends_with("2 made, 1 skipped; cards cost $0.0004 ($0.0004 of $2.00 today)."),
        "{text}"
    );
}
