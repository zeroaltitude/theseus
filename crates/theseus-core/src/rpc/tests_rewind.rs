//! Rewind (theseus-23rk, R20) through the whole core: an edit tool's files
//! are kept before it runs, `files.changes` lists each message with the files
//! changed since it, and `files.restore` puts back what the session's edits
//! changed since a message and nothing before it, only where a file still
//! holds what the last edit wrote; a restore is itself undone by a rewind to
//! the position before it; the fork holds the conversation up to the node
//! before the message; and a crash between the pre-image and the edit leaves
//! the store consistent.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::{
    error_code, FilesChangesResult, FilesRestoreResult, Message, Request, Response,
    SessionForkResult, SessionKind,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::approval::{Client, Surface};
use crate::bus::EventSink;
use crate::provider::{FakeProvider, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

struct Rig {
    core: Arc<Core>,
    root: PathBuf,
    sid: String,
    _dir: tempfile::TempDir,
}

fn rig(script: impl FnOnce(&Path) -> Vec<Scripted>) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.memory.mode = crate::config::MemoryMode::Off;
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script(&root)));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake, store)).unwrap();
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    Rig {
        core,
        root,
        sid: rec.session_id,
        _dir: dir,
    }
}

async fn say(r: &Rig, input: &str) {
    let rec = r
        .core
        .store
        .get_session::<SessionRecord>(&r.sid)
        .unwrap()
        .unwrap();
    let (live, _) = r.core.live_profile();
    let target = r
        .core
        .runner
        .resolve_target(&live, None, None, None)
        .unwrap();
    let sink = EventSink::new(r.core.bus.clone(), &rec.session_id, None);
    r.core
        .runner
        .run(TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
        .unwrap();
}

fn edit(id: &str, path: &Path, old: &str, new: &str) -> Scripted {
    Scripted::tools(
        "",
        &[(
            id,
            "fs_edit",
            json!({"path": path, "old_string": old, "new_string": new}),
        )],
    )
}

fn write(id: &str, path: &Path, content: &str) -> Scripted {
    Scripted::tools(
        "",
        &[(id, "fs_write", json!({"path": path, "content": content}))],
    )
}

/// One request on a connection of `client`'s surface, its whole response.
async fn call(core: &Arc<Core>, client: Client, method: &str, params: Value) -> Response {
    let (ours, theirs) = tokio::io::duplex(64 * 1024);
    let (sr, sw) = tokio::io::split(theirs);
    let srv = tokio::spawn(core.clone().serve_connection(sr, sw, client));
    let (cr, mut cw) = tokio::io::split(ours);
    let req = Request::new(theseus_protocol::Id::Num(1), method, params);
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = BufReader::new(cr).lines();
    let out = loop {
        let l = lines.next_line().await.unwrap().unwrap();
        if let Message::Response(r) = serde_json::from_str(&l).unwrap() {
            break r;
        }
    };
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = srv.await;
    out
}

fn cli() -> Client {
    Client::new("sock#1", Surface::Cli)
}

async fn changes(r: &Rig) -> FilesChangesResult {
    let resp = call(
        &r.core,
        cli(),
        "files.changes",
        json!({"session_id": r.sid}),
    )
    .await;
    serde_json::from_value(resp.result.unwrap_or_else(|| panic!("{:?}", resp.error))).unwrap()
}

async fn restore(r: &Rig, since: &str, dry_run: bool) -> FilesRestoreResult {
    let resp = call(
        &r.core,
        cli(),
        "files.restore",
        json!({"session_id": r.sid, "since_node": since, "dry_run": dry_run}),
    )
    .await;
    serde_json::from_value(resp.result.unwrap_or_else(|| panic!("{:?}", resp.error))).unwrap()
}

fn outcome<'a>(r: &'a FilesRestoreResult, name: &str) -> &'a theseus_protocol::RestoredFile {
    r.files
        .iter()
        .find(|f| f.path.ends_with(name))
        .unwrap_or_else(|| panic!("{name} is not in {:?}", r.files))
}

/// `one` is edited to `two` in the first message, to `three` in the second,
/// which also writes `fresh.txt`.
async fn two_messages() -> Rig {
    let r = rig(|root| {
        vec![
            edit("c1", &root.join("a.txt"), "one", "two"),
            Scripted::text("edited once"),
            edit("c2", &root.join("a.txt"), "two", "three"),
            write("c3", &root.join("fresh.txt"), "fresh\n"),
            Scripted::text("edited twice"),
        ]
    });
    std::fs::write(r.root.join("a.txt"), "one\n").unwrap();
    say(&r, "first message").await;
    say(&r, "second message").await;
    r
}

fn read(r: &Rig, name: &str) -> Option<String> {
    std::fs::read_to_string(r.root.join(name)).ok()
}

#[tokio::test]
async fn the_changes_since_each_message_are_listed_newest_first() {
    let r = two_messages().await;
    let c = changes(&r).await;
    let texts: Vec<_> = c.points.iter().map(|p| p.text.as_str()).collect();
    assert_eq!(texts, ["second message", "first message"]);
    let (second, first) = (&c.points[0], &c.points[1]);
    let names = |p: &theseus_protocol::RewindPoint| {
        let mut v: Vec<_> = p
            .files
            .iter()
            .map(|f| (f.path.rsplit('/').next().unwrap().to_string(), f.created))
            .collect();
        v.sort();
        v
    };
    assert_eq!(
        names(second),
        [
            ("a.txt".to_string(), false),
            ("fresh.txt".to_string(), true)
        ]
    );
    // The first message sees both edits of a.txt, summed, and the new file.
    let a = first
        .files
        .iter()
        .find(|f| f.path.ends_with("a.txt"))
        .unwrap();
    assert_eq!((a.added, a.removed), (2, 2));
    assert_eq!(first.files.len(), 2);
    // The first message has nothing before it to fork at; the second can.
    assert_eq!(first.fork_at, None);
    assert!(second.fork_at.is_some());
    assert_eq!(second.message, "second message");
}

#[tokio::test]
async fn a_restore_puts_back_what_the_edits_changed_since_the_message_and_nothing_before_it() {
    let r = two_messages().await;
    let c = changes(&r).await;
    let (second, first) = (&c.points[0], &c.points[1]);

    // A dry run says what would happen and writes nothing.
    let dry = restore(&r, &second.node_id, true).await;
    assert_eq!(outcome(&dry, "a.txt").outcome, "would restore");
    assert_eq!(outcome(&dry, "fresh.txt").outcome, "would remove");
    assert_eq!(read(&r, "a.txt").as_deref(), Some("three\n"));

    let done = restore(&r, &second.node_id, false).await;
    assert_eq!(outcome(&done, "a.txt").outcome, "restored");
    assert_eq!(outcome(&done, "fresh.txt").outcome, "removed");
    // Back to after the first message's edit, not before it.
    assert_eq!(read(&r, "a.txt").as_deref(), Some("two\n"));
    assert_eq!(read(&r, "fresh.txt"), None);

    let all = restore(&r, &first.node_id, false).await;
    assert_eq!(outcome(&all, "a.txt").outcome, "restored");
    assert_eq!(read(&r, "a.txt").as_deref(), Some("one\n"));
}

#[tokio::test]
async fn a_file_changed_by_hand_after_the_edit_is_skipped_and_listed() {
    let r = two_messages().await;
    let c = changes(&r).await;
    std::fs::write(r.root.join("a.txt"), "three\nand a line of mine\n").unwrap();
    let done = restore(&r, &c.points[0].node_id, false).await;
    let a = outcome(&done, "a.txt");
    assert_eq!(a.outcome, "skipped");
    assert!(a.why.contains("changed since"), "{}", a.why);
    // Left exactly as the hand made it; the file Theseus made was restored.
    assert_eq!(
        read(&r, "a.txt").as_deref(),
        Some("three\nand a line of mine\n")
    );
    assert_eq!(read(&r, "fresh.txt"), None);
    assert!(done.words.contains("skipped"), "{}", done.words);
}

#[tokio::test]
async fn a_file_only_a_command_changed_is_never_touched() {
    let r = two_messages().await;
    let c = changes(&r).await;
    // A program rewrote a file the session never edited, and made a new one.
    std::fs::write(r.root.join("log.txt"), "built\n").unwrap();
    std::fs::write(r.root.join("b.txt"), "by a command\n").unwrap();
    let done = restore(&r, &c.points[1].node_id, false).await;
    assert!(done.files.iter().all(|f| !f.path.ends_with("log.txt")));
    assert!(done.files.iter().all(|f| !f.path.ends_with("b.txt")));
    assert_eq!(read(&r, "log.txt").as_deref(), Some("built\n"));
    assert_eq!(read(&r, "b.txt").as_deref(), Some("by a command\n"));
}

#[tokio::test]
async fn a_file_made_by_the_session_and_changed_since_is_listed_not_removed() {
    let r = two_messages().await;
    let c = changes(&r).await;
    std::fs::write(r.root.join("fresh.txt"), "fresh\nmore\n").unwrap();
    let done = restore(&r, &c.points[0].node_id, false).await;
    assert_eq!(outcome(&done, "fresh.txt").outcome, "skipped");
    assert_eq!(read(&r, "fresh.txt").as_deref(), Some("fresh\nmore\n"));
}

#[tokio::test]
async fn a_file_over_a_mebibyte_is_noted_not_kept() {
    let r = rig(|root| {
        vec![
            edit("c1", &root.join("big.txt"), "needle", "thread"),
            Scripted::text("done"),
            edit("c2", &root.join("a.txt"), "one", "two"),
            Scripted::text("done"),
        ]
    });
    let big = format!(
        "needle\n{}",
        "x".repeat((crate::rewind::MAX_KEPT as usize) + 10)
    );
    std::fs::write(r.root.join("big.txt"), &big).unwrap();
    std::fs::write(r.root.join("a.txt"), "one\n").unwrap();
    say(&r, "edit the big one").await;
    say(&r, "and a small one").await;
    let c = changes(&r).await;
    let done = restore(&r, &c.points[1].node_id, false).await;
    let b = outcome(&done, "big.txt");
    assert_eq!(b.outcome, "skipped");
    assert!(b.why.contains("1 MiB"), "{}", b.why);
    assert!(read(&r, "big.txt").unwrap().starts_with("thread\n"));
    assert_eq!(outcome(&done, "a.txt").outcome, "restored");
}

#[tokio::test]
async fn a_restore_is_undone_by_a_rewind_to_the_position_before_it() {
    let r = two_messages().await;
    let c = changes(&r).await;
    let done = restore(&r, &c.points[0].node_id, false).await;
    assert_eq!(read(&r, "a.txt").as_deref(), Some("two\n"));
    let undo = restore(&r, &format!("@{}", done.undo_since), false).await;
    assert_eq!(outcome(&undo, "a.txt").outcome, "restored");
    assert_eq!(read(&r, "a.txt").as_deref(), Some("three\n"));
    assert_eq!(read(&r, "fresh.txt").as_deref(), Some("fresh\n"));
}

#[tokio::test]
async fn the_fork_holds_the_conversation_up_to_the_node_before_the_message() {
    let r = two_messages().await;
    let c = changes(&r).await;
    let second = &c.points[0];
    let resp = call(
        &r.core,
        cli(),
        "session.fork",
        json!({"session": r.sid, "at": second.fork_at}),
    )
    .await;
    let f: SessionForkResult = serde_json::from_value(resp.result.unwrap()).unwrap();
    let t = r.core.store.transcript(&f.session_id).unwrap();
    let texts: Vec<String> = t
        .iter()
        .filter_map(|(_, s)| match &s.body {
            crate::node::Body::UserMessage { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(texts, ["first message"]);
    // The fork's own edits are the rewind's too: it sees the source's first
    // message's edit through the fork point.
    let fc = call(
        &r.core,
        cli(),
        "files.changes",
        json!({"session_id": f.session_id}),
    )
    .await;
    let fc: FilesChangesResult = serde_json::from_value(fc.result.unwrap()).unwrap();
    assert_eq!(fc.points.len(), 1);
    assert_eq!(fc.points[0].files.len(), 1);
}

#[tokio::test]
async fn a_surface_that_reads_no_private_text_is_refused() {
    let r = two_messages().await;
    let resp = call(
        &r.core,
        Client::new("discord", Surface::Discord),
        "files.changes",
        json!({"session_id": r.sid}),
    )
    .await;
    assert_eq!(resp.error.unwrap().code, error_code::REFUSED);
}

#[tokio::test]
async fn a_crash_between_the_pre_image_and_the_edit_leaves_the_store_consistent() {
    let r = two_messages().await;
    let c = changes(&r).await;
    let before = r.core.store.last_position();
    // The blob is durable, then the process dies: no edit, no record.
    let tool = theseus_tools::fs::Edit;
    let ctx = theseus_tools::ToolCtx::for_tests(&r.root);
    let input = json!({"path": r.root.join("a.txt"), "old_string": "three", "new_string": "four"});
    let held = crate::rewind::hold(
        &crate::rewind::blobs_in(r.core.store.dir()),
        &tool,
        &input,
        &ctx,
    );
    assert!(!held.is_empty());
    drop(held);
    assert_eq!(r.core.store.last_position(), before);
    // And a restore still works as it did.
    let done = restore(&r, &c.points[0].node_id, false).await;
    assert_eq!(outcome(&done, "a.txt").outcome, "restored");
    assert_eq!(read(&r, "a.txt").as_deref(), Some("two\n"));
}

#[tokio::test]
async fn an_edit_with_no_record_is_a_change_since_and_the_file_is_left_alone() {
    // The edit ran and the process died before its frame: the file holds
    // bytes the last record never named.
    let r = two_messages().await;
    let c = changes(&r).await;
    std::fs::write(r.root.join("a.txt"), "four\n").unwrap();
    let done = restore(&r, &c.points[0].node_id, false).await;
    assert_eq!(outcome(&done, "a.txt").outcome, "skipped");
    assert_eq!(read(&r, "a.txt").as_deref(), Some("four\n"));
}

#[tokio::test]
async fn an_erased_sessions_blobs_go_with_it() {
    let r = two_messages().await;
    let dir = crate::rewind::blobs_in(r.core.store.dir())
        .dir()
        .to_path_buf();
    let n = || std::fs::read_dir(&dir).map_or(0, |d| d.count());
    assert!(n() >= 2, "the pre-images are in {}", dir.display());
    let gone = crate::rewind::forget(&r.core.store, &r.sid).unwrap();
    assert!(gone >= 2);
    assert_eq!(n(), 0);
}
