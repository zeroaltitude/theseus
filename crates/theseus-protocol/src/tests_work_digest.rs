use super::*;
use crate::work::WorkKind;

fn task(id: &str, title: &str) -> WorkRef {
    WorkRef {
        id: id.into(),
        kind: WorkKind::Task,
        session_id: None,
        title: title.into(),
    }
}

fn root(id: &str, title: &str) -> WorkRef {
    WorkRef {
        kind: WorkKind::Conversation,
        ..task(id, title)
    }
}

fn hm(ms: u64) -> String {
    format!("{:02}:{:02}", ms / 3_600_000 % 24, ms / 60_000 % 60)
}

fn entry(kind: DigestKind, at_ms: u64, id: &str, under: &WorkRef) -> (DigestLine, WorkRef) {
    (digest_line(kind, at_ms, task(id, id)), under.clone())
}

#[test]
fn a_line_is_worded_once() {
    let l = digest_line(DigestKind::Reported, 5, task("tsk_a", "Beta summary"));
    assert_eq!(l.words, "📋 Beta summary reported");
    let l = digest_line(DigestKind::Failed, 5, task("tsk_a", "Beta summary"));
    assert_eq!(l.words, "✗ Beta summary failed");
    let l = digest_line(DigestKind::Answered, 5, task("act_1", "fs.write notes.txt"));
    assert_eq!(l.words, "✓ you answered: fs.write notes.txt");
    // A work with no title is named by its kind and the end of its id.
    let l = digest_line(DigestKind::Cancelled, 5, task("tsk_abcdef123", ""));
    assert_eq!(l.words, "⊘ task def123 was cancelled");
}

#[test]
fn a_burst_of_three_folds_into_one_line_under_its_root() {
    let harbour = root("exe_h", "harbour survey");
    let lines = fold(vec![
        entry(DigestKind::Reported, 1_000, "a", &harbour),
        entry(DigestKind::Reported, 2_000, "b", &harbour),
        entry(DigestKind::Reported, 3_000, "c", &harbour),
    ]);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].words, "📋 3 tasks reported to harbour survey");
    assert_eq!(lines[0].count, 3);
    assert_eq!(lines[0].at_ms, 3_000);
    assert_eq!(lines[0].work.id, "exe_h");
}

#[test]
fn two_do_not_fold_and_neither_do_other_kinds_or_roots_or_a_long_gap() {
    let h = root("exe_h", "harbour survey");
    let k = root("exe_k", "kelp");
    let two = fold(vec![
        entry(DigestKind::Reported, 1, "a", &h),
        entry(DigestKind::Reported, 2, "b", &h),
    ]);
    assert_eq!(two.len(), 2);
    let mixed = fold(vec![
        entry(DigestKind::Reported, 1, "a", &h),
        entry(DigestKind::Failed, 2, "b", &h),
        entry(DigestKind::Reported, 3, "c", &k),
        entry(DigestKind::Reported, 4, "d", &h),
    ]);
    assert_eq!(mixed.len(), 4, "{mixed:?}");
    let far = BURST_GAP_MS + 1;
    let split = fold(vec![
        entry(DigestKind::Reported, 1, "a", &h),
        entry(DigestKind::Reported, 2, "b", &h),
        entry(DigestKind::Reported, 2 + far, "c", &h),
    ]);
    assert_eq!(split.len(), 3, "a gap ends a burst");
}

#[test]
fn a_burst_keeps_the_order_of_the_other_lines() {
    let h = root("exe_h", "harbour survey");
    let lines = fold(vec![
        entry(DigestKind::Reported, 10, "a", &h),
        entry(DigestKind::Failed, 20, "x", &h),
        entry(DigestKind::Reported, 30, "b", &h),
        entry(DigestKind::Reported, 40, "c", &h),
    ]);
    let order: Vec<(DigestKind, u32)> = lines.iter().map(|l| (l.kind, l.count)).collect();
    assert_eq!(
        order,
        vec![(DigestKind::Failed, 1), (DigestKind::Reported, 3)]
    );
}

#[test]
fn the_summary_is_one_line_or_none() {
    let h = root("exe_h", "harbour survey");
    assert_eq!(summary(&[], 0, &hm), None);
    let lines = vec![
        digest_line(DigestKind::Reported, 1, task("a", "a")),
        digest_line(DigestKind::Reported, 2, task("b", "b")),
        digest_line(DigestKind::Answered, 3, task("q", "q")),
    ];
    let since = 18 * 3_600_000 + 14 * 60_000;
    assert_eq!(
        summary(&lines, since, &hm).as_deref(),
        Some("◆ 2 reported, 1 answered by you since 18:14")
    );
    // A folded line counts what it stands for.
    let folded = fold(vec![
        entry(DigestKind::Failed, 1, "a", &h),
        entry(DigestKind::Failed, 2, "b", &h),
        entry(DigestKind::Failed, 3, "c", &h),
    ]);
    assert_eq!(summary(&folded, 0, &hm).as_deref(), Some("◆ 3 failed"));
}
