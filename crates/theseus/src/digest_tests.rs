use super::*;
use theseus_protocol::work::{AnswerOption, QuestionKind, Rollup, Style, WorkKind, WorkRef};
use theseus_protocol::{digest_line, DigestKind};

fn hm(ms: u64) -> String {
    format!("{:02}:{:02}", ms / 3_600_000 % 24, ms / 60_000 % 60)
}

fn work(id: &str, kind: WorkKind, title: &str) -> WorkRef {
    WorkRef {
        id: id.into(),
        kind,
        session_id: None,
        title: title.into(),
    }
}

fn at(h: u64, m: u64) -> u64 {
    h * 3_600_000 + m * 60_000
}

fn question() -> Question {
    Question {
        id: "act_0000a1b2c3".into(),
        kind: QuestionKind::Approval,
        asked_by: work("tsk_x", WorkKind::Task, "Kelp count"),
        prompt: "fs.write create notes.txt (40 B)".into(),
        detail: None,
        input: None,
        options: vec![AnswerOption {
            id: "approve".into(),
            label: "Approve".into(),
            style: Style::Primary,
            takes_note: false,
            effect: "runs it".into(),
        }],
        on_expiry: None,
        expires_at_ms: None,
        floor: false,
        asked_at_ms: 0,
        may_answer: true,
        tool: Some("fs.write".into()),
        choices: vec![],
    }
}

fn result() -> WorkDigestResult {
    WorkDigestResult {
        from: 100,
        to: 240,
        lines: vec![
            digest_line(
                DigestKind::Reported,
                at(18, 20),
                work("tsk_a", WorkKind::Task, "Beta summary"),
            ),
            digest_line(
                DigestKind::Failed,
                at(18, 31),
                work("tsk_b", WorkKind::Task, "Tide table"),
            ),
            digest_line(
                DigestKind::Answered,
                at(18, 40),
                work("act_1", WorkKind::Question, "fs.write notes.txt"),
            ),
        ],
        from_ledger: false,
        now: Rollup {
            needs_you: 1,
            working: 2,
            done: 4,
            failed: 1,
            total: 8,
        },
        questions: vec![question()],
        more: None,
        google: None,
    }
}

#[test]
fn the_digest_prints_its_summary_its_lines_the_board_and_the_questions() {
    let out = lines(&result(), at(18, 14), &hm).join("\n");
    assert_eq!(
        out,
        "◆ 1 reported, 1 failed, 1 answered by you since 18:14\n\
         \x20 18:20  📋 Beta summary reported\n\
         \x20 18:31  ✗ Tide table failed\n\
         \x20 18:40  ✓ you answered: fs.write notes.txt\n\
         now: 1 need you, 2 work, 1 failed of 8 on the board\n\
         ● fs.write create notes.txt (40 B)? · theseus answer a1b2c3"
    );
}

#[test]
fn an_empty_digest_says_so_and_a_ledger_read_says_where_it_came_from() {
    let mut r = result();
    r.lines.clear();
    r.questions.clear();
    r.now = Rollup::default();
    assert_eq!(lines(&r, at(9, 0), &hm), vec!["◆ nothing since 09:00"]);
    r.from_ledger = true;
    let out = lines(&r, 0, &hm);
    assert_eq!(out[0], "◆ nothing");
    assert!(out[1].contains("read from the ledger"), "{out:?}");
}

#[test]
fn the_status_line_is_the_summary_and_where_to_read_more_or_nothing() {
    assert_eq!(
        status_text(&result(), at(18, 14), &hm).as_deref(),
        Some("◆ 1 reported, 1 failed, 1 answered by you since 18:14 · theseus digest")
    );
    let mut r = result();
    r.lines.clear();
    assert_eq!(status_text(&r, at(18, 14), &hm), None);
}
