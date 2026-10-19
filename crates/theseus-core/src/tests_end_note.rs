//! The end note (theseus-53ujg.1), through whole cores with the judge off, so
//! the stop nudge declines: a turn that would end beside the files its
//! commands made, or with edits no command ran since, is told once and goes
//! on once; never in a shared place, never with the switch off, never in a
//! turn that edited nothing, never twice. The turn's result names the files.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::json;
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::ledger::LedgerRow;
use crate::node::Body;
use crate::policy::Posture;
use crate::provider::{FakeProvider, Scripted};
use crate::rpc::{Core, Parts};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::end_note_step::END_NOTE_AUTHOR;
use crate::Config;

const SOMEONE: u64 = 222_222_222_222_222_222;
const OWNER: u64 = 271_828_182_845_904_523;

struct Rig {
    core: Arc<Core>,
    fake: Arc<FakeProvider>,
    root: PathBuf,
    _dir: tempfile::TempDir,
}

fn rig(script: impl FnOnce(&Path) -> Vec<Scripted>, tweak: impl FnOnce(&mut Config)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(root.join("src")).unwrap();
    let root = root.canonicalize().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().join("state").to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = Posture::Notify;
    for t in ["proc.run", "fs.write", "fs.edit", "fs.patch"] {
        cfg.policy.tools.remove(t);
    }
    cfg.places.owner = Some(vec![format!("discord:{OWNER}")]);
    cfg.judge.enabled = false;
    assert!(cfg.model.end_note, "on by default");
    tweak(&mut cfg);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script(&root)));
    let core = Core::build(Parts::for_tests(cfg, fake.clone(), store)).unwrap();
    Rig {
        core,
        fake,
        root,
        _dir: dir,
    }
}

impl Rig {
    fn session(&self, place: Option<&str>) -> String {
        let r = SessionRecord::new(SessionKind::Conversation, None);
        self.core.store.put_session(&r.session_id, &r).unwrap();
        if let Some(p) = place {
            self.core.outbox.bind_place(p, &r.session_id).unwrap();
        }
        r.session_id
    }

    async fn turn(&self, sid: &str, input: &str) -> TurnSubmitResult {
        crate::test_support::turn_in(&self.core, Some(sid), input).await
    }

    fn path(&self, rel: &str) -> String {
        self.root.join(rel).display().to_string()
    }

    fn rows(&self, kind: &str) -> usize {
        self.core
            .store
            .ledger_tail::<LedgerRow>(10_000)
            .unwrap()
            .into_iter()
            .filter(|(_, r)| r.kind == kind)
            .count()
    }

    /// The session's end notes, as the model reads them.
    fn notes(&self, sid: &str) -> Vec<String> {
        self.core
            .store
            .session_nodes(sid)
            .unwrap()
            .into_iter()
            .filter(|(_, n)| n.author.as_deref() == Some(END_NOTE_AUTHOR))
            .filter_map(|(_, n)| match n.body {
                Body::UserMessage { text, .. } => Some(text),
                _ => None,
            })
            .collect()
    }

    fn results(&self, sid: &str) -> Vec<String> {
        self.core
            .store
            .session_nodes(sid)
            .unwrap()
            .into_iter()
            .filter_map(|(_, n)| match n.body {
                Body::ToolResult { content, .. } => Some(content),
                _ => None,
            })
            .collect()
    }
}

fn write(path: &str, text: &str) -> Scripted {
    Scripted::tools(
        "Writing it.",
        &[("w1", "fs_write", json!({"path": path, "content": text}))],
    )
}

fn run_in(cwd: &str, shell: &str) -> Scripted {
    let input = json!({"argv": ["sh", "-c", shell], "cwd": cwd});
    Scripted::tools("Running it.", &[("r1", "proc_run", input)])
}

fn at(root: &Path, rel: &str) -> String {
    root.join(rel).display().to_string()
}

/// A turn writes `src/main.rs`, compiles beside it, and stops: it is told
/// once what lies beside its edits, goes on once, and a second stop ends it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_that_leaves_build_outputs_beside_its_file_is_told_once() {
    let r = rig(
        |root| {
            vec![
                write(&at(root, "src/main.rs"), "fn main() {}"),
                run_in(&at(root, "src"), "echo built > main"),
                Scripted::text("Done: src/main.rs works."),
                Scripted::text("Still done."),
                Scripted::text("A fifth answer no turn asks for."),
            ]
        },
        |_| {},
    );
    let sid = r.session(None);
    let res = r.turn(&sid, "Write src/main.rs.").await;
    assert_eq!(
        r.fake.requests().len(),
        4,
        "one note, one more call, and no more"
    );
    assert_eq!((res.loops, res.stop_reason.as_str()), (4, "no_tool_calls"));
    assert_eq!(
        r.notes(&sid),
        [format!(
            "[The harness: this turn would end here. Left beside its edits: {}.]",
            r.path("src/main")
        )]
    );
    assert_eq!(r.rows("turn.end_noted"), 1);
    let files = res.files.clone().expect("files");
    assert_eq!(files.left.paths, [r.path("src/main")]);
    assert_eq!(files.changed.paths, [r.path("src/main.rs")]);
    let note = files.end_note.expect("end note");
    assert_eq!(
        (note.left, note.unrun, note.loop_index, note.acted),
        (1, 0, 2, false)
    );
    let lines = res.harness_lines();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].starts_with("Files: changed "), "{lines:?}");
}

/// A command that creates two files gets the `[new files]` line with both,
/// after its header, sizes shown; one that creates none gets none.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_commands_result_names_its_new_files() {
    let r = rig(
        |root| {
            vec![
                run_in(
                    &at(root, "src"),
                    "echo aaaa > one.out; sleep 0.05; head -c 2048 /dev/zero > two.out",
                ),
                run_in(&at(root, "src"), "true"),
                Scripted::text("Done."),
            ]
        },
        |_| {},
    );
    let sid = r.session(None);
    // Nothing was written by a file tool: the note stays quiet, the result says.
    let res = r.turn(&sid, "Make two files.").await;
    assert_eq!(r.fake.requests().len(), 3, "no edits, no note");
    let results = r.results(&sid);
    assert!(
        results[0].contains(&format!(
            "[new files: {} (5 B), {} (2 KB)]\n",
            r.path("src/one.out"),
            r.path("src/two.out")
        )),
        "{}",
        results[0]
    );
    assert!(!results[1].contains("[new files"), "{}", results[1]);
    assert!(
        res.files.is_some_and(|f| f.left.paths.len() == 2),
        "the result names what the commands left"
    );
}

/// Edits made after the last command ran are named; a turn that edits and
/// never ran a command is not told.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn edits_after_the_last_command_are_named() {
    let edit = |root: &Path| {
        let input =
            json!({"path": at(root, "notes.txt"), "old_string": "one", "new_string": "two"});
        Scripted::tools("Editing.", &[("e1", "fs_edit", input)])
    };
    let r = rig(
        |root| {
            std::fs::write(root.join("notes.txt"), "one\n").unwrap();
            vec![
                run_in(&at(root, ""), "true"),
                edit(root),
                Scripted::text("Done."),
                Scripted::text("Done again."),
            ]
        },
        |_| {},
    );
    let sid = r.session(None);
    let res = r.turn(&sid, "Edit the notes.").await;
    assert_eq!(r.fake.requests().len(), 4);
    assert_eq!(
        r.notes(&sid),
        [format!(
            "[The harness: this turn would end here. Changed after the last command ran: {}.]",
            r.path("notes.txt")
        )]
    );
    let note = res.files.unwrap().end_note.unwrap();
    assert_eq!((note.left, note.unrun), (0, 1));

    // Edited, never ran a command: nothing to say.
    let r = rig(
        |root| {
            std::fs::write(root.join("notes.txt"), "one\n").unwrap();
            vec![edit(root), Scripted::text("Done.")]
        },
        |_| {},
    );
    let sid = r.session(None);
    let res = r.turn(&sid, "Edit the notes.").await;
    assert_eq!(r.fake.requests().len(), 2);
    assert!(r.notes(&sid).is_empty() && r.rows("turn.end_noted") == 0);
    assert_eq!(res.files.unwrap().changed.paths, [r.path("notes.txt")]);
}

/// A turn that edited nothing is never told, whatever its commands left.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_that_edited_nothing_is_not_told() {
    let r = rig(
        |root| {
            vec![
                run_in(&at(root, "src"), "echo x > out.bin"),
                Scripted::text("Done."),
            ]
        },
        |_| {},
    );
    let sid = r.session(None);
    r.turn(&sid, "Run it.").await;
    assert_eq!(r.fake.requests().len(), 2);
    assert!(r.notes(&sid).is_empty());
}

/// The switch off: the turn is the old turn, and its result still names the files.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_end_note_with_the_switch_off() {
    let r = rig(
        |root| {
            vec![
                write(&at(root, "src/main.rs"), "fn main() {}"),
                run_in(&at(root, "src"), "echo built > main"),
                Scripted::text("Done."),
                Scripted::text("A fourth answer no turn asks for."),
            ]
        },
        |c| c.model.end_note = false,
    );
    let sid = r.session(None);
    let res = r.turn(&sid, "Write src/main.rs.").await;
    assert_eq!((r.fake.requests().len(), res.loops), (3, 3));
    assert!(r.notes(&sid).is_empty() && r.rows("turn.end_noted") == 0);
    let files = res.files.expect("the facts stay");
    assert!(files.end_note.is_none() && files.left.paths == [r.path("src/main")]);
}

/// A shared place's turn is never told, and its result names no files. The
/// facts are seeded: a shared place runs no command of its own.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_end_note_in_a_shared_place() {
    for (place, shared) in [(None, false), (Some(format!("dm:{SOMEONE}")), true)] {
        let r = rig(
            |_| {
                vec![
                    Scripted::text("Hello."),
                    Scripted::text("Done."),
                    Scripted::text("More."),
                ]
            },
            |_| {},
        );
        let sid = r.session(place.as_deref());
        let first = r.turn(&sid, "Hi.").await;
        let exec = first.execution_id.clone().unwrap();
        let (main, made) = (r.root.join("src/main.rs"), r.root.join("src/main"));
        std::fs::write(&made, "x").unwrap();
        crate::rewind::seed_written(&exec, &main, true, 2_000);
        crate::toolrun::job::beside::seed(&exec, vec![made], 1_000);
        let res = r.turn(&sid, "Go on.").await;
        let sent = r.fake.requests().len();
        assert_eq!(sent, if shared { 2 } else { 3 }, "shared: {shared}");
        assert_eq!(r.notes(&sid).len(), usize::from(!shared));
        assert_eq!(res.files.is_none(), shared, "shared: {shared}");
        assert!(res.harness_lines().iter().any(|l| l.starts_with("Files:")) != shared);
    }
}

/// The facts of one turn are not the next one's.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turns_files_are_dropped_at_its_end() {
    let r = rig(
        |root| {
            vec![
                write(&at(root, "a.txt"), "a"),
                Scripted::text("Done."),
                Scripted::text("Hello."),
            ]
        },
        |_| {},
    );
    let sid = r.session(None);
    let first = r.turn(&sid, "Write a.").await;
    assert!(first.files.is_some());
    let second = r.turn(&sid, "Hi.").await;
    assert!(second.files.is_none(), "{:?}", second.files);
}
