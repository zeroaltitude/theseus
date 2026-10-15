//! The pure parts of projects (theseus-n5ui2): progress from a subtree, the
//! slice's text and bound, the reminders' ids and notes, and the slice built
//! from memory in microseconds. The whole-core scenarios are
//! `tests_projects.rs`.

use std::time::Instant;

use theseus_protocol::tasks::{TaskKind, TaskRecord, TaskState};

use super::progress::{milestones, progress};
use super::slice::{render, MAX_TOKENS};
use crate::wake::local;

const DAY: u64 = 86_400_000;
const NOW: u64 = 1_790_000_000_000;

fn task(id: &str, parent: Option<&str>, kind: TaskKind, state: TaskState) -> TaskRecord {
    TaskRecord {
        id: id.into(),
        title: format!("{id} title"),
        parent: parent.map(String::from),
        kind,
        state,
        version: 1,
        owner: "agent".into(),
        created_at_ms: NOW - DAY + id.len() as u64,
        ..Default::default()
    }
}

fn due(mut t: TaskRecord, at: u64) -> TaskRecord {
    t.due_at_ms = Some(at);
    t
}

/// A project with three milestones (one done, one overdue, one ahead) and
/// four steps (two done, one abandoned).
fn harbour() -> Vec<TaskRecord> {
    let mut p = task("tsk_harbour", None, TaskKind::Project, TaskState::Accepted);
    p.title = "Harbour survey".into();
    p.objective = "Chart every mooring in the outer harbour".into();
    p.acceptance = vec![
        "every mooring has a depth".into(),
        "the chart is signed".into(),
    ];
    let ms = |id: &str, state, at| {
        due(
            task(id, Some("tsk_harbour"), TaskKind::Milestone, state),
            at,
        )
    };
    vec![
        p,
        ms("tsk_m1", TaskState::Done, NOW - 3 * DAY),
        ms("tsk_m22", TaskState::Accepted, NOW - DAY),
        ms("tsk_m333", TaskState::Accepted, NOW + 2 * DAY),
        task(
            "tsk_s1",
            Some("tsk_harbour"),
            TaskKind::Item,
            TaskState::Done,
        ),
        task("tsk_s22", Some("tsk_m22"), TaskKind::Item, TaskState::Done),
        task(
            "tsk_s333",
            Some("tsk_m22"),
            TaskKind::Item,
            TaskState::Accepted,
        ),
        task(
            "tsk_s4444",
            Some("tsk_harbour"),
            TaskKind::Item,
            TaskState::Abandoned,
        ),
    ]
}

#[test]
fn progress_counts_steps_and_milestones_under_the_project_and_the_overdue() {
    let t = harbour();
    let p = progress(&t, "tsk_harbour", NOW);
    assert_eq!((p.milestones_done, p.milestones_total), (1, 3));
    assert_eq!(
        (p.steps_done, p.steps_total),
        (2, 3),
        "an abandoned step is not counted"
    );
    assert_eq!(p.overdue, 1);
    assert_eq!(
        p.next_due_ms,
        Some(NOW - DAY),
        "the soonest open date, overdue or not"
    );
    assert_eq!(p.bar(3), "▰▱▱ 1/3");
    let order: Vec<&str> = milestones(&t, "tsk_harbour")
        .iter()
        .map(|m| m.id.as_str())
        .collect();
    assert_eq!(order, ["tsk_m1", "tsk_m22", "tsk_m333"], "by date");
    // Another project's records are no part of it.
    let mut other = t.clone();
    other.push(task(
        "tsk_elsewhere",
        None,
        TaskKind::Milestone,
        TaskState::Accepted,
    ));
    assert_eq!(progress(&other, "tsk_harbour", NOW), p);
}

#[test]
fn the_slice_names_the_goal_the_open_milestones_with_dates_and_the_next_steps() {
    let t = harbour();
    let s = render(&t, "tsk_harbour", NOW, &local).unwrap();
    assert!(s.text.starts_with(super::slice::HEAD), "{}", s.text);
    assert!(
        s.text.contains("1/3 milestones, 2/3 steps, 1 overdue"),
        "{}",
        s.text
    );
    assert!(s
        .text
        .contains("Goal: Chart every mooring in the outer harbour"));
    assert!(s.text.contains("OVERDUE since"), "{}", s.text);
    assert!(s.text.contains("tsk_m22 title"), "{}", s.text);
    assert!(
        !s.text.contains("tsk_m1 title"),
        "a done milestone is not listed"
    );
    assert!(
        s.text.contains("→ tsk_s333 title"),
        "the next step: {}",
        s.text
    );
    assert!(!s.text.contains("tsk_s22 title"), "a done step is not");
    assert!(s
        .text
        .contains("Done when: every mooring has a depth (+1 more)"));
    assert_eq!(s.summary.project, "tsk_harbour");
    assert!(
        s.summary.tokens <= MAX_TOKENS,
        "{} tokens",
        s.summary.tokens
    );
    // Nothing for a task that is not here.
    assert!(render(&t, "tsk_missing", NOW, &local).is_none());
}

#[test]
fn a_big_project_stays_within_its_bound_and_says_what_it_left_out() {
    let mut t = harbour();
    for i in 0..40 {
        let mut m = due(
            task(
                &format!("tsk_big{i}"),
                Some("tsk_harbour"),
                TaskKind::Milestone,
                TaskState::Accepted,
            ),
            NOW + (i + 5) * DAY,
        );
        m.title = format!("A milestone with a rather long title number {i} of the survey");
        t.push(m);
        let mut s = task(
            &format!("tsk_step{i}"),
            Some("tsk_harbour"),
            TaskKind::Item,
            TaskState::Accepted,
        );
        s.title = format!("A step with a rather long title number {i} of the survey");
        t.push(s);
    }
    let s = render(&t, "tsk_harbour", NOW, &local).unwrap();
    // The row's promise is a literal: at most 300 tokens (reviewer D's planted
    // revert raised MAX_TOKENS itself, and a test reading the constant passed).
    assert_eq!(MAX_TOKENS, 300);
    assert!(
        s.summary.tokens <= 300,
        "{} tokens: {}",
        s.summary.tokens,
        s.text
    );
    assert!(
        s.summary.left_out > 0 && s.text.contains("more: task.list"),
        "{}",
        s.text
    );
    assert!(s.text.contains("Goal:"), "the goal always stays");
}

/// Built from memory, in microseconds: the slice of a project of thirty
/// tasks renders in well under the 50 µs a release build is held to (a debug
/// build is held to twenty times that).
#[test]
fn the_slice_is_built_from_memory_in_microseconds() {
    let mut t = harbour();
    for i in 0..12 {
        t.push(due(
            task(
                &format!("tsk_x{i}"),
                Some("tsk_harbour"),
                TaskKind::Milestone,
                TaskState::Accepted,
            ),
            NOW + i * DAY,
        ));
        t.push(task(
            &format!("tsk_y{i}"),
            Some("tsk_harbour"),
            TaskKind::Item,
            TaskState::Accepted,
        ));
    }
    let n = 400;
    let mut times: Vec<u128> = (0..n)
        .map(|_| {
            let t0 = Instant::now();
            let s = render(&t, "tsk_harbour", NOW, &local);
            let us = t0.elapsed().as_micros();
            assert!(s.is_some());
            us
        })
        .collect();
    times.sort_unstable();
    let median = times[n / 2];
    eprintln!(
        "slice render: median {median} µs over {n} (p90 {} µs)",
        times[n * 9 / 10]
    );
    let bound = if cfg!(debug_assertions) { 1_000 } else { 50 };
    assert!(median <= bound, "median {median} µs over {bound}");
}

#[test]
fn a_milestones_wakes_have_ids_of_their_own_and_notes_that_say_which() {
    use super::remind::{due_id, note, overdue_id};
    let t = harbour();
    assert_ne!(due_id("tsk_m22"), overdue_id("tsk_m22"));
    assert_ne!(due_id("tsk_m22"), due_id("tsk_m333"));
    assert!(due_id("tsk_m22").starts_with("wak_"));
    let p = &t[0];
    let m = &t[2];
    assert_eq!(
        note(&t, p, m, false),
        "⚑ due today: Harbour survey, milestone 2: tsk_m22 title",
        "the ordinal is the order they were made in"
    );
    assert!(note(&t, p, m, true).starts_with("⚑ overdue: "));
}
