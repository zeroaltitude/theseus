//! The daemon's own work on the board (theseus-jn17m), without a core: rows
//! in, views and notices out. The system root, the jobs under it, the rules
//! of the notification policy for them, and who may see the root.

use std::time::Instant;

use serde_json::{json, Value};
use theseus_protocol::notices::{Urgency, Why};
use theseus_protocol::work::{WorkKind, WorkState, WorkView};
use theseus_protocol::work_system::{self, KEEP_MS, QUIET_MS, ROOT};

use super::*;

const T0: u64 = 1_000_000_000_000;

fn row(kind: &str, at: u64, data: Value) -> LedgerRow {
    LedgerRow {
        at_unix_ms: at,
        ..LedgerRow::named(kind, None, None, data)
    }
}

fn frame(position: u64, at: u64) -> Frame {
    Frame {
        at_ms: at,
        committed: Instant::now(),
        position,
        records: Vec::new(),
        nodes: Vec::new(),
    }
}

/// One frame carrying `rows`, taken as `apply` takes a frame's rows.
fn feed(w: &mut WorkBoard, position: u64, at: u64, rows: Vec<LedgerRow>) -> Out {
    let mut d = Dirty::default();
    w.take_rows(&rows, &mut d);
    w.system.sweep(at, &mut d.system);
    w.settle(&frame(position, at), d)
}

fn batch(at: u64, n: u64, of: u64, source: &str) -> LedgerRow {
    row(
        "import.batch",
        at,
        json!({"tag": "cc-1", "file": "/projects", "by": "cli", "imported": 4, "nodes": 40,
               "batch": n, "batches": of, "source": source}),
    )
}

fn job(out: &Out) -> &WorkView {
    out.views
        .iter()
        .find(|v| v.kind == WorkKind::System && v.id != ROOT)
        .expect("a system job changed")
}

/// An import shows batch by batch, with its title, its count and its time,
/// then ends with what it did, and the root counts it.
#[test]
fn an_import_shows_progress_batch_by_batch_and_ends() {
    let mut w = WorkBoard::default();
    let out = feed(&mut w, 1, T0, vec![batch(T0, 1, 3, "claude-code")]);
    let v = job(&out);
    assert_eq!(v.title, "Importing Claude Code history");
    assert_eq!(v.state, WorkState::Running);
    assert_eq!(v.attention.label, "batch 1 of 3");
    assert_eq!(v.parent.as_deref(), Some(ROOT));
    assert_eq!(v.root, ROOT);
    assert_eq!(
        work_system::line(v, T0 + 192_000),
        "Importing Claude Code history · batch 1 of 3 · 3:12"
    );
    let root = out.views.iter().find(|v| v.id == ROOT).expect("the root");
    assert_eq!(
        (root.kind, root.title.as_str()),
        (WorkKind::System, "Theseus")
    );
    assert_eq!(root.below.map(|b| (b.working, b.total)), Some((1, 1)));
    // Progress speaks to the views and to no one else.
    assert!(out.notices.is_empty());

    let out = feed(
        &mut w,
        2,
        T0 + 10_000,
        vec![batch(T0 + 10_000, 2, 3, "claude-code")],
    );
    assert_eq!(job(&out).attention.label, "batch 2 of 3");
    assert_eq!(job(&out).progress.as_ref().map(|p| p.steps_done), Some(2));
    assert!(out.notices.is_empty());

    let out = feed(
        &mut w,
        3,
        T0 + 20_000,
        vec![batch(T0 + 20_000, 3, 3, "claude-code")],
    );
    let v = job(&out);
    assert_eq!(v.state, WorkState::Done);
    assert_eq!(v.attention.label, "done · 12 sessions");
    assert_eq!(v.ended_at_ms, Some(T0 + 20_000));
    assert_eq!(
        work_system::line(v, T0 + 99_999_999),
        "Importing Claude Code history · done · 12 sessions · 0:20"
    );
    // A short import ends in silence.
    assert!(out.notices.is_empty(), "{:?}", out.notices);
    let root = out.views.iter().find(|v| v.id == ROOT).expect("the root");
    assert_eq!(root.below.map(|b| (b.working, b.done)), Some((0, 1)));
}

/// A sync that fails is one Inform with its reason; its batches before it
/// made none.
#[test]
fn a_failed_sync_is_one_inform_with_its_reason() {
    let mut w = WorkBoard::default();
    let mut notices = Vec::new();
    for (i, n) in [1u64, 2].into_iter().enumerate() {
        let at = T0 + i as u64 * 1000;
        let row = row(
            "import.batch",
            at,
            json!({"tag": "oc", "file": "main", "by": "cli", "imported": 1, "nodes": 3,
                   "batch": n, "source": "openclaw-sync"}),
        );
        let out = feed(&mut w, n, at, vec![row]);
        assert_eq!(job(&out).title, "Syncing OpenClaw history");
        assert_eq!(job(&out).attention.label, format!("batch {n}"));
        notices.extend(out.notices);
    }
    let failed = row(
        "import.synced",
        T0 + 5000,
        json!({"by": "cli", "started_ms": T0, "episodes": 2, "messages": 6, "imported": 2,
               "appended": 0, "skipped": 0, "rejected": 0,
               "error": "the exporter's cursor is ahead of the file"}),
    );
    let out = feed(&mut w, 3, T0 + 5000, vec![failed]);
    notices.extend(out.notices);
    assert_eq!(notices.len(), 1, "{notices:?}");
    let n = &notices[0];
    assert_eq!((n.urgency, n.why), (Urgency::Inform, Why::Failed));
    assert_eq!(
        n.line,
        "✗ Syncing OpenClaw history failed: the exporter's cursor is ahead of the file"
    );
    assert_eq!(n.root.id, ROOT);
    let v = w.views.values().find(|v| v.id != ROOT).unwrap();
    assert_eq!(v.state, WorkState::Failed);
    // A failure informs: it does not need the owner.
    assert_eq!(v.attention.level, theseus_protocol::Level::Idle);
    assert_eq!(
        v.attention.label,
        "failed: the exporter's cursor is ahead of the file"
    );
}

/// A sync that failed before it sent anything is a job, and a notice, too.
#[test]
fn a_sync_that_failed_at_once_still_informs() {
    let mut w = WorkBoard::default();
    let failed = row(
        "import.synced",
        T0,
        json!({"by": "timer", "started_ms": T0 - 2000, "episodes": 0, "messages": 0,
               "imported": 0, "appended": 0, "skipped": 0, "rejected": 0,
               "error": "python3 did not start"}),
    );
    let out = feed(&mut w, 1, T0, vec![failed]);
    assert_eq!(out.notices.len(), 1);
    assert!(out.notices[0]
        .line
        .ends_with("failed: python3 did not start"));
    assert_eq!(job(&out).started_at_ms, T0 - 2000);
}

/// A long import that ends is an Inform, which is a badge and no sound; a
/// short one ends in silence (the first test).
#[test]
fn a_long_import_ends_with_an_inform_and_no_sound() {
    let mut w = WorkBoard::default();
    let mut at = T0;
    feed(&mut w, 1, at, vec![batch(at, 1, 2, "codex")]);
    at += 6 * 60_000;
    let out = feed(&mut w, 2, at, vec![batch(at, 2, 2, "codex")]);
    assert_eq!(out.notices.len(), 1);
    let n = &out.notices[0];
    assert_eq!((n.urgency, n.why), (Urgency::Inform, Why::Finished));
    assert_eq!(
        n.line,
        "✓ Importing Codex history is done after 6 min · 8 sessions"
    );
    // An Inform is a badge for every viewer: it never rings.
    let viewer = theseus_protocol::notices::Viewer {
        sound: true,
        ..Default::default()
    };
    assert_eq!(
        theseus_protocol::notices::deliver(n, &viewer),
        theseus_protocol::notices::Delivery::Badge
    );
}

/// A rebuild: requested, then its share of the WAL, then finished.
#[test]
fn an_index_rebuild_shows_its_share_and_ends() {
    let mut w = WorkBoard::default();
    let ev = |at, event: &str, extra: Value| {
        let mut d = json!({"event": event, "run": T0});
        for (k, v) in extra.as_object().cloned().unwrap_or_default() {
            d[k] = v;
        }
        row("index.rebuild", at, d)
    };
    let out = feed(
        &mut w,
        1,
        T0,
        vec![ev(T0, "requested", json!({"by": "cli"}))],
    );
    assert_eq!(job(&out).title, "Rebuilding the index");
    assert_eq!(job(&out).state, WorkState::Running);
    assert_eq!(
        work_system::line(job(&out), T0 + 125_000),
        "Rebuilding the index · 2:05"
    );
    let out = feed(
        &mut w,
        2,
        T0 + 60_000,
        vec![ev(
            T0 + 60_000,
            "progress",
            json!({"done_bytes": 410, "total_bytes": 1000}),
        )],
    );
    assert_eq!(job(&out).attention.label, "41%");
    let out = feed(
        &mut w,
        3,
        T0 + 90_000,
        vec![ev(T0 + 90_000, "finished", json!({}))],
    );
    assert_eq!(job(&out).state, WorkState::Done);
    assert!(out.notices.is_empty());
    // A failed one informs with the tender's words.
    let mut w = WorkBoard::default();
    feed(&mut w, 1, T0, vec![ev(T0, "requested", json!({}))]);
    let out = feed(
        &mut w,
        2,
        T0 + 1000,
        vec![ev(
            T0 + 1000,
            "failed",
            json!({"why": "the tender stopped answering"}),
        )],
    );
    assert_eq!(
        out.notices[0].line,
        "✗ Rebuilding the index failed: the tender stopped answering"
    );
}

/// Consolidation counts its clusters from the rows it writes for them, and
/// ends at the run's last.
#[test]
fn consolidation_counts_its_clusters() {
    let mut w = WorkBoard::default();
    let cluster = |at: u64, n: u64| {
        row(
            "synthesis.proposed",
            at,
            json!({"synthesis_id": format!("syn_{n}"), "trigger": "nightly", "run": T0, "n": n, "of": 3}),
        )
    };
    let out = feed(&mut w, 1, T0 + 5000, vec![cluster(T0 + 5000, 1)]);
    assert_eq!(job(&out).title, "Consolidating memory");
    assert_eq!(job(&out).attention.label, "1 of 3 clusters");
    // The run began when its plan did.
    assert_eq!(job(&out).started_at_ms, T0);
    feed(&mut w, 2, T0 + 9000, vec![cluster(T0 + 9000, 2)]);
    let out = feed(&mut w, 3, T0 + 12_000, vec![cluster(T0 + 12_000, 3)]);
    assert_eq!(job(&out).state, WorkState::Done);
    assert_eq!(job(&out).attention.label, "done · 3 clusters, nightly");
    // A row from before the run was numbered makes no job.
    let mut w = WorkBoard::default();
    let old = row("synthesis.proposed", T0, json!({"synthesis_id": "syn_0"}));
    assert!(feed(&mut w, 1, T0, vec![old]).views.is_empty());
}

/// An erase: its frames' progress, then its receipt.
#[test]
fn an_erase_shows_its_progress_and_its_receipt() {
    let mut w = WorkBoard::default();
    let erased = |at: u64, extra: Value| {
        let mut d = json!({"tag": "cc-1", "by": "cli", "why": null, "sessions": 100, "nodes": 900});
        for (k, v) in extra.as_object().cloned().unwrap_or_default() {
            d[k] = v;
        }
        row("import.erased", at, d)
    };
    let frame_of = |at: u64, done: u64| {
        row(
            "import.erasing",
            at,
            json!({"tag": "cc-1", "by": "cli", "done": done, "of": 250}),
        )
    };
    let out = feed(&mut w, 1, T0, vec![frame_of(T0, 100)]);
    assert_eq!(job(&out).title, "Erasing the import cc-1");
    assert_eq!(job(&out).attention.label, "100 of 250 sessions");
    let out = feed(
        &mut w,
        2,
        T0 + 4000,
        vec![erased(T0 + 4000, json!({"sessions": 250}))],
    );
    assert_eq!(job(&out).state, WorkState::Done);
    assert_eq!(
        job(&out).attention.label,
        "done · 250 sessions, 900 nodes tombstoned"
    );
}

/// Topics and people are a run of frames, each saying how far it is.
#[test]
fn topics_and_people_are_runs_of_frames() {
    let mut w = WorkBoard::default();
    let topics = |at: u64, done: u64| {
        row(
            "import.topics",
            at,
            json!({"tag": "cc-1", "by": "cli", "made": 2, "joined": 10, "taken": 0, "retired": 0,
                   "done": done, "of": 20}),
        )
    };
    let out = feed(&mut w, 1, T0, vec![topics(T0, 12)]);
    assert_eq!(job(&out).title, "Placing the import cc-1 in its topics");
    assert_eq!(job(&out).attention.label, "12 of 20 records");
    let out = feed(&mut w, 2, T0 + 1000, vec![topics(T0 + 1000, 20)]);
    assert_eq!(job(&out).state, WorkState::Done);
    let back = row(
        "import.people",
        T0 + 2000,
        json!({"tag": "cc-1", "by": "cli", "made": 0, "joined": 0, "retired": 3}),
    );
    let out = feed(&mut w, 3, T0 + 2000, vec![back]);
    assert_eq!(job(&out).title, "Taking back the import cc-1's people");
    assert_eq!(job(&out).state, WorkState::Done);
}

/// A job whose rows stop is cancelled at the next frame after the quiet
/// time, and an ended one leaves an hour after it ended, taking the root.
#[test]
fn a_silent_job_stops_and_an_old_one_leaves_with_the_root() {
    let mut w = WorkBoard::default();
    feed(&mut w, 1, T0, vec![batch(T0, 2, 9, "claude-code")]);
    let out = feed(&mut w, 2, T0 + QUIET_MS + 1, vec![]);
    let v = job(&out);
    assert_eq!(v.state, WorkState::Cancelled);
    assert_eq!(v.attention.label, "stopped · at batch 2 of 9");
    assert!(out.notices.is_empty(), "{:?}", out.notices);
    // Still there at the hour, gone after it; the root with it.
    let out = feed(&mut w, 3, T0 + KEEP_MS - 1, vec![]);
    assert!(out.gone.is_empty());
    let out = feed(&mut w, 4, T0 + KEEP_MS + QUIET_MS, vec![]);
    let gone: Vec<&str> = out.gone.iter().map(|(id, _)| id.as_str()).collect();
    assert!(gone.contains(&ROOT), "{gone:?}");
    assert!(w.views.is_empty());
}

/// System work is the owner's: the snapshot gives the root to a scope that
/// sees the whole board, and to no scope that names its roots or reads no
/// private text, whatever it names.
#[test]
fn a_shared_place_never_sees_the_theseus_root() {
    let mut w = WorkBoard::default();
    feed(&mut w, 1, T0, vec![batch(T0, 1, 3, "claude-code")]);
    let ids = |s: &Scope| -> Vec<String> {
        w.snapshot(50, s)
            .1
            .into_iter()
            .map(|v| v.id)
            .collect::<Vec<_>>()
    };
    let whole = ids(&Scope::default());
    assert_eq!(whole.len(), 2, "the root and its job: {whole:?}");
    assert!(whole.contains(&ROOT.to_string()));
    for scope in [
        Scope {
            roots: Some(["exe_a".to_string()].into()),
            guarded: true,
        },
        Scope {
            roots: Some([ROOT.to_string()].into()),
            guarded: true,
        },
    ] {
        assert!(ids(&scope).is_empty(), "{scope:?}");
        assert!(!scope.sees(ROOT) && !scope.sees("anything under system"));
        assert_eq!(
            scope.gone(&[("x".into(), ROOT.into())]),
            Vec::<String>::new()
        );
    }
}

/// A scope that reads private text and names `system` has the root and its
/// jobs and nothing else, so a client that wants only the daemon's work does
/// not take the whole board.
#[test]
fn a_private_scope_may_ask_for_the_system_root_alone() {
    let mut w = WorkBoard::default();
    feed(&mut w, 1, T0, vec![batch(T0, 1, 3, "claude-code")]);
    let only = Scope {
        roots: Some([ROOT.to_string()].into()),
        guarded: false,
    };
    let got: Vec<String> = w.snapshot(0, &only).1.into_iter().map(|v| v.id).collect();
    assert_eq!(got.len(), 2, "{got:?}");
    assert!(only.sees(ROOT) && !only.sees("exe_a"));
}

/// After a restart the last hour's rows make the same jobs, and tell no
/// one a thing they were not told.
#[test]
fn the_last_hours_rows_make_the_jobs_again_and_no_notice() {
    let mut w = WorkBoard::default();
    let rows = vec![
        batch(T0, 1, 2, "claude-code"),
        batch(T0 + 1000, 2, 2, "claude-code"),
        row(
            "import.synced",
            T0 + 2000,
            json!({"by": "timer", "started_ms": T0, "imported": 0, "appended": 0, "error": "boom"}),
        ),
    ];
    let out = w.warm_system(rows, T0 + 3000).expect("views");
    assert!(out.notices.is_empty());
    assert!(w.notices_since(0).0.is_empty());
    let jobs: Vec<&WorkView> = w.views.values().filter(|v| v.id != ROOT).collect();
    assert_eq!(jobs.len(), 2);
    assert!(jobs.iter().any(|v| v.state == WorkState::Done));
    assert!(jobs.iter().any(|v| v.state == WorkState::Failed));
}
