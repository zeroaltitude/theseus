//! The self-join through the whole core (theseus-pw1q.9), over
//! `tests_tree`'s throwaway repository: a bare upstream (the remote) with a
//! stand-in `scripts/gate.sh` on main, a clone of it as `[self] repo`, a
//! stand-in gpg the repo's config names (so a merge is signed without a real
//! key), stand-in tests (`tests/*.sh`, run with bash), and a seeded green
//! self branch with its `self.branch.built` row, as self-pr leaves one.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use theseus_protocol::rsi::{SelfHaltParams, SelfMode, SelfResumeParams};
use theseus_protocol::tasks::{TaskRecord, TaskState};
use theseus_protocol::SessionKind;

use super::planted::Tests;
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

const FAKE_GPG: &str = "#!/bin/bash\n\
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

/// A reviewer that says `accept` and counts its asks.
#[derive(Default)]
struct Stand {
    asks: Mutex<Vec<Ask>>,
}

impl Reviewer for Stand {
    fn review<'a>(&'a self, _core: &'a Arc<Core>, ask: &'a Ask) -> Fut<'a> {
        self.asks.lock().unwrap().push(ask.clone());
        Box::pin(async {
            Ok(Review {
                accept: true,
                reasons: vec!["the stand-in accepts".into()],
                cost_usd: 0.25,
                session: None,
            })
        })
    }
}

/// `tests/*.sh`, each run with bash in the tree: all must exit 0.
struct Scripts;

impl Tests for Scripts {
    fn run(&self, _p: &Place, t: &Tree, tests: &[String]) -> anyhow::Result<(bool, String)> {
        let mut ok = true;
        for name in tests {
            let s = Command::new("bash")
                .arg(name)
                .current_dir(&t.dir)
                .status()?;
            ok &= s.success();
        }
        Ok((ok, format!("ran {}", tests.join(" "))))
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
    fn review<'a>(&'a self, _core: &'a Arc<Core>, _ask: &'a Ask) -> Fut<'a> {
        use std::sync::atomic::Ordering::SeqCst;
        self.asks.fetch_add(1, SeqCst);
        let now = self.now.fetch_add(1, SeqCst) + 1;
        self.most.fetch_max(now, SeqCst);
        Box::pin(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            self.now.fetch_sub(1, SeqCst);
            Ok(Review {
                accept: true,
                reasons: vec!["the slow stand-in accepts".into()],
                cost_usd: 0.25,
                session: None,
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
    let f = fixture();
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

    pub(crate) fn rows(&self, kind: &str) -> Vec<Value> {
        self.core
            .store
            .ledger_tail::<crate::ledger::LedgerRow>(100_000)
            .unwrap()
            .into_iter()
            .filter(|(_, r)| r.kind == kind)
            .map(|(_, r)| r.data)
            .collect()
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
    let pass = r.core.self_join_pass(&seams).await.unwrap();
    assert_eq!(
        pass.reviewed,
        [(branch.clone(), "accept".to_string())],
        "{pass:?}"
    );
    let v = &r.rows("self.review.verdict")[0];
    assert_eq!(v["planted_revert"], "failed");
    assert_eq!(v["reviewer"], "fable (claude-fable-5-1)");
    // The reviewer saw the claim (the report and the diff), on fable, in a
    // self session, and nothing of the builder's working.
    let reqs = r.says.requests.lock().unwrap().clone();
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].model, "claude-fable-5-1");
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
    let again = r.core.self_join_pass(&seams).await.unwrap();
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

/// The same model building and reviewing is no independent review: refused
/// with its reason, once, and nothing is joined.
#[tokio::test]
async fn the_same_model_for_builder_and_reviewer_is_refused() {
    let r = rig(SelfMode::Act);
    let (branch, _) = r.seed("trim-the-sails", PROVED, "claude-fable-5-1");
    let main = r.upstream_main();
    let pass = r.core.self_join_pass(&r.seams()).await.unwrap();
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
    r.core.self_join_pass(&r.seams()).await.unwrap();
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
    let pass = r.core.self_join_pass(&r.seams()).await.unwrap();
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
    r.core.self_join_pass(&r.seams()).await.unwrap();
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
    let one = r.core.self_join_pass(&seams).await.unwrap();
    assert_eq!(one.stopped, [(a.clone(), "keel".to_string())], "{one:?}");
    let two = r.core.self_join_pass(&seams).await.unwrap();
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
    let three = r.core.self_join_pass(&seams).await.unwrap();
    assert!(
        three.stopped.is_empty() && three.joined.is_empty(),
        "{three:?}"
    );
    assert_eq!(r.posts().len(), 2, "never retried at that head");
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
    let pass = tokio::spawn(async move { core.self_join_pass(&seams).await });
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
    let pass = r.core.self_join_pass(&seams).await.unwrap();
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
    let pass = r.core.self_join_pass(&r.seams()).await.unwrap();
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
    let pass = r.core.self_join_pass(&r.seams()).await.unwrap();
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
    let pass = r.core.self_join_pass(&seams).await.unwrap();
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
    let next = r.core.self_join_pass(&r.seams()).await.unwrap();
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
    let pass = r.core.self_join_pass(&r.seams()).await.unwrap();
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
    let pass = r.core.self_join_pass(&r.seams()).await.unwrap();
    assert_eq!(pass.reviewed[0].1, "accept", "{pass:?}");
    let v = &r.rows("self.review.verdict")[0];
    assert_eq!(v["model"], "claude-opus-5-5", "{v}");
    let asks = reviewer_asks(&jev);
    assert_eq!(asks.len(), 1, "one ask");
    // The state names the builder's model, and the change as the owner's
    // material: US options only.
    assert!(asks[0].contains("claude-sonnet-5-5"), "{}", asks[0]);
    assert!(asks[0].contains("not_allowed"), "{}", asks[0]);
    assert!(asks[0].contains("job"), "{}", asks[0]);
}

#[tokio::test]
async fn a_set_reviewer_profile_asks_jev_nothing() {
    let jev = theseus_judge::fake::FakeJev::start().unwrap();
    crate::tests_job_ask::pick(&jev, "haiku", 0.95);
    let r = rig_jev(&jev, Some("opus"));
    r.seed("trim-the-sails", PROVED, SONNET);
    r.core.self_join_pass(&r.seams()).await.unwrap();
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
    let pass = r.core.self_join_pass(&r.seams()).await.unwrap();
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
    let pass = r.core.self_join_pass(&r.seams()).await.unwrap();
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
    r.seed("trim-the-sails", PROVED, "claude-fable-5-1");
    let t0 = std::time::Instant::now();
    let pass = r.core.self_join_pass(&r.seams()).await.unwrap();
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
    let pass = r.core.self_join_pass(&r.seams()).await.unwrap();
    assert_eq!(pass.reviewed[0].1, "refused", "{pass:?}");
    assert_eq!(reviewer_asks(&jev).len(), 1, "Jev was asked");
    jev.release();
}

/// The change is the owner's material: a pick of Flash is refused by the data
/// rule, and the review runs on the live profile.
#[tokio::test]
async fn the_reviewer_is_never_a_china_origin_model() {
    let jev = theseus_judge::fake::FakeJev::start().unwrap();
    crate::tests_job_ask::pick(&jev, "deepseek_flash", 0.97);
    let r = rig_jev(&jev, None);
    r.seed("trim-the-sails", PROVED, "claude-fable-5-1");
    r.core.self_join_pass(&r.seams()).await.unwrap();
    let v = &r.rows("self.review.verdict")[0];
    assert_eq!(v["model"], SONNET, "{v}");
    assert!(reviewer_asks(&jev)[0].contains("not_allowed"));
}

/// While the acting pack asks no `model`, an unset key reviews on the profile
/// it always did (fable) and asks Jev nothing.
#[tokio::test]
async fn an_unset_reviewer_profile_is_fable_while_the_pack_asks_no_model() {
    let jev = theseus_judge::fake::FakeJev::start().unwrap();
    crate::tests_job_ask::pick(&jev, "haiku", 0.95);
    let r = rig_parts(
        SelfMode::Act,
        |c| {
            c.self_improve.reviewer_profile = None;
            c.judge.enabled = true;
            c.judge.api_base = jev.base();
            c.routing.max_wait_ms = 300;
        },
        |p| p.secrets = crate::tests_judge::board(),
    );
    r.seed("trim-the-sails", PROVED, SONNET);
    r.core.self_join_pass(&r.seams()).await.unwrap();
    let v = &r.rows("self.review.verdict")[0];
    assert_eq!(v["model"], "claude-fable-5-1", "{v}");
    assert!(reviewer_asks(&jev).is_empty());
}
