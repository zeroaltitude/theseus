//! The daemon's side of installing a joined change into the root-owned directory (issue 22): against a join in the
//! loop's own test rig, with a stand-in helper, unit and build. No root, no sudo, no systemd.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use theseus_protocol::rsi::SelfMode;

use super::answer::{restart_script, Go};
use super::*;
use crate::config::self_improve::SelfInstall;
use crate::rsi::join::tests::{git, rig_with, Rig, PROVED, SONNET};
use crate::rsi::join::{install, rows_of};

const OLD: &str = "/usr/local/lib/theseus/aaaaaaaa/theseusd";
const OWNER: &str = "discord:123456789012345678";

fn unit_text(program: &str) -> String {
    format!("# Written by `theseusd install --user`.\n[Service]\nType=notify\nExecStart={program} --config /etc/theseus/theseus.toml\nRestart=on-failure\n")
}

#[derive(Default)]
struct FakeHelper {
    absent: Option<String>,
    refuse: Option<String>,
    calls: Mutex<Vec<String>>,
    staged: Mutex<Vec<PathBuf>>,
    installed: Mutex<Vec<String>>,
}

impl Helper for FakeHelper {
    fn presence(&self) -> Presence {
        self.calls.lock().unwrap().push("presence".into());
        self.absent
            .clone()
            .map_or(Presence::Installed, Presence::NotInstalled)
    }

    fn stage(&self, release: &Path) -> Result<()> {
        self.calls.lock().unwrap().push("stage".into());
        for b in install::BINARIES {
            anyhow::ensure!(release.join(b).exists(), "{b} is missing from the build");
        }
        self.staged.lock().unwrap().push(release.to_path_buf());
        Ok(())
    }

    fn install(&self, commit: &str) -> Result<String> {
        self.calls.lock().unwrap().push("install".into());
        if let Some(why) = &self.refuse {
            anyhow::bail!("{why}");
        }
        self.installed.lock().unwrap().push(commit.into());
        Ok(format!("installed {commit}"))
    }
}

struct FakeUnits {
    unit: Mutex<Unit>,
    restarts: Mutex<Vec<String>>,
    fail_restart: bool,
}

impl FakeUnits {
    fn new(program: &str, fail_restart: bool) -> Arc<Self> {
        Arc::new(Self {
            unit: Mutex::new(Unit {
                path: PathBuf::from("/home/ada/.config/systemd/user/theseusd.service"),
                text: unit_text(program),
            }),
            restarts: Mutex::default(),
            fail_restart,
        })
    }

    fn text(&self) -> String {
        self.unit.lock().unwrap().text.clone()
    }
}

impl Units for FakeUnits {
    fn read(&self) -> Result<Unit> {
        Ok(self.unit.lock().unwrap().clone())
    }

    fn write(&self, _at: &Path, text: &str) -> Result<()> {
        self.unit.lock().unwrap().text = text.to_string();
        Ok(())
    }

    fn restart(&self, script: &str, _short: &str) -> Result<String> {
        anyhow::ensure!(!self.fail_restart, "systemd-run refused");
        self.restarts.lock().unwrap().push(script.to_string());
        Ok("theseus-root-install-test".into())
    }
}

/// What `scripts/build.sh --profile release` leaves in a build tree.
fn fake_build(_: &crate::rsi::tree::Place, t: &crate::rsi::tree::Tree, _: &str) -> Result<()> {
    let out = t.target.join("release");
    std::fs::create_dir_all(&out)?;
    for b in install::BINARIES {
        std::fs::write(out.join(b), format!("fake {b}"))?;
    }
    Ok(())
}

struct Rooted {
    r: Rig,
    helper: Arc<FakeHelper>,
    units: Arc<FakeUnits>,
}

fn rooted_with(
    mode: SelfInstall,
    proof: Option<bool>,
    helper: FakeHelper,
    units: Arc<FakeUnits>,
) -> Rooted {
    let r = rig_with(SelfMode::Act, |c| {
        c.self_improve.install = mode;
        c.places.owner = Some(vec![OWNER.into()]);
        // No index tender runs in a test, so a daemon that waits for it to answer would never be healthy.
        c.index.enabled = false;
    });
    let helper = Arc::new(helper);
    r.core.tools.root_install.stand_in(Seams {
        helper: helper.clone(),
        units: units.clone(),
        proof,
        build: Arc::new(fake_build),
    });
    Rooted { r, helper, units }
}

fn rooted(mode: SelfInstall, proof: Option<bool>) -> Rooted {
    rooted_with(
        mode,
        proof,
        FakeHelper::default(),
        FakeUnits::new(OLD, false),
    )
}

impl Rooted {
    /// A branch joined by the loop's own pass: the merge.
    async fn join(&self) -> String {
        self.r.seed("trim-the-sails", PROVED, SONNET);
        self.r.core.self_join_pass(&self.r.seams()).await.unwrap();
        self.r.upstream_main()
    }

    fn steps(&self) -> Vec<String> {
        let rows = rows_of(&self.r.core.store, &["self.root_install"])
            .unwrap()
            .unwrap_or_default();
        rows.iter()
            .map(|r| r.data["step"].as_str().unwrap_or("?").to_string())
            .collect()
    }

    fn why_of(&self, step: &str) -> String {
        let rows = rows_of(&self.r.core.store, &["self.root_install"])
            .unwrap()
            .unwrap_or_default();
        rows.iter()
            .filter(|r| r.data["step"] == step)
            .next_back()
            .map(|r| r.data["why"].as_str().unwrap_or("").to_string())
            .unwrap_or_default()
    }

    fn cards(&self) -> Vec<theseus_protocol::ConfirmRequest> {
        self.r
            .core
            .confirm_list()
            .unwrap()
            .into_iter()
            .filter(|c| c.tool == ACK)
            .collect()
    }

    fn notices(&self) -> Vec<String> {
        crate::tests_day_ceiling::posts(&self.r.core, "notice")
            .iter()
            .map(|b| b["text"].as_str().unwrap_or("").to_string())
            .collect()
    }

    /// Wait (the run is a task) until `done` holds.
    async fn until(&self, what: &str, done: impl Fn(&Self) -> bool) {
        for _ in 0..200 {
            if done(self) {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        panic!("never: {what}; steps {:?}", self.steps());
    }

    fn yes(&self, question: &str) {
        self.r
            .core
            .confirm_action(question, true, None, "cli")
            .unwrap();
    }
}

/// KEY: "ask" makes the card (after the join, no sooner), the yes stages the programs, runs the helper for the
/// joined commit, points the unit at the new directory and restarts once; every step is a ledger row, and the pipeline's
/// words wait for the confirmation.
#[tokio::test]
async fn ask_makes_the_card_and_the_yes_installs() {
    let t = rooted(SelfInstall::Ask, Some(true));
    let merge = t.join().await;
    let cards = t.cards();
    assert_eq!(cards.len(), 1, "one card: {:?}", t.steps());
    let c8 = short(&merge);
    assert!(
        cards[0]
            .reason
            .starts_with("Install the change Theseus joined: "),
        "{}",
        cards[0].reason
    );
    assert!(
        cards[0].reason.contains(c8) && cards[0].reason.contains("second proof"),
        "{}",
        cards[0].reason
    );
    assert!(
        t.helper.installed.lock().unwrap().is_empty(),
        "nothing is installed before the yes"
    );
    assert!(t.units.restarts.lock().unwrap().is_empty());
    assert_eq!(t.units.text(), unit_text(OLD));
    t.yes(&cards[0].correlation_id);
    t.until("the restart", |t| {
        !t.units.restarts.lock().unwrap().is_empty()
    })
    .await;
    assert_eq!(
        *t.helper.installed.lock().unwrap(),
        std::slice::from_ref(&merge)
    );
    assert_eq!(t.helper.staged.lock().unwrap().len(), 1);
    let new = program_of(&merge).display().to_string();
    assert!(t.units.text().contains(&format!(
        "ExecStart={new} --config /etc/theseus/theseus.toml"
    )));
    assert_eq!(
        t.units.restarts.lock().unwrap().len(),
        1,
        "one restart, started once"
    );
    let steps = t.steps();
    for want in [
        "asked",
        "approved",
        "started",
        "staged",
        "helper",
        "unit",
        "restarting",
    ] {
        assert!(steps.iter().any(|s| s == want), "{want} in {steps:?}");
    }
    // The memory of it is in the store, and the pipeline's words wait: not "installed and running" yet.
    let p = t.r.core.root_pending().unwrap();
    assert_eq!(
        (p.merge.as_str(), p.old_program.as_str(), p.stage),
        (merge.as_str(), OLD, Stage::Restarting)
    );
    assert_eq!(p.unit_before, unit_text(OLD));
    assert!(t.r.core.root_install_pending_for(&merge));
    assert_eq!(
        t.r.core.self_installed_as(&merge).unwrap(),
        None,
        "not announced before it is confirmed"
    );
}

/// "auto" makes no card and installs; "off" never calls the helper; a user's own layout is not this module's.
#[tokio::test]
async fn auto_installs_with_no_card_and_off_never_touches_the_helper() {
    let t = rooted(SelfInstall::Auto, None);
    let merge = t.join().await;
    assert!(t.cards().is_empty(), "no card for auto");
    t.until("the restart", |t| {
        !t.units.restarts.lock().unwrap().is_empty()
    })
    .await;
    assert_eq!(*t.helper.installed.lock().unwrap(), [merge]);

    let t = rooted(SelfInstall::Off, Some(true));
    t.join().await;
    assert!(
        t.helper.calls.lock().unwrap().is_empty(),
        "off never asks the helper anything"
    );
    assert!(
        t.cards().is_empty() && t.units.restarts.lock().unwrap().is_empty() && t.steps().is_empty()
    );

    let t = rooted_with(
        SelfInstall::Ask,
        Some(true),
        FakeHelper::default(),
        FakeUnits::new("/home/ada/.local/bin/theseusd", false),
    );
    t.join().await;
    assert!(
        t.helper.calls.lock().unwrap().is_empty(),
        "a user's own layout is not this module's"
    );
    assert!(t.cards().is_empty() && t.steps().is_empty());
}

/// No helper: no card, no install, and the owner is told plainly, with the step.
#[tokio::test]
async fn no_helper_means_no_install_and_a_plain_notice() {
    let helper = FakeHelper {
        absent: Some("there is no /usr/local/sbin/theseus-root-install".into()),
        ..Default::default()
    };
    let t = rooted_with(
        SelfInstall::Ask,
        Some(true),
        helper,
        FakeUnits::new(OLD, false),
    );
    t.join().await;
    assert!(t.cards().is_empty());
    assert!(
        t.helper.installed.lock().unwrap().is_empty()
            && t.units.restarts.lock().unwrap().is_empty()
    );
    let said = t.notices().join("\n");
    assert!(
        said.contains("helper")
            && said.contains("not installed")
            && said.contains("root-owned install"),
        "{said}"
    );
    assert!(t.steps().contains(&"no_helper".to_string()));
}

/// The second proof is asked: until it is built in no card is made (and no build is spent), and a yes that arrives
/// anyway is refused with the seam's words and installs nothing.
#[tokio::test]
async fn the_second_proof_is_asked_through_the_seam() {
    let t = rooted(SelfInstall::Ask, None);
    t.join().await;
    assert!(
        t.cards().is_empty(),
        "no card while the proof is not installed"
    );
    let said = t.notices().join("\n");
    assert!(
        said.contains(crate::config_change::proof_seam::NOT_INSTALLED),
        "{said}"
    );
    assert!(t
        .helper
        .calls
        .lock()
        .unwrap()
        .iter()
        .all(|c| c == "presence"));

    // A card made while the proof was given, answered after it no longer is.
    let t = rooted(SelfInstall::Ask, Some(true));
    t.join().await;
    let q = t.cards()[0].correlation_id.clone();
    t.r.core.tools.root_install.stand_in(Seams {
        helper: t.helper.clone(),
        units: t.units.clone(),
        proof: Some(false),
        build: Arc::new(fake_build),
    });
    t.yes(&q);
    assert!(t.steps().contains(&"declined".to_string()));
    assert!(
        t.helper.installed.lock().unwrap().is_empty()
            && t.units.restarts.lock().unwrap().is_empty()
    );
}

/// Only a commit the loop's own join row names, and that is main's head on the remote now.
#[tokio::test]
async fn a_commit_the_loop_did_not_join_or_that_is_not_mains_head_is_refused() {
    let t = rooted(SelfInstall::Ask, Some(true));
    let merge = t.join().await;
    let go = |merge: &str| Go {
        question: "q".into(),
        merge: merge.into(),
        branch: "b".into(),
        what: "w".into(),
        target: PathBuf::from("/nonexistent"),
        old_program: OLD.into(),
        approved_by: "cli".into(),
        via: "cli".into(),
    };
    t.r.core.clone().run_root_install(go(&"f".repeat(40))).await;
    assert!(
        t.why_of("failed")
            .contains("no join of this loop names that commit"),
        "{:?}",
        t.steps()
    );
    // The loop's own join, after main has moved on.
    let up = &t.r.f.upstream;
    let tree = git(up, &["rev-parse", "main^{tree}"]);
    let moved = git(
        up,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "commit-tree",
            &tree,
            "-p",
            &merge,
            "-m",
            "moved",
        ],
    );
    git(up, &["update-ref", "refs/heads/main", &moved]);
    t.r.core.clone().run_root_install(go(&merge)).await;
    assert!(
        t.why_of("failed").contains("no longer main's head"),
        "{}",
        t.why_of("failed")
    );
    assert!(
        t.helper
            .calls
            .lock()
            .unwrap()
            .iter()
            .all(|c| c == "presence"),
        "the helper was never asked to install"
    );
    assert_eq!(t.units.text(), unit_text(OLD));
    assert!(t.units.restarts.lock().unwrap().is_empty());
}

/// A helper that refuses leaves everything as it was: the unit, the store's marker, the service.
#[tokio::test]
async fn a_helper_that_fails_leaves_everything_as_it_was() {
    let helper = FakeHelper {
        refuse: Some("aaaaaaaa already exists and differs".into()),
        ..Default::default()
    };
    let t = rooted_with(
        SelfInstall::Ask,
        Some(true),
        helper,
        FakeUnits::new(OLD, false),
    );
    t.join().await;
    let q = t.cards()[0].correlation_id.clone();
    t.yes(&q);
    t.until("the failure", |t| t.steps().contains(&"failed".to_string()))
        .await;
    assert!(
        t.why_of("failed").contains("already exists and differs"),
        "{}",
        t.why_of("failed")
    );
    assert_eq!(t.units.text(), unit_text(OLD));
    assert!(t.units.restarts.lock().unwrap().is_empty());
    assert!(
        t.r.core.root_pending().is_none(),
        "no marker: nothing started"
    );
    assert!(t
        .notices()
        .join("\n")
        .contains("Nothing it runs has changed"));
}

/// A restart that cannot be started puts the unit back, and ends the install.
#[tokio::test]
async fn a_restart_that_cannot_start_puts_the_unit_back() {
    let t = rooted_with(
        SelfInstall::Auto,
        None,
        FakeHelper::default(),
        FakeUnits::new(OLD, true),
    );
    t.join().await;
    t.until("the failure", |t| t.steps().contains(&"failed".to_string()))
        .await;
    assert_eq!(t.units.text(), unit_text(OLD), "the unit is as it was");
    assert_eq!(t.r.core.root_pending().unwrap().stage, Stage::RolledBack);
}

/// The start after: the build that is the one named confirms itself; any other says the install went back.
#[tokio::test]
async fn the_start_after_confirms_or_says_it_went_back() {
    let t = rooted(SelfInstall::Auto, None);
    let merge = t.join().await;
    t.until("the restart", |t| {
        !t.units.restarts.lock().unwrap().is_empty()
    })
    .await;
    // The old daemon again (the unit went back): not the build named.
    t.r.core.clone().root_install_after_serving_as(
        Some("0".repeat(40).as_str()),
        PathBuf::from("/nonexistent.sock"),
    );
    assert_eq!(t.r.core.root_pending().unwrap().stage, Stage::RolledBack);
    assert!(t.steps().contains(&"rolled_back".to_string()));
    let said = t.notices().join("\n");
    assert!(
        said.contains("did not come up healthy")
            && said.contains(OLD)
            && said.contains("not installed"),
        "{said}"
    );
    assert!(
        !t.r.core.root_install_pending_for(&merge),
        "ended: its notice may be written"
    );
    // Said once: a later start finds nothing to do.
    let n = t.notices().len();
    t.r.core.clone().root_install_after_serving_as(
        Some("0".repeat(40).as_str()),
        PathBuf::from("/nonexistent.sock"),
    );
    assert_eq!(t.notices().len(), n);
}

/// The new build, healthy: confirmed in the store and by its file, and the join's notice says "installed and running".
#[tokio::test]
async fn a_healthy_new_build_confirms_and_the_notice_says_installed_and_running() {
    let t = rooted(SelfInstall::Auto, None);
    let merge = t.join().await;
    t.until("the restart", |t| {
        !t.units.restarts.lock().unwrap().is_empty()
    })
    .await;
    let sock = t.r.f.upstream.parent().unwrap().join("serve.sock");
    let _keep = tokio::net::UnixListener::bind(&sock).unwrap();
    t.r.core
        .clone()
        .root_install_after_serving_as(Some(&merge), sock);
    t.until("the confirmation", |t| {
        t.r.core
            .root_pending()
            .is_some_and(|p| p.stage == Stage::Confirmed)
    })
    .await;
    assert!(
        t.r.core.tools.root_install.signal().exists(),
        "the file the restart's unit looks for"
    );
    assert!(t.steps().contains(&"verified".to_string()));
    assert!(t
        .posts_of("self_join")
        .iter()
        .any(|p| p.contains("installed and running")));
}

impl Rooted {
    fn posts_of(&self, kind: &str) -> Vec<String> {
        crate::tests_day_ceiling::posts(&self.r.core, kind)
            .iter()
            .map(|b| b["text"].as_str().unwrap_or("").to_string())
            .collect()
    }
}

/// Nothing the daemon trusts about an install is read from a file under the state directory: planted "previous
/// directory" and "unit before" files change nothing, and the way back is the text in the store and in the script.
#[tokio::test]
async fn planted_files_under_the_state_directory_change_nothing() {
    let t = rooted(SelfInstall::Auto, None);
    let dir = t.r.core.tools.root_install.dir();
    std::fs::create_dir_all(&dir).unwrap();
    for f in [
        "pending.json",
        "previous",
        "unit.before",
        "rolled-back",
        "old_program",
    ] {
        std::fs::write(dir.join(f), "[Service]\nExecStart=/tmp/evil/theseusd\n").unwrap();
    }
    let merge = t.join().await;
    t.until("the restart", |t| {
        !t.units.restarts.lock().unwrap().is_empty()
    })
    .await;
    let p = t.r.core.root_pending().unwrap();
    assert_eq!(p.old_program, OLD);
    assert_eq!(p.unit_before, unit_text(OLD));
    let script = t.units.restarts.lock().unwrap()[0].clone();
    assert!(!script.contains("evil"), "{script}");
    assert!(t
        .units
        .text()
        .contains(&program_of(&merge).display().to_string()));
    // A confirmation file planted before is removed, not believed.
    assert!(!dir.join("confirmed").exists());
}

/// The restart's script, run for real with a stand-in `systemctl`: confirmed, it restarts once; not confirmed, or a
/// restart that fails, it puts the unit back and restarts once more: never more than two.
#[test]
fn the_restart_script_goes_back_unless_the_new_daemon_confirms() {
    let run = |confirm: bool, fail_first: bool| -> (Vec<String>, String) {
        let d = tempfile::tempdir().unwrap();
        let (bin, unit, signal, log) = (
            d.path().join("bin"),
            d.path().join("theseusd.service"),
            d.path().join("confirmed"),
            d.path().join("log"),
        );
        std::fs::create_dir(&bin).unwrap();
        std::fs::write(&unit, unit_text("/usr/local/lib/theseus/bbbbbbbb/theseusd")).unwrap();
        let ctl = bin.join("systemctl");
        // The stand-in logs every call and, when asked, fails its first restart (a reload is logged either way).
        let body = format!(
            "#!/bin/bash\necho \"$@\" >> {log}\nif [ \"$2\" = restart ] && [ {fail} = 1 ] && [ ! -e {once} ]; then : > {once}; exit 1; fi\n",
            log = log.display(),
            fail = u8::from(fail_first),
            once = d.path().join("once").display()
        );
        std::fs::write(&ctl, body).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&ctl, std::fs::Permissions::from_mode(0o755)).unwrap();
        if confirm {
            std::fs::write(&signal, "x").unwrap();
        }
        let before = unit_text(OLD);
        let script = restart_script(
            &signal,
            &unit,
            &before,
            "theseusd.service",
            &"b".repeat(40),
            2,
        );
        let out = std::process::Command::new("/bin/bash")
            .args(["-c", &script])
            .env_clear()
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .output()
            .unwrap();
        assert!(out.status.code().is_some(), "{out:?}");
        let calls = std::fs::read_to_string(&log)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect();
        (calls, std::fs::read_to_string(&unit).unwrap())
    };
    let new_unit = unit_text("/usr/local/lib/theseus/bbbbbbbb/theseusd");
    let (calls, unit) = run(true, false);
    assert_eq!(
        calls,
        ["--user daemon-reload", "--user restart theseusd.service"],
        "{calls:?}"
    );
    assert_eq!(unit, new_unit, "confirmed: the unit stays");
    let (calls, unit) = run(false, false);
    assert_eq!(
        calls.iter().filter(|c| c.contains("restart")).count(),
        2,
        "{calls:?}"
    );
    assert_eq!(unit, unit_text(OLD), "not confirmed: the unit is put back");
    let (calls, unit) = run(false, true);
    assert_eq!(
        calls.iter().filter(|c| c.contains("restart")).count(),
        2,
        "{calls:?}"
    );
    assert_eq!(
        unit,
        unit_text(OLD),
        "a restart that fails goes back at once"
    );
}

#[test]
fn the_restart_script_quotes_what_it_embeds() {
    let s = restart_script(
        Path::new("/s/it's"),
        Path::new("/u"),
        "a'b\n[Service]\n",
        "svc",
        &"c".repeat(40),
        5,
    );
    assert!(s.contains("signal='/s/it'\\''s'"), "{s}");
    assert!(s.contains("printf '%s' 'a'\\''b\n[Service]\n'"), "{s}");
    let ok = std::process::Command::new("bash")
        .args(["-n", "-c", &s])
        .status()
        .unwrap();
    assert!(ok.success());
}

/// The card's words name the commit, the title, the place and the way back, in plain words.
#[test]
fn the_card_says_what_where_and_the_way_back() {
    let go = Go {
        question: "q".into(),
        merge: "0123456789abcdef0123456789abcdef01234567".into(),
        branch: "self/x".into(),
        what: "make the sails trim themselves".into(),
        target: PathBuf::new(),
        old_program: OLD.into(),
        approved_by: String::new(),
        via: String::new(),
    };
    let q = offer::question_text(&go, "/usr/local/lib/theseus/aaaaaaaa");
    assert!(
        q.starts_with(
            "Install the change Theseus joined: make the sails trim themselves, 01234567?"
        ),
        "{q}"
    );
    for part in [
        "/usr/local/lib/theseus/01234567",
        "restart",
        "goes back to the build before",
        "second proof",
    ] {
        assert!(q.contains(part), "{part} in {q}");
    }
}
