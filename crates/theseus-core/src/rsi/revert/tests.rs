//! Auto-revert and the veto through the whole core (theseus-pw1q.10), over
//! `tests_tree`'s throwaway repository: a bare upstream with a stand-in
//! `scripts/gate.sh` on main (red while `broken.txt` exists), a clone of it as
//! `[self] repo` whose config names a stand-in gpg, and a second clone, the
//! "world", that lands merges on upstream's main: a self-join is a merge with
//! a `self.joined` row, an outer-loop join one without.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use theseus_protocol::rsi::{SelfHaltParams, SelfMode, SelfResumeParams, SelfRevertParams};

use super::attribute::{MainsGate, ProbeFut, Prober};
use super::signals::{Red, Signals};
use super::Seams;
use crate::approval::Answerer;
use crate::fact::rsi_join::SelfJoined;
use crate::policy::Posture;
use crate::provider::{FakeProvider, Provider};
use crate::rsi::join::install::Installer;
use crate::rsi::tests_tree::{fixture, fixture_with, hermetic, Fixture};
use crate::rsi::tree::Place;
use crate::store::Store;
use crate::{Config, Core};

mod fetch_retry;

const FAKE_GPG: &str = "#!/bin/bash\n\
echo '[GNUPG:] KEY_CONSIDERED FIXTURE 0' >&2\n\
echo '[GNUPG:] SIG_CREATED D 1 8 00 1792000000 FIXTURE' >&2\n\
cat >/dev/null\n\
printf -- '-----BEGIN PGP SIGNATURE-----\\n\\nZml4dHVyZQ==\\n-----END PGP SIGNATURE-----\\n'\n";

fn git(dir: &Path, args: &[&str]) -> String {
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

/// The signals a test says, and how often they were read.
#[derive(Default)]
struct Said {
    reds: Mutex<Vec<Red>>,
    reads: AtomicUsize,
}

impl Signals for Said {
    fn reds(&self, _core: &Core, _since_ms: u64, _now_ms: u64) -> anyhow::Result<Vec<Red>> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        Ok(self.reds.lock().unwrap().clone())
    }
}

/// Main's gate, counting the commits it was asked about.
#[derive(Default)]
struct Probed {
    asked: Mutex<Vec<String>>,
}

impl Prober for Probed {
    fn red_at<'a>(&'a self, p: &'a Place, commit: &'a str, benches: bool) -> ProbeFut<'a> {
        self.asked.lock().unwrap().push(commit.to_string());
        MainsGate.red_at(p, commit, benches)
    }
}

/// An installer that starts nothing and remembers the commit.
#[derive(Default)]
struct Installs(Mutex<Vec<String>>);

impl Installer for Installs {
    fn start(&self, _p: &Place, merge: &str) -> anyhow::Result<String> {
        self.0.lock().unwrap().push(merge.to_string());
        Ok("unit".into())
    }
}

struct Rig {
    core: Arc<Core>,
    f: Fixture,
    said: Arc<Said>,
    probed: Arc<Probed>,
    installs: Arc<Installs>,
    work: PathBuf,
    _dir: tempfile::TempDir,
}

fn rig(mode: SelfMode) -> Rig {
    rig_on(fixture(), mode, |_| {})
}

fn rig_on(f: Fixture, mode: SelfMode, tweak: impl FnOnce(&mut Config)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
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
    let work = root.join("world");
    git(
        &root,
        &["clone", "-q", f.upstream.to_str().unwrap(), "world"],
    );
    let mut cfg = Config::example();
    cfg.server.state_dir = f.place.state.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(dir.path().canonicalize().unwrap().display().to_string());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = Posture::Notify;
    cfg.self_improve.mode = mode;
    cfg.self_improve.repo = Some(f.place.repo.display().to_string());
    cfg.self_improve.remote = Some("origin".into());
    tweak(&mut cfg);
    let store = Store::open(&f.place.state.join("store")).unwrap();
    let provider: Arc<dyn Provider> = Arc::new(FakeProvider::scripted(vec![]));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, provider, store)).unwrap();
    core.tools.self_pr.set_env(hermetic());
    core.self_resume(&SelfResumeParams::default(), &Answerer::from("the CLI"))
        .unwrap();
    Rig {
        core,
        f,
        said: Arc::default(),
        probed: Arc::default(),
        installs: Arc::default(),
        work,
        _dir: dir,
    }
}

impl Rig {
    fn seams(&self) -> Seams {
        Seams {
            signals: self.said.clone(),
            prober: self.probed.clone(),
            installer: self.installs.clone(),
            lock_every: std::time::Duration::from_millis(50),
            lock_wait: std::time::Duration::from_secs(60),
            now_ms: None,
            on_phase: None,
        }
    }

    /// A merge of a branch holding `files` onto upstream's main; its commit.
    fn land(&self, name: &str, files: &[(&str, &str)]) -> String {
        let w = &self.work;
        git(w, &["checkout", "-q", "main"]);
        git(w, &["pull", "-q", "--ff-only"]);
        git(w, &["checkout", "-q", "-b", name]);
        for (path, text) in files {
            let p = w.join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        git(w, &["add", "-A"]);
        git(w, &["commit", "-q", "-m", &format!("work: {name}")]);
        git(w, &["checkout", "-q", "main"]);
        git(
            w,
            &[
                "merge",
                "-q",
                "--no-ff",
                "-m",
                &format!("Merge {name}"),
                name,
            ],
        );
        git(w, &["push", "-q", "origin", "main"]);
        git(w, &["rev-parse", "HEAD"])
    }

    /// The `self.joined` row a self-join writes for `merge`.
    fn joined(&self, name: &str, merge: &str) {
        self.core.rec(None).record(&SelfJoined {
            item: name.into(),
            branch: format!("self/20261014-{name}"),
            head: git(&self.work, &["rev-parse", &format!("{merge}^2")]),
            merge: merge.into(),
            gate: "green".into(),
            cost_usd: 1.0,
            why: format!("{name} was worth doing"),
            numbers: Value::Null,
        });
    }

    fn red(&self, signal: &'static str, commit: &str) {
        self.said.reds.lock().unwrap().push(Red {
            signal,
            at_ms: theseus_protocol::now_unix_ms(),
            commit: Some(commit[..9].to_string()),
            detail: format!("a stand-in {signal} read red at {commit}"),
        });
    }

    fn rows(&self, kind: &str) -> Vec<Value> {
        self.core
            .store
            .ledger_tail::<crate::ledger::LedgerRow>(100_000)
            .unwrap()
            .into_iter()
            .filter(|(_, r)| r.kind == kind)
            .map(|(_, r)| r.data)
            .collect()
    }

    fn upstream_main(&self) -> String {
        git(&self.f.upstream, &["rev-parse", "refs/heads/main"])
    }

    fn file_on_main(&self, path: &str) -> bool {
        Command::new("git")
            .arg("-C")
            .arg(&self.f.upstream)
            .args(["cat-file", "-e", &format!("refs/heads/main:{path}")])
            .envs(hermetic())
            .status()
            .unwrap()
            .success()
    }

    fn posts(&self) -> Vec<String> {
        crate::tests_day_ceiling::posts(&self.core, "self_join")
            .iter()
            .map(|b| b["text"].as_str().unwrap_or("").to_string())
            .collect()
    }

    async fn pass(&self) -> super::Pass {
        self.core
            .self_revert_pass(&self.seams(), &mut HashSet::new())
            .await
            .unwrap()
    }
}

/// A planted regression after a seeded self-join: main's gate is red at its
/// merge, the bisection finds that merge, and it is reverted off main by a
/// signed commit that passed main's gate, with the row, the owner's notice,
/// and the install handed over.
#[tokio::test]
async fn a_planted_regression_after_a_self_join_is_reverted_and_noticed() {
    let r = rig(SelfMode::Act);
    let good = r.land("mend-sails", &[("sails.txt", "mended\n")]);
    r.joined("mend-sails", &good);
    let bad = r.land(
        "trim-mast",
        &[("broken.txt", "oops\n"), ("mast.txt", "trimmed\n")],
    );
    r.joined("trim-mast", &bad);
    r.red("gate", &bad);
    let pass = r.pass().await;
    assert_eq!(pass.watched, 2, "{pass:?}");
    assert_eq!(
        pass.reverted,
        [(bad.clone(), "gate".to_string())],
        "{pass:?}"
    );
    let now = r.upstream_main();
    assert_ne!(now, bad, "main moved past the join");
    assert!(!r.file_on_main("broken.txt") && !r.file_on_main("mast.txt"));
    assert!(r.file_on_main("sails.txt"), "the good join stays");
    let rows = r.rows("self.reverted");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["merge"], bad.as_str());
    assert_eq!(rows[0]["revert"], now.as_str());
    assert_eq!(rows[0]["signal"], "gate");
    assert_eq!(rows[0]["window"], 24);
    assert!(rows[0]["numbers"]["detail"]
        .as_str()
        .unwrap()
        .contains("stand-in gate"));
    let raw = git(&r.f.upstream, &["cat-file", "commit", &now]);
    assert!(raw.contains("gpgsig"), "the revert is signed: {raw}");
    assert_eq!(git(&r.f.upstream, &["rev-parse", &format!("{now}^")]), bad);
    let posts = r.posts();
    assert_eq!(posts.len(), 1, "{posts:?}");
    assert!(
        posts[0].contains("reverted its own change") && posts[0].contains("gate signal"),
        "{}",
        posts[0]
    );
    // The good join's merge is not touched by a second pass: it has no red.
    r.said.reds.lock().unwrap().clear();
    let again = r.pass().await;
    assert!(again.reverted.is_empty() && again.watched == 1, "{again:?}");
}

/// The revert's commit is by the self repo's identity, which is what lets
/// GitHub show it Verified, and by Theseus's own when the repo names none
/// (theseus-smi8m). The environment's own identity would beat either, so the
/// rig gives git none.
#[tokio::test]
async fn a_revert_is_by_the_self_repos_identity() {
    const OWNER: &str = "102749+zeroaltitude@users.noreply.github.com";
    for configured in [true, false] {
        let r = rig(SelfMode::Act);
        r.core
            .tools
            .self_pr
            .set_env(crate::rsi::tests_identity::bare());
        let want = if configured {
            git(&r.f.place.repo, &["config", "user.email", OWNER]);
            git(&r.f.place.repo, &["config", "user.name", "The Owner"]);
            format!("The Owner <{OWNER}>")
        } else {
            "Theseus <theseus@localhost>".to_string()
        };
        let bad = r.land("trim-mast", &[("broken.txt", "oops\n")]);
        r.joined("trim-mast", &bad);
        r.red("gate", &bad);
        let pass = r.pass().await;
        assert_eq!(pass.reverted.len(), 1, "{pass:?}");
        let now = r.upstream_main();
        assert_ne!(now, bad);
        let up = &r.f.upstream;
        assert_eq!(git(up, &["log", "-1", "--format=%an <%ae>", &now]), want);
        assert_eq!(git(up, &["log", "-1", "--format=%cn <%ce>", &now]), want);
    }
}

/// The install is the self-join's path, handed the revert's commit, and only
/// with `[self] install`.
#[tokio::test]
async fn the_install_is_handed_the_revert_when_install_is_on() {
    let r = rig_on(fixture(), SelfMode::Act, |c| c.self_improve.install = true);
    let bad = r.land("trim-mast", &[("broken.txt", "oops\n")]);
    r.joined("trim-mast", &bad);
    r.red("gate", &bad);
    let pass = r.pass().await;
    assert_eq!(pass.reverted.len(), 1, "{pass:?}");
    let now = r.upstream_main();
    assert_eq!(*r.installs.0.lock().unwrap(), [now]);
}

/// A red that was there before the join is not the join's: main's gate is
/// red at the commit before it, and nothing is reverted.
#[tokio::test]
async fn a_red_that_was_there_before_the_join_leaves_it_alone() {
    let r = rig(SelfMode::Act);
    let _old = r.land("outer-old", &[("broken.txt", "already red\n")]);
    let mine = r.land("tidy-deck", &[("deck.txt", "tidy\n")]);
    r.joined("tidy-deck", &mine);
    r.red("gate", &mine);
    let before = r.upstream_main();
    let pass = r.pass().await;
    assert!(pass.reverted.is_empty(), "{pass:?}");
    assert_eq!(pass.left.len(), 1, "{pass:?}");
    assert!(pass.left[0].1.contains("before"), "{pass:?}");
    assert_eq!(r.upstream_main(), before, "main is untouched");
    assert!(r.rows("self.reverted").is_empty() && r.posts().is_empty());
}

/// An outer-loop join is never reverted: the red first shows at a merge no
/// `self.joined` row names, though a self-join sits before it.
#[tokio::test]
async fn an_outer_loop_join_is_never_reverted() {
    let r = rig(SelfMode::Act);
    let mine = r.land("tidy-deck", &[("deck.txt", "tidy\n")]);
    r.joined("tidy-deck", &mine);
    let outer = r.land("outer-new", &[("broken.txt", "from the outer loop\n")]);
    r.red("gate", &outer);
    let before = r.upstream_main();
    let pass = r.pass().await;
    assert!(pass.reverted.is_empty(), "{pass:?}");
    assert!(pass.left[0].1.contains("outer-loop"), "{pass:?}");
    assert_eq!(r.upstream_main(), before);
    assert!(r.file_on_main("deck.txt") && r.file_on_main("broken.txt"));
    assert!(r.rows("self.reverted").is_empty());
    // The same red is judged once: a second pass with the same memory is silent.
    let mut known = HashSet::new();
    let _ = r
        .core
        .self_revert_pass(&r.seams(), &mut known)
        .await
        .unwrap();
    let asked = r.probed.asked.lock().unwrap().len();
    let second = r
        .core
        .self_revert_pass(&r.seams(), &mut known)
        .await
        .unwrap();
    assert!(second.left.is_empty());
    assert_eq!(
        r.probed.asked.lock().unwrap().len(),
        asked,
        "no gate ran again"
    );
}

/// Bisection picks the right one of two self-joins: the red shows at main's
/// head, but the first self-join broke it, and the second, harmless one after
/// it stays. The gate ran on the parent, then the middle of the chain.
#[tokio::test]
async fn bisection_picks_the_right_one_of_two_self_joins() {
    let r = rig(SelfMode::Act);
    let first = r.land("trim-mast", &[("broken.txt", "oops\n")]);
    r.joined("trim-mast", &first);
    let second = r.land("tidy-deck", &[("deck.txt", "tidy\n")]);
    r.joined("tidy-deck", &second);
    r.red("gate", &second);
    let pass = r.pass().await;
    assert_eq!(
        pass.reverted,
        [(first.clone(), "gate".to_string())],
        "{pass:?}"
    );
    assert!(!r.file_on_main("broken.txt"), "the broken join is undone");
    assert!(
        r.file_on_main("deck.txt"),
        "the harmless one after it stays"
    );
    let asked = r.probed.asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 2, "the parent, then the first join: {asked:?}");
    assert_eq!(asked[1], first);
    assert_eq!(r.rows("self.reverted")[0]["merge"], first.as_str());
}

/// The owner's veto reverts within one step, drops the item and says who and
/// where; a second veto of the same merge changes nothing.
#[tokio::test]
async fn a_veto_reverts_at_once_and_a_second_changes_nothing() {
    let r = rig(SelfMode::Act);
    let m = r.land("trim-mast", &[("mast.txt", "trimmed\n")]);
    r.joined("trim-mast", &m);
    let p = SelfRevertParams {
        merge: m[..10].to_string(),
        ..Default::default()
    };
    let got = r
        .core
        .self_revert(&p, &Answerer::from("the CLI"), &r.seams())
        .await
        .unwrap();
    assert!(got.changed && got.merge == m, "{got:?}");
    assert_eq!(got.revert, r.upstream_main());
    assert!(!r.file_on_main("mast.txt"));
    let rows = r.rows("self.vetoed");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["merge"], m.as_str());
    assert_eq!(rows[0]["revert"], got.revert.as_str());
    assert!(rows[0]["by"].as_str().unwrap().contains("CLI"), "{rows:?}");
    assert!(rows[0]["place"].is_string());
    assert!(r.posts()[0].contains("Your veto"), "{:?}", r.posts());
    let before = r.upstream_main();
    let again = r
        .core
        .self_revert(&p, &Answerer::from("the CLI"), &r.seams())
        .await
        .unwrap();
    assert!(!again.changed && again.revert == got.revert);
    assert_eq!(r.upstream_main(), before);
    assert_eq!(r.rows("self.vetoed").len(), 1);
}

/// A job's shell is refused with an `approval.refused` row and nothing moves;
/// so is a merge no self-join made, and a name too short to mean one.
#[tokio::test]
async fn a_jobs_veto_is_refused_and_only_self_joins_can_be_vetoed() {
    let r = rig(SelfMode::Act);
    let mine = r.land("trim-mast", &[("mast.txt", "trimmed\n")]);
    r.joined("trim-mast", &mine);
    let outer = r.land("outer-new", &[("outer.txt", "outer\n")]);
    let before = r.upstream_main();
    let job = SelfRevertParams {
        merge: mine.clone(),
        from_job: Some("ses_a_job".into()),
        ..Default::default()
    };
    let e = r
        .core
        .self_revert(&job, &Answerer::from("the CLI"), &r.seams())
        .await
        .unwrap_err();
    assert!(
        e.downcast_ref::<crate::approval::Refusal>().is_some(),
        "{e:#}"
    );
    let refused = r.rows("approval.refused");
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert!(
        refused[0].to_string().contains("self.revert"),
        "{refused:?}"
    );
    let outer_veto = SelfRevertParams {
        merge: outer,
        ..Default::default()
    };
    let e = r
        .core
        .self_revert(&outer_veto, &Answerer::from("the CLI"), &r.seams())
        .await
        .unwrap_err();
    assert!(e.downcast_ref::<super::BadMerge>().is_some(), "{e:#}");
    assert!(format!("{e:#}").contains("outer-loop"), "{e:#}");
    let short = SelfRevertParams {
        merge: "abc".into(),
        ..Default::default()
    };
    let e = r
        .core
        .self_revert(&short, &Answerer::from("the CLI"), &r.seams())
        .await
        .unwrap_err();
    assert!(e.downcast_ref::<super::BadMerge>().is_some());
    assert_eq!(r.upstream_main(), before, "nothing was reverted");
    assert!(r.rows("self.vetoed").is_empty());
}

/// With the mode off, or the switch halted, nothing watches (the signals are
/// not even read) and nothing reverts on its own; the owner's veto still works.
#[tokio::test]
async fn mode_off_and_a_halt_watch_nothing_but_the_veto_still_works() {
    let off = rig(SelfMode::Off);
    let m = off.land("trim-mast", &[("broken.txt", "oops\n")]);
    off.joined("trim-mast", &m);
    off.red("gate", &m);
    let pass = off.pass().await;
    assert_eq!((pass.gate.as_str(), pass.watched), ("off", 0), "{pass:?}");
    assert_eq!(off.said.reads.load(Ordering::SeqCst), 0);
    assert_eq!(off.upstream_main(), m);
    let got = off
        .core
        .self_revert(
            &SelfRevertParams {
                merge: m.clone(),
                ..Default::default()
            },
            &Answerer::from("the CLI"),
            &off.seams(),
        )
        .await
        .unwrap();
    assert!(
        got.changed,
        "the veto is the owner's own act, mode off or not"
    );

    let halted = rig(SelfMode::Act);
    let m = halted.land("trim-mast", &[("broken.txt", "oops\n")]);
    halted.joined("trim-mast", &m);
    halted.red("gate", &m);
    halted
        .core
        .self_halt(&SelfHaltParams::default(), &Answerer::from("the CLI"))
        .unwrap();
    let pass = halted.pass().await;
    assert_eq!(pass.gate, "halted");
    assert_eq!(halted.said.reads.load(Ordering::SeqCst), 0);
    assert_eq!(halted.upstream_main(), m);
}

/// Past `revert_window_hours` a join is no longer watched, and with 0 hours
/// nothing is.
#[tokio::test]
async fn a_join_past_the_window_is_not_watched() {
    let r = rig(SelfMode::Act);
    let m = r.land("trim-mast", &[("broken.txt", "oops\n")]);
    r.joined("trim-mast", &m);
    r.red("gate", &m);
    let mut seams = r.seams();
    seams.now_ms = Some(theseus_protocol::now_unix_ms() + 25 * 3_600_000);
    let pass = r
        .core
        .self_revert_pass(&seams, &mut HashSet::new())
        .await
        .unwrap();
    assert_eq!(pass.watched, 0, "{pass:?}");
    assert_eq!(r.said.reads.load(Ordering::SeqCst), 0);
    assert_eq!(r.upstream_main(), m);
}

/// A revert that would itself trip the keel goes to the owner's card: the
/// gate on the revert says so, nothing is pushed, and the change stays on main.
#[tokio::test]
async fn a_revert_that_trips_the_keel_goes_to_the_owners_card() {
    // main's gate is a keel finding while `dep.txt` stands without `fix.txt`.
    let gate = "#!/bin/bash\ncd \"${THESEUS_GATE_ROOT:-$(dirname \"$0\")/..}\"\n\
        if [ -f dep.txt ] && [ ! -f fix.txt ]; then echo 'gate: FAILED in keel'; exit 1; fi\n\
        echo 'gate: ok'\n";
    let r = rig_on(fixture_with(gate), SelfMode::Act, |_| {});
    let m = r.land("add-fix", &[("fix.txt", "fixed\n")]);
    r.joined("add-fix", &m);
    let _outer = r.land("outer-dep", &[("dep.txt", "needs the fix\n")]);
    let before = r.upstream_main();
    let e = r
        .core
        .self_revert(
            &SelfRevertParams {
                merge: m.clone(),
                ..Default::default()
            },
            &Answerer::from("the CLI"),
            &r.seams(),
        )
        .await
        .unwrap_err();
    assert!(format!("{e:#}").contains("stopped at the keel"), "{e:#}");
    assert_eq!(r.upstream_main(), before, "nothing was pushed");
    let posts = r.posts();
    assert_eq!(posts.len(), 1, "{posts:?}");
    assert!(
        posts[0].contains("could not revert") && posts[0].contains("keel"),
        "{}",
        posts[0]
    );
    assert!(r.rows("self.vetoed").is_empty() && r.rows("self.reverted").is_empty());
    let stops = r.rows("self.join.stopped");
    assert_eq!(stops[0]["phase"], "keel");
    // The tree was closed: no revert tree is left behind.
    let trees = git(&r.f.place.repo, &["worktree", "list"]);
    assert!(!trees.contains("revert-"), "{trees}");
    // An automatic pass leaves a held revert alone afterwards.
    r.red("gate", &m);
    let pass = r.pass().await;
    assert!(pass.reverted.is_empty() && pass.watched == 0, "{pass:?}");
}
