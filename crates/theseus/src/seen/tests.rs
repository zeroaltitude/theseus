//! The seen file shared by the TUI and the CLI (theseus-yus0).

use std::io;
use std::path::Path;

use serde_json::json;
use theseus_protocol::ExecutionView;

use super::*;

fn view(exec: &str, position: u64, level: &str, turns: u64) -> ExecutionView {
    serde_json::from_value(json!({
        "position": position,
        "execution_id": exec,
        "session_id": format!("ses_{exec}"),
        "kind": "conversation",
        "state": "waiting",
        "turns": turns,
        "attention": {"level": level, "label": "", "since_ms": 0},
    }))
    .unwrap()
}

fn mark(displayed: u64, finished: u64, position: u64) -> Mark {
    Mark {
        displayed,
        finished,
        position,
        level: Level::Ready,
        turns: 1,
    }
}

fn disk(path: &Path) -> File {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// The TUI saw a session work and finish; it is done until seen.
fn tui_with_a_finished_session(path: &Path) -> Seen {
    let mut s = Seen::open(Some(path.to_path_buf()));
    s.applied(&view("a", 10, "working", 1), false);
    s.first_board_read();
    s.applied(&view("a", 20, "ready", 1), false);
    assert!(s.done(&view("a", 20, "ready", 1)));
    let text = s.text(&|_| true);
    write_merged(path, &text).unwrap();
    s
}

#[test]
fn a_session_the_cli_showed_is_not_done_until_seen_in_the_tui() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("theseus").join("seen.json");
    tui_with_a_finished_session(&path);
    // `theseus history` printed it, at the position it stood at.
    record(&path, &[view("a", 20, "ready", 1)]).unwrap();
    // The TUI starts again: the session is not new to it.
    let s = Seen::open(Some(path));
    assert!(!s.done(&view("a", 20, "ready", 1)));
}

#[test]
fn what_the_tui_did_not_see_is_still_done_beside_what_the_cli_showed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("seen.json");
    tui_with_a_finished_session(&path);
    record(&path, &[view("b", 5, "ready", 1)]).unwrap();
    let s = Seen::open(Some(path));
    assert!(s.done(&view("a", 20, "ready", 1)), "a was not shown");
    assert!(!s.done(&view("b", 5, "ready", 1)));
}

#[test]
fn a_file_made_by_the_cli_before_any_tui_still_shows_a_first_start_nothing_done() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("seen.json");
    record(&path, &[view("a", 20, "ready", 1)]).unwrap();
    let mut s = Seen::open(Some(path));
    let old = view("old", 10, "ready", 3);
    s.applied(&old, false);
    assert!(!s.done(&old), "the first board is seen as it is");
}

#[test]
fn two_writers_merge_by_the_greatest_position() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("seen.json");
    // The TUI holds a mark at 20; the CLI reads and records 30; the TUI
    // writes its older view after: nothing the CLI recorded is lost.
    let mut tui = Seen::open(Some(path.clone()));
    tui.applied(&view("a", 10, "working", 1), false);
    tui.first_board_read();
    tui.applied(&view("a", 20, "ready", 1), false);
    let tui_text = tui.text(&|_| true);
    record(
        &path,
        &[view("a", 30, "ready", 2), view("c", 7, "ready", 1)],
    )
    .unwrap();
    write_merged(&path, &tui_text).unwrap();
    let f = disk(&path);
    let a = f.executions["a"];
    assert_eq!((a.displayed, a.finished, a.position), (30, 20, 30));
    assert_eq!(a.turns, 2, "the view of the newer position");
    assert_eq!(f.executions["c"].displayed, 7, "the CLI's other session");
    // And the other way round: an older record never lowers a position.
    record(&path, &[view("a", 25, "ready", 1)]).unwrap();
    assert_eq!(disk(&path).executions["a"].displayed, 30);
    // The TUI takes up what the file holds.
    let merged = write_merged(&path, &tui_text).unwrap();
    tui.adopt(merged);
    assert!(!tui.done(&view("a", 30, "ready", 2)));
}

#[test]
fn many_writers_at_once_lose_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("seen.json");
    let writers: Vec<_> = (0..8u64)
        .map(|w| {
            let path = path.clone();
            std::thread::spawn(move || {
                for i in 0..10u64 {
                    record(
                        &path,
                        &[view(&format!("e{w}_{i}"), w * 100 + i + 1, "ready", 1)],
                    )
                    .unwrap();
                }
            })
        })
        .collect();
    for w in writers {
        w.join().unwrap();
    }
    assert_eq!(disk(&path).executions.len(), 80);
}

#[test]
fn a_reader_during_writes_sees_the_old_file_or_the_new_one_whole() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("seen.json");
    // A file big enough that a write is more than one disk block.
    let big: Vec<ExecutionView> = (0..3000)
        .map(|i| view(&format!("e{i}"), 1, "ready", 1))
        .collect();
    record(&path, &big).unwrap();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let writer = {
        let (path, stop) = (path.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut n = 2;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                let more: Vec<ExecutionView> = (0..3000)
                    .map(|i| view(&format!("e{i}"), n, "ready", 1))
                    .collect();
                record(&path, &more).unwrap();
                n += 1;
            }
        })
    };
    for _ in 0..300 {
        let text = std::fs::read_to_string(&path).unwrap();
        let f: File = serde_json::from_str(&text).expect("a whole file");
        assert_eq!(f.executions.len(), 3000);
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    writer.join().unwrap();
}

#[test]
fn a_failed_write_leaves_the_old_file_and_no_temporary() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("seen.json");
    record(&path, &[view("a", 10, "ready", 1)]).unwrap();
    let before = std::fs::read_to_string(&path).unwrap();
    let refuse = |_: &Path, _: &Path| Err(io::Error::other("refused"));
    let e = record_with(&path, &[view("a", 99, "ready", 1)], &refuse).unwrap_err();
    assert_eq!(e.to_string(), "refused");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    let left: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.ends_with(".tmp"))
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

#[test]
fn the_older_tui_file_is_read_when_the_new_one_is_absent() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("tui-seen.json");
    let text = serde_json::to_string(&File {
        version: VERSION,
        board_read: true,
        executions: HashMap::from([("a".to_string(), mark(3, 9, 9))]),
        forget: Vec::new(),
    })
    .unwrap();
    // The older file as the TUI wrote it: no `board_read`.
    let text = text.replace("\"board_read\":true,", "");
    std::fs::write(&old, text).unwrap();
    let path = dir.path().join("seen.json");
    let s = Seen::open(Some(path.clone()));
    assert!(s.done(&view("a", 9, "ready", 1)), "its marks were read");
    // The next write is to the new name, and keeps the old marks.
    record(&path, &[view("b", 4, "ready", 1)]).unwrap();
    let f = disk(&path);
    assert_eq!(f.executions["a"].finished, 9);
    assert!(f.board_read, "the TUI's file had read a board");
    assert!(old.exists(), "the old file is left alone");
}

#[test]
fn the_new_file_wins_over_the_old_one() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("tui-seen.json"),
        r#"{"version":1,"executions":{"a":{"displayed":0,"finished":9,"position":9,"level":"ready","turns":1}}}"#,
    )
    .unwrap();
    let path = dir.path().join("seen.json");
    record(&path, &[view("a", 12, "ready", 1)]).unwrap();
    let s = Seen::open(Some(path));
    assert!(!s.done(&view("a", 12, "ready", 1)));
}

#[test]
fn the_tui_forgets_what_its_board_dropped_but_keeps_what_only_the_cli_knows() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("seen.json");
    let mut tui = Seen::open(Some(path.clone()));
    tui.applied(&view("gone", 10, "ready", 1), false);
    tui.applied(&view("kept", 10, "ready", 1), false);
    write_merged(&path, &tui.text(&|_| true)).unwrap();
    record(&path, &[view("cli_only", 3, "ready", 1)]).unwrap();
    write_merged(&path, &tui.text(&|id| id != "gone")).unwrap();
    let f = disk(&path);
    let mut ids: Vec<_> = f.executions.keys().cloned().collect();
    ids.sort();
    assert_eq!(ids, ["cli_only", "kept"]);
    assert!(f.forget.is_empty(), "never written to disk");
}

#[test]
fn the_first_write_into_a_new_directory_is_locked_too() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state").join("theseus").join("seen.json");
    record(&path, &[view("a", 1, "ready", 1)]).unwrap();
    assert!(
        path.with_file_name("seen.json.lock").exists(),
        "the lock file was taken in a directory made for it"
    );
}

#[test]
fn done_count_is_the_finishes_not_yet_displayed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("seen.json");
    assert_eq!(done_count(&path), 0, "no file, none done");
    tui_with_a_finished_session(&path);
    assert_eq!(done_count(&path), 1);
    // The CLI showed it: nothing is done.
    record(&path, &[view("a", 20, "ready", 1)]).unwrap();
    assert_eq!(done_count(&path), 0);
}
