//! One task, opened from the board (theseus-2j2ka): an ephemeral Components V2
//! view of a task for the person who pressed, with what it was asked, what it
//! is doing, its last calls and its last words, and the buttons that act:
//! the question's answers, and `Stop`, which asks once more. Pure.

use serde_json::Value;
use theseus_protocol::usd;
use theseus_protocol::work::progress_line::aged_said;
use theseus_protocol::work::{AnswerOption, Question, WorkState, WorkView};

use super::board_v2::{ago, short};
use super::card_v2;
use super::clip;
use super::v2::{button, container, row, separator, text, Look};
use crate::runtime::emoji::state;
use crate::runtime::war_room::Comments;

/// `tb:stop:<id>`: Stop, which asks again.
pub const STOP: &str = "tb:stop:";
/// `tb:yes:<id>`: Stop, sure.
pub const STOP_YES: &str = "tb:yes:";
/// `tb:no:<id>`: keep it going.
pub const STOP_NO: &str = "tb:no:";
/// `tb:follow:<id>`: a silent message that mirrors the task.
pub const FOLLOW: &str = "tb:follow:";
/// `tb:retry:<id>` and `tb:dismiss:<id>`: a failed task's two verbs.
pub const RETRY: &str = "tb:retry:";
pub const DISMISS: &str = "tb:dismiss:";

/// A failed task's `[ ↻ Retry ] [ ✓ Dismiss ]`, when it is a failure the
/// owner can still act on (`theseus_protocol::retry::failure_options`).
pub fn failure_row(v: &WorkView, named: bool) -> Option<Value> {
    let tag = if named {
        format!(" …{}", short(&v.id))
    } else {
        String::new()
    };
    let options = theseus_protocol::failure_options(v);
    let has = |id: &str| options.iter().any(|o| o.id == id);
    let mut buttons = Vec::new();
    if has("retry") {
        buttons.push(button(
            format!("{RETRY}{}", v.id),
            &format!("↻ Retry{tag}"),
            Look::Primary,
        ));
    }
    if has("dismiss") {
        buttons.push(button(
            format!("{DISMISS}{}", v.id),
            &format!("✓ Dismiss{tag}"),
            Look::Secondary,
        ));
    }
    (!buttons.is_empty()).then(|| row(buttons))
}

fn state_words(v: &WorkView) -> String {
    if v.paused && !v.state.ended() {
        return "⏸ paused".to_string();
    }
    let (word, mark) = match v.state {
        WorkState::Queued => ("queued", Some("⏳".to_string())),
        WorkState::Running => ("running", Some(state(v.state))),
        WorkState::Waiting => ("waiting on its calls", Some(state(v.state))),
        WorkState::NeedsYou => ("needs you", Some(state(v.state))),
        WorkState::Ready => ("ready", Some(state(v.state))),
        WorkState::Done => ("done", Some(state(v.state))),
        WorkState::Failed => ("failed", Some(state(v.state))),
        WorkState::Cancelled => ("cancelled", Some(state(v.state))),
        WorkState::Expired => ("expired", Some(state(v.state))),
        WorkState::Unknown => ("working", None),
    };
    match mark {
        Some(m) => format!("{m} {word}"),
        None => word.to_string(),
    }
}

/// The view of task `v`: `calls` are the commands of its latest jobs (newest
/// first), `question` its open question with the options the card would draw.
pub fn detail(
    v: &WorkView,
    calls: &[String],
    question: Option<(&Question, &[AnswerOption])>,
    comments: &Comments,
    now_ms: u64,
) -> Vec<Value> {
    let mut inner = body(v, calls, now_ms);
    if let Some((q, options)) = question {
        inner.push(separator());
        inner.push(text(format!(
            "**Asks** · {}\nasked {}",
            clip(&q.prompt, 300),
            ago(q.asked_at_ms.max(v.state_since_ms))
        )));
        inner.extend(card_v2::buttons(&q.id, options));
    }
    if comments.total > 0 {
        inner.push(separator());
        inner.push(text(comments_words(comments)));
    }
    if !v.state.ended() && v.kind == theseus_protocol::work::WorkKind::Task {
        inner.push(separator());
        inner.push(row(vec![
            button(format!("{STOP}{}", v.id), "Stop", Look::Danger),
            button(format!("{FOLLOW}{}", v.id), "Follow", Look::Secondary),
        ]));
    }
    if let Some(r) = failure_row(v, false) {
        inner.push(separator());
        inner.push(r);
    }
    vec![container(Some(0x3E_63_DD), inner)]
}

/// A guest's comments, the latest five (theseus-bh3t.4).
fn comments_words(c: &Comments) -> String {
    let lines: Vec<String> = c
        .last
        .iter()
        .rev()
        .take(5)
        .map(|k| {
            let said = clip(&k.text.split_whitespace().collect::<Vec<_>>().join(" "), 80);
            format!("> **{}** · {}: {said}", clip(&k.name, 40), ago(k.at_ms))
        })
        .collect();
    format!("**Comments** ({})\n{}", c.total, lines.join("\n"))
}

/// The task's card for its forum post (theseus-i3do6): the view's words
/// without a button that acts (a post is no place that answers), the open
/// question as text, and a link to where the task can be answered.
pub fn post_card(v: &WorkView, calls: &[String], link: Option<&str>, now_ms: u64) -> Vec<Value> {
    let mut inner = body(v, calls, now_ms);
    if let Some(q) = v.question.as_ref().filter(|_| !v.state.ended()) {
        inner.push(separator());
        inner.push(text(format!(
            "**Asks** · {}\nasked {}",
            clip(&q.prompt, 300),
            ago(q.asked_at_ms.max(v.state_since_ms))
        )));
    }
    if let Some(url) = link {
        inner.push(row(vec![super::v2::link_button(url, "Open the task ↗")]));
    }
    vec![container(Some(0x3E_63_DD), inner)]
}

/// A task's head, doing, plan, last calls and report.
fn body(v: &WorkView, calls: &[String], now_ms: u64) -> Vec<Value> {
    let mut head = format!(
        "**{}** `…{}`\n{}",
        clip(v.title.trim(), 120),
        short(&v.id),
        state_words(v)
    );
    if v.started_at_ms > 0 {
        head.push_str(&format!(" · started {}", ago(v.started_at_ms)));
    }
    if let Some(c) = v.cost.filter(|c| c.spent_usd > 0.0 || c.limit_usd > 0.0) {
        head.push_str(&format!(
            " · {}{}",
            usd(c.spent_usd),
            if c.limit_usd > 0.0 {
                format!(" of {}", usd(c.limit_usd))
            } else {
                String::new()
            }
        ));
    }
    let mut inner = vec![text(head)];
    if let Some(d) = v.doing.as_ref().filter(|d| !d.label.is_empty()) {
        inner.push(text(format!(
            "**Doing** · {} · turn {}",
            clip(&d.label, 80),
            d.turn
        )));
    }
    if let Some(p) = v.progress.as_ref() {
        let mut lines = Vec::new();
        if p.steps_total > 0 {
            lines.push(format!(
                "**Plan** · {} of {} steps{}",
                p.steps_done,
                p.steps_total,
                p.current
                    .as_deref()
                    .map(|c| format!(" · now: {}", clip(c, 100)))
                    .unwrap_or_default()
            ));
        }
        if let Some(said) = p.said.as_deref().filter(|s| !s.trim().is_empty()) {
            let doing = v.doing.as_ref().map(|d| d.since_ms);
            let said = aged_said(said, p.at_ms, doing, now_ms);
            lines.push(format!(
                "**Last words**\n> {}",
                clip(&said.replace('\n', "\n> "), 500)
            ));
        }
        if !lines.is_empty() {
            inner.push(text(lines.join("\n")));
        }
    }
    if !calls.is_empty() {
        let lines: Vec<String> = calls
            .iter()
            .take(3)
            .map(|c| format!("`{}`", clip(&c.replace('`', "'"), 90)))
            .collect();
        inner.push(text(format!(
            "**{} Last calls**\n{}",
            crate::runtime::emoji::job(),
            lines.join("\n")
        )));
    }
    if let Some(r) = v.report.as_ref() {
        let said = r
            .first_line
            .as_deref()
            .or(r.reason.as_deref())
            .unwrap_or("");
        if !said.is_empty() {
            inner.push(text(format!(
                "**Report** · {}: {}",
                r.outcome,
                clip(said, 300)
            )));
        }
    }
    inner
}

/// Stop's second question.
pub fn confirm_stop(v: &WorkView) -> Vec<Value> {
    vec![container(
        Some(0xE5_48_4D),
        vec![
            text(format!(
                "Stop **{}** `…{}`? Its jobs end and it reports as cancelled.",
                clip(v.title.trim(), 80),
                short(&v.id)
            )),
            row(vec![
                button(
                    format!("{STOP_YES}{}", v.id),
                    &format!("Stop {}", short(&v.id)),
                    Look::Danger,
                ),
                button(format!("{STOP_NO}{}", v.id), "Keep it", Look::Secondary),
            ]),
        ],
    )]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::v2::{custom_ids, texts, validate};
    use theseus_protocol::work::{Doing, DoingKind, Progress, WorkKind};
    use theseus_protocol::{Attention, Level};

    fn task() -> WorkView {
        WorkView {
            moved_to: None,
            id: "exe_01J000000000000000task01".into(),
            kind: WorkKind::Task,
            position: 1,
            at_ms: 0,
            parent: None,
            root: "exe_chat".into(),
            session_id: None,
            title: "Rename the modules".into(),
            place: None,
            state: WorkState::Running,
            previous: None,
            attention: Attention {
                level: Level::Working,
                label: String::new(),
                since_ms: 0,
            },
            started_at_ms: 1_790_000_000_000,
            state_since_ms: 1_790_000_000_000,
            ended_at_ms: None,
            doing: Some(Doing {
                what: DoingKind::Tool,
                label: "fs.edit".into(),
                since_ms: 0,
                turn: 4,
                loop_index: 0,
                last_line: None,
            }),
            progress: Some(Progress {
                steps_done: 2,
                steps_total: 5,
                current: Some("rename the store".into()),
                said: Some("Two of five modules renamed.".into()),
                at_ms: 0,
            }),
            question: None,
            cost: None,
            eta: None,
            below: None,
            why: None,
            report: None,
            woke: None,
            expect: None,
            flagged: false,
            dismissed: false,
            retry_of: None,
            forecast: None,
            thread: None,
            purpose: None,
            next_at_ms: None,
            series: None,
            paused: false,
            routine: None,
            watch: None,
            project: None,
            data: None,
        }
    }

    #[test]
    fn a_running_task_shows_its_doing_plan_words_and_calls_and_a_stop_button() {
        let calls = vec!["cargo build".to_string(), "git diff".to_string()];
        let c = detail(&task(), &calls, None, &Comments::default(), 0);
        assert_eq!(validate(&c), Ok(()));
        let t = texts(&c).join("\n");
        assert!(t.contains("**Rename the modules** `…task01`"), "{t}");
        assert!(t.contains("**Doing** · fs.edit · turn 4"), "{t}");
        assert!(
            t.contains("**Plan** · 2 of 5 steps · now: rename the store"),
            "{t}"
        );
        assert!(t.contains("> Two of five modules renamed."), "{t}");
        assert!(t.contains("`cargo build`\n`git diff`"), "{t}");
        assert_eq!(
            custom_ids(&c),
            [
                "tb:stop:exe_01J000000000000000task01",
                "tb:follow:exe_01J000000000000000task01"
            ]
        );
    }

    /// The last words carry their age when they are over a minute older than
    /// what the task does now, and none when they are fresh (theseus-ztkhg.1).
    #[test]
    fn old_last_words_carry_their_age() {
        let mut v = task();
        v.doing.as_mut().unwrap().since_ms = 200_000;
        v.progress.as_mut().unwrap().at_ms = 80_000;
        let t = texts(&detail(&v, &[], None, &Comments::default(), 200_000)).join("\n");
        assert!(
            t.contains("> Two of five modules renamed. (2 min ago)"),
            "{t}"
        );
        v.progress.as_mut().unwrap().at_ms = 170_000;
        let t = texts(&detail(&v, &[], None, &Comments::default(), 200_000)).join("\n");
        assert!(
            t.contains("> Two of five modules renamed.\n") || t.ends_with("renamed."),
            "{t}"
        );
        assert!(!t.contains("ago)"), "{t}");
    }

    #[test]
    fn a_done_task_has_no_stop_and_the_second_question_has_two_answers() {
        let mut v = task();
        v.state = WorkState::Done;
        assert!(custom_ids(&detail(&v, &[], None, &Comments::default(), 0)).is_empty());
        let ids = custom_ids(&confirm_stop(&task()));
        assert_eq!(
            ids,
            [
                "tb:yes:exe_01J000000000000000task01",
                "tb:no:exe_01J000000000000000task01"
            ]
        );
    }
}
