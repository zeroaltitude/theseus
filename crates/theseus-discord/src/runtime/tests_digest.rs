use super::*;
use theseus_protocol::work::{Rollup, WorkKind, WorkRef};
use theseus_protocol::{digest_line, DigestKind};

fn hm(ms: u64) -> String {
    format!("{:02}:{:02}", ms / 3_600_000 % 24, ms / 60_000 % 60)
}

fn at(h: u64, m: u64) -> u64 {
    h * 3_600_000 + m * 60_000
}

fn work(id: &str, kind: WorkKind, title: &str) -> WorkRef {
    WorkRef {
        id: id.into(),
        kind,
        session_id: None,
        title: title.into(),
    }
}

fn result(lines: Vec<theseus_protocol::DigestLine>) -> WorkDigestResult {
    WorkDigestResult {
        from: 1,
        to: 9,
        lines,
        from_ledger: false,
        now: Rollup::default(),
        questions: vec![],
        more: None,
    }
}

#[test]
fn the_digest_is_a_summary_and_a_line_each() {
    let res = result(vec![
        digest_line(
            DigestKind::Reported,
            at(18, 20),
            work("tsk_a", WorkKind::Task, "Beta summary"),
        ),
        digest_line(
            DigestKind::Answered,
            at(18, 40),
            work("act_1", WorkKind::Question, "fs.write notes.txt"),
        ),
    ]);
    assert_eq!(
        text(&res, at(18, 14), &hm),
        "◆ 1 reported, 1 answered by you since 18:14\n\
         • 18:20 📋 Beta summary reported\n\
         • 18:40 ✓ you answered: fs.write notes.txt"
    );
}

#[test]
fn an_empty_digest_says_nothing_happened() {
    assert_eq!(
        text(&result(vec![]), at(9, 0), &hm),
        "◆ nothing since 09:00"
    );
}

#[test]
fn a_long_digest_keeps_the_newest_lines_within_discords_limit_and_counts_the_rest() {
    let lines: Vec<_> = (0..200)
        .map(|n| {
            digest_line(
                DigestKind::Failed,
                at(10, 0) + n * 1000,
                work(
                    &format!("tsk_{n:03}"),
                    WorkKind::Task,
                    &format!("a task with a longish title number {n}"),
                ),
            )
        })
        .collect();
    let out = text(&result(lines), 0, &hm);
    assert!(out.len() <= LIMIT, "{} bytes", out.len());
    assert!(out.contains("… and "), "{out}");
    assert!(
        out.ends_with("number 199 failed"),
        "the newest stays: {out}"
    );
}

#[test]
fn a_dm_is_welcomed_back_after_four_quiet_hours_and_only_then() {
    let now = 100 * 3_600_000;
    assert!(!welcome_due(0, now), "a place not seen in this process");
    assert!(!welcome_due(now - WELCOME_AFTER_MS + 1, now));
    assert!(welcome_due(now - WELCOME_AFTER_MS, now));
}
