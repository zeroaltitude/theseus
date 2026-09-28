//! Consequences through the whole core (spec §3.9, P5c item 1): scripted
//! `proc.run` calls against a real repository whose remote is a local bare
//! repository, judged by the gate at each enforcement level. An irreversible
//! call parks for approval at every level; everything else keeps its old
//! treatment.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::node::{Body, ResultStatus};
use crate::policy::Enforcement;
use crate::provider::Scripted;
use crate::session::SessionRecord;
use crate::tests_m3::{results, rig_with, turn, Rig};

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Tester")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "Tester")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// The workspace becomes a repository with tracked work (`somedir`), an
/// ignored build output (`target`), and `origin`, a local bare repository
/// holding the first commit. Locally there is one more commit (a plain push
/// fast-forwards) and the first was amended (only a force push lands it).
struct World {
    remote: PathBuf,
}

impl World {
    fn new(root: &Path, diverged: bool) -> World {
        let w = |p: &str, s: &str| {
            let f = root.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, s).unwrap();
        };
        w(".gitignore", "target/\n");
        w("README.md", "one\n");
        w("somedir/a.txt", "tracked work\n");
        git(root, &["init", "-q", "-b", "main"]);
        git(root, &["add", "."]);
        git(root, &["commit", "-q", "-m", "first"]);
        let remote = root.parent().unwrap().join("remote.git");
        git(root, &["init", "-q", "--bare", remote.to_str().unwrap()]);
        git(root, &["remote", "add", "origin", remote.to_str().unwrap()]);
        git(root, &["push", "-q", "origin", "main"]);
        if diverged {
            w("README.md", "rewritten\n");
            git(
                root,
                &["commit", "-q", "-a", "--amend", "-m", "first, rewritten"],
            );
        } else {
            w("README.md", "two\n");
            git(root, &["commit", "-q", "-a", "-m", "second"]);
        }
        w("target/debug/app", "built\n");
        World { remote }
    }

    fn remote_head(&self) -> String {
        git(&self.remote, &["rev-parse", "main"])
    }
}

/// One scripted `proc.run`, then a closing text, at an enforcement level.
struct Run {
    rig: Rig,
    world: World,
    res: TurnSubmitResult,
    notices: Vec<Value>,
}

async fn run(level: Enforcement, argv: Value, diverged: bool) -> Run {
    let rig = rig_with(
        vec![
            Scripted::tools("", &[("t1", "proc_run", json!({ "argv": argv }))]),
            Scripted::text("Done."),
        ],
        |cfg| cfg.policy.enforcement = level,
    );
    let world = World::new(&rig.root, diverged);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    rig.core.store.put_session(&rec.session_id, &rec).unwrap();
    rig.core.bus.watch(&rec.session_id, "watcher", tx);
    let res = turn(&rig.core, Some(&rec.session_id), "go").await;
    let mut notices = vec![];
    while let Ok(m) = rx.try_recv() {
        if let theseus_protocol::Message::Notification(n) = m {
            if n.method == theseus_protocol::notify::POLICY_NOTIFIED {
                notices.push(n.params);
            }
        }
    }
    Run {
        rig,
        world,
        res,
        notices,
    }
}

impl Run {
    fn ledger(&self, kind: &str) -> Vec<Value> {
        self.rig
            .core
            .store
            .ledger_tail::<crate::ledger::LedgerRow>(400)
            .unwrap()
            .into_iter()
            .filter(|(_, r)| r.kind == kind)
            .map(|(_, r)| r.data)
            .collect()
    }

    /// The gate record on the call's `tool_call` node.
    fn gate(&self) -> Value {
        self.rig
            .core
            .store
            .session_nodes(&self.res.session_id)
            .unwrap()
            .into_iter()
            .find_map(|(_, n)| match n.body {
                Body::ToolCall { gate, .. } => Some(gate),
                _ => None,
            })
            .expect("a tool_call node")
    }

    /// It parked, irreversible, naming `kind`; the card, the ledger row, and
    /// the node all say so.
    fn assert_parked_irreversible(&self, kind: &str) {
        assert!(
            self.res.awaiting_confirm.is_some(),
            "must wait for approval: {:?}",
            self.res
        );
        assert_eq!(self.res.stop_reason, "awaiting_confirm");
        let pending = self
            .rig
            .core
            .pending_confirms(&self.res.session_id)
            .unwrap();
        assert_eq!(pending.len(), 1);
        let c = &pending[0];
        assert!(c.irreversible, "{c:?}");
        assert_eq!(c.consequences[0].kind, kind);
        assert!(c.consequences[0].irreversible);
        assert!(
            c.reason.starts_with(&format!("irreversible: {kind}")),
            "{}",
            c.reason
        );
        let row = &self.ledger("tool.confirm_requested")[0];
        assert_eq!(row["irreversible"], true);
        assert_eq!(row["consequences"][0]["kind"], kind);
        let gate = self.gate();
        assert_eq!(gate["decision"]["irreversible"], true);
        assert_eq!(gate["decision"]["consequences"][0]["kind"], kind);
        assert!(self.notices.is_empty(), "a wait is not a notice");
        assert!(results(&self.rig.core, &self.res.session_id).is_empty());
    }

    /// It ran, with the usual amber notice and no consequence named.
    fn assert_ran_with_notice(&self) {
        assert!(self.res.awaiting_confirm.is_none(), "{:?}", self.res);
        let rs = results(&self.rig.core, &self.res.session_id);
        assert_eq!(rs[0].0, ResultStatus::Ok, "{rs:?}");
        assert!(rs[0].1.contains("[exit code 0]"), "{}", rs[0].1);
        assert_eq!(self.notices.len(), 1, "{:?}", self.notices);
        assert_eq!(self.notices[0]["kind"], "approval_skipped");
        assert_eq!(self.notices[0]["setting"], "enforcement = notify");
        assert!(self.notices[0]
            .get("consequences")
            .is_none_or(|c| c.as_array().unwrap().is_empty()));
        assert_eq!(self.ledger("tool.notified").len(), 1);
        assert!(self.gate()["decision"].get("irreversible").is_none());
    }
}

fn have_git() -> bool {
    Command::new("git").arg("--version").output().is_ok()
}

#[tokio::test]
async fn notify_parks_a_force_push_until_approved_and_then_it_lands() {
    if !have_git() {
        return;
    }
    let r = run(
        Enforcement::Notify,
        json!(["git", "push", "--force", "origin", "main"]),
        true,
    )
    .await;
    let before = r.world.remote_head();
    r.assert_parked_irreversible("history_rewrite");
    assert_eq!(
        r.world.remote_head(),
        before,
        "nothing pushed while waiting"
    );
    // Approval still works: the approved force push runs on the continuation.
    let corr = r.res.awaiting_confirm.clone().unwrap();
    r.rig
        .core
        .confirm_action(&corr, true, None, "test")
        .unwrap();
    let exec = r.res.execution_id.clone().unwrap();
    let cont = r.rig.core.continue_execution(&exec).await.unwrap().unwrap();
    assert_eq!(cont.output, "Done.");
    let rs = results(&r.rig.core, &r.res.session_id);
    assert_eq!(rs[0].0, ResultStatus::Ok, "{rs:?}");
    assert_eq!(
        r.world.remote_head(),
        git(&r.rig.root, &["rev-parse", "main"]),
        "the rewritten history landed"
    );
}

#[tokio::test]
async fn notify_runs_a_plain_push_with_a_notice() {
    if !have_git() {
        return;
    }
    let r = run(
        Enforcement::Notify,
        json!(["git", "push", "origin", "main"]),
        false,
    )
    .await;
    r.assert_ran_with_notice();
    assert_eq!(
        r.world.remote_head(),
        git(&r.rig.root, &["rev-parse", "main"]),
        "the fast-forward landed"
    );
}

#[tokio::test]
async fn notify_parks_a_recursive_delete_of_tracked_work() {
    if !have_git() {
        return;
    }
    let r = run(Enforcement::Notify, json!(["rm", "-rf", "somedir"]), false).await;
    r.assert_parked_irreversible("bulk_delete");
    assert!(r.rig.root.join("somedir/a.txt").exists());
}

#[tokio::test]
async fn notify_runs_a_recursive_delete_of_an_ignored_build_output_with_a_notice() {
    if !have_git() {
        return;
    }
    let r = run(Enforcement::Notify, json!(["rm", "-rf", "target"]), false).await;
    r.assert_ran_with_notice();
    assert!(
        !r.rig.root.join("target").exists(),
        "the build output is gone"
    );
}

#[tokio::test]
async fn notify_parks_a_force_push_inside_a_shell_string() {
    if !have_git() {
        return;
    }
    let r = run(
        Enforcement::Notify,
        json!(["bash", "-c", "git push -f origin main"]),
        true,
    )
    .await;
    r.assert_parked_irreversible("history_rewrite");
    let pending = r.rig.core.pending_confirms(&r.res.session_id).unwrap();
    assert_eq!(pending[0].consequences[0].detail, "git push -f origin main");
}

#[tokio::test]
async fn open_still_parks_a_force_push() {
    if !have_git() {
        return;
    }
    let r = run(
        Enforcement::Open,
        json!(["git", "push", "--force", "origin", "main"]),
        true,
    )
    .await;
    let before = r.world.remote_head();
    r.assert_parked_irreversible("history_rewrite");
    assert_eq!(r.world.remote_head(), before);
}

#[tokio::test]
async fn strict_changes_nothing_else() {
    if !have_git() {
        return;
    }
    // Every proc.run waited under strict before consequences existed, and still
    // does; only the irreversible mark is new.
    for (argv, irreversible) in [
        (json!(["git", "push", "origin", "main"]), false),
        (json!(["rm", "-rf", "target"]), false),
        (json!(["git", "push", "--force", "origin", "main"]), true),
    ] {
        let r = run(Enforcement::Strict, argv.clone(), false).await;
        assert!(r.res.awaiting_confirm.is_some(), "{argv}");
        let pending = r.rig.core.pending_confirms(&r.res.session_id).unwrap();
        assert_eq!(pending[0].irreversible, irreversible, "{argv}");
        assert_eq!(pending[0].consequences.is_empty(), !irreversible, "{argv}");
        assert!(r.notices.is_empty());
    }
    // And a read still runs unasked.
    let rig = rig_with(
        vec![
            Scripted::tools("", &[("t1", "fs_read", json!({"path": "README.md"}))]),
            Scripted::text("Read."),
        ],
        |cfg| cfg.policy.enforcement = Enforcement::Strict,
    );
    World::new(&rig.root, false);
    let res = turn(&rig.core, None, "read").await;
    assert!(res.awaiting_confirm.is_none());
    assert_eq!(results(&rig.core, &res.session_id)[0].0, ResultStatus::Ok);
}

/// A `tool_call` node as the gate wrote it before consequences existed: no
/// `consequences`, no `rules`, and at `notify` a proc.run ran with a notice.
fn node_from_before(root: &Path, session_id: &str, argv: &[&str]) -> crate::node::Node {
    let summary = format!("run `{}` in {}", argv.join(" "), root.display());
    let rule = format!("{summary}: proc.run is `confirm` for run tools");
    crate::node::Node::tool_call(
        session_id,
        Some("turn_before"),
        Some(0),
        Body::ToolCall {
            tool_use_id: crate::new_id("toolu"),
            tool: "proc.run".into(),
            wire_name: "proc_run".into(),
            input: json!({ "argv": argv }),
            assistant_node: "asst_before".into(),
            correlation_id: Some(crate::new_id("act")),
            gate: json!({
                "result": {"gate": "allow"},
                "validated": true,
                "decision": {"mode": "allow", "reason": format!("ran without approval: {rule}"),
                    "notify": {"kind": "approval_skipped", "setting": "enforcement = notify", "rule": rule}},
                "plan": {"resources": [{"path": root, "access": "exec"}], "argv": argv, "summary": summary},
            }),
        },
    )
}

#[tokio::test]
async fn replay_judges_past_calls_with_the_current_rules_and_writes_nothing() {
    if !have_git() {
        return;
    }
    // Today's gate parked this one; replay finds nothing to change about it.
    let r = run(
        Enforcement::Notify,
        json!(["git", "push", "--force", "origin", "main"]),
        true,
    )
    .await;
    // Two calls from before the rule table: a force push that ran with only a
    // notice, and a plain push.
    let store = &r.rig.core.store;
    let sid = &r.res.session_id;
    for argv in [
        &["git", "push", "--force", "origin", "main"][..],
        &["git", "push", "origin", "main"][..],
    ] {
        let n = node_from_before(&r.rig.root, sid, argv);
        store.append(vec![n.record().unwrap()]).unwrap();
    }
    let (nodes, rows) = (store.node_count().unwrap(), store.ledger_len().unwrap());
    let out = r.rig.core.policy_replay(10).unwrap();
    assert_eq!((out.examined, out.changed), (3, 1), "{out:#?}");
    assert_eq!(out.rules_version, theseus_tools::consequence::RULES_VERSION);
    assert_eq!(out.enforcement, "notify");
    let c = &out.calls[0];
    assert_eq!(c.tool, "proc.run");
    assert!(c.summary.contains("git push --force"), "{}", c.summary);
    assert_eq!(c.then.treatment, "ran without approval");
    assert!(c.then.consequences.is_empty() && c.then.rules.is_none());
    assert_eq!(c.now.treatment, "waited");
    assert_eq!(c.now.consequences[0].kind, "history_rewrite");
    assert!(c.now.consequences[0].irreversible);
    assert_eq!(
        (store.node_count().unwrap(), store.ledger_len().unwrap()),
        (nodes, rows),
        "replay is read-only"
    );
    let limited = r.rig.core.policy_replay(1).unwrap();
    assert_eq!(
        (limited.examined, limited.changed),
        (1, 0),
        "only the newest call"
    );
    // The rule table as the operator sees it.
    let rules = r.rig.core.tools.policy.rules_info();
    assert!(rules
        .kinds
        .iter()
        .any(|k| k.name == "history_rewrite" && k.irreversible && k.source == "built_in"));
    assert!(rules
        .rules
        .iter()
        .any(|x| x.id == "git.push.force" && !x.must.is_empty()));
}

// ---------------------------------------------------------------- step 2a+

/// Under `notify`, the floor refuses a program it reaches through a wrapper, and
/// the refusal is the tool result the model sees (spec §3.9, step 2a+).
#[tokio::test]
async fn notify_refuses_env_theseusd_version_and_the_model_hears_it() {
    let rig = rig_with(
        vec![
            Scripted::tools(
                "",
                &[(
                    "t1",
                    "proc_run",
                    json!({"argv": ["env", "theseusd", "--version"]}),
                )],
            ),
            Scripted::text("Noted."),
        ],
        |cfg| cfg.policy.enforcement = Enforcement::Notify,
    );
    let res = turn(&rig.core, None, "check the version").await;
    assert!(
        res.awaiting_confirm.is_none(),
        "the floor refuses, it does not park"
    );
    let rs = results(&rig.core, &res.session_id);
    assert_eq!(rs[0].0, ResultStatus::Denied, "{rs:?}");
    assert!(
        rs[0].1.contains("floor") && rs[0].1.contains("theseusd"),
        "the refusal names the floor and reaches the model: {}",
        rs[0].1
    );
}

/// Under `notify`, a brace-spelled force push inside `bash -c` parks as
/// irreversible, and the local bare remote does not move.
#[tokio::test]
async fn notify_parks_a_brace_force_push_and_the_remote_does_not_move() {
    if !have_git() {
        return;
    }
    let r = run(
        Enforcement::Notify,
        json!(["bash", "-c", "git push origin main --{force,}"]),
        true,
    )
    .await;
    let before = r.world.remote_head();
    r.assert_parked_irreversible("history_rewrite");
    assert_eq!(
        r.world.remote_head(),
        before,
        "nothing pushed while waiting"
    );
}

/// Under `open`, a floor path named as an argument is refused as floor — where
/// before it ran with only a red notice. The store sits at `../store` from the
/// working directory, so no absolute path needs to be known ahead of the rig.
#[tokio::test]
async fn open_refuses_cat_of_a_store_file_as_floor() {
    let r = run(
        Enforcement::Open,
        json!(["cat", "../store/some-node"]),
        false,
    )
    .await;
    assert!(
        r.res.awaiting_confirm.is_none(),
        "the floor refuses, it does not park"
    );
    let rs = results(&r.rig.core, &r.res.session_id);
    assert_eq!(rs[0].0, ResultStatus::Denied, "{rs:?}");
    assert!(
        rs[0].1.contains("floor"),
        "open does not lift the floor: {}",
        rs[0].1
    );
}
