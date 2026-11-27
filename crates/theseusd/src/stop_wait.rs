//! The runtime's end at a stop and at a restart in place (issue 63).

use std::sync::Arc;
use std::time::Duration;

/// A test's plant (a debug build's `THESEUS_TEST_SELF_JOB`): a self build's command, the variable's shell text,
/// started at serving through the gate's own launcher (`Jail::run`, in L1, waited for on the blocking pool), as
/// the self pipeline starts its gate. What it does not hold: the pipeline before the gate (its tree, its lock,
/// may-build). Its result, should one come back to it, is logged and nothing else. A release build has no plant.
pub fn planted_self_job(core: &Arc<theseus_core::Core>, state_dir: &std::path::Path) {
    #[cfg(debug_assertions)]
    if let Ok(body) = std::env::var("THESEUS_TEST_SELF_JOB") {
        use std::ffi::OsString;
        use theseus_core::rsi::jail::{Jail, Job};
        use theseus_core::rsi::tree::{Place, Tree};
        let root = state_dir.join("planted-self-job");
        let jail = Jail::new(core.tools.sandbox.clone());
        tokio::spawn(async move {
            for d in ["repo", "state", "tree", "target"] {
                let _ = std::fs::create_dir_all(root.join(d));
            }
            let tree = Tree {
                slug: "planted".into(),
                branch: "self/planted".into(),
                dir: root.join("tree"),
                target: root.join("target"),
            };
            let place = Place {
                repo: root.join("repo"),
                state: root.join("state"),
                env: Vec::new(),
                jail: None,
                notify: None,
                stop: None,
                owner: None,
            };
            let log = root.join(format!("gate-{}.log", theseus_protocol::now_unix_ms()));
            let job = Job {
                tree: &tree,
                ro: Vec::new(),
                argv: ["sh", "-c", body.as_str()].map(OsString::from).to_vec(),
                env: Vec::new(),
                log: &log,
                limit: Duration::from_secs(3600),
            };
            tracing::info!("planted self job: started");
            let r = jail.run(&place, job).await;
            tracing::info!(result = ?r.map_err(|e| format!("{e:#}")), "planted self job: its result came back");
        });
    }
    #[cfg(not(debug_assertions))]
    let _ = (core, state_dir);
}
