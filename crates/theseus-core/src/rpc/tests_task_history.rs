//! `task.list`'s filters (theseus-ypnxt.1, A): ended, since, query, id,
//! limit, and the default call that answers what it always did.

use theseus_protocol::TaskListParams;

use crate::task_graph::TaskState;
use crate::tests_task_graph::{rig, session, turn, until, Rig};

/// Runs one task to its report and returns its short id. `ask` holds the
/// quote the task's arrangement cites.
async fn finish(r: &Rig, s: &str, title: &str, ask: &str, report: &str) -> String {
    r.model.report_with(report);
    let before = r.tasks().len();
    r.calls(&[(
        "task_create",
        serde_json::json!({"brief": format!("{ask} and report what you found."),
               "title": title,
               "arrangement": {"pieces": [{"quote": ask, "role": "objective"}]}}),
    )]);
    turn(&r.core, s, ask).await;
    let t = r.by_title(title);
    assert_eq!(r.tasks().len(), before + 1);
    until("the task's report", || {
        r.task(&t.id).state == TaskState::Done
    })
    .await;
    t.short().to_string()
}

fn list(r: &Rig, p: TaskListParams) -> theseus_protocol::TaskListResult {
    r.core.task_list(p).unwrap()
}

fn titles(l: &theseus_protocol::TaskListResult) -> Vec<String> {
    l.tasks.iter().filter_map(|t| t.title.clone()).collect()
}

/// Three ended tasks, oldest first, with a pause between them.
async fn three() -> (Rig, [u64; 2]) {
    let r = rig();
    let s = session(&r.core);
    finish(
        &r,
        &s,
        "Chart the buoys",
        "chart every buoy of the outer harbour",
        "Charted 41 buoys; two were adrift.",
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let mid = theseus_protocol::now_unix_ms();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    finish(
        &r,
        &s,
        "Sound the channel",
        "sound the depth of the north channel",
        "Soundings done.\nThe channel is 9 m at low tide.",
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let late = theseus_protocol::now_unix_ms();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    finish(
        &r,
        &s,
        "Paint the quay",
        "paint the railings along the quay",
        "Painted the quay railings green.",
    )
    .await;
    (r, [mid, late])
}

/// The keys of a task in the answer to a plain `task.list`, as read before the
/// filters were added (a pinned copy of what it always said).
const PINNED_TASK_KEYS: [&str; 19] = [
    "arrangement",
    "attention",
    "cost_usd",
    "created_at_ms",
    "depth",
    "ended_reason",
    "execution_id",
    "limit_usd",
    "parent_execution_id",
    "parent_session_id",
    "pending_confirms",
    "short",
    "spent_usd",
    "state",
    "target",
    "task_id",
    "title",
    "turns",
    "updated_at_ms",
];

/// The default call answers what it did before the filters: the same two
/// keys, the same keys on a task, no `details`.
#[tokio::test]
async fn the_default_call_answers_what_it_always_did() {
    let (r, _) = three().await;
    let v = serde_json::to_value(list(&r, TaskListParams::default())).unwrap();
    let keys =
        |v: &serde_json::Value| -> Vec<String> { v.as_object().unwrap().keys().cloned().collect() };
    assert_eq!(keys(&v), ["records", "tasks"]);
    assert_eq!(keys(&v["tasks"][0]), PINNED_TASK_KEYS);
    assert_eq!(v["tasks"].as_array().unwrap().len(), 3);
    assert!(v.get("details").is_none());
}

#[tokio::test]
async fn ended_keeps_the_ended_and_the_open_apart() {
    let (r, _) = three().await;
    let ended = list(
        &r,
        TaskListParams {
            ended: Some(true),
            ..Default::default()
        },
    );
    assert_eq!(
        titles(&ended),
        ["Paint the quay", "Sound the channel", "Chart the buoys"]
    );
    assert_eq!(
        ended.details[0].report_line.as_deref(),
        Some("Painted the quay railings green.")
    );
    let open = list(
        &r,
        TaskListParams {
            ended: Some(false),
            ..Default::default()
        },
    );
    assert!(open.tasks.is_empty() && open.details.is_empty());
}

#[tokio::test]
async fn since_cuts_by_the_time_a_task_ended() {
    let (r, [mid, late]) = three().await;
    let after = |ms| {
        titles(&list(
            &r,
            TaskListParams {
                since_ms: Some(ms),
                ..Default::default()
            },
        ))
    };
    assert_eq!(after(mid), ["Paint the quay", "Sound the channel"]);
    assert_eq!(after(late), ["Paint the quay"]);
    assert!(after(theseus_protocol::now_unix_ms() + 60_000).is_empty());
}

#[tokio::test]
async fn query_finds_a_word_of_the_title_the_brief_or_the_report() {
    let (r, _) = three().await;
    let q = |text: &str| {
        titles(&list(
            &r,
            TaskListParams {
                query: Some(text.into()),
                ..Default::default()
            },
        ))
    };
    // A word only the report of one task holds, in any case.
    assert_eq!(q("ADRIFT"), ["Chart the buoys"]);
    // The report's first line only: its second line is not searched.
    assert!(q("low tide").is_empty());
    // A title word, and a brief word the title lacks.
    assert_eq!(q("quay"), ["Paint the quay"]);
    assert_eq!(q("railings"), ["Paint the quay"]);
    // All the words must appear; they may come from different places.
    assert_eq!(q("sound soundings"), ["Sound the channel"]);
    assert!(q("quay adrift").is_empty());
}

#[tokio::test]
async fn id_names_one_task_by_the_end_of_its_id_with_its_whole_report() {
    let (r, _) = three().await;
    let sound = r.by_title("Sound the channel");
    let end = &sound.id[sound.id.len() - 8..];
    let one = list(
        &r,
        TaskListParams {
            id: Some(end.into()),
            ..Default::default()
        },
    );
    assert_eq!(titles(&one), ["Sound the channel"]);
    assert_eq!(one.records.len(), 1);
    let d = &one.details[0];
    assert_eq!(d.report_line.as_deref(), Some("Soundings done."));
    assert_eq!(
        d.report.as_deref(),
        Some("Soundings done.\nThe channel is 9 m at low tide.")
    );
    assert!(!d.paused);
    let none = r
        .core
        .task_list(TaskListParams {
            id: Some("nosuchtask".into()),
            ..Default::default()
        })
        .unwrap_err();
    assert_eq!(none.code, theseus_protocol::error_code::NOT_FOUND);
}

#[tokio::test]
async fn limit_takes_the_newest() {
    let (r, _) = three().await;
    let l = list(
        &r,
        TaskListParams {
            limit: Some(2),
            ..Default::default()
        },
    );
    assert_eq!(titles(&l), ["Paint the quay", "Sound the channel"]);
    assert_eq!(l.records.len(), 2);
}

/// A task that failed or was stopped has no report node: it is listed with
/// why it ended.
#[tokio::test]
async fn a_task_with_no_report_node_reports_why_it_ended() {
    let r = rig();
    assert_eq!(
        r.core.report_text(None, Some("the model was unavailable")),
        Some("the model was unavailable".into())
    );
    assert_eq!(r.core.report_text(None, Some("  ")), None);
    assert_eq!(r.core.report_text(None, None), None);
}

/// What reading the report line costs on a store of 5,000 ended tasks (a measure, not a gate; run it with
/// `--run-ignored only --no-capture`): the task records read as `task.list` reads them (`all_shown`, which
/// the plain call pays too), then each record's report line, which only `ended`, `query` and `id` pay.
#[tokio::test]
#[ignore = "a measure: run with --run-ignored only --no-capture"]
async fn the_report_line_costs_this_much_on_5000_tasks() {
    use crate::node::{Body, Node};
    use crate::task_graph::{self as graph, TaskRecord};
    use theseus_protocol::tasks::TaskEvidence;
    const N: usize = 5_000;
    let r = rig();
    let mut frame = Vec::new();
    for i in 0..N {
        let sid = format!("ses_measure{i:05}");
        let body: Body = serde_json::from_value(serde_json::json!({
            "kind": "assistant_message", "model": "m", "provider": "p",
            "blocks": [{"type": "text", "text": format!("Surveyed berth {i}.\nTwo coats, green.\n{}", "x".repeat(900))}]
        }))
        .unwrap();
        let node = Node::assistant(&sid, "turn_measure", 0, body);
        let rec = TaskRecord {
            id: graph::of_session(&sid),
            version: 2,
            title: format!("Survey {i}"),
            state: TaskState::Done,
            session: Some(sid.clone()),
            evidence: vec![TaskEvidence {
                node: Some(node.id.clone()),
                identity: format!("report:{sid}"),
                note: None,
                by: "report".into(),
                at_ms: 1,
            }],
            ..Default::default()
        };
        frame.push(node.record().unwrap());
        frame.push(graph::record(&rec).unwrap());
        if frame.len() >= 1_000 {
            r.core.store.append(&std::mem::take(&mut frame)).unwrap();
        }
    }
    r.core.store.append(&frame).unwrap();
    let t = std::time::Instant::now();
    let all = graph::all_shown(&r.core.store, &r.core.kernel).unwrap();
    let read_records = t.elapsed();
    assert_eq!(all.len(), N);
    let t = std::time::Instant::now();
    let lines = all
        .iter()
        .filter(|rec| r.core.report_text(Some(rec), None).is_some())
        .count();
    let read_reports = t.elapsed();
    assert_eq!(lines, N);
    println!(
        "{N} tasks: records read (the plain call's cost) {read_records:?}; their report lines {read_reports:?} ({:?} each)",
        read_reports / N as u32
    );
}

// ---- the digest names each task (D) ---------------------------------------------------------

use std::collections::HashSet;

use theseus_protocol::{DigestKind, WorkDigestResult};
use theseus_store::{kinds, NewRecord};

use crate::ledger::LedgerRow;
use crate::push::WorkScope;
use theseus_protocol::LedgerKind;

fn digest(r: &Rig) -> WorkDigestResult {
    r.core
        .digest_blocking(0, None, &WorkScope::default(), &HashSet::new(), true)
        .unwrap()
}

fn reported(d: &WorkDigestResult) -> Vec<String> {
    d.lines
        .iter()
        .filter(|l| l.kind == DigestKind::Reported)
        .map(|l| l.words.clone())
        .collect()
}

/// Before: three finished tasks were `📋 3 tasks reported to <root>`. Now each is named, with its
/// report's first line.
#[tokio::test]
async fn three_reported_tasks_are_three_titled_lines() {
    let (r, _) = three().await;
    let d = digest(&r);
    let mut words = reported(&d);
    words.sort();
    assert_eq!(
        words,
        [
            "📋 Chart the buoys reported: Charted 41 buoys; two were adrift.",
            "📋 Paint the quay reported: Painted the quay railings green.",
            "📋 Sound the channel reported: Soundings done.",
        ]
    );
    assert!(d.more.is_none());
}

/// Seven under one root: five are told, and the other two named together.
#[tokio::test]
async fn seven_reported_tasks_are_five_lines_and_the_rest_named() {
    let r = rig();
    let s = session(&r.core);
    for i in 1..=7 {
        finish(
            &r,
            &s,
            &format!("Survey {i}"),
            &format!("survey the number {i} berth of the quay"),
            "Surveyed.",
        )
        .await;
    }
    let words = reported(&digest(&r));
    assert_eq!(words.len(), 6, "{words:?}");
    assert_eq!(
        words[..5]
            .iter()
            .filter(|w| w.ends_with("reported: Surveyed."))
            .count(),
        5
    );
    assert!(
        words[5].starts_with("📋 and 2 more: Survey "),
        "{}",
        words[5]
    );
}

/// `n` ledger rows of a kind the digest reads, written after whatever is there.
fn rows_after(r: &Rig, n: usize) {
    for chunk in (0..n).collect::<Vec<_>>().chunks(500) {
        let frame: Vec<NewRecord> = chunk
            .iter()
            .map(|_| {
                let row = LedgerRow::new(
                    LedgerKind::WakeFired,
                    Some("ses_nobody"),
                    None,
                    serde_json::json!({"execution_id": "exe_nobody", "purpose": "check"}),
                );
                NewRecord::json(kinds::LEDGER, None, &row).unwrap()
            })
            .collect();
        r.core.store.append(&frame).unwrap();
    }
}

/// Before: the fallback read the newest 2,000 rows, so a task reported at the start of a window
/// with 2,500 rows after it was gone. Now the window is read from its start.
#[tokio::test]
async fn a_window_of_2500_rows_still_finds_the_task_reported_at_its_start() {
    let r = rig();
    let s = session(&r.core);
    finish(
        &r,
        &s,
        "Chart the buoys",
        "chart every buoy of the outer harbour",
        "Charted them.",
    )
    .await;
    rows_after(&r, 2_500);
    let d = digest(&r);
    assert_eq!(reported(&d), ["📋 Chart the buoys reported: Charted them."]);
    assert!(d.more.is_none(), "{:?}", d.more);
}

/// Past the hard stop the window is cut at the time the read reached, and the digest says so.
#[tokio::test]
async fn past_the_hard_stop_the_digest_says_where_it_stopped() {
    let r = rig();
    let s = session(&r.core);
    finish(
        &r,
        &s,
        "Chart the buoys",
        "chart every buoy of the outer harbour",
        "Charted them.",
    )
    .await;
    rows_after(&r, 2_500);
    let (rows, more) = r.core.digest_window(0, 1_000).unwrap();
    let more = more.expect("the read stopped short");
    assert!(
        more.command.starts_with("theseus tasks --ended --since "),
        "{}",
        more.command
    );
    assert!(rows.len() < 2_500 + 50, "{}", rows.len());
    assert!(
        rows.iter().any(|(_, row)| row.is_kind("task.ended")),
        "the first rows are kept"
    );
    // With room for all of it, nothing is cut.
    assert!(r.core.digest_window(0, 20_000).unwrap().1.is_none());
}
