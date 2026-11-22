//! The pipeline's view names the loop's own install while it is under way (issue 41): an install
//! from outside the loop that waits for a quiet loop sees it. Once the install's unit is accepted the
//! state follows the unit (systemd is stood in for here): a unit that ended without its restart
//! clears it with a row, and a start that finds the unit still running shows it again.

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

/// systemd, stood in for: what it says of any unit, and the install units it says still run.
struct Said {
    state: Option<String>,
    running: Vec<String>,
}

impl Units for Said {
    fn state(&self, _unit: &str) -> Option<String> {
        self.state.clone()
    }
    fn running(&self, _root: &std::path::Path) -> Vec<String> {
        self.running.clone()
    }
}

fn said(state: &str, running: &[String]) -> Arc<Said> {
    Arc::new(Said {
        state: Some(state.into()),
        running: running.to_vec(),
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
    r.core.self_join_pass(&seams).await.unwrap();
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
    // The restart: this daemon's memory of the install is gone.
    *r.core.rsi.pipeline.installing.lock().unwrap() = None;
    let unit = format!("theseus-self-install-{}", &merge[..12]);
    r.core
        .set_units(said("active", std::slice::from_ref(&unit)));
    assert!(r.core.recover_installing());
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
