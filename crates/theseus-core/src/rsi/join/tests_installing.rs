//! The pipeline's view names the loop's own install while it is under way (issue 41): an install
//! from outside the loop that waits for a quiet loop sees it.

use std::sync::{Arc, Mutex, OnceLock};

use theseus_protocol::rsi::{SelfMode, SelfPipelineItem, SelfPipelineParams};

use super::install::{self, Installer, NotInstalled, Started};
use super::tests::{rig_with, PROVED, SONNET};
use super::Seams;
use crate::rpc::Core;
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

async fn joined_with(unit: bool) -> (Arc<Core>, Arc<Peek>, String) {
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
    (r.core.clone(), peek, r.upstream_main())
}

/// KEY: while the install runs, the view has it in `installing`, with its branch and merge; once its
/// unit is started it stays there (the unit restarts the daemon), and nothing else names it.
#[tokio::test]
async fn an_install_under_way_is_a_state_of_its_own_in_the_view() {
    let (core, peek, merge) = joined_with(true).await;
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
    let (core, peek, _) = joined_with(false).await;
    assert_eq!(peek.during.lock().unwrap().len(), 1);
    let after = core.self_pipeline(&SelfPipelineParams::default()).unwrap();
    assert_eq!(after.installing, Vec::<SelfPipelineItem>::new());
}
