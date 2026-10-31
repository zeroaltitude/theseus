//! Whole-turn replays through the whole core (own-benchmark part (c),
//! theseus-pw1q.20): a stand-in coding turn replayed on a second profile, the
//! real directory untouched; the owner-facing tools stubbed; a turn whose
//! directory cannot be put back refused; the caps; the machine label.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, Scripted as Jev};
use theseus_kernel::job::L1;
use theseus_kernel::probe_run::ProbeRun;
use theseus_protocol::own_bench::{TurnReplayParams, TurnReplayResult};
use theseus_protocol::LedgerKind;

use super::hands::{Hands, Programs, Recorded};
use super::runloop::TEST_PROGRAMS;
use super::undo::{collect, undo, Recorded as Call, Written};
use crate::config::MemoryMode;
use crate::provider::Scripted;
use crate::session::SessionRecord;
use crate::tests_recall::Rig;
use theseus_protocol::SessionKind;

const ASK: &str = "Make the greeting say beta and run the check.";
const DONE: &str = "Edited lib.txt; the check passes.";

/// One test at a time sets the stand-in for L1: they share the static.
pub(super) static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A stand-in for L1 (which refuses a root daemon, and a test may run as
/// one): `sh` in the directory it is given, recording where.
#[derive(Default)]
pub(crate) struct Sh {
    pub(crate) cwds: std::sync::Mutex<Vec<PathBuf>>,
}

impl Programs for Sh {
    fn run(
        &self,
        _l1: &L1,
        argv: &[String],
        cwd: &Path,
        _env: &[(String, String)],
        _ms: u64,
    ) -> ProbeRun {
        self.cwds.lock().unwrap().push(cwd.to_path_buf());
        let out = std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .current_dir(cwd)
            .output();
        match out {
            Ok(o) => ProbeRun {
                code: o.status.code(),
                stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
                failed: None,
            },
            Err(e) => ProbeRun {
                code: None,
                stdout: String::new(),
                stderr: String::new(),
                failed: Some(e.to_string()),
            },
        }
    }
}

pub(super) struct World {
    pub(super) rig: Rig,
    pub(super) work: PathBuf,
    pub(super) sid: String,
    _dir: tempfile::TempDir,
}

/// Every file under `dir`, by relative path.
pub(super) fn snapshot(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, at: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for e in std::fs::read_dir(at).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(base, &p, out);
            } else {
                out.insert(
                    p.strip_prefix(base).unwrap().display().to_string(),
                    std::fs::read(&p).unwrap(),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

/// Replay scratch directories this process left in the temp directory.
fn leftovers() -> Vec<String> {
    let prefix = format!("theseus-replay-{}-", std::process::id());
    std::fs::read_dir(std::env::temp_dir())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(&prefix))
        .collect()
}

pub(super) fn world(tweak: impl FnOnce(&mut crate::Config)) -> World {
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path().join("proj").canonicalize().unwrap_or_else(|_| {
        std::fs::create_dir_all(dir.path().join("proj")).unwrap();
        dir.path().join("proj").canonicalize().unwrap()
    });
    std::fs::write(work.join("lib.txt"), "greeting: alpha\n").unwrap();
    std::fs::write(work.join("check.sh"), "grep -q beta lib.txt\n").unwrap();
    let roots = vec![work.to_string_lossy().into_owned()];
    let rig = crate::tests_recall::rig_with(MemoryMode::Off, |c| {
        c.tools.roots = roots;
        tweak(c);
    });
    let mut rec = SessionRecord::new(SessionKind::Conversation, None);
    rec.dir = Some(work.to_string_lossy().into_owned());
    rig.core.store.put_session(&rec.session_id, &rec).unwrap();
    World {
        rig,
        work,
        sid: rec.session_id,
        _dir: dir,
    }
}

pub(super) fn push(rig: &Rig, s: Scripted) {
    rig.model.script.lock().unwrap().push_back(s);
}

pub(super) fn edit(path: &Path) -> Value {
    json!({"path": path.join("lib.txt"), "old_string": "alpha", "new_string": "beta"})
}

pub(super) fn check() -> Value {
    json!({"argv": ["sh", "check.sh", "test"]})
}

/// The original turn: the edit, the check, and the answer.
pub(super) async fn original(w: &World) -> String {
    push(
        &w.rig,
        Scripted::tools(
            "",
            &[
                ("e1", "fs_edit", edit(&w.work)),
                ("p1", "proc_run", check()),
            ],
        ),
    );
    push(&w.rig, Scripted::text(DONE));
    let r = crate::tests_recall::turn(&w.rig.core, &w.sid, ASK).await;
    assert_eq!(
        std::fs::read_to_string(w.work.join("lib.txt")).unwrap(),
        "greeting: beta\n"
    );
    r.turn_id
}

pub(super) fn params(w: &World, turn: &str) -> TurnReplayParams {
    TurnReplayParams {
        session: w.sid.clone(),
        turn: turn.into(),
        profile: "haiku".into(),
        effort: None,
        scope: Some("whole_turn".into()),
        compare: Some(false),
    }
}

pub(super) async fn replay(w: &World, turn: &str) -> anyhow::Result<TurnReplayResult> {
    w.rig.core.turn_replay(params(w, turn), "cli").await
}

/// The replay's script: the same edit and a new file, the check, a call that
/// would wake the owner, a fetch, and a read outside the directory; then its
/// answer.
fn replay_script(w: &World) {
    push(
        &w.rig,
        Scripted::tools(
            "",
            &[
                ("r1", "fs_edit", edit(&w.work)),
                (
                    "r2",
                    "fs_write",
                    json!({"path": w.work.join("notes.txt"), "content": "scratch notes\n"}),
                ),
                (
                    "r3",
                    "proc_run",
                    json!({"argv": ["sh", "check.sh", "test"], "cwd": w.work}),
                ),
                (
                    "r4",
                    "wake_at",
                    json!({"at": "tomorrow 9:00", "note": "x", "for": "owner"}),
                ),
                (
                    "r5",
                    "http_fetch",
                    json!({"url": "https://example.invalid/page"}),
                ),
                ("r6", "fs_read", json!({"path": "/etc/hostname"})),
            ],
        ),
    );
    push(&w.rig, Scripted::text("Edited and the check passes."));
}

/// The whole turn is run again on a second profile with its tools, and
/// nothing outside its scratch tree changes: the real directory is the same
/// bytes after, the program ran in the copy and not in the directory, the
/// copy is gone, and the model saw the directory's paths and never the
/// copy's. The row names what ran, what was stubbed and what was refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_whole_turn_replay_changes_nothing_outside_its_scratch_tree() {
    let _g = SERIAL.lock().await;
    let sh = Arc::new(Sh::default());
    *TEST_PROGRAMS.lock().unwrap() = Some(sh.clone());
    let w = world(|_| {});
    let t = original(&w).await;
    let before = snapshot(&w.work);
    replay_script(&w);
    let r = replay(&w, &t).await.unwrap();
    *TEST_PROGRAMS.lock().unwrap() = None;

    assert_eq!(snapshot(&w.work), before, "the real directory is untouched");
    assert!(!w.work.join("notes.txt").exists());
    assert!(
        leftovers().is_empty(),
        "the scratch copy is removed: {:?}",
        leftovers()
    );
    let whole = r.whole.as_ref().expect("a whole replay");
    let how: Vec<(&str, &str)> = whole
        .calls
        .iter()
        .map(|c| (c.tool.as_str(), c.how.as_str()))
        .collect();
    assert_eq!(
        how,
        [
            ("fs.edit", "ran"),
            ("fs.write", "ran"),
            ("proc.run", "ran"),
            ("wake.at", "stub"),
            ("http.fetch", "stub"),
            ("fs.read", "refused"),
        ]
    );
    assert_eq!(whole.calls[2].exit, Some(0));
    assert_eq!(whole.loops, 2);
    assert!(whole.capped.is_none());
    assert_eq!(whole.changed, ["lib.txt", "notes.txt"]);
    assert_eq!(whole.original_changed, ["lib.txt"]);
    assert!(
        !whole.same.files,
        "the replay wrote a file the original did not"
    );
    assert_eq!(
        whole.same.tests,
        Some(true),
        "both ran the check and it passed"
    );
    assert_eq!(whole.tests.len(), 1);
    assert_eq!(whole.original_tests.len(), 1);
    assert_eq!(r.replay.text, "Edited and the check passes.");
    assert_eq!(r.original.text, DONE);
    assert_eq!(r.scope, "whole_turn");
    // The program ran in a copy, never in the directory itself.
    let cwds = sh.cwds.lock().unwrap().clone();
    assert_eq!(cwds.len(), 1);
    assert!(
        !cwds[0].starts_with(&w.work) && cwds[0].starts_with(std::env::temp_dir()),
        "{cwds:?}"
    );
    // The model was shown the directory's own paths, not the copy's.
    let sent = w.rig.model.requests();
    let last = serde_json::to_string(&sent.last().unwrap().messages).unwrap();
    assert!(
        last.contains("tool_result") && !last.contains("theseus-replay-"),
        "{last}"
    );
    assert!(
        last.contains("Not run: http_fetch") || last.contains("Not run: http.fetch"),
        "{last}"
    );
    assert!(
        last.contains("outside the directory the replay copied"),
        "{last}"
    );
    // The row names each call, with the stubs apart.
    let rows = w.rig.core.rows_since(LedgerKind::TurnReplayed, 0).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].data["scope"], "whole_turn");
    assert_eq!(
        rows[0].data["whole"]["stubs"],
        json!(["wake.at", "http.fetch"])
    );
    assert_eq!(rows[0].data["whole"]["same"]["tests"], true);
    // The session itself gained no node.
    assert!(rows[0].data["whole"]["calls"].as_array().unwrap().len() == 6);
}

/// A turn whose writes cannot be put back is refused, naming the file, and
/// nothing is called or copied: here an overwrite whose diff was cut.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_without_pre_images_is_refused_naming_the_file() {
    let _g = SERIAL.lock().await;
    let w = world(|_| {});
    let old: String = (0..2000).map(|i| format!("line {i} alpha\n")).collect();
    let new: String = (0..2000).map(|i| format!("line {i} gamma\n")).collect();
    std::fs::write(w.work.join("big.txt"), &old).unwrap();
    push(
        &w.rig,
        Scripted::tools(
            "",
            &[(
                "w1",
                "fs_write",
                json!({"path": w.work.join("big.txt"), "content": new}),
            )],
        ),
    );
    push(&w.rig, Scripted::text("Rewrote it."));
    let t = crate::tests_recall::turn(&w.rig.core, &w.sid, "Rewrite big.txt.")
        .await
        .turn_id;
    let calls = w.rig.model.requests().len();
    let err = replay(&w, &t).await.unwrap_err();
    let said = format!("{err:#}");
    assert!(
        said.contains("big.txt") && said.contains("no whole diff was recorded"),
        "{said}"
    );
    assert_eq!(w.rig.model.requests().len(), calls, "no model was called");
    assert!(leftovers().is_empty());
}

/// The cap stops a replay that keeps calling tools: two model calls at
/// `whole_loops = 2`, and the row says which cap. A spend cap of nothing
/// refuses the first call before it is made.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_cap_stops_a_long_replay() {
    let _g = SERIAL.lock().await;
    let w = world(|c| c.route_bench.whole_loops = 2);
    let t = original(&w).await;
    let calls = w.rig.model.requests().len();
    for i in 0..4 {
        push(
            &w.rig,
            Scripted::tools(
                "",
                &[(&format!("l{i}"), "fs_list", json!({"path": w.work}))],
            ),
        );
    }
    let r = replay(&w, &t).await.unwrap();
    let whole = r.whole.unwrap();
    assert_eq!((whole.loops, whole.capped.as_deref()), (2, Some("loops")));
    assert_eq!(w.rig.model.requests().len(), calls + 2);
    assert!(leftovers().is_empty());

    let w = world(|c| c.route_bench.whole_usd = 0.0);
    let t = original(&w).await;
    let calls = w.rig.model.requests().len();
    push(&w.rig, Scripted::text("never"));
    let err = replay(&w, &t).await.unwrap_err();
    assert!(
        err.downcast_ref::<crate::own_bench::replay::CapReached>()
            .is_some(),
        "{err:#}"
    );
    assert_eq!(w.rig.model.requests().len(), calls, "the call was not made");
}

/// A tool the original called returns what the original got, and one it did
/// not call returns a sentence saying nothing was done; neither runs.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stub_is_the_recorded_result_or_a_sentence() {
    let w = world(|_| {});
    let rt = w.rig.core.runner.tools.clone();
    let scratch = w.work.clone();
    let mut hands = Hands::new(
        &rt,
        (w.work.clone(), scratch),
        vec![Recorded {
            tool: "wake.at".into(),
            content: "wake set for 09:00".into(),
        }],
        Arc::new(Sh::default()),
    );
    let a = hands.call("wake_at", &json!({"at": "9:00"})).await;
    assert_eq!(a.text, "wake set for 09:00");
    let b = hands.call("wake_at", &json!({"at": "9:00"})).await;
    assert!(b.text.starts_with("Not run: wake.at"), "{}", b.text);
    let c = hands.call("no_such_tool", &json!({})).await;
    assert!(c.is_error && c.text.contains("no tool named"));
    let how: Vec<&str> = hands.calls.iter().map(|c| c.how.as_str()).collect();
    assert_eq!(how, ["stub", "stub", "refused"]);
    assert!(hands.changed.is_empty());
}

/// What the session recorded is put back, newest first, or refused: an edit
/// is taken back where its text is, a created file is removed, an overwrite
/// goes back by its whole diff, and each of those that cannot be refuses.
#[test]
fn recorded_writes_are_put_back_or_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (orig, scratch) = (dir.path().join("o"), dir.path().join("s"));
    std::fs::create_dir_all(&scratch).unwrap();
    std::fs::write(scratch.join("a.txt"), "one\ntwo beta\nthree\n").unwrap();
    std::fs::write(scratch.join("new.txt"), "made\n").unwrap();
    let edit = Written::Edit {
        path: orig.join("a.txt"),
        old: "alpha".into(),
        new: "beta".into(),
        all: false,
    };
    let made = Written::Created {
        path: orig.join("new.txt"),
    };
    let notes = undo(&orig, &scratch, &[edit, made]).unwrap();
    assert!(notes.is_empty());
    assert_eq!(
        std::fs::read_to_string(scratch.join("a.txt")).unwrap(),
        "one\ntwo alpha\nthree\n"
    );
    assert!(!scratch.join("new.txt").exists());
    // An edit whose text is not there once is refused.
    let gone = Written::Edit {
        path: orig.join("a.txt"),
        old: "x".into(),
        new: "nowhere".into(),
        all: false,
    };
    assert!(undo(&orig, &scratch, &[gone])
        .unwrap_err()
        .why
        .contains("no longer holds"));
    // An overwrite goes back by its diff, and is refused without one.
    let unified = theseus_tools::change::file_diff("a.txt", Some("old line\n"), "new line\n");
    std::fs::write(scratch.join("a.txt"), "new line\n").unwrap();
    let over = |diff| Written::Overwrote {
        path: orig.join("a.txt"),
        diff,
    };
    undo(&orig, &scratch, &[over(Some(unified))]).unwrap();
    assert_eq!(
        std::fs::read_to_string(scratch.join("a.txt")).unwrap(),
        "old line\n"
    );
    assert!(undo(&orig, &scratch, &[over(None)])
        .unwrap_err()
        .why
        .contains("no whole diff"));
    // A path outside the directory is not in the copy: said, not failed.
    let far = Written::Created {
        path: PathBuf::from("/elsewhere/x"),
    };
    assert_eq!(undo(&orig, &scratch, &[far]).unwrap().len(), 1);
}

/// `collect` reads the calls: programs are counted, a failed call is no
/// change, `fs.patch` and any other writing tool refuse.
#[test]
fn the_span_names_what_cannot_be_put_back() {
    let m = json!({"path": "/p/a.txt", "created": true});
    let none = Value::Null;
    let ok = |tool, input, meta| Call {
        tool,
        input,
        ok: true,
        meta,
    };
    let input = json!({"path": "/p/a.txt", "old_string": "a", "new_string": "b"});
    let edit_meta = json!({"path": "/p/a.txt"});
    let span = collect(
        [
            ok("fs.write", &none, &m),
            ok("fs.edit", &input, &edit_meta),
            ok("proc.run", &none, &none),
            Call {
                tool: "fs.patch",
                input: &none,
                ok: false,
                meta: &none,
            },
            ok("fs.read", &none, &none),
        ],
        |_| false,
    )
    .unwrap();
    assert_eq!((span.written.len(), span.programs), (2, 1));
    assert!(collect([ok("fs.patch", &none, &none)], |_| false).is_err());
    assert!(collect([ok("lsp.rename", &none, &none)], |t| t == "lsp.rename").is_err());
}

// ---- the machine label

/// A route judgment for `turn` that answered `mode`, and its `route.decided`
/// row, as the judge's sink and the turn write them.
fn plant_route(w: &World, turn: &str, pick: &str) -> String {
    use crate::tests_learning::{call_row, choice, judgment};
    let options = ["haiku", "sonnet", "opus", "fable", "deepseek_flash"];
    let j = judgment(
        "jdg_route_whole",
        "route.v10",
        vec![choice("model", pick, 0.95, &options)],
        json!({"session": w.sid, "turn": turn}),
        10,
    );
    let mut row = crate::ledger::LedgerRow::new(
        LedgerKind::RouteDecided,
        Some(&w.sid),
        Some(turn),
        json!({"mode": pick, "judgment": j.id, "profile": "sonnet"}),
    );
    row.at_unix_ms = theseus_protocol::now_unix_ms();
    let decided = theseus_store::NewRecord::json(theseus_store::kinds::LEDGER, None, &row).unwrap();
    w.rig
        .core
        .store
        .append(&[call_row(&j, theseus_protocol::now_unix_ms()), decided])
        .unwrap();
    j.id
}

fn judged_world(jev: &FakeJev) -> World {
    let board = crate::own_bench::tests::board();
    let base = jev.base();
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path().join("proj");
    std::fs::create_dir_all(&work).unwrap();
    let work = work.canonicalize().unwrap();
    std::fs::write(work.join("lib.txt"), "greeting: alpha\n").unwrap();
    std::fs::write(work.join("check.sh"), "grep -q beta lib.txt\n").unwrap();
    let roots = vec![work.to_string_lossy().into_owned()];
    let rig = crate::tests_recall::rig_built(
        MemoryMode::Off,
        |c| {
            c.tools.roots = roots;
            c.judge.enabled = true;
            c.judge.api_base = base;
            c.judge.connect_secs = 1;
            c.judge.total_secs = 2;
        },
        |p| p.secrets = board,
    );
    let mut rec = SessionRecord::new(SessionKind::Conversation, None);
    rec.dir = Some(work.to_string_lossy().into_owned());
    rig.core.store.put_session(&rec.session_id, &rec).unwrap();
    World {
        rig,
        work,
        sid: rec.session_id,
        _dir: dir,
    }
}

fn labels(w: &World) -> Vec<crate::ledger::LedgerRow> {
    w.rig
        .core
        .rows_since(LedgerKind::JudgeLabel, 0)
        .unwrap()
        .into_iter()
        .filter(|r| r.data["source"] == "replay")
        .collect()
}

/// A judged replay that matches, with the facts not against it, is written as
/// a machine label on the turn's route judgment: source `replay`, weight 0.4,
/// the class whose profile list leads with the rung; once. A verdict under
/// the bar writes none, and says why in the row.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_judged_match_becomes_a_machine_label_once() {
    let _g = SERIAL.lock().await;
    *TEST_PROGRAMS.lock().unwrap() = Some(Arc::new(Sh::default()));
    let jev = FakeJev::start().unwrap();
    jev.script(
        "compare.v1/verdict",
        Jev::Choice {
            option: "same".into(),
            confidence: 0.96,
        },
    );
    let w = judged_world(&jev);
    let t = original(&w).await;
    plant_route(&w, &t, "opus");
    // The replay does what the original did: the edit and the check.
    let same = |w: &World| {
        push(
            &w.rig,
            Scripted::tools(
                "",
                &[
                    ("r1", "fs_edit", edit(&w.work)),
                    ("r3", "proc_run", check()),
                ],
            ),
        );
        push(&w.rig, Scripted::text(DONE));
    };
    same(&w);
    let mut p = params(&w, &t);
    p.compare = None;
    let r = w.rig.core.turn_replay(p.clone(), "cli").await.unwrap();
    let whole = r.whole.as_ref().unwrap();
    assert!(
        whole.same.files && whole.same.tests == Some(true),
        "{whole:?}"
    );
    assert_eq!(
        whole.label.as_deref(),
        Some("route.v10 model=haiku on jdg_route_whole")
    );
    let l = labels(&w);
    assert_eq!(l.len(), 1);
    assert_eq!(l[0].data["judgment"], "jdg_route_whole");
    assert_eq!(l[0].data["question"], "model");
    assert_eq!(l[0].data["label"], "haiku");
    assert_eq!(l[0].data["weight"], crate::learning::REPLAY_WEIGHT);
    assert_eq!(l[0].data["pack"], "route.v10");
    // The same turn again writes no second label.
    same(&w);
    let again = w.rig.core.turn_replay(p, "cli").await.unwrap();
    assert!(again
        .whole
        .unwrap()
        .label
        .unwrap()
        .contains("already labeled"));
    assert_eq!(labels(&w).len(), 1);
    // The report counts it, apart from the owner's.
    let report = w
        .rig
        .core
        .route_report_now(false, theseus_protocol::now_unix_ms())
        .unwrap();
    assert_eq!(report.labels_fed, 1);
    let cell = report
        .cells
        .iter()
        .find(|c| c.whole_replayed > 0)
        .expect("a whole-turn cell");
    assert_eq!(
        (cell.whole_replayed, cell.whole_matched, cell.tests_same),
        (2, 2, 2)
    );
    *TEST_PROGRAMS.lock().unwrap() = None;
}

/// No label when the words say weaker, when the tests differ, or when the
/// replay ended at a cap: each says why in the row.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_weak_or_contradicted_replay_writes_no_label() {
    let _g = SERIAL.lock().await;
    *TEST_PROGRAMS.lock().unwrap() = Some(Arc::new(Sh::default()));
    let jev = FakeJev::start().unwrap();
    jev.script(
        "compare.v1/verdict",
        Jev::Choice {
            option: "weaker".into(),
            confidence: 0.9,
        },
    );
    let w = judged_world(&jev);
    let t = original(&w).await;
    plant_route(&w, &t, "opus");
    push(
        &w.rig,
        Scripted::tools(
            "",
            &[
                ("r1", "fs_edit", edit(&w.work)),
                ("r3", "proc_run", check()),
            ],
        ),
    );
    push(&w.rig, Scripted::text(DONE));
    let mut p = params(&w, &t);
    p.compare = None;
    let r = w.rig.core.turn_replay(p, "cli").await.unwrap();
    let why = r.whole.unwrap().label.unwrap();
    assert!(
        why.starts_with("none: compare.v1 put same or better at"),
        "{why}"
    );
    assert!(labels(&w).is_empty());
    *TEST_PROGRAMS.lock().unwrap() = None;
}

/// Which option a rung is: the `model` option whose catalog model the rung's
/// profile runs.
#[test]
fn a_rung_is_labeled_with_the_option_that_names_its_model() {
    assert_eq!(super::label::option_for("claude-haiku-5-5"), Some("haiku"));
    assert_eq!(super::label::option_for("claude-opus-5-5"), Some("opus"));
    assert_eq!(super::label::option_for("glm-5.3-flash"), None);
}
