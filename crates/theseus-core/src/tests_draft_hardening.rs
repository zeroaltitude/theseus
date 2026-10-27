//! The draft review's hardening (theseus-b0ozi): unsettled drafts, the git a
//! restricted model's tree cannot steer, one step of a draft at a time, and a
//! draft that did not open leaving nothing behind. The rig is `tests_draft`'s.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::draft::{self, meta_key};
use crate::tests_dm_rig::{say, try_say};
use crate::tests_draft::{draft_edit, git, rig, Draft, ASK};

/// A draft run to its end, and its draft session and review.
async fn finished(d: &Draft) -> (String, String, String) {
    d.plan.lock().unwrap().draft_call = Some(draft_edit());
    let sid = d.dm(&d.repo);
    say(&d.r.core, &sid, ASK, None).await;
    d.drive();
    d.settled().await;
    let tasks = d.tasks(&sid);
    (
        sid,
        tasks[0].session_id.clone(),
        tasks[1].session_id.clone(),
    )
}

/// Leave the draft's record as a crash before the settling frame would.
fn unsettle(d: &Draft, drafted: &str) {
    let core = &d.r.core;
    let mut rec: draft::Draft = core.store.get_meta(&meta_key(drafted)).unwrap().unwrap();
    assert!(rec.settled);
    rec.settled = false;
    core.store.put_meta(&meta_key(drafted), &rec).unwrap();
}

fn settled_in_store(d: &Draft, drafted: &str) -> bool {
    let rec: draft::Draft =
        d.r.core
            .store
            .get_meta(&meta_key(drafted))
            .unwrap()
            .unwrap();
    rec.settled
}

/// A draft whose review ended but whose record is not settled (a death
/// between the review's end frame and the apply) is settled once by the start
/// sweep, with one more `task.draft` row, and by no second sweep.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unsettled_draft_is_settled_at_start() {
    let d = rig(|_| {});
    let (_, drafted, _) = finished(&d).await;
    assert_eq!(d.drafts().len(), 1);
    unsettle(&d, &drafted);
    d.r.core.runner.sweep_drafts(None).await;
    assert!(settled_in_store(&d, &drafted));
    assert_eq!(d.drafts().len(), 2, "settled once");
    d.r.core.runner.sweep_drafts(None).await;
    assert_eq!(d.drafts().len(), 2, "and not again");
}

/// A conversation cancelled while its draft runs: the draft's end finds no
/// parent to open a review under, and settles the draft unreviewed instead of
/// leaving it for ever (the sweep after a cancel is the same step again).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_draft_of_a_cancelled_conversation_is_settled_unreviewed() {
    let d = rig(|_| {});
    d.plan.lock().unwrap().draft_call = Some(draft_edit());
    let sid = d.dm(&d.repo);
    say(&d.r.core, &sid, ASK, None).await;
    let rec: crate::session::SessionRecord = d.r.core.store.get_session(&sid).unwrap().unwrap();
    d.r.core
        .cancel_execution(rec.execution_id.as_deref().unwrap(), "test")
        .await
        .unwrap();
    d.drive();
    let row = d.settled().await;
    assert!(row["review_task"].is_null(), "{row}");
    assert_eq!(row["applied"], false, "{row}");
    assert!(
        row["why"]
            .as_str()
            .unwrap()
            .contains("conversation was cancelled"),
        "{row}"
    );
    assert_eq!(d.file(), "alpha\nbeta\n", "nothing applied");
    assert_eq!(d.tasks(&sid).len(), 1, "no review opened");
}

/// Two ends of one draft at once (a cancel and the turn's own end) settle it
/// once: one row, not two.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_ends_at_once_settle_one_draft() {
    let d = rig(|_| {});
    let (_, drafted, review) = finished(&d).await;
    unsettle(&d, &drafted);
    let runner = &d.r.core.runner;
    tokio::join!(runner.draft_ended(&review), runner.draft_ended(&review));
    assert_eq!(d.drafts().len(), 2, "one more row, not two");
    assert!(settled_in_store(&d, &drafted));
}

/// An executable that leaves a file when git runs it.
fn marker_script(dir: &Path, name: &str) -> (PathBuf, PathBuf) {
    let (script, mark) = (dir.join(name), dir.join(format!("{name}.ran")));
    std::fs::write(&script, format!("#!/bin/sh\ntouch {}\n", mark.display())).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    (script, mark)
}

/// A restricted model's tree cannot name the git dir the core's git runs in:
/// its `.git` file replaced, to a directory with a hook and a config that run
/// programs, the core commits, diffs and applies in the dir recorded when the
/// tree was made, and none of the programs runs.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_draft_trees_git_config_changes_nothing_the_core_runs() {
    let d = rig(|_| {});
    let scratch = tempfile::tempdir().unwrap();
    git(scratch.path(), &["init", "-q", "evil"]);
    let evil = scratch.path().join("evil/.git");
    let (hook, hooked) = marker_script(scratch.path(), "hook");
    let (monitor, monitored) = marker_script(scratch.path(), "monitor");
    std::fs::create_dir_all(evil.join("hooks")).unwrap();
    std::fs::copy(&hook, evil.join("hooks/post-commit")).unwrap();
    let mut config = std::fs::read_to_string(evil.join("config")).unwrap();
    config.push_str(&format!("[core]\n\tfsmonitor = {}\n", monitor.display()));
    std::fs::write(evil.join("config"), config).unwrap();
    let at = evil.clone();
    d.plan.lock().unwrap().tamper = Some(Box::new(move |tree: &Path| {
        std::fs::write(tree.join(".git"), format!("gitdir: {}\n", at.display())).unwrap();
    }));
    d.plan.lock().unwrap().draft_call = Some(draft_edit());
    let sid = d.dm(&d.repo);
    say(&d.r.core, &sid, ASK, None).await;
    d.drive();
    let row = d.settled().await;
    assert!(!hooked.exists(), "the tree's hook ran");
    assert!(!monitored.exists(), "the tree's fsmonitor ran");
    assert_eq!(row["applied"], true, "{row}");
    assert_eq!(d.file(), "alpha\ngamma\n");
}

/// A config the core's git reads (here the repository's own) cannot choose a
/// program either: `core.fsmonitor` and `diff.external` are switched off on
/// every call, so a draft still applies and neither ran.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_cores_git_runs_no_program_a_config_names() {
    let d = rig(|_| {});
    let scratch = tempfile::tempdir().unwrap();
    let (monitor, monitored) = marker_script(scratch.path(), "monitor");
    let (external, externed) = marker_script(scratch.path(), "external");
    d.plan.lock().unwrap().draft_call = Some(draft_edit());
    let sid = d.dm(&d.repo);
    say(&d.r.core, &sid, ASK, None).await;
    git(
        &d.repo,
        &["config", "core.fsmonitor", &monitor.to_string_lossy()],
    );
    git(
        &d.repo,
        &["config", "diff.external", &external.to_string_lossy()],
    );
    d.drive();
    let row = d.settled().await;
    assert!(!monitored.exists(), "core.fsmonitor ran");
    assert!(!externed.exists(), "diff.external ran");
    assert_eq!(row["applied"], true, "{row}");
    assert_eq!(d.file(), "alpha\ngamma\n");
}

/// A draft that cannot open (the conversation's limit leaves nothing to
/// carve) leaves neither its tree nor its branch: the turn runs as before.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_open_leaves_no_branch() {
    let d = rig(|cfg| {
        cfg.kernel.spend_limit_mode = crate::config::SpendLimitMode::Ask;
        cfg.kernel.spend_limit_usd = 0.000_001;
    });
    let sid = d.dm(&d.repo);
    let res = try_say(&d.r.core, &sid, ASK, None).await;
    assert!(
        res.as_ref().map_or(true, |r| r.stop_reason != "drafted"),
        "{res:?}"
    );
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert_eq!(git(&d.repo, &["branch", "--list", "draft/*"]), "");
    assert_eq!(
        git(&d.repo, &["worktree", "list", "--porcelain"])
            .matches("worktree ")
            .count(),
        1
    );
    let _ = Arc::strong_count(&d.r.core);
}
