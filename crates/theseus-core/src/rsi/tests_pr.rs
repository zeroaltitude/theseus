//! `self.propose_change` through the whole core (theseus-pw1q.8), over
//! `tests_tree`'s throwaway repository and its stand-in gate: a stand-in
//! model calls the tool, edits the tree with `fs.write`, and calls it again.
//! - mode off: no tool; mode act: offered;
//! - a session whose principal is not `"self"` is refused, writing nothing;
//! - a green attempt: one `self.branch.built` row, a frozen report ending
//!   `<!-- REPORT COMPLETE -->`, the branch pushed to `self/*`, the tree
//!   closed;
//! - a halt between phases stops it, and the row says before which;
//! - a keel finding stops it at the owner's card, and its later calls are
//!   refused, never retried around.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use theseus_kernel::Authority;
use theseus_protocol::rsi::{SelfHaltParams, SelfMode, SelfResumeParams};
use theseus_protocol::SessionKind;

use super::pr::{COMPLETE, PROPOSE};
use super::tests_tree::{fixture, hermetic, Fixture};
use crate::approval::Answerer;
use crate::bus::EventSink;
use crate::policy::Posture;
use crate::provider::{
    DeltaSink, FakeProvider, Provider, ProviderFuture, ProviderRequest, Scripted,
};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

const WIRE: &str = "self_propose_change";

/// Answers each turn's first request with the call queued for it, and a
/// call's result with a line of text; keeps every request.
#[derive(Default)]
struct Model {
    calls: Mutex<Vec<(String, Value)>>,
    requests: Mutex<Vec<ProviderRequest>>,
}

impl Provider for Model {
    fn name(&self) -> &str {
        "fake"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        self.requests.lock().unwrap().push(req.clone());
        let answers_a_call = req
            .messages
            .last()
            .and_then(|m| m["content"].as_array())
            .is_some_and(|c| c.iter().any(|b| b["type"] == "tool_result"));
        let answer = match self.calls.lock().unwrap().pop() {
            Some((tool, input)) if !answers_a_call => {
                Scripted::tools("", &[("c1", tool.as_str(), input)])
            }
            _ => Scripted::text("Done."),
        };
        Box::pin(async move {
            FakeProvider::scripted(vec![answer])
                .stream_message(req, on_delta)
                .await
        })
    }
}

struct Rig {
    core: Arc<Core>,
    model: Arc<Model>,
    f: Fixture,
    _work: tempfile::TempDir,
}

fn rig(mode: SelfMode, remote: Option<&str>) -> Rig {
    rig_with(mode, remote, |_| {})
}

fn rig_with(mode: SelfMode, remote: Option<&str>, edit: impl FnOnce(&mut Config)) -> Rig {
    let f = fixture();
    let work = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = f.place.state.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(work.path().canonicalize().unwrap().display().to_string());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = Posture::Notify;
    cfg.self_improve.mode = mode;
    cfg.self_improve.repo = Some(f.place.repo.display().to_string());
    cfg.self_improve.remote = remote.map(str::to_string);
    edit(&mut cfg);
    let store = Store::open(&f.place.state.join("store")).unwrap();
    let model = Arc::new(Model::default());
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, model.clone(), store)).unwrap();
    core.tools.self_pr.set_env(hermetic());
    let cli = Answerer::from("the CLI");
    core.self_resume(&SelfResumeParams::default(), &cli)
        .unwrap();
    Rig {
        core,
        model,
        f,
        _work: work,
    }
}

impl Rig {
    /// A session whose execution's principal is `principal`.
    fn session(&self, principal: &str) -> String {
        let mut r = SessionRecord::new(SessionKind::Conversation, None);
        let e = self
            .core
            .kernel
            .open_execution(
                &r.session_id,
                r.kind,
                Authority {
                    principal: principal.into(),
                    ..Default::default()
                },
                None,
                None,
            )
            .unwrap();
        r.execution_id = Some(e.id);
        self.core.store.put_session(&r.session_id, &r).unwrap();
        r.session_id
    }

    /// One turn whose model makes `call`, and the result it read.
    async fn turn(&self, sid: &str, tool: &str, input: Value) -> String {
        self.model.calls.lock().unwrap().push((tool.into(), input));
        let rec: SessionRecord = self.core.store.get_session(sid).unwrap().unwrap();
        let (live, _) = self.core.live_profile();
        let target = self
            .core
            .runner
            .resolve_target(&live, None, None, None)
            .unwrap();
        self.core
            .runner
            .run(TurnRequest {
                prompt: None,
                surface: None,
                session: rec,
                input: Some("go on".into()),
                target,
                sink: EventSink::new(self.core.bus.clone(), sid, None),
                author: "test".into(),
                recompile: None,
                attachments: vec![],
                arrived: None,
                reply_to: None,
                answer_at: None,
            })
            .await
            .unwrap();
        self.last_result()
    }

    fn last_result(&self) -> String {
        let reqs = self.model.requests.lock().unwrap();
        let last = reqs.last().unwrap();
        let blocks = last.messages.last().unwrap()["content"].clone();
        blocks
            .as_array()
            .into_iter()
            .flatten()
            .filter(|b| b["type"] == "tool_result")
            .map(|b| b["content"].to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn offered(&self) -> bool {
        let reqs = self.model.requests.lock().unwrap();
        let tools = serde_json::to_string(&reqs.last().unwrap().tools).unwrap();
        tools.contains(WIRE)
    }

    fn rows(&self) -> Vec<Value> {
        self.core
            .store
            .ledger_tail::<crate::ledger::LedgerRow>(100_000)
            .unwrap()
            .into_iter()
            .filter(|(_, r)| r.kind == "self.branch.built")
            .map(|(_, r)| r.data)
            .collect()
    }

    /// Open the tree, and write `file` into it with `fs.write`.
    async fn open_and_edit(&self, sid: &str, file: &str) -> PathBuf {
        let said = self.turn(sid, WIRE, propose()).await;
        assert!(said.contains("Tree open for trim-the-sails"), "{said}");
        let tree = self
            .f
            .place
            .trees()
            .canonicalize()
            .unwrap()
            .join("trim-the-sails");
        let path = tree.join(file);
        let said = self
            .turn(
                sid,
                "fs_write",
                json!({"path": path, "content": "trimmed\n"}),
            )
            .await;
        assert!(path.exists(), "fs.write in the tree ran: {said}");
        tree
    }
}

fn upstream_refs(f: &Fixture) -> String {
    let o = std::process::Command::new("git")
        .arg("-C")
        .arg(&f.upstream)
        .args(["for-each-ref", "--format=%(refname)"])
        .envs(hermetic())
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

#[tokio::test]
async fn mode_off_offers_no_tool_and_act_does() {
    let off = rig(SelfMode::Off, None);
    assert!(off.core.tools.registry.get(PROPOSE).is_none());
    // An operator's turn: with the mode off a self session's turn spends
    // nothing, so it never reaches the model (the self budget refuses it).
    let sid = off.session("operator");
    off.turn(&sid, "fs_list", json!({"path": "."})).await;
    assert!(!off.offered(), "mode off: no tool");
    let act = rig(SelfMode::Act, None);
    assert!(act.core.tools.registry.get(PROPOSE).is_some());
    let sid = act.session("self");
    act.turn(&sid, "fs_list", json!({"path": "."})).await;
    assert!(act.offered(), "mode act: offered");
}

#[tokio::test]
async fn a_session_that_is_not_self_cannot_call_it() {
    let r = rig(SelfMode::Act, None);
    let sid = r.session("operator");
    let said = r.turn(&sid, WIRE, propose()).await;
    assert!(said.contains("runs only in a self session"), "{said}");
    assert!(
        !r.f.place.trees().join("trim-the-sails").exists(),
        "no tree"
    );
    assert!(r.rows().is_empty(), "no row");
}

/// A self build runs its code in L1 (theseus-pw1q.21.1), below the floor that
/// hides a sealed set, so a set being there no longer stops it: the branch
/// builds, and the gate judges it.
#[tokio::test]
async fn a_sealed_set_does_not_stop_a_self_build_that_runs_in_l1() {
    let sealed = tempfile::tempdir().unwrap();
    let dir = sealed.path().canonicalize().unwrap().display().to_string();
    let r = rig_with(SelfMode::Act, Some("origin"), |cfg| {
        cfg.self_improve.sealed.dir = Some(dir);
    });
    let sid = r.session("self");
    let tree = r.open_and_edit(&sid, "sails.txt").await;
    assert!(
        tree.starts_with(r.f.place.trees()),
        "a tree is open: {tree:?}"
    );
    let said = r.turn(&sid, WIRE, propose()).await;
    assert!(said.contains("the gate is green"), "{said}");
    assert_eq!(r.rows().len(), 1, "one attempt, one row");
}

/// A host that cannot run L1 builds nothing, and says so: no tree, no row,
/// and the reason is L1's own (here the root daemon's refusal, theseus-pv6i).
#[tokio::test]
async fn a_host_that_cannot_run_l1_builds_nothing() {
    let r = rig(SelfMode::Act, Some("origin"));
    r.core.tools.self_pr.set_l1(Some(
        "checking the job's process limit: the daemon runs as root",
    ));
    let sid = r.session("self");
    let said = r.turn(&sid, WIRE, propose()).await;
    assert!(
        said.contains("L1 cannot run on this host: checking the job's process limit"),
        "{said}"
    );
    assert!(
        !r.f.place.trees().join("trim-the-sails").exists(),
        "no tree"
    );
    assert!(r.rows().is_empty(), "no row");
}

/// Halted, nothing begins, and the probe for L1 is not even run: the halt's
/// words are the answer, not L1's.
#[tokio::test]
async fn a_halted_switch_does_not_probe_l1() {
    let r = rig(SelfMode::Act, Some("origin"));
    r.core.tools.self_pr.set_l1(Some("the probe must not run"));
    r.core
        .self_halt(&SelfHaltParams::default(), &Answerer::from("the CLI"))
        .unwrap();
    let sid = r.session("self");
    let said = r.turn(&sid, WIRE, propose()).await;
    assert!(!said.contains("the probe must not run"), "{said}");
    assert!(
        said.contains("halted") || said.contains("kill switch"),
        "{said}"
    );
    assert!(r.rows().is_empty(), "no row");
}

#[tokio::test]
async fn a_green_attempt_reports_pushes_and_closes() {
    let r = rig(SelfMode::Act, Some("origin"));
    let sid = r.session("self");
    let tree = r.open_and_edit(&sid, "sails.txt").await;
    let said = r.turn(&sid, WIRE, propose()).await;
    assert!(said.contains("the gate is green"), "{said}");
    let rows = r.rows();
    assert_eq!(rows.len(), 1, "{rows:?}");
    let row = &rows[0];
    assert_eq!(row["gate"], "green");
    assert_eq!(row["item"], "trim-the-sails");
    assert!(row["stopped_at"].is_null());
    assert_eq!(row["numbers"]["pushed"], true);
    let branch = row["branch"].as_str().unwrap().to_string();
    assert!(branch.starts_with("self/") && branch.ends_with("-trim-the-sails"));
    // The frozen report: where S4 says, complete, read-only.
    let report = PathBuf::from(row["report"].as_str().unwrap());
    assert_eq!(
        report,
        r.f.place
            .reports()
            .join(format!("{}.md", branch.replace('/', "-")))
    );
    let text = std::fs::read_to_string(&report).unwrap();
    assert!(text.trim_end().ends_with(COMPLETE), "{text}");
    for want in [
        "- Gate: green",
        "- Item: trim-the-sails",
        "- Issue: theseus-sail1",
        "Trim the sails",
        "(trim-the-sails)",
    ] {
        assert!(text.contains(want), "{want}: {text}");
    }
    assert!(std::fs::metadata(&report).unwrap().permissions().readonly());
    // The commit names the item; the branch reached self/* and main stayed.
    assert_eq!(
        upstream_refs(&r.f),
        format!("refs/heads/main\nrefs/heads/{branch}")
    );
    // The tree and its build tree are gone; the branch stays in the repo.
    assert!(!tree.exists());
    assert!(!r.f.place.targets().join("trim-the-sails").exists());
    let listed = r.core.self_branches(&Default::default()).unwrap();
    assert_eq!(listed.branches.len(), 1);
    assert_eq!(listed.branches[0].branch, branch);
    assert_eq!(listed.branches[0].gate.as_deref(), Some("green"));
    assert!(listed.branches[0].tree.is_none());
}

#[tokio::test]
async fn a_red_attempt_keeps_the_tree_and_the_next_is_a_new_branch() {
    let r = rig(SelfMode::Act, Some("origin"));
    let sid = r.session("self");
    let tree = r.open_and_edit(&sid, "broken.txt").await;
    let said = r.turn(&sid, WIRE, propose()).await;
    assert!(said.contains("the gate is red"), "{said}");
    assert!(tree.exists(), "a red tree stays for another attempt");
    assert_eq!(
        upstream_refs(&r.f),
        "refs/heads/main",
        "a red branch is not pushed"
    );
    std::fs::remove_file(tree.join("broken.txt")).unwrap();
    let said = r.turn(&sid, WIRE, propose()).await;
    assert!(said.contains("the gate is green"), "{said}");
    let rows = r.rows();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        (rows[0]["gate"].as_str(), rows[1]["gate"].as_str()),
        (Some("red"), Some("green"))
    );
    let (b1, b2) = (
        rows[0]["branch"].as_str().unwrap(),
        rows[1]["branch"].as_str().unwrap(),
    );
    assert_eq!(
        b2,
        format!("{b1}-2"),
        "each attempt its own branch and report"
    );
    assert!(Path::new(rows[0]["report"].as_str().unwrap()).exists());
}

#[tokio::test]
async fn a_halt_between_phases_stops_it_and_the_row_says_where() {
    let r = rig(SelfMode::Act, Some("origin"));
    let sid = r.session("self");
    let tree = r.open_and_edit(&sid, "sails.txt").await;
    let p = SelfHaltParams {
        why: Some("the wind turned".into()),
        ..Default::default()
    };
    r.core.self_halt(&p, &Answerer::from("the CLI")).unwrap();
    let said = r.turn(&sid, WIRE, propose()).await;
    assert!(said.contains("Stopped before the commit"), "{said}");
    let rows = r.rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["gate"], "halted");
    assert_eq!(rows[0]["stopped_at"], "commit");
    assert!(rows[0]["report"].is_null());
    assert!(tree.exists(), "nothing more was done");
    assert_eq!(upstream_refs(&r.f), "refs/heads/main");
}

#[tokio::test]
async fn a_keel_finding_stops_at_the_owners_card_and_is_never_retried() {
    let r = rig(SelfMode::Act, Some("origin"));
    let sid = r.session("self");
    r.open_and_edit(&sid, "keel.txt").await;
    let said = r.turn(&sid, WIRE, propose()).await;
    assert!(said.contains("stopped at the keel"), "{said}");
    let rows = r.rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["gate"], "keel");
    assert_eq!(rows[0]["stopped_at"], "push");
    let cards = crate::tests_day_ceiling::posts(&r.core, "self_keel");
    assert_eq!(cards.len(), 1, "the owner's card");
    assert!(cards[0]["text"]
        .as_str()
        .unwrap()
        .contains("stopped at the keel"));
    assert_eq!(upstream_refs(&r.f), "refs/heads/main");
    let said = r.turn(&sid, WIRE, propose()).await;
    assert!(said.contains("waits for the owner"), "{said}");
    assert_eq!(r.rows().len(), 1, "no attempt around it");
}

fn propose() -> Value {
    json!({"item": "trim-the-sails", "issue": "theseus-sail1", "why": "Trim the sails before the wind."})
}

/// A self task's own tree (theseus-pw1q.21.9), through the whole core with
/// the state dir on the approve list, as a daemon's default has it: the self
/// session's file tools work in the tree of its own task and nowhere else of
/// the state dir. Each scenario is a self session of its own, with its task.
mod own_tree {
    use super::*;
    use theseus_protocol::tasks::{TaskOrigin, TaskRecord, TaskState};

    fn held(edit: impl FnOnce(&mut Config)) -> Rig {
        rig_with(SelfMode::Act, None, |cfg| {
            cfg.tools.approve_paths = vec![cfg.server.state_dir.clone()];
            edit(cfg);
        })
    }

    /// A self session, the task of that name it belongs to, and the task's tree.
    fn task(r: &Rig, id: &str) -> (String, PathBuf) {
        let sid = r.session("self");
        let t = TaskRecord {
            id: id.into(),
            version: 1,
            title: id.into(),
            state: TaskState::InProgress,
            session: Some(sid.clone()),
            origin: TaskOrigin {
                session: "self:backlog".into(),
                principal: "self".into(),
                ..Default::default()
            },
            ..TaskRecord::default()
        };
        r.core
            .store
            .append(&[crate::task_graph::record(&t).unwrap()])
            .unwrap();
        let tree = r.f.place.state.join("self").join("trees").join(id);
        std::fs::create_dir_all(&tree).unwrap();
        (sid, tree.canonicalize().unwrap())
    }

    /// Whether the session has a call waiting for the owner.
    fn asked(r: &Rig, sid: &str) -> bool {
        !r.core.pending_confirms(sid).unwrap().is_empty()
    }

    #[tokio::test]
    async fn the_self_task_reads_and_writes_its_own_tree_without_asking() {
        let r = held(|_| {});
        let (sid, tree) = task(&r, "sails");
        let file = tree.join("a.txt");
        let input = json!({"path": file, "content": "trimmed\n"});
        r.turn(&sid, "fs_write", input).await;
        assert!(!asked(&r, &sid), "a write in its own tree");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "trimmed\n");
        let said = r.turn(&sid, "fs_read", json!({"path": file})).await;
        assert!(!asked(&r, &sid), "a read in its own tree");
        assert!(said.contains("trimmed"), "{said}");
    }

    /// Planted reverts: each of these asks the owner, from a self session
    /// that can work in its own tree.
    #[tokio::test]
    async fn nothing_else_is_open_to_it() {
        let r = held(|_| {});
        let state = r.f.place.state.canonicalize().unwrap();
        let (_, other) = task(&r, "tides");
        std::fs::write(other.join("theirs.txt"), "not yours\n").unwrap();
        std::fs::write(state.join("notes.txt"), "the state dir\n").unwrap();
        std::fs::create_dir_all(state.join("store")).unwrap();
        std::fs::write(state.join("store/db"), "the store\n").unwrap();
        for (i, (what, target)) in [
            ("another task's tree", other.join("theirs.txt")),
            ("the rest of the state dir", state.join("notes.txt")),
            ("a link out to the state dir", state.join("notes.txt")),
            ("a link out to the store", state.join("store/db")),
            ("the store", state.join("store/db")),
        ]
        .into_iter()
        .enumerate()
        {
            let (sid, tree) = task(&r, &format!("sails{i}"));
            let path = if what.starts_with("a link") {
                std::os::unix::fs::symlink(&target, tree.join("link")).unwrap();
                tree.join("link")
            } else {
                target
            };
            r.turn(&sid, "fs_read", json!({"path": path})).await;
            assert!(asked(&r, &sid), "{what} waits for the owner");
            let reason = &r.core.pending_confirms(&sid).unwrap()[0].reason;
            assert!(reason.contains("fs.read — approve"), "{what}: {reason}");
        }
        // A write to the state dir's other files asks too.
        let (sid, _) = task(&r, "sails-w");
        let path = state.join("planted.txt");
        r.turn(&sid, "fs_write", json!({"path": path, "content": "x"}))
            .await;
        assert!(asked(&r, &sid), "a write to the rest of the state dir");
        assert!(!path.exists(), "and it did not run");
    }

    #[tokio::test]
    async fn a_session_that_is_not_self_asks_in_the_tree_too() {
        let r = held(|_| {});
        let (_, tree) = task(&r, "sails");
        std::fs::write(tree.join("a.txt"), "trimmed\n").unwrap();
        let sid = r.session("operator");
        r.turn(&sid, "fs_read", json!({"path": tree.join("a.txt")}))
            .await;
        assert!(asked(&r, &sid), "an operator's read of the tree");
        // And a self session with no task of its own has no tree to work in.
        let sid = r.session("self");
        r.turn(&sid, "fs_read", json!({"path": tree.join("a.txt")}))
            .await;
        assert!(asked(&r, &sid), "a self session of no task");
    }

    /// The rule loosens file tools only: a command in the tree still asks
    /// (its code runs in L1 or nowhere), and the sealed sets stay refused.
    #[tokio::test]
    async fn a_command_still_asks_and_the_sealed_sets_stay_refused() {
        let sealed = tempfile::tempdir().unwrap();
        let dir = sealed.path().canonicalize().unwrap();
        std::fs::write(dir.join("set.json"), "sealed\n").unwrap();
        let r = held(|cfg| cfg.self_improve.sealed.dir = Some(dir.display().to_string()));
        let (sid, tree) = task(&r, "sails");
        let input = json!({"argv": ["ls"], "cwd": tree});
        r.turn(&sid, "proc_run", input).await;
        assert!(asked(&r, &sid), "proc.run in the tree");
        let (sid, _) = task(&r, "sails2");
        let said = r
            .turn(&sid, "fs_read", json!({"path": dir.join("set.json")}))
            .await;
        assert!(said.contains("Not run"), "{said}");
        assert!(!said.contains("sealed\\n"), "{said}");
    }
}
