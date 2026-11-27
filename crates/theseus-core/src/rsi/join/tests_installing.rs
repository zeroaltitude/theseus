//! The pipeline's view names the loop's own install while it is under way (issue 41): an install
//! from outside the loop that waits for a quiet loop sees it. Once the install's unit is accepted the
//! state follows the unit (systemd is stood in for here): a unit that ended without its restart
//! clears it with a row, and a start that finds the unit still running shows it again.

use crate::rsi::join::tests_publish::PassYes as _;
use std::sync::{Arc, Mutex, OnceLock};

use theseus_protocol::rsi::{SelfMode, SelfPipelineItem, SelfPipelineParams};

use super::install::{self, Installer, NotInstalled, Started};
use super::tests::{rig_with, Rig, PROVED, SONNET};
use super::Seams;
use crate::rpc::Core;
use crate::rsi::pipeline::installing::Units;
use crate::rsi::tree::Place;

/// An installer that reads the pipeline's view while its install runs, then ends as `unit` says.
struct Peek {
    core: OnceLock<Arc<Core>>,
    during: Mutex<Vec<SelfPipelineItem>>,
    unit: bool,
}

impl Installer for Peek {
    fn start(&self, p: &Place, merge: &str) -> anyhow::Result<Started> {
        let view = self
            .core
            .get()
            .unwrap()
            .self_pipeline(&SelfPipelineParams::default())?;
        *self.during.lock().unwrap() = view.installing;
        Ok(match self.unit {
            true => Started::Unit("theseus-self-install-stand-in".into()),
            false => Started::NotInstalled(NotInstalled {
                why: "a stand-in for a build the loop cannot replace".into(),
                target: install::target_of(p, merge),
                step: "the owner's step".into(),
            }),
        })
    }
}

/// systemd, stood in for: what it says of any unit, and the install units it says still run (or that it cannot be
/// asked).
struct Said {
    state: Option<String>,
    running: Result<Vec<String>, String>,
}

impl Units for Said {
    fn state(&self, _unit: &str) -> Option<String> {
        self.state.clone()
    }
    fn running(&self, _root: &std::path::Path) -> Result<Vec<String>, String> {
        self.running.clone()
    }
}

fn said(state: &str, running: &[String]) -> Arc<Said> {
    Arc::new(Said {
        state: Some(state.into()),
        running: Ok(running.to_vec()),
    })
}

async fn joined_with(unit: bool) -> (Rig, Arc<Peek>, String) {
    let r = rig_with(SelfMode::Act, |c| {
        c.self_improve.install = crate::config::self_improve::SelfInstall::Auto
    });
    r.seed("trim-the-sails", PROVED, SONNET);
    let peek = Arc::new(Peek {
        core: OnceLock::new(),
        during: Mutex::new(Vec::new()),
        unit,
    });
    let _ = peek.core.set(r.core.clone());
    let seams = Seams {
        installer: peek.clone(),
        ..r.seams()
    };
    r.core.self_join_pass_yes(&seams).await.unwrap();
    let merge = r.upstream_main();
    (r, peek, merge)
}

/// KEY: while the install runs, the view has it in `installing`, with its branch and merge; once its
/// unit is started it stays there (the unit restarts the daemon), and nothing else names it.
#[tokio::test]
async fn an_install_under_way_is_a_state_of_its_own_in_the_view() {
    let (r, peek, merge) = joined_with(true).await;
    let core = &r.core;
    core.set_units(said("active", &[]));
    let during = peek.during.lock().unwrap().clone();
    assert_eq!(during.len(), 1, "{during:?}");
    assert_eq!(during[0].queue, "installing");
    assert_eq!(
        during[0].branch.as_deref(),
        Some("self/20261014-trim-the-sails")
    );
    assert!(
        during[0].why.as_deref().unwrap().contains(&merge[..12]),
        "{during:?}"
    );
    let after = core.self_pipeline(&SelfPipelineParams::default()).unwrap();
    assert_eq!(
        after.installing.len(),
        1,
        "until the unit restarts the daemon"
    );
    assert!(after.joining.iter().all(|i| i.queue != "installing"));
}

/// An install that ends with no restart to come (the build is kept for the owner's step) is under
/// way while it runs, and not after.
#[tokio::test]
async fn an_install_that_ends_without_a_restart_leaves_the_view() {
    let (r, peek, _) = joined_with(false).await;
    let core = &r.core;
    assert_eq!(peek.during.lock().unwrap().len(), 1);
    let after = core.self_pipeline(&SelfPipelineParams::default()).unwrap();
    assert_eq!(after.installing, Vec::<SelfPipelineItem>::new());
}

/// A unit accepted, then failed (its copy or its restart under `set -e`): the next read finds it ended, the state
/// is cleared, and a row says which unit, what systemd said, and that the install did not finish.
#[tokio::test]
async fn a_unit_accepted_then_failed_clears_installing_with_its_row() {
    let (r, _, merge) = joined_with(true).await;
    r.core.set_units(said("failed", &[]));
    let after = r
        .core
        .self_pipeline(&SelfPipelineParams::default())
        .unwrap();
    assert_eq!(after.installing, Vec::<SelfPipelineItem>::new());
    let rows: Vec<_> = r
        .core
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(10_000)
        .unwrap()
        .into_iter()
        .filter(|(_, row)| row.kind == "self.pipeline.ended")
        .map(|(_, row)| row.data)
        .collect();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["unit"], "theseus-self-install-stand-in");
    assert_eq!(rows[0]["state"], "failed");
    assert_eq!(rows[0]["merge"], merge.as_str());
    assert!(
        rows[0]["why"].as_str().unwrap().contains("did not finish"),
        "{rows:?}"
    );
}

/// A daemon restarted while the install's unit still copies: its memory is empty, and its start finds the unit of
/// its state directory running, so the view shows the install again, named for the join it installs.
#[tokio::test]
async fn a_restart_while_the_unit_runs_shows_the_install_again() {
    let (r, _, merge) = joined_with(true).await;
    // The restart: this daemon's memory of the install is gone, and its start has not looked yet.
    *r.core.rsi.pipeline.installing.lock().unwrap() = None;
    *r.core.rsi.pipeline.discovered.0.lock().unwrap() = None;
    let unit = format!("theseus-self-install-{}", &merge[..12]);
    r.core
        .set_units(said("active", std::slice::from_ref(&unit)));
    let before = r
        .core
        .self_pipeline(&SelfPipelineParams::default())
        .unwrap();
    assert_eq!(
        before.installing.len(),
        1,
        "not known is not quiet: {before:?}"
    );
    assert_eq!(before.installing[0].title, "not known yet");
    assert!(r.core.discover_installing());
    let after = r
        .core
        .self_pipeline(&SelfPipelineParams::default())
        .unwrap();
    assert_eq!(after.installing.len(), 1, "{after:?}");
    let why = after.installing[0].why.clone().unwrap();
    assert!(why.contains(&unit) && why.contains(&merge[..12]), "{why}");
    assert_eq!(
        after.installing[0].branch.as_deref(),
        Some("self/20261014-trim-the-sails")
    );
}

/// The start's discovery fails (systemd cannot be asked): the loop is not known, never quiet, and the loop's own
/// next install does not start; once a try succeeds and finds nothing, it is quiet.
#[tokio::test]
async fn a_failed_discovery_is_not_known_and_never_quiet() {
    let (r, _, _) = joined_with(false).await;
    *r.core.rsi.pipeline.discovered.0.lock().unwrap() = None;
    r.core.set_units(Arc::new(Said {
        state: None,
        running: Err("Failed to connect to bus".into()),
    }));
    assert!(!r.core.discover_installing());
    let v = r
        .core
        .self_pipeline(&SelfPipelineParams::default())
        .unwrap();
    assert_eq!(v.installing.len(), 1, "{v:?}");
    let why = v.installing[0].why.clone().unwrap();
    assert!(
        why.contains("not known yet") && why.contains("Failed to connect to bus"),
        "{why}"
    );
    assert!(
        r.core.install_blocked().is_some(),
        "no install starts while it is not known"
    );
    r.core.set_units(said("inactive", &[]));
    assert!(r.core.discover_installing());
    let v = r
        .core
        .self_pipeline(&SelfPipelineParams::default())
        .unwrap();
    assert_eq!(v.installing, Vec::<SelfPipelineItem>::new());
    assert_eq!(r.core.install_blocked(), None);
}

/// A recovered unit that ends while this daemon runs the install's commit: the install worked (this daemon is its
/// restart), so the state clears with no failure row. `deactivating` is not ended.
#[tokio::test]
async fn a_recovered_unit_that_ends_on_its_own_build_is_a_success() {
    let (r, _, merge) = joined_with(true).await;
    *r.core.rsi.pipeline.installing.lock().unwrap() = None;
    let unit = format!("theseus-self-install-{}", &merge[..12]);
    r.core
        .set_units(said("active", std::slice::from_ref(&unit)));
    assert!(r.core.discover_installing());
    *r.core.rsi.pipeline.running.lock().unwrap() = Some(merge);
    r.core.set_units(said("deactivating", &[]));
    *r.core.rsi.pipeline.unit_seen.lock().unwrap() = None;
    let v = r
        .core
        .self_pipeline(&SelfPipelineParams::default())
        .unwrap();
    assert_eq!(v.installing.len(), 1, "deactivating still runs: {v:?}");
    r.core.set_units(said("inactive", &[]));
    *r.core.rsi.pipeline.unit_seen.lock().unwrap() = None;
    let v = r
        .core
        .self_pipeline(&SelfPipelineParams::default())
        .unwrap();
    assert_eq!(v.installing, Vec::<SelfPipelineItem>::new());
    let ended = r
        .core
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(10_000)
        .unwrap()
        .into_iter()
        .filter(|(_, row)| row.kind == "self.pipeline.ended")
        .count();
    assert_eq!(ended, 0, "a success writes no failure row");
}

/// A running install unit of this state directory that no join of this loop names is reported, not adopted.
#[tokio::test]
async fn a_unit_no_join_names_is_not_adopted() {
    let (r, _, _) = joined_with(false).await;
    r.core.set_units(said(
        "active",
        &["theseus-self-install-0000feedface".into()],
    ));
    assert!(r.core.discover_installing());
    let v = r
        .core
        .self_pipeline(&SelfPipelineParams::default())
        .unwrap();
    assert_eq!(v.installing, Vec::<SelfPipelineItem>::new());
}

/// A stand-in `systemctl` that answers `list-units` with `list` (a shell snippet) and every `show` with this state
/// directory's exact script for the unit asked about: the real adapter (`installing::Systemd`) runs on it.
fn fake_systemctl(r: &Rig, list: &str) -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let root = r.core.cfg.state_dir().join("self");
    let script = format!(
        "#!/bin/sh\ncase \"$2\" in\n  list-units) {list} ;;\n  show) u=\"$6\"; s=${{u#theseus-self-install-}}; \
         echo \"{{ path=/bin/bash ; argv[]=/bin/bash {root}/install-$s.sh ; ignore_errors=no }}\" ;;\n  \
         is-active) echo active ;;\nesac\n",
        root = root.display()
    );
    let path = d.path().join("systemctl");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    *crate::rsi::pipeline::installing::SYSTEMCTL.lock().unwrap() = Some(path);
    r.core
        .set_units(Arc::new(crate::rsi::pipeline::installing::Systemd));
    d
}

/// Not known, and the loop starts no install: what every false-success path below must come to.
fn not_known(r: &Rig) {
    let v = r
        .core
        .self_pipeline(&SelfPipelineParams::default())
        .unwrap();
    assert_eq!(v.installing.len(), 1, "{v:?}");
    assert_eq!(v.installing[0].title, "not known yet", "{v:?}");
    assert!(
        r.core.install_blocked().is_some(),
        "no install starts while it is not known"
    );
}

/// A `systemctl` that fails (no bus: nothing on stdout, the error on stderr, exit 1) is no answer, not an empty
/// list: discovery does not succeed, and the loop is not known.
#[tokio::test]
async fn a_failed_systemctl_is_no_answer_not_an_empty_list() {
    let (r, _, _) = joined_with(false).await;
    *r.core.rsi.pipeline.discovered.0.lock().unwrap() = None;
    let _fake = fake_systemctl(&r, "echo 'Failed to connect to bus' >&2; exit 1");
    assert!(
        !r.core.discover_installing(),
        "a failed list-units is no answer"
    );
    not_known(&r);
}

/// While the ledger's index is still being built (a real start's condition) the joins cannot be read: a running
/// install unit cannot be told from a stranger's, so discovery does not succeed, and the loop is not known.
#[tokio::test]
async fn an_unread_ledger_leaves_discovery_not_known() {
    let (r, _, merge) = joined_with(false).await;
    *r.core.rsi.pipeline.discovered.0.lock().unwrap() = None;
    let unit = format!("theseus-self-install-{}", &merge[..12]);
    r.core
        .set_units(said("active", std::slice::from_ref(&unit)));
    r.core.store.shape_window(true);
    assert!(!r.core.discover_installing(), "the joins could not be read");
    not_known(&r);
    r.core.store.shape_window(false);
    assert!(r.core.discover_installing());
    let v = r
        .core
        .self_pipeline(&SelfPipelineParams::default())
        .unwrap();
    assert!(
        v.installing[0].why.as_deref().unwrap().contains(&unit),
        "{v:?}"
    );
}

/// More install units than discovery looks at: the one it did not look at could be this loop's, so discovery is
/// incomplete, and the loop is not known.
#[tokio::test]
async fn an_overflowing_unit_list_leaves_discovery_not_known() {
    let (r, _, _) = joined_with(false).await;
    *r.core.rsi.pipeline.discovered.0.lock().unwrap() = None;
    let _fake = fake_systemctl(
        &r,
        "for i in $(seq 10 26); do echo \"theseus-self-install-0000000000$i.service loaded active running x\"; done",
    );
    assert!(
        !r.core.discover_installing(),
        "17 units, 16 looked at: incomplete"
    );
    not_known(&r);
}
