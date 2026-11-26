//! The self-join through the whole core (theseus-pw1q.9), over
//! `tests_tree`'s throwaway repository: a bare upstream (the remote) with a
//! stand-in `scripts/gate.sh` on main, a clone of it as `[self] repo`, a
//! stand-in gpg the repo's config names (so a merge is signed without a real
//! key), stand-in tests (`tests/*.sh`, run with bash), and a seeded green
//! self branch with its `self.branch.built` row, as self-pr leaves one.

use crate::rsi::join::tests_publish::PassYes as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use theseus_protocol::rsi::{SelfHaltParams, SelfMode, SelfResumeParams};
use theseus_protocol::tasks::{TaskRecord, TaskState};
use theseus_protocol::SessionKind;

use super::install::{self, Installer};
use super::planted::{Ran, Tests};
use super::review::{Ask, Fut, Review, Reviewer};
use super::{lock, Seams};
use crate::approval::Answerer;
use crate::fact::rsi_pr::SelfBranchBuilt;
use crate::policy::Posture;
use crate::provider::{
    DeltaSink, FakeProvider, Provider, ProviderFuture, ProviderRequest, Scripted,
};
use crate::rsi::tests_tree::{fixture, hermetic, Fixture};
use crate::rsi::tree::{Place, Tree};
use crate::session::{SessionRecord, TargetRef};
use crate::store::Store;
use crate::{Config, Core};

pub(crate) const FAKE_GPG: &str = "#!/bin/bash\n\
echo '[GNUPG:] KEY_CONSIDERED FIXTURE 0' >&2\n\
echo '[GNUPG:] SIG_CREATED D 1 8 00 1792000000 FIXTURE' >&2\n\
cat >/dev/null\n\
printf -- '-----BEGIN PGP SIGNATURE-----\\n\\nZml4dHVyZQ==\\n-----END PGP SIGNATURE-----\\n'\n";

/// The model's answer to every request, and every request it saw.
struct Says {
    text: String,
    requests: Mutex<Vec<ProviderRequest>>,
}

impl Provider for Says {
    fn name(&self) -> &str {
        "fake"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        self.requests.lock().unwrap().push(req.clone());
        let text = self.text.clone();
        Box::pin(async move {
            FakeProvider::scripted(vec![Scripted::text(&text)])
                .stream_message(req, on_delta)
                .await
        })
    }
}

/// An installer that opens and builds its tree the way the machine's own
/// does — [`install::built_tree`], so the tree's lifecycle under test is the
/// real one — with a stand-in for the L1 build in place of cargo, and
/// neither the unit nor the restart. Each build tree it was left with.
#[derive(Default)]
pub(super) struct Builds {
    targets: Mutex<Vec<PathBuf>>,
}

impl Installer for Builds {
    fn start(&self, p: &Place, merge: &str) -> anyhow::Result<install::Started> {
        let target = install::built_tree(p, merge, &|_, t, _| {
            // The tree is open while its build runs, and is a worktree.
            assert!(
                t.dir.join(".git").exists(),
                "{} is no worktree",
                t.dir.display()
            );
            // What `scripts/build.sh --profile release` leaves behind: the
            // binaries in the build tree, which is its `CARGO_TARGET_DIR`.
            let out = t.target.join("release");
            std::fs::create_dir_all(&out)?;
            for b in install::BINARIES {
                std::fs::write(out.join(b), format!("fake {b}"))?;
            }
            Ok(())
        })?;
        self.targets.lock().unwrap().push(target);
        Ok(install::Started::Unit(
            "theseus-self-install-stand-in".into(),
        ))
    }
}

/// A reviewer that says `accept` and counts its asks.
#[derive(Default)]
struct Stand {
    asks: Mutex<Vec<Ask>>,
}

impl Reviewer for Stand {
    fn review<'a>(&'a self, _core: &'a Arc<Core>, ask: &'a Ask) -> Fut<'a> {
        self.asks.lock().unwrap().push(ask.clone());
        // It ran where it was asked to.
        let (profile, model) = (ask.target.profile.clone(), ask.target.model.clone());
        Box::pin(async {
            Ok(Review {
                accept: true,
                fix: false,
                reasons: vec!["the stand-in accepts".into()],
                cost_usd: 0.25,
                session: None,
                profile,
                model,
            })
        })
    }
}

/// `tests/*.sh`, each run with bash in the tree: all must exit 0.
pub(crate) struct Scripts;

impl Tests for Scripts {
    fn run(&self, _p: &Place, t: &Tree, tests: &[String]) -> anyhow::Result<(Ran, String)> {
        let mut ok = true;
        for name in tests {
            let s = Command::new("bash")
                .arg(name)
                .current_dir(&t.dir)
                .status()?;
            ok &= s.success();
        }
        let ran = if ok { Ran::Passed } else { Ran::Failed };
        Ok((ran, format!("ran {}", tests.join(" "))))
    }
}

pub(super) fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .envs(hermetic())
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

pub(crate) struct Rig {
    pub(crate) core: Arc<Core>,
    says: Arc<Says>,
    pub(crate) f: Fixture,
    stand: Arc<Stand>,
    _work: tempfile::TempDir,
}

/// A reviewer that says `accept` after a while, and counts its asks and
/// how many ran at once (the self pipeline's test, theseus-pw1q.16).
#[derive(Default)]
pub(crate) struct Slow {
    asks: std::sync::atomic::AtomicUsize,
    now: std::sync::atomic::AtomicUsize,
    most: std::sync::atomic::AtomicUsize,
}

impl Slow {
    pub(crate) fn asks(&self) -> usize {
        self.asks.load(std::sync::atomic::Ordering::SeqCst)
    }
    pub(crate) fn most(&self) -> usize {
        self.most.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl Reviewer for Slow {
    fn review<'a>(&'a self, _core: &'a Arc<Core>, ask: &'a Ask) -> Fut<'a> {
        use std::sync::atomic::Ordering::SeqCst;
        let (profile, model) = (ask.target.profile.clone(), ask.target.model.clone());
        self.asks.fetch_add(1, SeqCst);
        let now = self.now.fetch_add(1, SeqCst) + 1;
        self.most.fetch_max(now, SeqCst);
        Box::pin(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            self.now.fetch_sub(1, SeqCst);
            Ok(Review {
                accept: true,
                fix: false,
                reasons: vec!["the slow stand-in accepts".into()],
                cost_usd: 0.25,
                session: None,
                profile,
                model,
            })
        })
    }
}

pub(crate) fn rig(mode: SelfMode) -> Rig {
    rig_with(mode, |_| {})
}

/// [`rig`] with `edit` applied to the config before the core is built.
pub(crate) fn rig_with(mode: SelfMode, edit: impl FnOnce(&mut Config)) -> Rig {
    rig_parts(mode, edit, |_| {})
}

/// [`rig_with`], with `parts` edited before the core is built.
pub(crate) fn rig_parts(
    mode: SelfMode,
    edit: impl FnOnce(&mut Config),
    parts: impl FnOnce(&mut crate::rpc::Parts),
) -> Rig {
    rig_on(fixture(), mode, edit, parts)
}

/// [`rig_parts`] over a fixture the test made (a gate of its own).
pub(crate) fn rig_on(
    f: Fixture,
    mode: SelfMode,
    edit: impl FnOnce(&mut Config),
    parts: impl FnOnce(&mut crate::rpc::Parts),
) -> Rig {
    let root = f.place.state.parent().unwrap().to_path_buf();
    let gpg = root.join("fake-gpg.sh");
    std::fs::write(&gpg, FAKE_GPG).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&gpg, std::fs::Permissions::from_mode(0o755)).unwrap();
    git(
        &f.place.repo,
        &["config", "gpg.program", gpg.to_str().unwrap()],
    );
    git(&f.place.repo, &["config", "user.signingkey", "FIXTUREKEY"]);
    let work = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = f.place.state.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(work.path().canonicalize().unwrap().display().to_string());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = Posture::Notify;
    cfg.self_improve.mode = mode;
    cfg.self_improve.repo = Some(f.place.repo.display().to_string());
    cfg.self_improve.remote = Some("origin".into());
    // The owner's standing choice, a pin: an unset key asks Jev, who is not here.
    cfg.self_improve.reviewer_profile = Some("fable".into());
    edit(&mut cfg);
    let store = Store::open(&f.place.state.join("store")).unwrap();
    let says = Arc::new(Says {
        text: "The test fails without the fix and the diff is small.\nVERDICT: accept\n- its planted revert fails as it should".into(),
        requests: Mutex::default(),
    });
    let mut p = crate::rpc::Parts::for_tests(cfg, says.clone(), store);
    parts(&mut p);
    let core = Core::build(p).unwrap();
    core.tools.self_pr.set_env(hermetic());
    core.self_resume(&SelfResumeParams::default(), &Answerer::from("the CLI"))
        .unwrap();
    Rig {
        core,
        says,
        f,
        stand: Arc::default(),
        _work: work,
    }
}

impl Rig {
    pub(crate) fn seams(&self) -> Seams {
        Seams {
            reviewer: self.stand.clone(),
            tests: Arc::new(Scripts),
            lock_every: Duration::from_millis(50),
            lock_wait: Duration::from_secs(60),
            ..Seams::default()
        }
    }

    /// A self branch off origin/main holding `files`, its row as self-pr
    /// writes one (green), built by a session that ran on `builder`.
    pub(crate) fn seed(
        &self,
        name: &str,
        files: &[(&str, &str)],
        builder: &str,
    ) -> (String, String) {
        let branch = format!("self/20261014-{name}");
        let wt = self
            .f
            .place
            .state
            .parent()
            .unwrap()
            .join(format!("seed-{name}"));
        let repo = &self.f.place.repo;
        git(
            repo,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                &branch,
                wt.to_str().unwrap(),
                "origin/main",
            ],
        );
        for (path, text) in files {
            let p = wt.join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        git(&wt, &["add", "-A"]);
        git(
            &wt,
            &["commit", "-q", "-m", &format!("self: {name} ({name})")],
        );
        let head = git(&wt, &["rev-parse", "HEAD"]);
        git(
            repo,
            &["worktree", "remove", "--force", wt.to_str().unwrap()],
        );
        let mut s = SessionRecord::new(SessionKind::Conversation, None);
        s.last_target = Some(TargetRef {
            profile: "builder".into(),
            provider: "anthropic".into(),
            model: builder.into(),
        });
        self.core.store.put_session(&s.session_id, &s).unwrap();
        // The builder's own working, which the reviewer must never see.
        let report = self
            .f
            .place
            .reports()
            .join(format!("{}.md", branch.replace('/', "-")));
        std::fs::create_dir_all(self.f.place.reports()).unwrap();
        std::fs::write(
            &report,
            format!("# Self branch {branch}\n\n- Gate: green\n\n<!-- REPORT COMPLETE -->\n"),
        )
        .unwrap();
        self.core.rec(Some(&s.session_id)).record(&SelfBranchBuilt {
            item: name,
            issue: "theseus-sail1",
            branch: &branch,
            head: Some(&head),
            gate: "green",
            report: Some(&report.to_string_lossy()),
            cost_usd: 1.5,
            stopped_at: None,
            why: "Trim the sails before the wind.",
            numbers: Value::Null,
        });
        (branch, head)
    }

    /// The rows of `kind`, but a join's wait for the owner's card (`publish`, not held): a test of another phase
    /// reads the stops it is about. [`Self::rows_all`] has them all.
    pub(crate) fn rows(&self, kind: &str) -> Vec<Value> {
        let waits =
            |d: &Value| d["phase"] == super::publish::PHASE && d["numbers"]["parked"] == true;
        (self.rows_all(kind).into_iter())
            .filter(|d| kind != "self.join.stopped" || !waits(d))
            .collect()
    }

    pub(crate) fn rows_all(&self, kind: &str) -> Vec<Value> {
        self.core
            .store
            .ledger_tail::<crate::ledger::LedgerRow>(100_000)
            .unwrap()
            .into_iter()
            .filter(|(_, r)| r.kind == kind)
            .map(|(_, r)| r.data)
            .collect()
    }

    /// The same store, config and provider under a new core, its self loop resumed: a restart.
    pub(crate) fn restarted(&self) -> Arc<Core> {
        let p = crate::rpc::Parts::for_tests(
            (*self.core.cfg).clone(),
            self.says.clone(),
            self.core.store.clone(),
        );
        let core = Core::build(p).unwrap();
        core.tools.self_pr.set_env(hermetic());
        core.self_resume(&SelfResumeParams::default(), &Answerer::from("the CLI"))
            .unwrap();
        core
    }

    pub(super) fn upstream_main(&self) -> String {
        git(&self.f.upstream, &["rev-parse", "refs/heads/main"])
    }

    pub(super) fn posts(&self) -> Vec<String> {
        crate::tests_day_ceiling::posts(&self.core, "self_join")
            .iter()
            .map(|b| b["text"].as_str().unwrap_or("").to_string())
            .collect()
    }
}

/// The dry run on a git with no `merge-tree --write-tree` (before 2.38, the
/// system git a daemon's PATH may find first) is a merge in a throwaway tree:
/// a clean head merges, two heads that each add one file differently conflict
/// on it, and no tree is left behind.
#[tokio::test]
async fn the_dry_run_in_a_tree_answers_without_merge_tree() {
    let r = rig(SelfMode::Act);
    let (_, clean) = r.seed("clean", &[("a.txt", "a\n")], SONNET);
    let (_, one) = r.seed("one", &[("same.txt", "one\n")], SONNET);
    let (_, two) = r.seed("two", &[("same.txt", "two\n")], SONNET);
    let p = &r.f.place;
    let base = git(&p.repo, &["rev-parse", "origin/main"]);
    let trees = git(&p.repo, &["worktree", "list"]);
    assert_eq!(
        super::git::dry_run_in_tree(p, &base, &clean).unwrap(),
        Ok(())
    );
    let why = super::git::dry_run_in_tree(p, &one, &two)
        .unwrap()
        .unwrap_err();
    assert!(why.contains("same.txt"), "{why}");
    assert_eq!(git(&p.repo, &["worktree", "list"]), trees, "no tree left");
}

/// The fix and the test that proves it: fails without `fix.txt`.
pub(crate) const PROVED: &[(&str, &str)] = &[
    ("fix.txt", "fixed\n"),
    ("tests/fix.sh", "test -f fix.txt\n"),
];
pub(crate) const SONNET: &str = "claude-sonnet-5-5";

/// A green branch: reviewed by another model (the check turn itself, on
/// fable), its planted revert failing as it should, joined to the remote's
/// main by a plain signed merge, the lock released, and the restart's notice
/// sent once, never twice.
#[tokio::test]
async fn a_green_branch_is_reviewed_joined_signed_and_noticed_once() {
    let r = rig(SelfMode::Act);
    let (branch, head) = r.seed("trim-the-sails", PROVED, SONNET);
    let base = r.upstream_main();
    let seams = Seams {
        reviewer: Arc::new(super::review::CheckTurn),
        ..r.seams()
    };
    let pass = r.core.self_join_pass_yes(&seams).await.unwrap();
    assert_eq!(
        pass.reviewed,
        [(branch.clone(), "accept".to_string())],
        "{pass:?}"
    );
    let v = &r.rows("self.review.verdict")[0];
    assert_eq!(v["planted_revert"], "failed");
    assert_eq!(v["reviewer"], "fable (claude-fable-5-2)");
    // The reviewer saw the claim (the report and the diff), on fable, in a
    // self session, and nothing of the builder's working.
    let reqs = r.says.requests.lock().unwrap().clone();
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].model, "claude-fable-5-2");
    let sent = serde_json::to_string(&reqs[0].messages).unwrap();
    assert!(
        sent.contains("Check: you check task") && sent.contains("+test -f fix.txt"),
        "{sent}"
    );
    assert!(sent.contains("Self branch self/20261014-trim-the-sails"));
    // Joined: main is the signed merge, two parents, main's old head first.
    let merge = r.upstream_main();
    assert_eq!(pass.joined, [(branch.clone(), merge.clone())]);
    let parents = git(&r.f.upstream, &["rev-list", "--parents", "-n", "1", &merge]);
    assert_eq!(parents, format!("{merge} {base} {head}"));
    let raw = git(&r.f.upstream, &["cat-file", "commit", &merge]);
    assert!(
        raw.contains("gpgsig -----BEGIN PGP SIGNATURE-----"),
        "{raw}"
    );
    let j = &r.rows("self.joined")[0];
    assert_eq!(j["merge"], merge.as_str());
    assert_eq!(j["undo"], format!("theseus self revert {merge}"));
    assert!(
        !super::lock_dir(&r.core.cfg).exists(),
        "the lock is released"
    );
    // A second pass does nothing more.
    let again = r.core.self_join_pass_yes(&seams).await.unwrap();
    assert!(
        again.reviewed.is_empty() && again.joined.is_empty(),
        "{again:?}"
    );
    // The restart's notice: once for the build of the merge, never twice.
    assert_eq!(
        r.core.self_installed_as("0000").unwrap(),
        None,
        "another build"
    );
    assert_eq!(
        r.core.self_installed_as(&merge).unwrap(),
        Some(merge.clone())
    );
    assert_eq!(r.core.self_installed_as(&merge).unwrap(), None);
    let posts = r.posts();
    assert_eq!(posts.len(), 1, "{posts:?}");
    for want in [
        "joined main",
        "Why: Trim the sails",
        "Cost: $",
        &format!("theseus self revert {merge}"),
    ] {
        assert!(posts[0].contains(want), "{want}: {}", posts[0]);
    }
    assert_eq!(r.rows("self.installed").len(), 1);
}

/// KEY (the owner, 2026-11-13): a join with an install leaves ZERO trees
/// open. The install's tree was the one nothing reaped. It is named for the
/// merge (`install-<short>`), not for a backlog item, and the pipeline's
/// close pass walks the task records (`pipeline::queues`'s `closed`), so no
/// item ever owns it; meanwhile `capacity::trees_open` counts every
/// directory under `self/trees/`, so one left behind holds the single
/// pipeline slot for good and every later start reads `hold=slots`
/// (measured 2026-11-12: a 102 MB `install-72af5c3fd1ff` open 15h42m across
/// two start evaluations, with nothing able to clear it).
///
/// Its build tree is what stays: the unit's L0 half copies the binaries out
/// of it after this returns and across the daemon's restart, and a build
/// tree is no open tree.
#[tokio::test]
async fn a_join_with_an_install_leaves_no_tree_open() {
    let r = rig_with(SelfMode::Act, |c| {
        c.self_improve.install = crate::config::self_improve::SelfInstall::Auto
    });
    let (branch, _) = r.seed("trim-the-sails", PROVED, SONNET);
    let p = &r.f.place;
    let before = git(&p.repo, &["worktree", "list"]);
    let installs = Arc::new(Builds::default());
    let seams = Seams {
        installer: installs.clone(),
        ..r.seams()
    };
    let pass = r.core.self_join_pass_yes(&seams).await.unwrap();
    let merge = r.upstream_main();
    assert_eq!(pass.joined, [(branch, merge)], "{pass:?}");
    // The install ran, in a tree of its own, and its build tree still holds
    // what the unit copies in.
    let targets = installs.targets.lock().unwrap().clone();
    assert_eq!(targets.len(), 1, "the install ran once: {targets:?}");
    for b in install::BINARIES {
        let at = targets[0].join("release").join(b);
        assert_eq!(
            std::fs::read_to_string(&at).unwrap_or_default(),
            format!("fake {b}"),
            "{} is still there for the unit",
            at.display()
        );
    }
    // THE POINT: not one tree open, and no stale worktree registration left
    // in the repository either.
    let left: Vec<String> = std::fs::read_dir(p.trees())
        .map(|d| {
            d.filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(
        crate::rsi::capacity::trees_open(&p.trees()),
        0,
        "the join left a tree open, which holds the pipeline's slot: {left:?}"
    );
    assert_eq!(
        git(&p.repo, &["worktree", "list"]),
        before,
        "no worktree registration left"
    );
}

/// The `true` of an older file installs on a user's own layout as it always did; the default (`"ask"`) and `"off"` do
/// not (`true` never installs into a root-owned directory by itself: `root_install`'s tests hold that).
#[tokio::test]
async fn the_old_true_installs_on_a_users_own_layout_and_the_default_does_not() {
    use crate::config::self_improve::SelfInstall;
    for (mode, installs) in [
        (SelfInstall::Legacy, 1),
        (SelfInstall::Auto, 1),
        (SelfInstall::default(), 0),
        (SelfInstall::Off, 0),
    ] {
        let r = rig_with(SelfMode::Act, |c| c.self_improve.install = mode);
        r.seed("trim-the-sails", PROVED, SONNET);
        let builds = Arc::new(Builds::default());
        let seams = Seams {
            installer: builds.clone(),
            ..r.seams()
        };
        r.core.self_join_pass_yes(&seams).await.unwrap();
        assert_eq!(builds.targets.lock().unwrap().len(), installs, "{mode:?}");
    }
}

/// The same model building and reviewing is no independent review: refused
/// with its reason, once, and nothing is joined.
#[tokio::test]
async fn the_same_model_for_builder_and_reviewer_is_refused() {
    let r = rig(SelfMode::Act);
    let (branch, _) = r.seed("trim-the-sails", PROVED, "claude-fable-5-2");
    let main = r.upstream_main();
    let pass = r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    assert_eq!(pass.reviewed, [(branch, "refused".to_string())]);
    let v = &r.rows("self.review.verdict")[0];
    assert!(
        v["reasons"][0]
            .as_str()
            .unwrap()
            .contains("the model that built the branch"),
        "{v}"
    );
    assert!(
        r.stand.asks.lock().unwrap().is_empty(),
        "no reviewer was asked"
    );
    assert_eq!(r.upstream_main(), main);
    r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    assert_eq!(r.rows("self.review.verdict").len(), 1, "refused once");
}

/// A branch whose test passes without its fix proves nothing: rejected,
/// with no reviewer paid, and its item goes back to the backlog with the
/// reasons in its evidence.
#[tokio::test]
async fn an_unproved_branch_is_rejected_and_its_item_returns_to_the_backlog() {
    let r = rig(SelfMode::Act);
    let item = TaskRecord {
        id: "tsk_sails".into(),
        version: 3,
        title: "Trim the sails".into(),
        state: TaskState::InProgress,
        session: Some("ses_builder".into()),
        owner: "self".into(),
        ..TaskRecord::default()
    };
    r.core
        .store
        .append(&[crate::task_graph::record(&item).unwrap()])
        .unwrap();
    let (branch, _) = r.seed(
        "tsk_sails",
        &[("fix.txt", "fixed\n"), ("tests/fix.sh", "true\n")],
        SONNET,
    );
    let main = r.upstream_main();
    let pass = r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    assert_eq!(pass.reviewed, [(branch, "reject".to_string())]);
    let v = &r.rows("self.review.verdict")[0];
    assert_eq!(v["planted_revert"], "passed");
    assert!(
        v["reasons"][0].as_str().unwrap().starts_with("unproved"),
        "{v}"
    );
    assert!(r.stand.asks.lock().unwrap().is_empty());
    assert_eq!(r.upstream_main(), main);
    let t = crate::task_graph::get(&r.core.store, "tsk_sails")
        .unwrap()
        .unwrap();
    assert_eq!(
        (t.state, t.version, t.session),
        (TaskState::Proposed, 4, None)
    );
    let e = t.evidence.last().unwrap();
    assert!(
        e.identity.starts_with("review:self/20261014-tsk_sails@"),
        "{e:?}"
    );
    assert!(e.note.as_deref().unwrap().contains("unproved"), "{e:?}");
    // The branch is kept.
    assert!(!git(
        &r.f.place.repo,
        &["branch", "--list", "self/20261014-tsk_sails"]
    )
    .is_empty());
}

/// A branch with no test the revert can run is unproved too.
#[tokio::test]
async fn a_branch_without_tests_is_rejected() {
    let r = rig(SelfMode::Act);
    r.seed("trim-the-sails", &[("fix.txt", "fixed\n")], SONNET);
    r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    assert_eq!(
        r.rows("self.review.verdict")[0]["planted_revert"],
        "no-tests"
    );
}

/// Each keel class stops at the owner's card (the classes one by one are
/// `keel::tests`): a ceiling's file, and main's keel guard's finding. The
/// card goes once, main is untouched, and the head is never tried again.
#[tokio::test]
async fn a_keel_class_stops_at_the_owners_card_and_waits() {
    let r = rig(SelfMode::Act);
    let guard = "import sys, os\nif os.path.exists(os.path.join(sys.argv[sys.argv.index('--repo') + 1], 'erosion.txt')):\n    print('tests/fix.sh:1: test-removed: a -> b')\n    sys.exit(1)\n";
    let root = r.f.place.state.parent().unwrap().to_path_buf();
    let other = root.join("guard");
    git(
        &root,
        &[
            "clone",
            "-q",
            r.f.upstream.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    std::fs::write(other.join("scripts/keel-guard.py"), guard).unwrap();
    git(&other, &["add", "-A"]);
    git(&other, &["commit", "-q", "-m", "the guard"]);
    git(&other, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    let main = r.upstream_main();
    let mut files = PROVED.to_vec();
    files.push(("scripts/long-files.txt", "crates/x.rs 9999\n"));
    let (a, _) = r.seed("raise-a-ceiling", &files, SONNET);
    let mut files = PROVED.to_vec();
    files.push(("erosion.txt", "gone\n"));
    let (b, _) = r.seed("erode-a-test", &files, SONNET);
    let seams = r.seams();
    let one = r.core.self_join_pass_yes(&seams).await.unwrap();
    assert_eq!(one.stopped, [(a.clone(), "keel".to_string())], "{one:?}");
    let two = r.core.self_join_pass_yes(&seams).await.unwrap();
    assert_eq!(two.stopped, [(b.clone(), "keel".to_string())], "{two:?}");
    let posts = r.posts();
    assert_eq!(posts.len(), 2, "{posts:?}");
    assert!(
        posts[0].contains("keel (") && posts[0].contains("scripts/long-files.txt"),
        "{}",
        posts[0]
    );
    assert!(
        posts[1].contains("keel-guard: tests/fix.sh:1: test-removed"),
        "{}",
        posts[1]
    );
    assert_eq!(r.upstream_main(), main, "nothing joined");
    let three = r.core.self_join_pass_yes(&seams).await.unwrap();
    assert!(
        three.stopped.is_empty() && three.joined.is_empty(),
        "{three:?}"
    );
    assert_eq!(r.posts().len(), 2, "never retried at that head");
}

/// KEY: what main gained after a branch was cut is not what the branch removed. Main's
/// guard reads two trees; given main's tip it read a test added to main since as a test the
/// branch took away, and an accepted branch stopped at the owner's card (2026-11-15). Given
/// where the branch left main, it finds nothing, and the branch joins.
#[tokio::test]
async fn a_test_main_gained_since_the_cut_is_not_one_the_branch_removed() {
    let r = rig(SelfMode::Act);
    // A guard that reads its range as the real one does: the two trees it is given.
    let guard = concat!(
        "import subprocess, sys\n",
        "a = sys.argv\n",
        "repo, base, head = (a[a.index(k) + 1] for k in ('--repo', '--base', '--head'))\n",
        "out = subprocess.run(['git', '-C', repo, 'diff', '--no-renames', '--name-status', base, head],\n",
        "                     capture_output=True, text=True).stdout\n",
        "gone = [l.split()[1] for l in out.splitlines() if l.startswith('D') and l.split()[1].startswith('tests/')]\n",
        "for g in gone:\n",
        "    print(g + ':1: test-removed: (there) -> (gone)')\n",
        "sys.exit(1 if gone else 0)\n",
    );
    let root = r.f.place.state.parent().unwrap().to_path_buf();
    let other = root.join("guard");
    git(
        &root,
        &[
            "clone",
            "-q",
            r.f.upstream.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    std::fs::write(other.join("scripts/keel-guard.py"), guard).unwrap();
    git(&other, &["add", "-A"]);
    git(&other, &["commit", "-q", "-m", "the guard"]);
    git(&other, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    git(&r.f.place.repo, &["fetch", "-q", "origin"]);
    // The branch is cut here, and then main gains a test the branch never had.
    let (branch, _) = r.seed("trim-the-sails", PROVED, SONNET);
    std::fs::create_dir_all(other.join("tests")).unwrap();
    std::fs::write(other.join("tests/later.sh"), "true\n").unwrap();
    git(&other, &["add", "-A"]);
    git(&other, &["commit", "-q", "-m", "a test main gains later"]);
    git(&other, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    let pass = r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    assert!(pass.stopped.is_empty(), "{pass:?}: {:?}", r.posts());
    assert_eq!(pass.joined.len(), 1, "{pass:?}");
    assert_eq!(pass.joined[0].0, branch);
    // Main holds both: the branch's fix and the test it gained meanwhile.
    let main = r.upstream_main();
    let tree = git(&r.f.upstream, &["ls-tree", "-r", "--name-only", &main]);
    assert!(
        tree.contains("fix.txt") && tree.contains("tests/later.sh"),
        "{tree}"
    );
}

/// The data rule's files stop at the owner's card too (theseus-ozplq): one
/// plant per file the finding named, each a self branch whose only extra
/// change is that file. Before this row they were in the keel guard's
/// `KEEL_FILES` and in no class, so a branch could ack the guard's finding
/// with its own `Keel:` trailer and join; a class is a path and nothing a
/// branch writes clears it.
#[tokio::test]
async fn the_data_rules_files_stop_at_the_owners_card() {
    const PLANTS: &[&str] = &[
        "crates/theseus-core/src/data_gate.rs",
        "crates/theseus-core/src/exits.rs",
        "crates/theseus-core/src/content.rs",
        "crates/theseus-core/src/turn/data_step.rs",
        "crates/theseus-core/src/peer.rs",
        "crates/theseusd/src/web.rs",
    ];
    let r = rig(SelfMode::Act);
    let main = r.upstream_main();
    for (i, path) in PLANTS.iter().enumerate() {
        let mut files = PROVED.to_vec();
        files.push((path, "// the plant\n"));
        r.seed(&format!("plant-{i}"), &files, SONNET);
    }
    let seams = r.seams();
    for (i, path) in PLANTS.iter().enumerate() {
        let pass = r.core.self_join_pass_yes(&seams).await.unwrap();
        assert_eq!(
            pass.stopped,
            [(format!("self/20261014-plant-{i}"), "keel".to_string())],
            "{path}: {pass:?}"
        );
    }
    let posts = r.posts();
    assert_eq!(posts.len(), PLANTS.len(), "a card each: {posts:?}");
    for (post, path) in posts.iter().zip(PLANTS) {
        assert!(
            post.contains("data-rule") && post.contains(path),
            "{path}: {post}"
        );
    }
    assert_eq!(r.upstream_main(), main, "nothing joined");
}

/// A held lock waits: nothing merges while another holds it, and the join
/// goes on once it is released with its done: line.
#[tokio::test]
async fn a_held_lock_waits() {
    let r = rig(SelfMode::Act);
    r.seed("trim-the-sails", PROVED, SONNET);
    let main = r.upstream_main();
    let dir = super::lock_dir(&r.core.cfg);
    let held = lock::try_take(&dir, "the outer loop's join of lane/x")
        .unwrap()
        .unwrap();
    let (core, seams) = (r.core.clone(), r.seams());
    let pass = tokio::spawn(async move { core.self_join_pass_yes(&seams).await });
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert!(!pass.is_finished(), "it waits for the lock");
    assert_eq!(r.upstream_main(), main);
    assert!(r.rows("self.joined").is_empty());
    held.release("merged lane/x").unwrap();
    let pass = pass.await.unwrap().unwrap();
    assert_eq!(pass.joined.len(), 1, "{pass:?}");
    assert_ne!(r.upstream_main(), main);
}

/// Main moving between the gate and the push redoes the dry run, the merge
/// and the gate: the merge that lands is on the new main.
#[tokio::test]
async fn main_moving_mid_join_redoes_the_dry_run() {
    let r = rig(SelfMode::Act);
    r.seed("trim-the-sails", PROVED, SONNET);
    let moved: Arc<Mutex<Option<String>>> = Arc::default();
    let (m, root, up) = (
        moved.clone(),
        r.f.place.state.parent().unwrap().to_path_buf(),
        r.f.upstream.clone(),
    );
    let seams = Seams {
        before_push: Some(Arc::new(move |attempt| {
            if attempt == 1 {
                let other = root.join("mover");
                git(
                    &root,
                    &["clone", "-q", up.to_str().unwrap(), other.to_str().unwrap()],
                );
                std::fs::write(other.join("elsewhere.txt"), "elsewhere\n").unwrap();
                git(&other, &["add", "-A"]);
                git(&other, &["commit", "-q", "-m", "elsewhere"]);
                git(&other, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
                *m.lock().unwrap() = Some(git(&other, &["rev-parse", "HEAD"]));
            }
        })),
        ..r.seams()
    };
    let pass = r.core.self_join_pass_yes(&seams).await.unwrap();
    assert_eq!(pass.joined.len(), 1, "{pass:?}");
    let j = &r.rows("self.joined")[0];
    assert_eq!(j["numbers"]["tries"], 2);
    let new_main = moved.lock().unwrap().clone().unwrap();
    assert_eq!(j["numbers"]["base"], new_main.as_str());
    let merge = r.upstream_main();
    assert_eq!(
        git(&r.f.upstream, &["rev-parse", &format!("{merge}^1")]),
        new_main
    );
}

/// No signing key: the owner's card, and nothing reaches main.
#[tokio::test]
async fn nothing_is_pushed_without_a_signature() {
    let r = rig(SelfMode::Act);
    git(&r.f.place.repo, &["config", "--unset", "user.signingkey"]);
    let (branch, _) = r.seed("trim-the-sails", PROVED, SONNET);
    let main = r.upstream_main();
    let pass = r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    assert_eq!(pass.stopped, [(branch, "sign".to_string())]);
    assert_eq!(r.upstream_main(), main);
    assert!(r.rows("self.joined").is_empty());
    assert!(r.posts()[0].contains("an unsigned merge never reaches main"));
    assert!(
        !super::lock_dir(&r.core.cfg).exists(),
        "the lock is released"
    );
    // An unsigned commit is refused at the push itself, too.
    let t = super::git::detached(&r.f.place, "probe", "self/probe", "origin/main").unwrap();
    let head = git(&t.dir, &["rev-parse", "HEAD"]);
    let e = super::git::push_main(&r.f.place, &t, "origin", &head, &head).unwrap_err();
    assert!(format!("{e:#}").contains("is not signed"), "{e:#}");
    crate::rsi::tree::close(&r.f.place, &t).unwrap();
}

/// A red gate on the merge leaves main untouched, and says so.
#[tokio::test]
async fn a_red_gate_on_the_merge_leaves_main_untouched() {
    let r = rig(SelfMode::Act);
    let mut files = PROVED.to_vec();
    files.push(("broken.txt", "red\n"));
    let (branch, _) = r.seed("trim-the-sails", &files, SONNET);
    let main = r.upstream_main();
    let pass = r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    assert_eq!(pass.stopped, [(branch, "gate".to_string())], "{pass:?}");
    assert_eq!(r.upstream_main(), main);
    let s = &r.rows("self.join.stopped")[0];
    assert!(
        s["why"].as_str().unwrap().contains("main is untouched"),
        "{s}"
    );
    assert!(!super::lock_dir(&r.core.cfg).exists());
    // Its trees are closed.
    let trees: Vec<PathBuf> = std::fs::read_dir(r.f.place.trees())
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    assert!(trees.is_empty(), "{trees:?}");
}

/// A halt between phases stops the join there, and the row says where.
#[tokio::test]
async fn a_halt_between_phases_stops_it_and_says_where() {
    let r = rig(SelfMode::Act);
    let (branch, _) = r.seed("trim-the-sails", PROVED, SONNET);
    let main = r.upstream_main();
    let core = r.core.clone();
    let seams = Seams {
        on_phase: Some(Arc::new(move |phase| {
            if phase == "sign" {
                let p = SelfHaltParams {
                    why: Some("the wind turned".into()),
                    ..Default::default()
                };
                core.self_halt(&p, &Answerer::from("the CLI")).unwrap();
            }
        })),
        ..r.seams()
    };
    let pass = r.core.self_join_pass_yes(&seams).await.unwrap();
    assert_eq!(pass.stopped, [(branch, "sign".to_string())]);
    let s = &r.rows("self.join.stopped")[0];
    assert_eq!(s["phase"], "sign");
    assert!(
        s["why"].as_str().unwrap().contains("the wind turned"),
        "{s}"
    );
    assert_eq!(r.upstream_main(), main);
    assert!(!super::lock_dir(&r.core.cfg).exists());
    // Halted: the next pass reads nothing and writes nothing.
    let n = r
        .core
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(100_000)
        .unwrap()
        .len();
    let next = r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    assert_eq!(next.gate, "halted");
    assert_eq!(
        r.core
            .store
            .ledger_tail::<crate::ledger::LedgerRow>(100_000)
            .unwrap()
            .len(),
        n
    );
}

/// With the mode off nothing runs: no row, no lock, no tree, no notice.
#[tokio::test]
async fn mode_off_does_nothing() {
    let r = rig(SelfMode::Off);
    r.seed("trim-the-sails", PROVED, SONNET);
    let n = r
        .core
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(100_000)
        .unwrap()
        .len();
    let pass = r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    assert_eq!(pass.gate, "off");
    assert_eq!(
        pass,
        super::Pass {
            gate: "off".into(),
            ..Default::default()
        }
    );
    assert_eq!(
        r.core
            .store
            .ledger_tail::<crate::ledger::LedgerRow>(100_000)
            .unwrap()
            .len(),
        n
    );
    assert!(!super::lock_dir(&r.core.cfg).exists());
    assert!(!r.f.place.trees().exists());
    assert_eq!(r.core.self_installed_as("anything").unwrap(), None);
}

// Route.v10 row 4 (theseus-ej6bj): `[self] reviewer_profile` is a pin when set;
// unset, Jev chooses the reviewer, told the builder's model and that the
// change is the owner's material (US options only). The independence rule
// stays a hard rule either way: it is review independence, not a cap.

/// A rig whose Jev is the fake, with the model pack acting and `reviewer_profile`
/// as the test says.
fn rig_jev(jev: &theseus_judge::fake::FakeJev, reviewer: Option<&str>) -> Rig {
    let reviewer = reviewer.map(str::to_string);
    let r = rig_parts(
        SelfMode::Act,
        move |c| {
            c.self_improve.reviewer_profile = reviewer;
            c.judge.enabled = true;
            c.judge.api_base = jev.base();
            c.judge.connect_secs = 1;
            c.judge.total_secs = 30;
            c.routing.max_wait_ms = 300;
            c.name_every_provider_key();
        },
        |p| {
            p.secrets = crate::tests_judge::board();
            p.providers
                .insert("deepseek".into(), Arc::new(FakeProvider::default()));
        },
    );
    r.core
        .runner
        .judge
        .job_pack_with(crate::tests_job_ask::model_pack());
    crate::tests_judge::warm(&r.core);
    r
}

fn reviewer_asks(jev: &theseus_judge::fake::FakeJev) -> Vec<String> {
    jev.seen()
        .iter()
        .map(|s| s.body["state"].to_string())
        .filter(|s| s.contains("Review this change"))
        .collect()
}

#[tokio::test]
async fn an_unset_reviewer_profile_asks_jev_and_reviews_on_its_pick() {
    let jev = theseus_judge::fake::FakeJev::start().unwrap();
    crate::tests_job_ask::pick(&jev, "opus", 0.95);
    let r = rig_jev(&jev, None);
    r.seed("trim-the-sails", PROVED, SONNET);
    let pass = r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    assert_eq!(pass.reviewed[0].1, "accept", "{pass:?}");
    let v = &r.rows("self.review.verdict")[0];
    assert_eq!(v["model"], "claude-opus-5-5", "{v}");
    let asks = reviewer_asks(&jev);
    assert_eq!(asks.len(), 1, "one ask");
    // The state names the builder's model; no option is kept off by origin
    // (2026-11-21).
    assert!(asks[0].contains("claude-sonnet-5-5"), "{}", asks[0]);
    assert!(
        !asks[0].contains("restricted_models") && !asks[0].contains("not_allowed"),
        "{}",
        asks[0]
    );
    assert!(asks[0].contains("job"), "{}", asks[0]);
}

#[tokio::test]
async fn a_set_reviewer_profile_asks_jev_nothing() {
    let jev = theseus_judge::fake::FakeJev::start().unwrap();
    crate::tests_job_ask::pick(&jev, "haiku", 0.95);
    let r = rig_jev(&jev, Some("opus"));
    r.seed("trim-the-sails", PROVED, SONNET);
    r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    let v = &r.rows("self.review.verdict")[0];
    assert_eq!(v["model"], "claude-opus-5-5", "{v}");
    assert!(reviewer_asks(&jev).is_empty());
}

/// Review independence is a hard rule, not a cap: when Jev, asked, names the
/// builder's own model, no review runs.
#[tokio::test]
async fn the_independence_rule_refuses_a_jev_pick_that_is_the_builders_model() {
    let jev = theseus_judge::fake::FakeJev::start().unwrap();
    crate::tests_job_ask::pick(&jev, "sonnet", 0.95);
    let r = rig_jev(&jev, None);
    let (branch, _) = r.seed("trim-the-sails", PROVED, SONNET);
    let pass = r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    assert_eq!(pass.reviewed, [(branch, "refused".to_string())]);
    let v = &r.rows("self.review.verdict")[0];
    assert!(
        v["reasons"][0]
            .as_str()
            .unwrap()
            .contains("the model that built the branch"),
        "{v}"
    );
    assert!(
        r.stand.asks.lock().unwrap().is_empty(),
        "no reviewer was asked"
    );
}

/// Both sides Jev-chosen: the builder ran on Opus because Jev named it (its
/// session's model says so), and the reviewer, asked, names Opus too. The rule
/// is about the models, not about who chose them, so no review runs.
#[tokio::test]
async fn the_independence_rule_holds_when_both_sides_are_jevs_picks() {
    let jev = theseus_judge::fake::FakeJev::start().unwrap();
    crate::tests_job_ask::pick(&jev, "opus", 0.95);
    let r = rig_jev(&jev, None);
    let (branch, _) = r.seed("trim-the-sails", PROVED, "claude-opus-5-5");
    let pass = r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    assert_eq!(pass.reviewed, [(branch, "refused".to_string())]);
    let v = &r.rows("self.review.verdict")[0];
    assert_eq!(v["model"], "claude-opus-5-5", "{v}");
    assert!(
        v["reasons"][0]
            .as_str()
            .unwrap()
            .contains("the model that built the branch"),
        "{v}"
    );
    assert_eq!(reviewer_asks(&jev).len(), 1, "Jev named the reviewer");
    assert!(
        r.stand.asks.lock().unwrap().is_empty(),
        "no reviewer was asked"
    );
}

/// A Jev that is slow never holds the review: past `max_wait_ms` the reviewer
/// is the live profile.
#[tokio::test]
async fn a_slow_jev_does_not_hold_the_review() {
    let jev = theseus_judge::fake::FakeJev::start().unwrap();
    crate::tests_job_ask::pick(&jev, "opus", 0.95);
    jev.set_mode(theseus_judge::fake::FakeMode::Held);
    let r = rig_jev(&jev, None);
    // Built on Fable: the live profile (Sonnet) is another model, so it reviews.
    r.seed("trim-the-sails", PROVED, "claude-fable-5-2");
    let t0 = std::time::Instant::now();
    let pass = r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    assert_eq!(pass.reviewed[0].1, "accept", "{pass:?}");
    assert_eq!(
        reviewer_asks(&jev).len(),
        1,
        "Jev was asked, and held its answer"
    );
    assert!(t0.elapsed() < Duration::from_secs(30), "{:?}", t0.elapsed());
    let v = &r.rows("self.review.verdict")[0];
    assert_eq!(v["model"], SONNET, "the live profile reviewed: {v}");
    jev.release();
}

/// The independence rule holds of the live profile as of any: built on the
/// model that is live, and with Jev slow, the review is refused, not hung.
#[tokio::test]
async fn a_slow_jev_and_a_live_profile_that_built_the_branch_is_refused_not_hung() {
    let jev = theseus_judge::fake::FakeJev::start().unwrap();
    crate::tests_job_ask::pick(&jev, "opus", 0.95);
    jev.set_mode(theseus_judge::fake::FakeMode::Held);
    let r = rig_jev(&jev, None);
    r.seed("mend-the-nets", PROVED, SONNET);
    let pass = r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    assert_eq!(pass.reviewed[0].1, "refused", "{pass:?}");
    assert_eq!(reviewer_asks(&jev).len(), 1, "Jev was asked");
    jev.release();
}

// The model that answered (theseus-107, issue 20): the verdict records it, the independence rule is
// checked against it, and `[self] reviewer_profile` is a pin that routing does not move.

/// A reviewer whose turn ended up on `ran`, whatever it was asked for: the case a routed turn or a
/// refusal's fallback makes. It accepts, so a test can see whether the accept stood.
struct Strays {
    ran: &'static str,
    asks: std::sync::atomic::AtomicUsize,
}

impl Strays {
    fn on(ran: &'static str) -> Arc<Self> {
        Arc::new(Self {
            ran,
            asks: Default::default(),
        })
    }
}

impl Reviewer for Strays {
    fn review<'a>(&'a self, _core: &'a Arc<Core>, _ask: &'a Ask) -> Fut<'a> {
        self.asks.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Box::pin(async {
            Ok(Review {
                accept: true,
                fix: false,
                reasons: vec!["the stray accepts".into()],
                cost_usd: 0.25,
                session: None,
                profile: "stray".into(),
                model: self.ran.into(),
            })
        })
    }
}

/// The row says what ran, not what was asked for: asked for Fable, the turn ended on Opus.
#[tokio::test]
async fn the_verdict_records_the_model_that_answered() {
    let r = rig(SelfMode::Act);
    let (branch, _) = r.seed("trim-the-sails", PROVED, SONNET);
    let stray = Strays::on("claude-opus-5-5");
    let seams = Seams {
        reviewer: stray.clone(),
        ..r.seams()
    };
    let pass = r.core.self_join_pass_yes(&seams).await.unwrap();
    assert_eq!(pass.reviewed, [(branch, "accept".to_string())], "{pass:?}");
    let v = &r.rows("self.review.verdict")[0];
    assert_eq!(v["model"], "claude-opus-5-5", "{v}");
    assert_eq!(v["reviewer"], "stray (claude-opus-5-5)", "{v}");
    assert_eq!(v["asked"], "claude-fable-5-2", "{v}");
}

/// A review that ran on the builder's model is no independent review: it does not accept, and
/// nothing is joined, however the reviewer answered. The model is the real one, and it is refused
/// once, not again every pass.
#[tokio::test]
async fn a_review_that_ran_on_the_builders_model_does_not_accept() {
    let r = rig(SelfMode::Act);
    let (branch, _) = r.seed("trim-the-sails", PROVED, SONNET);
    let main = r.upstream_main();
    let stray = Strays::on(SONNET);
    let seams = Seams {
        reviewer: stray.clone(),
        ..r.seams()
    };
    let pass = r.core.self_join_pass_yes(&seams).await.unwrap();
    assert_eq!(pass.reviewed, [(branch, "refused".to_string())], "{pass:?}");
    assert!(pass.joined.is_empty(), "{pass:?}");
    assert_eq!(r.upstream_main(), main, "main did not move");
    let v = &r.rows("self.review.verdict")[0];
    assert_eq!(v["verdict"], "refused", "{v}");
    assert_eq!(v["model"], SONNET, "{v}");
    assert_eq!(v["planted_revert"], "failed", "it ran: {v}");
    let why = v["reasons"][0].as_str().unwrap();
    assert!(
        why.contains("the model that built the branch") && why.contains("does not count"),
        "{why}"
    );
    r.core.self_join_pass_yes(&seams).await.unwrap();
    assert_eq!(r.rows("self.review.verdict").len(), 1, "refused once");
    assert_eq!(stray.asks.load(std::sync::atomic::Ordering::SeqCst), 1);
}

/// A dated snapshot of the builder's model is the builder's model.
#[tokio::test]
async fn a_dated_snapshot_of_the_builders_model_does_not_accept_either() {
    let r = rig(SelfMode::Act);
    r.seed("trim-the-sails", PROVED, SONNET);
    let seams = Seams {
        reviewer: Strays::on("claude-sonnet-5-5-20261101"),
        ..r.seams()
    };
    let pass = r.core.self_join_pass_yes(&seams).await.unwrap();
    assert_eq!(pass.reviewed[0].1, "refused", "{pass:?}");
    assert!(pass.joined.is_empty(), "{pass:?}");
}

/// A turn that does not say which model answered cannot be shown to differ from the builder's.
#[tokio::test]
async fn a_review_that_does_not_say_what_ran_does_not_accept() {
    let r = rig(SelfMode::Act);
    r.seed("trim-the-sails", PROVED, SONNET);
    let seams = Seams {
        reviewer: Strays::on(""),
        ..r.seams()
    };
    let pass = r.core.self_join_pass_yes(&seams).await.unwrap();
    assert_eq!(pass.reviewed[0].1, "refused", "{pass:?}");
    assert!(pass.joined.is_empty(), "{pass:?}");
    let v = &r.rows("self.review.verdict")[0];
    assert!(
        v["reasons"][0]
            .as_str()
            .unwrap()
            .contains("did not say which model answered"),
        "{v}"
    );
}

/// KEY: `[self] reviewer_profile` is a pin. With the router asked to put every message on Opus, the
/// review turn still runs on Fable, the profile the owner set: the request that reaches the
/// provider, and the row, say so. (Before it was held, the turn went where the router sent it, and
/// the row still said Fable: the three reviews of 2026-11-14 and 15 ran on Opus.)
#[tokio::test]
async fn the_reviewer_profile_is_a_pin_the_router_does_not_move() {
    let jev = theseus_judge::fake::FakeJev::start().unwrap();
    crate::tests_job_ask::pick(&jev, "opus", 0.99);
    let r = rig_jev(&jev, Some("fable"));
    r.seed("trim-the-sails", PROVED, SONNET);
    let seams = Seams {
        reviewer: Arc::new(super::review::CheckTurn),
        ..r.seams()
    };
    let pass = r.core.self_join_pass_yes(&seams).await.unwrap();
    assert_eq!(pass.reviewed[0].1, "accept", "{pass:?}");
    let reqs = r.says.requests.lock().unwrap().clone();
    let models: Vec<&str> = reqs.iter().map(|q| q.model.as_str()).collect();
    assert_eq!(models, ["claude-fable-5-2"], "every call of the review");
    let v = &r.rows("self.review.verdict")[0];
    assert_eq!(v["model"], "claude-fable-5-2", "{v}");
    assert_eq!(v["asked"], "claude-fable-5-2", "{v}");
}

// A verdict row with no `asked` is from before the row said what answered (`unproven_accept`). The
// ledger's rows are read by key, and a key that is not there reads as empty, so a row written here
// with `asked` empty is the row the old code wrote.

/// A verdict row as the code before the model that answered wrote it: the setting's model, no `asked`.
fn legacy_row(r: &Rig, branch: &str, head: &str, verdict: &str) {
    r.core
        .rec(None)
        .record(&crate::fact::rsi_join::SelfReviewVerdict {
            item: "trim-the-sails".into(),
            branch: branch.into(),
            head: head.into(),
            reviewer: "fable (claude-fable-5-2)".into(),
            model: "claude-fable-5-2".into(),
            asked: String::new(),
            verdict: verdict.into(),
            planted_revert: "failed".into(),
            proof_check: None,
            reasons: vec!["from before".into()],
            cost_usd: 0.25,
        });
}

/// KEY: a head whose only review is an old accept (the setting said Fable and the turn could have run
/// anywhere) is reviewed again, and joins on the new review's accept.
#[tokio::test]
async fn a_head_with_only_a_legacy_accept_is_reviewed_again_and_joins_on_the_new_accept() {
    let r = rig(SelfMode::Act);
    let (branch, head) = r.seed("trim-the-sails", PROVED, SONNET);
    legacy_row(&r, &branch, &head, "accept");
    let pass = r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    assert_eq!(
        r.stand.asks.lock().unwrap().len(),
        1,
        "the old accept did not stand for a review"
    );
    assert_eq!(pass.reviewed, [(branch.clone(), "accept".to_string())]);
    assert_eq!(pass.joined.len(), 1, "{pass:?}");
    let rows = r.rows("self.review.verdict");
    assert_eq!(rows.len(), 2, "the old row, and the new review's");
    assert_eq!(rows[1]["asked"], "claude-fable-5-2", "{rows:?}");
}

/// KEY: where the new review does not accept, the old accept joins nothing. Here it ran on the
/// builder's model: refused, main unmoved, and asked once, not again every pass.
#[tokio::test]
async fn a_legacy_accept_does_not_join_a_head_the_new_review_refuses() {
    let r = rig(SelfMode::Act);
    let (branch, head) = r.seed("trim-the-sails", PROVED, SONNET);
    legacy_row(&r, &branch, &head, "accept");
    let main = r.upstream_main();
    let stray = Strays::on(SONNET);
    let seams = Seams {
        reviewer: stray.clone(),
        ..r.seams()
    };
    for _ in 0..3 {
        let pass = r.core.self_join_pass_yes(&seams).await.unwrap();
        assert!(pass.joined.is_empty(), "{pass:?}");
    }
    assert_eq!(r.upstream_main(), main, "main did not move");
    assert_eq!(stray.asks.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(
        r.rows("self.review.verdict").len(),
        2,
        "old, and one refusal"
    );
}

/// An old reject or an old refusal is still seen: it is not asked again, and joins nothing.
#[tokio::test]
async fn a_legacy_reject_or_refusal_is_still_seen() {
    for verdict in ["reject", "refused"] {
        let r = rig(SelfMode::Act);
        let (branch, head) = r.seed("trim-the-sails", PROVED, SONNET);
        legacy_row(&r, &branch, &head, verdict);
        let main = r.upstream_main();
        for _ in 0..2 {
            let pass = r.core.self_join_pass_yes(&r.seams()).await.unwrap();
            assert!(
                pass.reviewed.is_empty() && pass.joined.is_empty(),
                "{verdict}: {pass:?}"
            );
        }
        assert!(
            r.stand.asks.lock().unwrap().is_empty(),
            "{verdict}: asked again"
        );
        assert_eq!(
            r.rows("self.review.verdict").len(),
            1,
            "{verdict}: a row was added"
        );
        assert_eq!(r.upstream_main(), main);
    }
}

/// An accept from the new code carries `asked` and is trusted as before: not reviewed again.
#[tokio::test]
async fn an_accept_that_says_what_was_asked_is_trusted() {
    let r = rig(SelfMode::Act);
    let (branch, head) = r.seed("trim-the-sails", PROVED, SONNET);
    r.core
        .rec(None)
        .record(&crate::fact::rsi_join::SelfReviewVerdict {
            item: "trim-the-sails".into(),
            branch,
            head,
            reviewer: "fable (claude-fable-5-2)".into(),
            model: "claude-fable-5-2".into(),
            asked: "claude-fable-5-2".into(),
            verdict: "accept".into(),
            planted_revert: "failed".into(),
            proof_check: None,
            reasons: vec![],
            cost_usd: 0.25,
        });
    let pass = r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    assert!(r.stand.asks.lock().unwrap().is_empty(), "{pass:?}");
    assert_eq!(pass.joined.len(), 1, "{pass:?}");
}
