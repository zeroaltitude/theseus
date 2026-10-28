//! A file tool's diff on `tool.ended` and a question's preview (theseus-z6au):
//! the preview of an edit is the diff its run makes, a planted secret shows in
//! no question, the ended diff is the node's own, and the diff is capped.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::{notify, Message, SessionKind, ToolDiff, TurnSubmitResult};
use tokio::sync::mpsc::UnboundedReceiver;

use crate::bus::EventSink;
use crate::provider::{FakeProvider, Scripted};
use crate::scrub::Scrubber;
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

struct Rig {
    core: Arc<Core>,
    /// Inside the workspace's roots: writes run without a question.
    root: PathBuf,
    /// Outside the roots and on the approve list: a write here waits.
    guarded: PathBuf,
    sid: String,
    rx: UnboundedReceiver<Message>,
    _dir: tempfile::TempDir,
}

const SECRET: &str = "tide-gauge-key-7f3a91c2e8";

fn rig(script: impl FnOnce(&Path, &Path) -> Vec<Scripted>, secret: bool) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    let guarded = dir.path().join("guarded");
    for d in [&root, &guarded] {
        std::fs::create_dir_all(d).unwrap();
    }
    let (root, guarded) = (
        root.canonicalize().unwrap(),
        guarded.canonicalize().unwrap(),
    );
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.tools.approve_paths = vec![guarded.to_string_lossy().into_owned()];
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script(&root, &guarded)));
    let mut parts = crate::rpc::Parts::for_tests(cfg, fake, store);
    if secret {
        parts.scrubber = Arc::new(Scrubber::with_values(vec![(
            SECRET.into(),
            "gauge_key".into(),
        )]));
    }
    let core = Core::build(parts).unwrap();
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    core.bus.watch(&rec.session_id, "watcher", tx);
    Rig {
        core,
        root,
        guarded,
        sid: rec.session_id,
        rx,
        _dir: dir,
    }
}

async fn say(r: &Rig, input: &str) -> TurnSubmitResult {
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
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            surface: None,
            answer_at: None,
        })
        .await
        .unwrap()
}

/// Every notification the session's watchers heard since the last call.
fn drain(rx: &mut UnboundedReceiver<Message>) -> Vec<(String, Value)> {
    let mut out = Vec::new();
    while let Ok(Message::Notification(n)) = rx.try_recv() {
        out.push((n.method, n.params));
    }
    out
}

/// What `drain` heard of one `method`.
fn of(all: &[(String, Value)], method: &str) -> Vec<Value> {
    all.iter()
        .filter(|(m, _)| m == method)
        .map(|(_, p)| p.clone())
        .collect()
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

fn ended_diff(v: &Value) -> ToolDiff {
    serde_json::from_value(v["diff"].clone()).expect("the event carries a diff")
}

#[tokio::test]
async fn an_edits_question_previews_the_diff_its_run_makes() {
    let mut r = rig(
        |_, guarded| {
            vec![
                edit(
                    "e1",
                    &guarded.join("tides.py"),
                    "lowest - highest",
                    "highest - lowest",
                ),
                Scripted::text("done"),
            ]
        },
        false,
    );
    std::fs::write(
        r.guarded.join("tides.py"),
        "def r(h):\n    return lowest - highest\n",
    )
    .unwrap();
    let res = say(&r, "fix it").await;
    let asked = of(&drain(&mut r.rx), notify::CONFIRM_REQUESTED);
    let preview = asked[0]["preview"].clone();
    assert_eq!(preview["kind"], "diff", "{asked:?}");
    r.core
        .confirm_action(res.awaiting_confirm.as_deref().unwrap(), true, None, "test")
        .unwrap();
    r.core
        .continue_execution(res.execution_id.as_deref().unwrap())
        .await
        .unwrap()
        .unwrap();
    let ended = of(&drain(&mut r.rx), notify::TOOL_ENDED);
    let diff = ended_diff(&ended[0]);
    assert_eq!(
        preview["body"], diff.unified,
        "the preview is the diff the run made"
    );
    assert_eq!((diff.added, diff.removed, diff.truncated), (1, 1, false));
    assert!(diff
        .unified
        .contains("-    return lowest - highest\n+    return highest - lowest\n"));
}

/// The preview is made from the file as it is when the call is planned: a
/// file that changed before the question was asked is what it shows.
#[tokio::test]
async fn a_preview_reads_the_file_as_it_is_when_planned() {
    let mut r = rig(
        |_, guarded| {
            vec![
                edit("e1", &guarded.join("a.txt"), "two", "2"),
                Scripted::text("done"),
            ]
        },
        false,
    );
    std::fs::write(r.guarded.join("a.txt"), "one\ntwo\nthree\n").unwrap();
    say(&r, "edit").await;
    let body = of(&drain(&mut r.rx), notify::CONFIRM_REQUESTED)[0]["preview"]["body"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        body.contains("-two\n+2\n") && body.contains(" one\n"),
        "{body}"
    );
}

#[tokio::test]
async fn a_planted_secret_shows_in_no_preview_and_the_ended_diff_is_the_nodes() {
    let mut r = rig(
        |root, guarded| {
            vec![
                edit("e1", &guarded.join("cfg.txt"), "mode = old", "mode = new"),
                Scripted::tools(
                    "",
                    &[(
                        "w1",
                        "fs_write",
                        json!({"path": root.join("w.txt"), "content": format!("key = {SECRET}\n")}),
                    )],
                ),
                Scripted::text("done"),
            ]
        },
        true,
    );
    std::fs::write(
        r.guarded.join("cfg.txt"),
        format!("key = {SECRET}\nmode = old\n"),
    )
    .unwrap();
    let res = say(&r, "edit").await;
    r.core
        .confirm_action(res.awaiting_confirm.as_deref().unwrap(), true, None, "test")
        .unwrap();
    r.core
        .continue_execution(res.execution_id.as_deref().unwrap())
        .await
        .unwrap()
        .unwrap();
    let all = drain(&mut r.rx);
    let (asked, ended) = (
        of(&all, notify::CONFIRM_REQUESTED),
        of(&all, notify::TOOL_ENDED),
    );
    assert!(
        !asked.is_empty() && ended.len() >= 2,
        "{} {}",
        asked.len(),
        ended.len()
    );
    for v in &asked {
        assert!(
            !v.to_string().contains(SECRET),
            "a secret reached a question: {v}"
        );
    }
    // The write's diff is there, as the node holds it: the record is raw, and
    // the `tool.ended` event is the node's. The question's preview is words
    // for an approval, and stays scrubbed.
    let w = ended
        .iter()
        .find(|e| e["tool"] == "fs.write")
        .expect("the write ended");
    assert!(ended_diff(w).unified.contains(&format!("+key = {SECRET}")));
}

#[tokio::test]
async fn writes_end_with_their_diffs() {
    let mut r = rig(
        |root, _| {
            vec![
                Scripted::tools(
                    "",
                    &[(
                        "w1",
                        "fs_write",
                        json!({"path": root.join("new.txt"), "content": "a\nb\n"}),
                    )],
                ),
                Scripted::tools(
                    "",
                    &[(
                        "w2",
                        "fs_write",
                        json!({"path": root.join("tides.py"), "content": "x = 1\ny = 3\n"}),
                    )],
                ),
                Scripted::text("done"),
            ]
        },
        false,
    );
    std::fs::write(r.root.join("tides.py"), "x = 1\ny = 2\n").unwrap();
    say(&r, "go").await;
    let ended = of(&drain(&mut r.rx), notify::TOOL_ENDED);
    let w = ended
        .iter()
        .find(|e| e["tool"] == "fs.write")
        .expect("write");
    let d = ended_diff(w);
    assert_eq!((d.added, d.removed), (2, 0));
    assert!(
        d.unified.starts_with("--- /dev/null\n+++ "),
        "a new file: {}",
        d.unified
    );
    // A write over a file diffs against the old text.
    let over = ended
        .iter()
        .find(|e| {
            e["tool"] == "fs.write"
                && e["diff"]["unified"]
                    .as_str()
                    .is_some_and(|u| u.contains("-y = 2"))
        })
        .expect("the overwrite");
    assert_eq!((ended_diff(over).added, ended_diff(over).removed), (1, 1));
}

#[tokio::test]
async fn a_long_diff_is_cut_at_the_cap_and_says_so() {
    let big: String = (0..3000).map(|i| format!("line number {i}\n")).collect();
    let mut r = rig(
        |root, _| {
            vec![
                Scripted::tools(
                    "",
                    &[(
                        "w1",
                        "fs_write",
                        json!({"path": root.join("big.txt"), "content": big}),
                    )],
                ),
                Scripted::text("done"),
            ]
        },
        false,
    );
    say(&r, "go").await;
    let ended = of(&drain(&mut r.rx), notify::TOOL_ENDED);
    let d = ended_diff(ended.iter().find(|e| e["tool"] == "fs.write").unwrap());
    assert!(d.truncated && d.unified.len() <= theseus_protocol::diff::DIFF_CAP);
    assert_eq!(d.added, 3000, "counted over the whole diff");
}

/// An older daemon's `tool.ended` and `confirm.requested` have neither field.
#[test]
fn old_peers_decode_without_the_new_fields() {
    let e: theseus_protocol::ToolEnded =
        serde_json::from_value(json!({"tool": "fs.edit", "status": "ok"})).unwrap();
    assert!(e.diff.is_none());
    let bytes = serde_json::to_value(&e).unwrap();
    assert!(bytes.get("diff").is_none());
    let c: theseus_protocol::ConfirmRequest = serde_json::from_value(json!({
        "correlation_id": "c", "session_id": "s", "execution_id": "x", "tool": "fs.edit", "input": {},
        "reason": "r", "by": "b", "requested_at_ms": 0, "expires_at_ms": 0
    }))
    .unwrap();
    assert!(c.preview.is_none());
}
