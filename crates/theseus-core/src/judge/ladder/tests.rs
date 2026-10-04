//! The ladder's pure parts: a version's rows folded into its standing, a
//! brake lapsing, and the arms.

use theseus_protocol::packs::PackModeRow;

use super::*;

fn row(mode: &str, at: u64, position: u64) -> PackModeRow {
    PackModeRow {
        pack: "loop.v1".into(),
        mode: mode.into(),
        who: "owner".into(),
        why: "because".into(),
        at_unix_ms: at,
        position,
        ..PackModeRow::default()
    }
}

/// No row: the wired line. The latest row wins, a declined one changes
/// nothing, and a canary keeps its share.
#[test]
fn the_latest_row_is_the_mode_and_a_declined_one_is_none() {
    let s = fold(PackMode::Shadow, &[], 10);
    assert_eq!((s.rung, s.why.as_str()), (Rung::Shadow, WIRED_WHY));
    assert_eq!(s.words(), "shadow");
    let mut canary = row("canary", 1, 5);
    canary.share = Some(0.2);
    let mut declined = row("live", 2, 6);
    declined.declined = true;
    let s = fold(PackMode::Shadow, &[canary, declined], 10);
    assert_eq!((s.rung, s.share, s.position), (Rung::Canary, Some(0.2), 5));
    assert_eq!(s.words(), "canary 0.2 (owner: because)");
    let s = fold(
        PackMode::Shadow,
        &[row("live", 1, 5), row("rolled_back", 2, 6)],
        10,
    );
    assert_eq!((s.rung, s.rolled_back_at), (Rung::RolledBack, Some(2)));
    assert_eq!(
        s.rung.acts_as(),
        PackMode::Shadow,
        "rolled back acts as shadow"
    );
}

/// A brake (a rollback with `until`) lapses at its midnight to what it
/// stood on; a rollback without one stands until a promotion.
#[test]
fn a_brake_lapses_at_its_until_and_a_rollback_stands() {
    let mut brake = row("rolled_back", 20, 6);
    brake.until_ms = Some(100);
    brake.rule = Some("notices_per_day".into());
    let rows = [row("live", 10, 5), brake];
    let s = fold(PackMode::Shadow, &rows, 99);
    assert_eq!(s.rung, Rung::RolledBack);
    assert!(s.words().starts_with("rolled back until "), "{}", s.words());
    assert!(s.words().ends_with("(notices_per_day)"), "{}", s.words());
    let s = fold(PackMode::Shadow, &rows, 100);
    assert_eq!((s.rung, s.position), (Rung::Live, 5), "lapsed at midnight");
    assert_eq!(s.rolled_back_at, Some(20), "its rollback is still known");
    let rows = [row("live", 10, 5), row("rolled_back", 20, 6)];
    assert_eq!(
        fold(PackMode::Shadow, &rows, u64::MAX).rung,
        Rung::RolledBack
    );
}

/// A canary's sessions are its arm's: sticky, and monotone as the share
/// grows (every canary session stays one); the control judges in shadow.
#[test]
fn arms_are_sticky_and_monotone() {
    let sessions: Vec<String> = (0..500).map(|i| format!("ses_{i:04}")).collect();
    let canary = |share: f64| -> Vec<&String> {
        sessions
            .iter()
            .filter(|s| given_of(PackMode::Canary, Some(share), "loop.v1", s).arm == ArmOf::Canary)
            .collect()
    };
    let (small, large) = (canary(0.2), canary(0.6));
    assert!((60..140).contains(&small.len()), "{}", small.len());
    assert!(small.iter().all(|s| large.contains(s)), "monotone");
    assert_eq!(canary(0.2), small, "sticky");
    let g = given_of(PackMode::Canary, Some(0.2), "loop.v1", small[0]);
    assert_eq!(
        (g.mode, g.judge_mode()),
        (PackMode::Live, theseus_judge::Mode::Canary)
    );
    let control = sessions.iter().find(|s| !small.contains(s)).unwrap();
    let g = given_of(PackMode::Canary, Some(0.2), "loop.v1", control);
    assert_eq!(
        (g.mode, g.arm, g.judge_mode()),
        (
            PackMode::Shadow,
            ArmOf::Control,
            theseus_judge::Mode::Shadow
        )
    );
    assert_eq!(canary(1.0).len(), sessions.len());
}

/// The next local midnight is after now, at most a day and an hour away.
#[test]
fn the_next_midnight_is_tomorrows() {
    let now = 1_790_000_000_000;
    let m = rules::next_midnight(now);
    assert!(m > now && m - now <= 25 * 3_600_000, "{}", m - now);
    assert_eq!(crate::learning::local_midnight(m), m);
}
