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

/// The end note and the doubt are two continuations, not one
/// (theseus-rqu7j): a turn the end note sent on, whose next closing message
/// names something it never ran, is sent back once more for that, and a third
/// such message ends it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_the_end_note_continued_is_still_sent_back_once_for_a_doubt() {
    let r = rig(
        |root| {
            vec![
                write(&at(root, "src/main.rs"), "fn main() {}"),
                run_in(&at(root, "src"), "echo built > main"),
                Scripted::text("Done: src/main.rs, which I never ran."),
                Scripted::text("Still, I never ran it."),
                Scripted::text("It is done, and I never ran it."),
                Scripted::text("A sixth answer no turn asks for."),
            ]
        },
        |_| {},
    );
    let sid = r.session(None);
    let res = r.turn(&sid, "Write src/main.rs.").await;
    let reqs = r.fake.requests();
    assert_eq!(
        reqs.len(),
        5,
        "the end note's call, the doubt's, and no more"
    );
    assert_eq!((res.loops, res.stop_reason.as_str()), (5, "no_tool_calls"));
    let said = |i: usize| serde_json::to_string(&reqs[i].messages).unwrap();
    assert!(said(3).contains("this turn would end here"));
    assert!(!said(3).contains("names something it did not check"));
    assert!(said(4).contains("names something it did not check or run"));
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

/// The residue a build leaves: a closed list of names (`end_note_step::RESIDUE`).
const RESIDUE_SHELL: &str = "mkdir -p __pycache__ build .pytest_cache .venv; \
     echo x > a.out; echo x > libx.so; echo x > mod.pyc";

/// theseus-pp5hi: a turn that would end with only build residue beside its
/// edits ends. Nothing reopens it, the end note's row is not written, and
/// what the commands left is still said on the result's `Files:` line.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn build_residue_alone_does_not_reopen_the_turn() {
    let r = rig(
        |root| {
            vec![
                write(&at(root, "src/main.py"), "print(1)"),
                run_in(&at(root, "src"), RESIDUE_SHELL),
                Scripted::text("Done: src/main.py works."),
                Scripted::text("A fourth answer no turn asks for."),
            ]
        },
        |_| {},
    );
    let sid = r.session(None);
    let res = r.turn(&sid, "Write src/main.py.").await;
    assert_eq!(r.fake.requests().len(), 3, "no note, no extra call");
    assert_eq!((res.loops, res.stop_reason.as_str()), (3, "no_tool_calls"));
    assert!(r.notes(&sid).is_empty() && r.rows("turn.end_noted") == 0);
    let files = res.files.clone().expect("the facts stay");
    for name in [
        "__pycache__",
        "build",
        ".pytest_cache",
        ".venv",
        "a.out",
        "libx.so",
        "mod.pyc",
    ] {
        assert!(
            files.left.paths.contains(&r.path(&format!("src/{name}"))),
            "{name} is still said: {:?}",
            files.left.paths
        );
    }
    let lines = res.harness_lines();
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("Files:") && l.contains("a.out")),
        "{lines:?}"
    );
}

/// The protection that matters most (theseus-pp5hi): a path the task's own
/// words name is a deliverable, however residue-shaped, and a turn that would
/// end beside it is told once, as before. Same for a leftover that is no
/// residue at all.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_path_the_task_names_still_reopens_the_turn() {
    for (ask, shell, named) in [
        (
            "Build the shared library src/libfoo.so from src/foo.c.",
            "echo x > libfoo.so; mkdir -p __pycache__",
            "libfoo.so",
        ),
        (
            "Compile src/main.c so that ./a.out runs.",
            "echo x > a.out",
            "a.out",
        ),
        (
            "Write src/main.rs, and leave the output under src/build/.",
            "mkdir -p build",
            "build",
        ),
        (
            "Write src/main.rs.",
            "echo x > main; mkdir -p __pycache__",
            "main",
        ),
    ] {
        let r = rig(
            |root| {
                vec![
                    write(&at(root, "src/main.c"), "int main(){}"),
                    run_in(&at(root, "src"), shell),
                    Scripted::text("Done."),
                    Scripted::text("Still done."),
                    Scripted::text("A fifth answer no turn asks for."),
                ]
            },
            |_| {},
        );
        let sid = r.session(None);
        let res = r.turn(&sid, ask).await;
        assert_eq!(r.fake.requests().len(), 4, "{ask}: one note, one more call");
        assert_eq!(res.loops, 4, "{ask}");
        let notes = r.notes(&sid);
        assert_eq!(notes.len(), 1, "{ask}: {notes:?}");
        assert!(
            notes[0].contains(&r.path(&format!("src/{named}"))),
            "{notes:?}"
        );
        assert_eq!(r.rows("turn.end_noted"), 1, "{ask}");
    }
}

/// Edits no command ran since are no residue: a turn that edited after its
/// last command is told even when everything its commands left is residue.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unrun_edits_still_reopen_beside_residue() {
    let edit = |root: &Path| {
        let input = json!({"path": at(root, "src/main.py"), "old_string": "1", "new_string": "2"});
        Scripted::tools("Editing.", &[("e1", "fs_edit", input)])
    };
    let r = rig(
        |root| {
            vec![
                write(&at(root, "src/main.py"), "print(1)"),
                run_in(&at(root, "src"), RESIDUE_SHELL),
                edit(root),
                Scripted::text("Done."),
                Scripted::text("Still done."),
            ]
        },
        |_| {},
    );
    let sid = r.session(None);
    let res = r.turn(&sid, "Write src/main.py.").await;
    assert_eq!(r.fake.requests().len(), 5);
    let notes = r.notes(&sid);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(
        notes[0].contains("Changed after the last command ran"),
        "{notes:?}"
    );
    assert!(
        !notes[0].contains("Left beside"),
        "residue is not the note's: {notes:?}"
    );
    let note = res.files.unwrap().end_note.unwrap();
    assert_eq!(note.unrun, 1);
}

/// One note is one note: a reopened turn whose model keeps working is not
/// reopened again by what the next stop leaves.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_note_does_not_reopen_the_turn_twice() {
    let r = rig(
        |root| {
            vec![
                write(&at(root, "src/main.c"), "int main(){}"),
                run_in(&at(root, "src"), "echo x > main"),
                Scripted::text("Done."),
                run_in(&at(root, "src"), "echo y > main2"),
                Scripted::text("Done again."),
                Scripted::text("A seventh answer no turn asks for."),
            ]
        },
        |_| {},
    );
    let sid = r.session(None);
    let res = r.turn(&sid, "Write src/main.c.").await;
    assert_eq!(
        r.fake.requests().len(),
        5,
        "the note's one extra loop, then the end"
    );
    assert_eq!(r.notes(&sid).len(), 1);
    assert!(res.files.unwrap().end_note.unwrap().acted);
}
