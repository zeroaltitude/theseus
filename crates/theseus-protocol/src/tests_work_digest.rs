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
fn a_burst_of_three_failures_folds_into_one_line_under_its_root() {
    let harbour = root("exe_h", "harbour survey");
    let lines = fold_digest(vec![
        entry(DigestKind::Failed, 1_000, "a", &harbour),
        entry(DigestKind::Failed, 2_000, "b", &harbour),
        entry(DigestKind::Failed, 3_000, "c", &harbour),
    ]);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].words, "✗ 3 tasks failed under harbour survey");
    assert_eq!(lines[0].count, 3);
    assert_eq!(lines[0].at_ms, 3_000);
    assert_eq!(lines[0].work.id, "exe_h");
}

fn reported(
    at_ms: u64,
    id: &str,
    title: &str,
    report: Option<&str>,
    under: &WorkRef,
) -> (DigestLine, WorkRef) {
    (
        digest_line_with(
            DigestKind::Reported,
            at_ms,
            task(id, title),
            report.map(String::from),
            None,
        ),
        under.clone(),
    )
}

/// Before (theseus-ypnxt.1): three reported tasks under one root folded to
/// `📋 3 tasks reported to harbour survey`, no title and no report line. Now each is told.
#[test]
fn three_reported_tasks_are_three_titled_lines_with_their_reports() {
    let harbour = root("exe_h", "harbour survey");
    let lines = fold_digest(vec![
        reported(
            1_000,
            "a",
            "Chart the buoys",
            Some("Charted 41 buoys."),
            &harbour,
        ),
        reported(
            2_000,
            "b",
            "Sound the channel",
            Some("9 m at low tide."),
            &harbour,
        ),
        reported(3_000, "c", "Paint the quay", None, &harbour),
    ]);
    let words: Vec<&str> = lines.iter().map(|l| l.words.as_str()).collect();
    assert_eq!(
        words,
        [
            "📋 Chart the buoys reported: Charted 41 buoys.",
            "📋 Sound the channel reported: 9 m at low tide.",
            "📋 Paint the quay reported",
        ]
    );
    assert!(lines.iter().all(|l| l.count == 1));
}

#[test]
fn past_five_the_rest_are_named_together() {
    let harbour = root("exe_h", "harbour survey");
    let seven: Vec<_> = (1..=7u64)
        .map(|i| {
            reported(
                i * 1_000,
                &format!("t{i}"),
                &format!("Task {i}"),
                Some("ok"),
                &harbour,
            )
        })
        .collect();
    let lines = fold_digest(seven);
    assert_eq!(lines.len(), 6, "{lines:?}");
    assert_eq!(lines[4].words, "📋 Task 5 reported: ok");
    assert_eq!(lines[5].words, "📋 and 2 more: Task 6, Task 7");
    assert_eq!(lines[5].count, 2);
    assert_eq!(lines[5].at_ms, 7_000);
    // The summary counts every task, named or not.
    assert_eq!(
        digest_summary(&lines, 0, &hm).as_deref(),
        Some("◆ 7 reported")
    );
    // A long rest lists its first names and says it goes on.
    let many: Vec<_> = (1..=30u64)
        .map(|i| {
            reported(
                i * 1_000,
                &format!("t{i}"),
                &format!("T{i}"),
                None,
                &harbour,
            )
        })
        .collect();
    let rest = fold_digest(many).pop().unwrap();
    assert!(
        rest.words.starts_with("📋 and 25 more: T6, T7,"),
        "{}",
        rest.words
    );
    assert!(rest.words.ends_with("T17, …"), "{}", rest.words);
}

#[test]
fn the_ledger_note_says_where_the_read_stopped() {
    let mut res = WorkDigestResult {
        from: 0,
        to: 9,
        lines: vec![],
        from_ledger: true,
        now: Default::default(),
        questions: vec![],
        more: None,
        google: None,
    };
    assert_eq!(digest_more(&res, &hm), None);
    res.more = Some(DigestMore {
        since_ms: 6 * 3_600_000 + 12 * 60_000,
        command: "theseus tasks --ended --since 14h".into(),
    });
    assert_eq!(
        digest_more(&res, &hm).as_deref(),
        Some("… and more since 06:12; theseus tasks --ended --since 14h")
    );
    assert!(digest_text(&res, 0, &hm, 2000).contains("… and more since 06:12;"));
}

#[test]
fn two_do_not_fold_and_neither_do_other_kinds_or_roots_or_a_long_gap() {
    let h = root("exe_h", "harbour survey");
    let k = root("exe_k", "kelp");
    let two = fold_digest(vec![
        entry(DigestKind::Reported, 1, "a", &h),
        entry(DigestKind::Reported, 2, "b", &h),
    ]);
    assert_eq!(two.len(), 2);
    let mixed = fold_digest(vec![
        entry(DigestKind::Reported, 1, "a", &h),
        entry(DigestKind::Failed, 2, "b", &h),
        entry(DigestKind::Reported, 3, "c", &k),
        entry(DigestKind::Reported, 4, "d", &h),
    ]);
    assert_eq!(mixed.len(), 4, "{mixed:?}");
    let far = BURST_GAP_MS + 1;
    let split = fold_digest(vec![
        entry(DigestKind::Reported, 1, "a", &h),
        entry(DigestKind::Reported, 2, "b", &h),
        entry(DigestKind::Reported, 2 + far, "c", &h),
    ]);
    assert_eq!(split.len(), 3, "a gap ends a burst");
}

#[test]
fn a_burst_keeps_the_order_of_the_other_lines() {
    let h = root("exe_h", "harbour survey");
    let lines = fold_digest(vec![
        entry(DigestKind::Cancelled, 10, "a", &h),
        entry(DigestKind::Failed, 20, "x", &h),
        entry(DigestKind::Cancelled, 30, "b", &h),
        entry(DigestKind::Cancelled, 40, "c", &h),
    ]);
    let order: Vec<(DigestKind, u32)> = lines.iter().map(|l| (l.kind, l.count)).collect();
    assert_eq!(
        order,
        vec![(DigestKind::Failed, 1), (DigestKind::Cancelled, 3)]
    );
}

#[test]
fn the_summary_is_one_line_or_none() {
    let h = root("exe_h", "harbour survey");
    assert_eq!(digest_summary(&[], 0, &hm), None);
    let lines = vec![
        digest_line(DigestKind::Reported, 1, task("a", "a")),
        digest_line(DigestKind::Reported, 2, task("b", "b")),
        digest_line(DigestKind::Answered, 3, task("q", "q")),
    ];
    let since = 18 * 3_600_000 + 14 * 60_000;
    assert_eq!(
        digest_summary(&lines, since, &hm).as_deref(),
        Some("◆ 2 reported, 1 answered by you since 18:14")
    );
    // A folded line counts what it stands for.
    let folded = fold_digest(vec![
        entry(DigestKind::Failed, 1, "a", &h),
        entry(DigestKind::Failed, 2, "b", &h),
        entry(DigestKind::Failed, 3, "c", &h),
    ]);
    assert_eq!(
        digest_summary(&folded, 0, &hm).as_deref(),
        Some("◆ 3 failed")
    );
}

/// An agent's answer is told as the agent's (theseus-5rvss): "you answered"
/// only for the owner's own, and a line with no `by`, as every line written
/// before it, is the owner's.
#[test]
fn an_agents_answer_says_so_and_the_owners_and_an_old_lines_say_you() {
    let q = || task("act_1", "aws.stack.apply");
    let by = |a: &str| Some(a.to_string());
    let l = digest_line_with(DigestKind::Answered, 5, q(), None, by("no terminal"));
    assert_eq!(
        l.words,
        "✓ answered by an agent (no terminal): aws.stack.apply"
    );
    let l = digest_line_with(DigestKind::Answered, 5, q(), None, by("claude"));
    assert_eq!(l.words, "✓ answered by an agent (claude): aws.stack.apply");
    let l = digest_line_with(DigestKind::Answered, 5, q(), None, None);
    assert_eq!(l.words, "✓ you answered: aws.stack.apply");
    // A line from before the field has none, and reads as the owner's.
    let old = r#"{"at_ms":5,"kind":"answered","work":{"id":"act_1","kind":"task","title":"aws.stack.apply"},"words":"✓ you answered: aws.stack.apply"}"#;
    let l: DigestLine = serde_json::from_str(old).unwrap();
    assert_eq!(l.by, None);
    assert!(!serde_json::to_string(&l).unwrap().contains("\"by\""));
    // The `by` a row carries names the agent, and only an agent.
    assert_eq!(agent_by("claude"), "agent:claude");
    assert_eq!(agent_of("agent:no terminal"), Some("no terminal"));
    assert_eq!(agent_of("the CLI"), None);
    assert_eq!(by_words("agent:claude"), "an agent (claude)");
    assert_eq!(by_words("the CLI"), "the CLI");
}

/// The owner's answers and an agent's are never folded into one line, and the
/// summary counts them apart.
#[test]
fn an_agents_answers_fold_and_count_apart_from_the_owners() {
    let h = root("exe_h", "harbour survey");
    let line = |at: u64, id: &str, by: Option<&str>| {
        (
            digest_line_with(
                DigestKind::Answered,
                at,
                task(id, id),
                None,
                by.map(String::from),
            ),
            h.clone(),
        )
    };
    let lines = fold_digest(vec![
        line(1_000, "a", Some("claude")),
        line(2_000, "b", Some("claude")),
        line(3_000, "c", Some("claude")),
        line(4_000, "d", None),
    ]);
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(
        lines[0].words,
        "✓ an agent (claude) answered 3 questions under harbour survey"
    );
    assert_eq!(lines[1].words, "✓ you answered: d");
    assert_eq!(
        digest_summary(&lines, 0, &hm).as_deref(),
        Some("◆ 1 answered by you, 3 answered by an agent")
    );
}
