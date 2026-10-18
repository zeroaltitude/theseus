//! The task tree's tests: the regatta board drawn at 80, 120 and 200 columns
//! (goldens in `tests/golden_tree/`, rewritten by `THESEUS_GOLDEN=write`), a
//! line for each task, the command beneath a row that needs you, and the
//! words of each event.

use serde_json::{json, Value};

use super::*;

const NOW: u64 = 1_000_000;

fn view(v: Value) -> WorkView {
    let mut base = json!({
        "id": "exe_1", "kind": "task", "position": 9, "at_ms": 9, "root": "ses_conv",
        "parent": "ses_conv", "title": "", "state": "running",
        "attention": {"level": "working", "label": "turn 1", "since_ms": NOW - 60_000},
        "started_at_ms": NOW - 60_000, "state_since_ms": NOW - 60_000
    });
    for (k, x) in v.as_object().unwrap() {
        base[k] = x.clone();
    }
    serde_json::from_value(base).unwrap()
}

fn question(id: &str, kind: &str, prompt: &str, options: Value) -> Value {
    json!({
        "id": id, "kind": kind, "prompt": prompt, "tool": "fs.write",
        "asked_by": {"id": "exe_paint", "kind": "task", "title": "Paint log"},
        "options": options, "floor": false, "may_answer": true, "asked_at_ms": NOW - 50_000
    })
}

fn approve_options() -> Value {
    json!([
        {"id": "approve", "label": "Approve", "style": "primary", "takes_note": false, "effect": "the call runs"},
        {"id": "decline", "label": "Decline", "style": "plain", "takes_note": true, "effect": "the call does not run"}
    ])
}

/// Five tasks under one conversation: one asks, one runs a job, one thinks,
/// one failed, one reported.
fn regatta() -> Vec<WorkView> {
    vec![
        view(json!({
            "id": "exe_conv", "kind": "conversation", "session_id": "ses_conv", "parent": null,
            "root": "ses_conv", "title": "Regatta prep", "state": "waiting",
            "attention": {"level": "working", "label": "waiting on 5 tasks", "since_ms": NOW - 90_000},
            "started_at_ms": NOW - 900_000
        })),
        view(json!({
            "id": "exe_paint", "session_id": "ses_paint2a329e", "title": "Paint log",
            "state": "needs_you",
            "attention": {"level": "needs_you", "label": "confirm fs.write", "since_ms": NOW - 50_000},
            "question": question("act_x2a329e", "approval",
                "write regatta/paint-log.md (37 B)", approve_options())
        })),
        view(json!({
            "id": "exe_buoy", "session_id": "ses_buoy5a877c", "title": "Buoy survey",
            "started_at_ms": NOW - 200_000, "state": "waiting",
            "doing": {"what": "job", "label": "cargo run", "since_ms": NOW - 134_000, "turn": 3, "loop_index": 0},
            "progress": {"steps_done": 4, "steps_total": 9, "current": "leg 5", "said": "sounding the channel", "at_ms": NOW - 5_000},
            "attention": {"level": "working", "label": "cargo run", "since_ms": NOW - 134_000}
        })),
        view(json!({
            "id": "act_job", "kind": "job", "session_id": null, "parent": "exe_buoy",
            "title": "cargo run", "started_at_ms": NOW - 134_000, "state": "running",
            "doing": {"what": "job", "label": "cargo run", "since_ms": NOW - 134_000, "turn": 3, "loop_index": 0,
                "last_line": "leg 5: 4.5 fathoms"},
            "attention": {"level": "working", "label": "running", "since_ms": NOW - 134_000}
        })),
        view(json!({
            "id": "exe_tide", "session_id": "ses_tide77c0de", "title": "Tide tables with the long name of a very thorough analysis",
            "started_at_ms": NOW - 31_000,
            "doing": {"what": "thinking", "label": "turn 1", "since_ms": NOW - 31_000, "turn": 1, "loop_index": 0}
        })),
        view(json!({
            "id": "exe_rig", "session_id": "ses_rig0f1e2d", "title": "Rigging check",
            "state": "failed", "started_at_ms": NOW - 400_000, "ended_at_ms": NOW - 100_000,
            "attention": {"level": "needs_you", "label": "failed: the provider refused the request (400)", "since_ms": NOW - 100_000},
            "report": {"outcome": "failed", "reason": "the provider refused the request (400)"}
        })),
        view(json!({
            "id": "exe_wx", "session_id": "ses_wxbeef01", "title": "Weather brief",
            "state": "done", "started_at_ms": NOW - 600_000, "ended_at_ms": NOW - 300_000,
            "attention": {"level": "idle", "label": "complete", "since_ms": NOW - 300_000},
            "report": {"outcome": "done", "first_line": "Fair winds Saturday, gusts to 18 knots\nNext line"}
        })),
    ]
}

fn text(lines: &[Line]) -> String {
    lines
        .iter()
        .map(|l| l.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn golden(name: &str, got: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden_tree")
        .join(name);
    if std::env::var("THESEUS_GOLDEN").as_deref() == Ok("write") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(got, want, "{name}: THESEUS_GOLDEN=write rewrites it");
}

#[test]
fn the_regatta_at_80_120_and_200_columns() {
    for w in [80, 120, 200] {
        let lines = tree(&regatta(), NOW, w, 0);
        for l in &lines {
            assert!(
                l.text.chars().count() <= w,
                "{w}: a line is wider than the terminal: {}",
                l.text
            );
        }
        golden(&format!("tree_{w}.txt"), &text(&lines));
    }
}

#[test]
fn each_task_is_one_line_under_its_conversation() {
    let lines = tree(&regatta(), NOW, 120, 0);
    let rows: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
    for id in ["2a329e", "5a877c", "77c0de", "beef01", "0f1e2d"] {
        let n = rows
            .iter()
            .filter(|r| r.contains(&format!(" {id}  ")))
            .count();
        assert!(n >= 1, "{id} has a row");
    }
    // The conversation's header, five tasks (the job is no row), a command
    // under the asking one and under the failed one, and the footer.
    assert_eq!(rows.len(), 1 + 5 + 2 + 1, "{rows:#?}");
    assert!(rows[0].contains("Regatta prep"));
    assert!(
        rows.iter().all(|r| !r.contains("cargo run  ")),
        "a job is no row"
    );
    assert_eq!(
        rows.last().copied(),
        Some("1 needs you · 2 working · 1 failed · 1 done")
    );
}

#[test]
fn the_row_that_asks_has_its_command_beneath() {
    let lines = tree(&regatta(), NOW, 120, 0);
    let at = lines
        .iter()
        .position(|l| l.text.contains("asks: write regatta/paint-log.md (37 B)"))
        .expect("the asking row");
    assert_eq!(
        lines[at + 1].text.trim(),
        "theseus answer 2a329e approve · theseus answer 2a329e decline --note …"
    );
    // At 80 columns the second command gives up its repeated start.
    let narrow = tree(&regatta(), NOW, 80, 0);
    let at = narrow
        .iter()
        .position(|l| l.text.contains("asks: "))
        .expect("the asking row");
    assert_eq!(
        narrow[at + 1].text.trim(),
        "theseus answer 2a329e approve · … decline --note …"
    );
}

#[test]
fn an_ask_question_offers_its_choices_as_commands() {
    let mut views = regatta();
    views[1].question = serde_json::from_value(question(
        "act_ask000abc",
        "ask",
        "which hull colour?",
        json!([
            {"id": "choice:0", "label": "red", "style": "plain", "takes_note": false, "effect": "x"},
            {"id": "choice:1", "label": "blue", "style": "plain", "takes_note": false, "effect": "x"},
            {"id": "choice:other", "label": "Something else", "style": "plain", "takes_note": true, "effect": "x"}
        ]),
    ))
    .unwrap();
    views[1].question.as_mut().unwrap().kind = QuestionKind::Ask;
    let lines = tree(&views, NOW, 120, 0);
    let at = lines
        .iter()
        .position(|l| l.text.contains("asks: which hull"))
        .unwrap();
    assert_eq!(
        lines[at + 1].text.trim(),
        "theseus answer 000abc choice:0 · theseus answer 000abc choice:1"
    );
}

#[test]
fn the_title_gives_way_before_the_activity() {
    let narrow = tree(&regatta(), NOW, 80, 0);
    let tide = narrow.iter().find(|l| l.text.contains("77c0de")).unwrap();
    assert!(
        tide.text.contains('…'),
        "the long title is cut: {}",
        tide.text
    );
    assert!(
        tide.text.ends_with("model 0:31"),
        "the activity is whole: {}",
        tide.text
    );
}

#[test]
fn a_job_and_a_plan_step_show_in_the_activity() {
    let lines = tree(&regatta(), NOW, 200, 0);
    let buoy = lines
        .iter()
        .find(|l| l.text.contains("5a877c  Buoy"))
        .unwrap();
    assert!(
        buoy.text
            .contains("job 2:14 › leg 5: 4.5 fathoms · step 5 of 9 · sounding"),
        "{}",
        buoy.text
    );
}

#[test]
fn nothing_going_on_says_so() {
    assert_eq!(text(&tree(&[], NOW, 80, 0)), "no tasks\n");
    // A quiet conversation with no tasks is no row.
    let idle = view(json!({
        "id": "exe_idle", "kind": "conversation", "session_id": "ses_idle", "parent": null,
        "title": "Quiet", "state": "ready",
        "attention": {"level": "ready", "label": "ready", "since_ms": 1}
    }));
    assert_eq!(text(&tree(&[idle], NOW, 80, 0)), "no tasks\n");
}

#[test]
fn events_are_worded_once_each() {
    let views = regatta();
    let paint = &views[1];
    let words: Vec<String> = events(None, paint)
        .iter()
        .map(|e| event_line(e).text)
        .collect();
    assert_eq!(
        words,
        [
            "◐ started: Paint log (2a329e)",
            "● Paint log asks: write regatta/paint-log.md (37 B)?"
        ]
    );
    // The same view again says nothing.
    assert!(events(Some(paint), paint).is_empty());
    let failed = event_line(&events(None, &views[5])[0]).text;
    assert_eq!(
        failed,
        "✗ Rigging check failed: the provider refused the request (400)"
    );
    let done = events(None, &views[6]);
    assert_eq!(done.len(), 1, "a task first seen ended is not 'started'");
    assert_eq!(
        event_line(&done[0]).text,
        "✓ Weather brief reported: Fair winds Saturday, gusts to 18 knots"
    );
    // A conversation's own changes are not task events.
    assert!(events(None, &views[0]).is_empty());
}

/// A failed task names the two verbs that deal with it, `retry` and
/// `dismiss` (theseus-7zgsm), beneath its row and under its event; one
/// already dismissed names none.
#[test]
fn a_failed_task_names_retry_and_dismiss() {
    let lines = tree(&regatta(), NOW, 120, 0);
    let at = lines
        .iter()
        .position(|l| l.text.contains("Rigging check"))
        .unwrap();
    assert_eq!(
        lines[at + 1].text.trim(),
        "theseus retry 0f1e2d · theseus dismiss 0f1e2d",
        "{lines:#?}"
    );
    let mut views = regatta();
    for v in &mut views {
        if v.id == "exe_rig" {
            v.dismissed = true;
        }
    }
    let lines = tree(&views, NOW, 120, 0);
    assert!(
        !lines.iter().any(|l| l.text.contains("theseus retry")),
        "{lines:#?}"
    );
}

fn spent(views: &mut [WorkView], id: &str, usd: f64) {
    let v = views.iter_mut().find(|v| v.id == id).unwrap();
    v.cost = Some(theseus_protocol::work::Cost {
        spent_usd: usd,
        limit_usd: 5.0,
        turn_usd: None,
    });
}

#[test]
fn the_footer_shows_what_a_running_task_has_spent_and_the_tree_does_not() {
    let mut views = regatta();
    spent(&mut views, "exe_buoy", 1.2);
    spent(&mut views, "exe_tide", 0.0042);
    let f = footer(&views, NOW, 100, 5);
    golden("footer_cost_100.txt", &text(&f.rows));
    // The tree's columns are the ones it always had.
    let tree_lines = tree(&views, NOW, 120, 0);
    assert!(tree_lines.iter().all(|l| !l.text.contains('$')));
}

#[test]
fn a_routine_a_watch_and_a_project_are_rows_but_no_task() {
    let mut views = regatta();
    let before = text(&tree(&views, NOW, 120, 0));
    views.push(view(json!({
        "id": "exe_rout", "kind": "routine", "session_id": "ses_rout9d0939", "parent": null,
        "title": "Morning tide check", "state": "waiting", "paused": true,
        "attention": {"level": "idle", "label": "paused", "since_ms": NOW - 5_000}
    })));
    views.push(view(json!({
        "id": "exe_watch", "kind": "watch", "session_id": "ses_watchc0ffee", "parent": null,
        "title": "Harbour gate open", "state": "waiting", "next_at_ms": NOW + 600_000,
        "attention": {"level": "idle", "label": "checking every 5m", "since_ms": NOW - 5_000}
    })));
    views.push(view(json!({
        "id": "exe_proj", "kind": "project", "session_id": "ses_projfeed01", "parent": null,
        "title": "Refit the hull", "state": "running",
        "attention": {"level": "idle", "label": "2 of 7 milestones", "since_ms": NOW - 5_000}
    })));
    let lines = tree(&views, NOW, 120, 0);
    let all = text(&lines);
    for (id, what) in [
        ("9d0939  Morning tide check", "routine · paused"),
        ("c0ffee  Harbour gate open", "watch · next "),
        ("feed01  Refit the hull", "project · 2 of 7 milestones"),
    ] {
        let row = lines
            .iter()
            .find(|l| l.text.contains(id))
            .unwrap_or_else(|| panic!("{id}:\n{all}"));
        assert!(row.text.contains(what), "{}", row.text);
    }
    // They are no tasks: the footer's counts are the tasks' alone.
    assert_eq!(
        lines.last().unwrap().text,
        before.lines().last().unwrap(),
        "{all}"
    );
}
