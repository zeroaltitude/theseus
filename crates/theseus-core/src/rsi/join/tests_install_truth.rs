//! A joined change is not what runs afterwards (issue 20): the install reads where the service's
//! program is. Where the loop can write there it installs, restarts, and the new daemon checks its
//! own commit against the marker the unit left; where it cannot (a root-owned build) it copies
//! nothing, restarts nothing, and tells the owner once, with the one step. The pipeline's words
//! follow.

use crate::rsi::join::tests_publish::PassYes as _;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use serde_json::json;
use theseus_protocol::rsi::{SelfMode, SelfPipelineParams};

use super::install::{self, Installer, Started};
use super::layout::{self, Host, Layout};
use super::tests::{git, rig_with, Rig, PROVED, SONNET};
use super::words::{last_join, Install};
use super::{verify, Seams};
use crate::ledger::LedgerRow;
use crate::rsi::tree::{Place, Tree};

/// A machine as a test describes it: the service's program, the daemon's own, and which
/// directories the loop can write (the real permissions, so a directory the test makes unwritable
/// is one).
#[derive(Default)]
struct Where {
    unit: Option<PathBuf>,
    running: Option<PathBuf>,
}

impl Host for Where {
    fn unit_program(&self) -> Option<PathBuf> {
        self.unit.clone()
    }
    fn running_program(&self) -> Option<PathBuf> {
        self.running.clone()
    }
    fn writable(&self, dir: &Path) -> bool {
        crate::binary::writable(dir)
    }
}

/// What `scripts/build.sh --profile release` leaves in the build tree.
fn fake_build(_: &Place, t: &Tree, _: &str) -> anyhow::Result<()> {
    let out = t.target.join("release");
    std::fs::create_dir_all(&out)?;
    for b in install::BINARIES {
        std::fs::write(out.join(b), format!("fake {b}"))?;
    }
    Ok(())
}

/// The machine's installer over a described machine: the build is a stand-in, and the unit is
/// run by `Runs` (so a test sees every `systemd-run` it was handed, and what the script did).
struct On {
    host: Where,
    /// Every unit's argv, as handed over.
    ran: Mutex<Vec<Vec<String>>>,
    /// `systemctl`'s stand-in: where it records its calls; none means the unit is not run.
    calls: Option<PathBuf>,
    bin: PathBuf,
}

impl On {
    fn new(host: Where) -> Self {
        Self {
            host,
            ran: Mutex::default(),
            calls: None,
            bin: PathBuf::new(),
        }
    }

    /// The unit runs its script against a `systemctl` that records its arguments in `calls`.
    fn running(mut self, bin: &Path) -> Self {
        std::fs::create_dir_all(bin).unwrap();
        let ctl = bin.join("systemctl");
        let calls = bin.join("calls");
        std::fs::write(
            &ctl,
            format!("#!/bin/sh\necho \"$@\" >> {}\n", calls.display()),
        )
        .unwrap();
        std::fs::set_permissions(&ctl, std::fs::Permissions::from_mode(0o755)).unwrap();
        self.calls = Some(calls);
        self.bin = bin.to_path_buf();
        self
    }

    fn restarts(&self) -> Vec<String> {
        self.calls
            .as_ref()
            .and_then(|c| std::fs::read_to_string(c).ok())
            .map(|t| t.lines().map(str::to_string).collect())
            .unwrap_or_default()
    }
}

impl Installer for On {
    fn start(&self, p: &Place, merge: &str) -> anyhow::Result<Started> {
        install::start_with(p, merge, &self.host, &fake_build, &|argv| {
            self.ran.lock().unwrap().push(argv.to_vec());
            let Some(_) = &self.calls else { return Ok(()) };
            // The unit's own work: its script, run as the unit runs it.
            let script = argv.last().unwrap();
            let mut c = Command::new("/bin/bash");
            c.arg(script)
                .env_clear()
                .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()));
            let st = theseus_kernel::children::spawn(
                theseus_kernel::children::Kind::Owned,
                || c.spawn(),
                |c| Some(c.id()),
            )
            .and_then(|mut child| child.wait())?;
            anyhow::ensure!(st.success(), "the unit's script failed");
            Ok(())
        })
    }
}

/// A directory the loop cannot write, holding the service's program: a stand-in for the
/// administrator's. None as the superuser, who can write anywhere.
fn root_owned(under: &Path) -> Option<(PathBuf, PathBuf)> {
    if unsafe { libc::geteuid() } == 0 {
        return None;
    }
    let dir = under.join("admin/b6ffa7de");
    std::fs::create_dir_all(&dir).unwrap();
    let program = dir.join("theseusd");
    std::fs::write(&program, "the build before").unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    Some((dir, program))
}

/// Put the permissions back, so the tempdir can be removed.
fn reopen(dir: &Path) {
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn seams_with(r: &Rig, installer: std::sync::Arc<dyn Installer>) -> Seams {
    Seams {
        installer,
        ..r.seams()
    }
}

/// A seeded branch, joined by a pass that installs through `installer`; the merge.
async fn join_with(r: &Rig, installer: std::sync::Arc<dyn Installer>) -> String {
    r.seed("trim-the-sails", PROVED, SONNET);
    let seams = seams_with(r, installer);
    let pass = r.core.self_join_pass_yes(&seams).await.unwrap();
    assert_eq!(pass.joined.len(), 1, "{pass:?}");
    r.upstream_main()
}

fn short(c: &str) -> &str {
    &c[..c.len().min(12)]
}

// ---- the layout ------------------------------------------------------------------------------

/// The unit's `ExecStart`, as `systemctl show` prints it, names the program a restart runs.
#[test]
fn the_units_program_is_read_from_what_systemctl_shows() {
    let shown = "{ path=/usr/local/lib/theseus/b6ffa7de/theseusd ; argv[]=/usr/local/lib/theseus/\
                 b6ffa7de/theseusd --config /etc/theseus/theseus.toml ; ignore_errors=no ; \
                 start_time=[n/a] }\n";
    assert_eq!(
        layout::program_of(shown),
        Some(PathBuf::from("/usr/local/lib/theseus/b6ffa7de/theseusd"))
    );
    for none in ["", "[not set]", "{ path=relative/theseusd ; argv[]= }"] {
        assert_eq!(layout::program_of(none), None, "{none:?}");
    }
}

/// A directory the loop can write is the user's own layout; one it cannot is root-owned; the
/// unit's program decides where there is one, and the daemon's own where there is not; with
/// neither, the loop does not guess.
#[test]
fn the_layout_is_read_from_the_service_and_the_permissions() {
    let t = tempfile::tempdir().unwrap();
    let mine = t.path().join("bin");
    std::fs::create_dir_all(&mine).unwrap();
    let user = Where {
        unit: Some(mine.join("theseusd")),
        ..Where::default()
    };
    assert_eq!(layout::read(&user), Layout::User { dir: mine.clone() });
    // The unit says where a restart goes, whatever the daemon is running from.
    let both = Where {
        unit: Some(mine.join("theseusd")),
        running: Some(PathBuf::from("/usr/local/lib/theseus/b6ffa7de/theseusd")),
    };
    assert_eq!(layout::read(&both), Layout::User { dir: mine.clone() });
    let only_running = Where {
        running: Some(mine.join("theseusd")),
        ..Where::default()
    };
    assert_eq!(layout::read(&only_running), Layout::User { dir: mine });
    let neither = layout::read(&Where::default());
    assert!(matches!(neither, Layout::Unknown { .. }), "{neither:?}");
    if let Some((dir, program)) = root_owned(t.path()) {
        let rooted = Where {
            unit: Some(program.clone()),
            ..Where::default()
        };
        assert_eq!(layout::read(&rooted), Layout::Rooted { program });
        reopen(&dir);
    }
}

/// The one step: a program in a directory named for a commit gets a directory for the new one
/// and a unit pointed at it; any other is overwritten where it is. Both name the built programs.
#[test]
fn the_step_installs_the_build_where_the_service_runs() {
    let merge = "0123456789abcdef0123456789abcdef01234567";
    let target = Path::new("/w/targets/install-0123456789ab");
    let repo = Path::new("/w/theseus");
    let per_commit = layout::step(
        Path::new("/usr/local/lib/theseus/b6ffa7de/theseusd"),
        target,
        repo,
        merge,
    );
    assert!(
        per_commit.starts_with("sudo install -d -m 0755 /usr/local/lib/theseus/01234567 && "),
        "{per_commit}"
    );
    for want in [
        "/w/targets/install-0123456789ab/release/{theseusd,theseus,theseus-tui,theseus-index}",
        "PATH=/usr/local/lib/theseus/01234567:$PATH /w/theseus/scripts/user-service.sh install --yes",
        "/w/theseus/scripts/user-service.sh restart",
    ] {
        assert!(per_commit.contains(want), "{want}: {per_commit}");
    }
    let in_place = layout::step(Path::new("/opt/theseus/theseusd"), target, repo, merge);
    assert!(
        in_place.starts_with("sudo install -m 0755 /w/targets/")
            && in_place.contains(" /opt/theseus/ && "),
        "{in_place}"
    );
    assert!(!in_place.contains("install -d"), "{in_place}");
}

// ---- a root-owned layout -----------------------------------------------------------------------

/// KEY: the first self join ever to reach its install copied the build into the user's own
/// directory and restarted a service that runs a root-owned build elsewhere, so the daemon came
/// back on the old build and nothing said so. Now: the build is made and kept, no unit runs
/// (nothing is copied, nothing restarted), and the owner is told once, in a note that names the
/// commit, says it is not installed, and quotes the one step.
#[tokio::test]
async fn a_root_owned_layout_installs_nothing_restarts_nothing_and_tells_once() {
    let t = tempfile::tempdir().unwrap();
    let Some((dir, program)) = root_owned(t.path()) else {
        return;
    };
    let r = rig_with(SelfMode::Act, |c| {
        c.self_improve.install = crate::config::self_improve::SelfInstall::Auto
    });
    let on = std::sync::Arc::new(On::new(Where {
        unit: Some(program.clone()),
        ..Where::default()
    }));
    let merge = join_with(&r, on.clone()).await;
    // Nothing restarted, nothing copied, no script, no marker.
    assert!(on.ran.lock().unwrap().is_empty(), "a unit was started");
    let p = &r.f.place;
    assert!(!p
        .root()
        .join(format!("install-{}.sh", short(&merge)))
        .exists());
    assert!(!verify::marker_of(p).exists(), "a marker with no restart");
    assert_eq!(
        std::fs::read_to_string(&program).unwrap(),
        "the build before"
    );
    // The build waits, for the owner's step.
    let target = install::target_of(p, &merge);
    assert!(target.join("release/theseusd").exists());
    // One row, with the step; no `self.installed`, ever.
    let rows = r.rows("self.install.not_installed");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["merge"], merge.as_str());
    let step = rows[0]["step"].as_str().unwrap().to_string();
    assert!(step.starts_with("sudo install -d -m 0755 "), "{step}");
    assert!(
        step.contains(&format!("{}/release/", target.display())),
        "{step}"
    );
    assert!(
        rows[0]["why"]
            .as_str()
            .unwrap()
            .contains(&program.display().to_string()),
        "{rows:?}"
    );
    assert!(r.rows("self.installed").is_empty());
    // One note to the owner, an action, saying what is true.
    let posts = r.posts();
    assert_eq!(posts.len(), 1, "{posts:?}");
    let note = &posts[0];
    assert!(crate::owner_note::is_headed(note), "{note}");
    for want in [
        "✋ Action",
        "joined main as",
        short(&merge),
        "is not installed",
        "copied nothing and restarted nothing",
        &format!("`{step}`"),
        &format!("theseus self revert {merge}"),
    ] {
        assert!(note.contains(want), "{want}: {note}");
    }
    assert!(
        !note.contains("installed and running"),
        "the notice of an install that happened: {note}"
    );
    // Another pass, and the same merge asked about again, say nothing more.
    let seams = seams_with(&r, on.clone());
    r.core.self_join_pass_yes(&seams).await.unwrap();
    let n = install::NotInstalled {
        why: "x".into(),
        target,
        step,
    };
    let again = r.core.self_install_not_installed("b", "i", &merge, &n);
    assert!(!again.unwrap(), "told twice");
    assert_eq!(r.posts().len(), 1);
    assert_eq!(r.rows("self.install.not_installed").len(), 1);
    reopen(&dir);
}

/// A layout the loop cannot read is not guessed at: nothing is copied or restarted either, and
/// the owner is told where the build waits.
#[tokio::test]
async fn a_layout_it_cannot_read_installs_nothing_and_says_so() {
    let r = rig_with(SelfMode::Act, |c| {
        c.self_improve.install = crate::config::self_improve::SelfInstall::Auto
    });
    let on = std::sync::Arc::new(On::new(Where::default()));
    let merge = join_with(&r, on.clone()).await;
    assert!(on.ran.lock().unwrap().is_empty());
    let rows = r.rows("self.install.not_installed");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert!(
        rows[0]["why"].as_str().unwrap().contains("could not tell"),
        "{rows:?}"
    );
    assert!(r.rows("self.installed").is_empty());
    assert!(r.posts()[0].contains(short(&merge)));
}

// ---- a user layout -------------------------------------------------------------------------------

/// KEY: where the service runs the user's own copy, the install copies there (and nowhere else),
/// restarts, leaves the marker; the new daemon, running the merge, writes `self.installed` and
/// `self.install.verified` and says "installed and running", once.
#[tokio::test]
async fn a_user_layout_installs_restarts_and_verifies() {
    let t = tempfile::tempdir().unwrap();
    let mine = t.path().join("mine");
    let other = t.path().join("elsewhere");
    for d in [&mine, &other] {
        std::fs::create_dir_all(d).unwrap();
        for b in install::BINARIES {
            std::fs::write(d.join(b), "old").unwrap();
        }
    }
    let r = rig_with(SelfMode::Act, |c| {
        c.self_improve.install = crate::config::self_improve::SelfInstall::Auto
    });
    let on = std::sync::Arc::new(
        On::new(Where {
            unit: Some(mine.join("theseusd")),
            ..Where::default()
        })
        .running(&t.path().join("ctl")),
    );
    let merge = join_with(&r, on.clone()).await;
    // Installed where the service runs, and only there; restarted once.
    for b in install::BINARIES {
        assert_eq!(
            std::fs::read_to_string(mine.join(b)).unwrap(),
            format!("fake {b}")
        );
        assert_eq!(std::fs::read_to_string(other.join(b)).unwrap(), "old");
    }
    assert_eq!(on.ran.lock().unwrap().len(), 1);
    assert_eq!(
        on.restarts(),
        [format!("--user restart {}", install::SERVICE)]
    );
    assert!(r.rows("self.install.not_installed").is_empty());
    assert!(
        r.posts().is_empty(),
        "nothing is told before the restart is checked"
    );
    // The unit left its marker, naming the merge.
    assert_eq!(
        std::fs::read_to_string(verify::marker_of(&r.f.place)).unwrap(),
        format!("{merge}\n")
    );
    // The new daemon, running the merge: verified, told once.
    assert_eq!(
        r.core.self_installed_running(Some(&merge)).unwrap(),
        Some(merge.clone())
    );
    assert_eq!(r.rows("self.installed").len(), 1);
    let v = r.rows("self.install.verified");
    assert_eq!(v.len(), 1, "{v:?}");
    assert_eq!(v[0]["merge"], merge.as_str());
    assert!(r.rows("self.install.mismatch").is_empty());
    assert!(
        !verify::marker_of(&r.f.place).exists(),
        "the marker is spent"
    );
    let posts = r.posts();
    assert_eq!(posts.len(), 1, "{posts:?}");
    assert!(posts[0].contains("installed and running"), "{}", posts[0]);
    assert_eq!(r.core.self_installed_running(Some(&merge)).unwrap(), None);
    assert_eq!(r.posts().len(), 1);
    let j = r.core.self_last_join().unwrap();
    assert_eq!(j.install, Install::Running);
    assert_eq!(
        j.line,
        format!("joined at {}; installed and running", short(&merge))
    );
    assert_eq!(r.core.self_joined_not_running(), None);
}

/// KEY: a restart that comes back on another commit is a mismatch: the owner is told once that the
/// change is joined and is not what runs, and nothing says it is installed.
#[tokio::test]
async fn a_restart_that_comes_back_on_another_commit_is_a_mismatch_and_is_told() {
    let t = tempfile::tempdir().unwrap();
    let mine = t.path().join("mine");
    std::fs::create_dir_all(&mine).unwrap();
    let r = rig_with(SelfMode::Act, |c| {
        c.self_improve.install = crate::config::self_improve::SelfInstall::Auto
    });
    let on = std::sync::Arc::new(
        On::new(Where {
            unit: Some(mine.join("theseusd")),
            ..Where::default()
        })
        .running(&t.path().join("ctl")),
    );
    let merge = join_with(&r, on.clone()).await;
    let before = git(&r.f.upstream, &["rev-parse", &format!("{merge}^1")]);
    assert_ne!(before, merge);
    // The service came back on the build before the join.
    assert_eq!(
        r.core.self_installed_running(Some(&before)).unwrap(),
        Some(merge.clone())
    );
    let rows = r.rows("self.install.mismatch");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["merge"], merge.as_str());
    assert_eq!(rows[0]["running"], before.as_str());
    assert!(r.rows("self.installed").is_empty(), "never installed");
    assert!(r.rows("self.install.verified").is_empty());
    assert!(
        !verify::marker_of(&r.f.place).exists(),
        "the marker is spent"
    );
    let posts = r.posts();
    assert_eq!(posts.len(), 1, "{posts:?}");
    assert!(crate::owner_note::is_headed(&posts[0]), "{}", posts[0]);
    for want in [
        "⚠️ Alert",
        short(&merge),
        short(&before),
        "is joined and is not what runs",
    ] {
        assert!(posts[0].contains(want), "{want}: {}", posts[0]);
    }
    assert!(!posts[0].contains("installed and running"), "{}", posts[0]);
    // Told once, and the daemon running the merge later is not a verified install of this one.
    assert_eq!(r.core.self_installed_running(Some(&before)).unwrap(), None);
    assert_eq!(r.posts().len(), 1);
    let j = r.core.self_last_join().unwrap();
    assert_eq!(j.install, Install::Mismatch);
    assert_eq!(r.core.self_joined_not_running(), Some(merge));
}

/// A build that names no commit cannot be checked, and an install that cannot be checked is not
/// reported as one that happened. No marker, no check: an ordinary start says nothing.
#[tokio::test]
async fn a_build_with_no_commit_is_a_mismatch_and_an_ordinary_start_is_not() {
    let r = rig_with(SelfMode::Act, |c| {
        c.self_improve.install = crate::config::self_improve::SelfInstall::Auto
    });
    let on = std::sync::Arc::new(On::new(Where::default()));
    let merge = join_with(&r, on).await;
    assert_eq!(
        r.core.self_installed_running(Some("0000000")).unwrap(),
        None
    );
    assert_eq!(r.core.self_installed_running(None).unwrap(), None);
    assert!(r.rows("self.install.mismatch").is_empty());
    std::fs::write(verify::marker_of(&r.f.place), format!("{merge}\n")).unwrap();
    assert_eq!(r.core.self_installed_running(None).unwrap(), Some(merge));
    let rows = r.rows("self.install.mismatch");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert!(rows[0]["running"].is_null());
    assert!(r
        .posts()
        .last()
        .unwrap()
        .contains("does not name its commit"));
}

/// A marker is the install's own word and a daemon with the mode off acts on none.
#[tokio::test]
async fn a_marker_is_not_read_with_the_mode_off() {
    let r = rig_with(SelfMode::Off, |_| {});
    std::fs::create_dir_all(r.f.place.root()).unwrap();
    std::fs::write(
        verify::marker_of(&r.f.place),
        "0123456789abcdef0123456789abcdef01234567\n",
    )
    .unwrap();
    assert_eq!(
        r.core.self_installed_running(Some("abcdef0")).unwrap(),
        None
    );
    assert!(r.rows("self.install.mismatch").is_empty());
    assert!(r.posts().is_empty());
}

// ---- the pipeline's words ------------------------------------------------------------------------

fn row(kind: &str, data: serde_json::Value) -> LedgerRow {
    LedgerRow::named(kind, None, None, data)
}

/// The words for the last join, whichever way its install went. A newer join is the one spoken of,
/// and an older join's install rows are not its own.
#[test]
fn the_words_for_the_last_join_say_whether_it_is_what_runs() {
    let (a, b) = ("a".repeat(40), "b".repeat(40));
    assert_eq!(last_join(&[]), None);
    let j = |m: &str| row("self.joined", json!({"merge": m}));
    let pending = last_join(&[j(&a)]).unwrap();
    assert_eq!(pending.install, Install::Unconfirmed);
    assert_eq!(
        pending.line,
        format!("joined at {}; not confirmed running", short(&a))
    );
    let running = last_join(&[j(&a), row("self.installed", json!({"merge": a}))]).unwrap();
    assert_eq!(
        running.line,
        format!("joined at {}; installed and running", short(&a))
    );
    let not = last_join(&[
        j(&a),
        row(
            "self.install.not_installed",
            json!({"merge": a, "why": "the service runs a root-owned build", "step": "sudo install x"}),
        ),
    ])
    .unwrap();
    assert_eq!(not.install, Install::NotInstalled);
    assert_eq!(
        not.line,
        format!(
            "joined at {}; joined but not installed: the service runs a root-owned build. \
             To install it: sudo install x",
            short(&a)
        )
    );
    let mismatch = last_join(&[
        j(&a),
        row("self.install.mismatch", json!({"merge": a, "running": b})),
    ])
    .unwrap();
    assert_eq!(mismatch.install, Install::Mismatch);
    assert!(mismatch.line.contains(short(&b)) && mismatch.line.contains("not what runs"));
    // A later join is spoken of; the earlier one's not-installed is not its.
    let later = last_join(&[
        j(&a),
        row(
            "self.install.not_installed",
            json!({"merge": a, "why": "w", "step": "s"}),
        ),
        j(&b),
    ])
    .unwrap();
    assert_eq!(later.merge, b);
    assert_eq!(later.install, Install::Unconfirmed);
    // Installed by hand afterwards: the owner's step took, and the words follow.
    let hand = last_join(&[
        j(&a),
        row(
            "self.install.not_installed",
            json!({"merge": a, "why": "w", "step": "s"}),
        ),
        row("self.installed", json!({"merge": a})),
    ])
    .unwrap();
    assert_eq!(hand.install, Install::Running);
}

/// The pipeline's own answer and `health` carry the words, from the ledger, on a root-owned
/// machine: joined but not installed, with the step; and the joined-but-not-running commit.
#[tokio::test]
async fn the_pipeline_and_health_say_a_joined_change_is_not_running() {
    let t = tempfile::tempdir().unwrap();
    let Some((dir, program)) = root_owned(t.path()) else {
        return;
    };
    let r = rig_with(SelfMode::Act, |c| {
        c.self_improve.install = crate::config::self_improve::SelfInstall::Auto
    });
    let on = std::sync::Arc::new(On::new(Where {
        unit: Some(program),
        ..Where::default()
    }));
    let merge = join_with(&r, on).await;
    let view = r
        .core
        .self_pipeline(&SelfPipelineParams { week: false })
        .unwrap();
    let said = view.last_join.expect("the pipeline names the last join");
    assert!(
        said.starts_with(&format!(
            "joined at {}; joined but not installed",
            short(&merge)
        )),
        "{said}"
    );
    assert!(said.contains("To install it: sudo install"), "{said}");
    assert_eq!(r.core.health().joined_not_running, Some(merge));
    let log = r
        .core
        .self_log(&theseus_protocol::rsi::SelfLogParams::default())
        .unwrap();
    assert_eq!(log.last_join.as_deref(), Some(said.as_str()));
    assert!(log
        .rows
        .iter()
        .any(|row| row.kind == "self.install.not_installed"));
    reopen(&dir);
}
