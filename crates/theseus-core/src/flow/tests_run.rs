//! A flow through the whole core, in a temp git repository with an upstream
//! (invented names): the model calls `flow__repo_land`, and each step is a
//! `proc.run` that goes the model's own way (gate, posture, job, rows).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

use super::spec::repo_id;
use super::tests::land;
use crate::node::{Body, ResultStatus};
use crate::provider::{ProviderRequest, Scripted};
use crate::tests_places::{answers_a_call, rig_scripted, session, turn, Rig};

/// `git` in `dir`, on nobody's config; its stdout.
pub(super) fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

pub(super) struct Repo {
    pub(super) work: PathBuf,
    pub(super) origin: PathBuf,
}

impl Repo {
    /// The work tree `work` made a repository on `main`, tracking a bare
    /// origin beside it.
    pub(super) fn init(work: &Path) -> Repo {
        let origin = work.parent().unwrap().join("origin.git");
        std::fs::create_dir_all(&origin).unwrap();
        git(&origin, &["init", "-q", "--bare", "-b", "main"]);
        git(work, &["init", "-q", "-b", "main"]);
        for (k, v) in [
            ("user.name", "Harbor Tester"),
            ("user.email", "tester@harbor.invalid"),
            ("rerere.enabled", "true"),
        ] {
            git(work, &["config", k, v]);
        }
        std::fs::write(work.join("a.txt"), "one\n").unwrap();
        git(work, &["add", "-A"]);
        git(work, &["commit", "-qm", "base"]);
        git(work, &["remote", "add", "origin", origin.to_str().unwrap()]);
        git(work, &["push", "-q", "-u", "origin", "main"]);
        Repo {
            work: work.into(),
            origin,
        }
    }

    /// Branch `name` off main with `file` holding `text`; back on main.
    pub(super) fn branch(&self, name: &str, file: &str, text: &str) {
        git(&self.work, &["checkout", "-q", "-b", name, "main"]);
        std::fs::write(self.work.join(file), text).unwrap();
        git(&self.work, &["add", "-A"]);
        git(&self.work, &["commit", "-qm", name]);
        git(&self.work, &["checkout", "-q", "main"]);
    }

    /// A commit on main of `text` in `file`, pushed.
    fn main_commit(&self, file: &str, text: &str) {
        std::fs::write(self.work.join(file), text).unwrap();
        git(&self.work, &["commit", "-qam", "main moves"]);
        git(&self.work, &["push", "-q"]);
    }

    fn origin_head(&self) -> String {
        git(&self.origin, &["rev-parse", "main"])
    }

    fn work_head(&self) -> String {
        git(&self.work, &["rev-parse", "main"])
    }
}

/// The model calls the flow `tool` once with `input`, then ends.
fn calls_named(
    tool: &'static str,
    input: Value,
) -> impl Fn(&ProviderRequest) -> Scripted + Send + Sync + 'static {
    move |req| {
        if answers_a_call(req) {
            Scripted::text("Done.")
        } else {
            Scripted::tools("Landing.", &[("L1", tool, input.clone())])
        }
    }
}

fn calls(input: Value) -> impl Fn(&ProviderRequest) -> Scripted + Send + Sync + 'static {
    calls_named("flow__repo_land", input)
}

fn rig(script: impl Fn(&ProviderRequest) -> Scripted + Send + Sync + 'static) -> (Rig, Repo) {
    let r = rig_scripted(
        |c| {
            c.policy.tools.remove("proc.run");
            c.tools.defer.enabled = false;
        },
        script,
    );
    let repo = Repo::init(&r.root);
    (r, repo)
}

/// Load `spec` as the owner would.
pub(super) fn load(r: &Rig, spec: &Value) -> theseus_protocol::flow::FlowInfo {
    let path = r.root.parent().unwrap().join(format!(
        "{}.flow.json",
        spec["name"].as_str().unwrap_or("flow")
    ));
    std::fs::write(&path, spec.to_string()).unwrap();
    r.core.flow_load(&path, "test", "test").unwrap()
}

fn spec_for(r: &Rig) -> Value {
    land(&repo_id(&r.root))
}

/// The status, text and meta of the result for call `id`.
pub(super) fn result(r: &Rig, sid: &str, id: &str) -> (ResultStatus, String, Value) {
    r.core
        .store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match n.body {
            Body::ToolResult {
                tool_use_id,
                status,
                content,
                meta,
                late: false,
                ..
            } if tool_use_id == id => Some((status, content, meta)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no result for {id}"))
}

/// The rows of ledger kind `kind`, oldest first.
fn rows(r: &Rig, kind: &str) -> Vec<Value> {
    r.core
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(5000)
        .unwrap()
        .into_iter()
        .filter(|(_, row)| row.kind == kind)
        .map(|(_, row)| row.data)
        .collect()
}

/// Run a turn that calls the flow and return its result.
async fn land_it(r: &Rig) -> (ResultStatus, String, Value) {
    let sid = session(&r.core, None);
    turn(&r.core, &sid, "land it").await;
    result(r, &sid, "L1")
}

fn handback(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|e| panic!("not JSON ({e}): {text}"))
}

#[tokio::test]
async fn a_clean_merge_is_tested_merged_retested_and_pushed() {
    let (r, repo) = rig(calls(json!({"branch": "topic"})));
    repo.branch("topic", "b.txt", "tide\n");
    load(&r, &spec_for(&r));
    let (status, text, meta) = land_it(&r).await;
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert_eq!(text, "landed topic");
    assert_eq!(meta["exit"], "ok");
    let ids: Vec<&str> = meta["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["step"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["test", "merge", "retest", "push"]);
    assert_eq!(repo.origin_head(), repo.work_head(), "pushed");
    assert!(git(&repo.work, &["log", "--format=%s", "-1"]).starts_with("Merge branch 'topic'"));
    let ran = rows(&r, "flow.ran");
    assert_eq!(ran.len(), 1);
    assert_eq!(ran[0]["exit"], "ok");
    assert_eq!(ran[0]["steps"], 4);
    assert_eq!(ran[0]["name"], "repo_land");
}

#[tokio::test]
async fn a_conflict_rerere_knows_is_resolved_committed_and_pushed() {
    let (r, repo) = rig(calls(json!({"branch": "topic"})));
    repo.branch("topic", "a.txt", "topic\n");
    repo.main_commit("a.txt", "main\n");
    // Teach rerere the resolution once, then take the merge back.
    let w = &repo.work;
    let before = repo.work_head();
    let _ = Command::new("git")
        .arg("-C")
        .arg(w)
        .args(["merge", "--no-ff", "topic"])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    std::fs::write(w.join("a.txt"), "resolved\n").unwrap();
    git(w, &["add", "a.txt"]);
    git(w, &["commit", "-qm", "taught"]);
    git(w, &["reset", "-q", "--hard", &before]);
    load(&r, &spec_for(&r));
    let (status, text, meta) = land_it(&r).await;
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert_eq!(text, "landed topic");
    let ids: Vec<&str> = meta["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["step"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        ["test", "merge", "rerere", "stage", "commit", "retest", "push"]
    );
    assert_eq!(
        std::fs::read_to_string(w.join("a.txt")).unwrap(),
        "resolved\n"
    );
    assert_eq!(repo.origin_head(), repo.work_head());
    assert_ne!(repo.work_head(), before);
}

#[tokio::test]
async fn a_conflict_rerere_does_not_know_is_handed_back_with_its_files() {
    let (r, repo) = rig(calls(json!({"branch": "topic"})));
    repo.branch("topic", "a.txt", "topic\n");
    repo.main_commit("a.txt", "main\n");
    let before = repo.origin_head();
    load(&r, &spec_for(&r));
    let (status, text, meta) = land_it(&r).await;
    assert_eq!(status, ResultStatus::Ok, "{text}");
    let h = handback(&text);
    assert_eq!(h["handback"], "conflicts-left");
    assert_eq!(h["step"], "rerere");
    assert_eq!(h["state"]["conflict_files"], json!(["a.txt"]));
    assert_eq!(h["state"]["rerere_resolved"], json!([]));
    assert!(h["state"]["output_tail"]
        .as_str()
        .unwrap()
        .contains("a.txt"));
    assert_eq!(meta["exit"], "handback:conflicts-left");
    assert_eq!(repo.origin_head(), before, "nothing was pushed");
    assert_eq!(rows(&r, "flow.ran")[0]["exit"], "handback:conflicts-left");
}

#[tokio::test]
async fn red_tests_stop_the_flow_before_any_merge() {
    let (r, repo) = rig(calls(json!({"branch": "topic", "test": ["false"]})));
    repo.branch("topic", "b.txt", "tide\n");
    let (head, origin) = (repo.work_head(), repo.origin_head());
    load(&r, &spec_for(&r));
    let (status, text, meta) = land_it(&r).await;
    assert_eq!(status, ResultStatus::Ok, "{text}");
    assert_eq!(text, "the tests are red: nothing of topic is pushed");
    assert_eq!(meta["exit"], "tests-red");
    assert_eq!(meta["steps"].as_array().unwrap().len(), 1, "one step ran");
    assert_eq!((repo.work_head(), repo.origin_head()), (head, origin));
}

/// A step the place refuses stops the flow with the refusal as its result:
/// here the sealed holdout dir, which no call may reach. The step after it
/// never runs.
#[tokio::test]
async fn a_step_refused_by_policy_stops_the_flow() {
    let r = rig_scripted(
        |c| {
            c.policy.tools.remove("proc.run");
            c.tools.defer.enabled = false;
            // Beside the work tree: a relative path reaches it.
            let sealed = std::path::Path::new(&c.server.state_dir).join("sealed");
            std::fs::create_dir_all(&sealed).unwrap();
            c.self_improve.sealed.dir = Some(sealed.to_string_lossy().into_owned());
        },
        calls_named("flow__peek", json!({})),
    );
    let _ = Repo::init(&r.root);
    let mut spec = spec_for(&r);
    spec["name"] = json!("peek");
    spec["inputs"] = json!({});
    spec["steps"] = json!([
        {"id": "first", "tool": "proc.run", "args": {"argv": ["true"]}},
        {"id": "read", "tool": "fs.read", "args": {"path": "../sealed/item.json"}},
        {"id": "never", "tool": "proc.run", "args": {"argv": ["true"]}}
    ]);
    spec["exits"] = json!({});
    load(&r, &spec);
    let sid = session(&r.core, None);
    turn(&r.core, &sid, "peek").await;
    let (status, text, _) = result(&r, &sid, "L1");
    assert_eq!(status, ResultStatus::Error, "{text}");
    assert!(text.contains("flow peek stopped at step read"), "{text}");
    assert!(text.contains("Not run"), "{text}");
    result(&r, &sid, "L1.1");
    result(&r, &sid, "L1.2");
    let n = r
        .core
        .store
        .session_nodes(&sid)
        .unwrap()
        .into_iter()
        .filter(|(_, n)| matches!(&n.body, Body::ToolResult { tool_use_id, .. } if tool_use_id.starts_with("L1.")))
        .count();
    assert_eq!(n, 2, "the step after the refusal never ran");
    let ran = rows(&r, "flow.ran");
    assert_eq!(
        (ran[0]["exit"].as_str(), ran[0]["steps"].as_u64()),
        (Some("refused"), Some(2))
    );
}

/// A step the model's own call would wait on does not run and is not asked
/// by the flow: it is handed back as the exact call, which the model makes
/// itself and so waits as any call does.
#[tokio::test]
async fn a_step_that_would_wait_is_handed_back_as_the_exact_call() {
    let r = rig_scripted(
        |c| {
            // Everything waits, the flow's own call included, unless the gate
            // knows that a flow's call only starts steps.
            c.policy.enforcement = crate::policy::Posture::Approve;
            c.policy
                .tools
                .insert("proc.run".into(), crate::policy::Posture::Approve);
            c.tools.defer.enabled = false;
        },
        calls(json!({"branch": "topic"})),
    );
    let repo = Repo::init(&r.root);
    repo.branch("topic", "b.txt", "tide\n");
    let before = repo.origin_head();
    load(&r, &spec_for(&r));
    let (status, text, meta) = land_it(&r).await;
    assert_eq!(status, ResultStatus::Ok, "{text}");
    let h = handback(&text);
    assert_eq!(h["handback"], "needs-approval");
    assert_eq!(h["step"], "test");
    assert_eq!(h["state"]["call"]["tool"], "proc_run");
    assert_eq!(h["state"]["call"]["input"]["argv"], json!(["true"]));
    assert_eq!(meta["steps"], json!([]), "nothing ran");
    assert_eq!(repo.origin_head(), before);
    assert!(
        r.core.confirm_list().unwrap().is_empty(),
        "the flow asked nothing"
    );
}
