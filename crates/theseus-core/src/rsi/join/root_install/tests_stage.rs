//! The staging directory (issue 20, the owner's machine: the four programs of an install that rolled back were still
//! there an hour later). The daemon replaces what is there whole at the next `stage`, and empties it once the helper
//! has copied or refused it.

use crate::rsi::join::tests_publish::PassYes as _;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use super::machine::{stage_into, unstage_from};
use super::tests::{rooted_with, FakeHelper, FakeUnits, Rooted, OLD};
use super::*;
use crate::config::self_improve::SelfInstall;
use crate::rsi::join::install;

/// A build tree's `release/` with the four programs, each saying which build it is.
fn release(dir: &Path, build: &str) -> std::path::PathBuf {
    let out = dir.join("release");
    std::fs::create_dir_all(&out).unwrap();
    for b in install::BINARIES {
        std::fs::write(out.join(b), format!("{build} {b}")).unwrap();
    }
    out
}

fn listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// What an earlier commit left (four programs, and a half-made copy of a crashed stage) never gets in the way of the
/// next: `stage` replaces the directory's content whole, and `unstage` empties it. A directory in it is refused.
#[test]
fn a_leftover_of_an_earlier_commit_is_replaced_whole_and_unstage_empties() {
    let d = tempfile::tempdir().unwrap();
    let stage = d.path().join("stage");
    std::fs::create_dir(&stage).unwrap();
    let first = release(&d.path().join("first"), "first");
    stage_into(&stage, &first).unwrap();
    std::fs::write(stage.join(".theseusd.new"), "half").unwrap();
    // The second build replaces all of it.
    let second = release(&d.path().join("second"), "second");
    stage_into(&stage, &second).unwrap();
    let mut want: Vec<String> = install::BINARIES.iter().map(|b| (*b).to_string()).collect();
    want.sort();
    assert_eq!(listing(&stage), want, "exactly the four, no leftover");
    for b in install::BINARIES {
        assert_eq!(
            std::fs::read_to_string(stage.join(b)).unwrap(),
            format!("second {b}")
        );
        assert_eq!(
            std::fs::metadata(stage.join(b))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
    }
    unstage_from(&stage).unwrap();
    assert!(listing(&stage).is_empty());
    // Emptying an empty directory is fine; a directory inside is refused, not removed.
    unstage_from(&stage).unwrap();
    std::fs::create_dir(stage.join("sub")).unwrap();
    assert!(format!("{:#}", stage_into(&stage, &second).unwrap_err()).contains("is a directory"));
    assert!(unstage_from(&stage).is_err() && stage.join("sub").is_dir());
    // A missing directory says whose step makes it.
    let e = unstage_from(&d.path().join("none")).unwrap_err();
    assert!(format!("{e:#}").contains("administrator"), "{e:#}");
}

fn order(t: &Rooted) -> Vec<String> {
    t.helper
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|c| *c != "presence" && *c != "check_installed")
        .cloned()
        .collect()
}

/// The programs are removed once the helper has copied them, and also when it refuses.
#[tokio::test]
async fn the_staged_programs_are_removed_after_the_helper_runs_or_refuses() {
    let t = rooted_with(
        SelfInstall::Auto,
        None,
        FakeHelper::default(),
        FakeUnits::new(OLD, false),
    );
    t.join().await;
    t.until("the restart", |t| {
        !t.units.restarts.lock().unwrap().is_empty()
    })
    .await;
    assert_eq!(order(&t), ["stage", "install", "unstage"]);

    let helper = FakeHelper {
        refuse: Some("aaaaaaaa already exists and differs".into()),
        ..Default::default()
    };
    let t = rooted_with(SelfInstall::Auto, None, helper, FakeUnits::new(OLD, false));
    t.join().await;
    t.until("the failure", |t| t.steps().contains(&"failed".to_string()))
        .await;
    assert_eq!(
        order(&t),
        ["stage", "install", "unstage"],
        "a refusal empties it too"
    );
}

/// A second install after a first that rolled back: the first went back (the start command is as it was, the marker
/// says RolledBack), and the next change is staged, installed and restarted onto, with nothing of the first in its way.
#[tokio::test]
async fn a_second_install_after_a_first_that_rolled_back_goes_through() {
    let t = rooted_with(
        SelfInstall::Auto,
        None,
        FakeHelper::default(),
        FakeUnits::new(OLD, false),
    );
    let first = t.join().await;
    t.until("the first restart", |t| {
        !t.units.restarts.lock().unwrap().is_empty()
    })
    .await;
    // The new build never confirmed: the old daemon is back, and the install says it went back.
    t.r.core.clone().root_install_after_serving_as(
        Some("0".repeat(40).as_str()),
        std::path::PathBuf::from("/nonexistent.sock"),
    );
    assert_eq!(t.r.core.root_pending().unwrap().stage, Stage::RolledBack);
    // What the script's way back did: the unit as it was.
    let at = t.units.read().unwrap().path;
    t.units.write(&at, &super::tests::unit_text(OLD)).unwrap();
    // The next change joins and installs.
    t.r.seed(
        "second-change",
        &[
            ("fix2.txt", "again\n"),
            ("tests/fix2.sh", "test -f fix2.txt\n"),
        ],
        crate::rsi::join::tests::SONNET,
    );
    t.r.core.self_join_pass_yes(&t.r.seams()).await.unwrap();
    let second = t.r.upstream_main();
    assert_ne!(first, second, "a second join");
    t.until("the second restart", |t| {
        t.units.restarts.lock().unwrap().len() == 2
    })
    .await;
    assert_eq!(*t.helper.installed.lock().unwrap(), [first, second.clone()]);
    assert!(t
        .units
        .text()
        .contains(&program_of(&second).display().to_string()));
    assert_eq!(
        order(&t),
        ["stage", "install", "unstage", "stage", "install", "unstage"]
    );
    assert_eq!(t.r.core.root_pending().unwrap().merge, second);
}
