//! The core the simulator drives: a whole `Core`, built from public parts as
//! the binding's tests build theirs, over an unsynced store in a temp dir, with
//! the sim's model as its provider and the stand-in tools over the built-ins.

use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use theseus_core::context_files::{ContextEntry, ContextFileEntry, ContextReaders};
use theseus_core::{Config, Core};

use super::model::Leaky;
use super::tools::{Fetch, Run};
use super::world::{Shared, OWNER};

/// The core, and the temp dir its store and work tree are in (removed when
/// the rig is dropped).
pub struct Rig {
    pub core: Arc<Core>,
    _dir: tempfile::TempDir,
}

/// The work tree's files and the context files, written where the config
/// says they are.
fn write_tree(work: &Path, shared: &Shared) -> Result<()> {
    for f in shared.files.iter().chain(&shared.context) {
        let path = work.join(&f.path);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, &f.text).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(())
}

/// The config: the owner on Discord, the public tree, both context files, and
/// postures that let every call run (notify), so nothing waits on a question
/// the sim does not mean to ask.
fn config(dir: &Path, work: &Path, shared: &Shared) -> Config {
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(work.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = theseus_core::policy::Posture::Notify;
    cfg.policy.external_text = theseus_core::external::Mode::Notify;
    cfg.kernel.spend_limit_usd = 1_000_000.0;
    // The narrative is the owner's surface (`narrative.watch`), never a place's:
    // nothing it says reaches an audience, and its sentences cost each fact.
    cfg.narrative = false;
    cfg.labels.owner = Some(vec![format!("discord:{OWNER}")]);
    cfg.labels.public_paths = vec![work.join("open").to_string_lossy().into_owned()];
    cfg.context.files = shared
        .context
        .iter()
        .map(|f| {
            ContextEntry::Table(ContextFileEntry {
                path: work.join(&f.path).to_string_lossy().into_owned(),
                readers: if f.public {
                    ContextReaders::Public
                } else {
                    ContextReaders::Owner
                },
            })
        })
        .collect();
    cfg
}

pub fn build(shared: &Arc<Mutex<Shared>>) -> Result<Rig> {
    let dir = tempfile::tempdir()?;
    let work = dir.path().join("work");
    std::fs::create_dir_all(&work)?;
    let work = work.canonicalize()?;
    let cfg = {
        let s = shared.lock().map_err(|_| anyhow::anyhow!("poisoned"))?;
        write_tree(&work, &s)?;
        config(dir.path(), &work, &s)
    };
    let store = theseus_core::store::Store::open_unsynced(&dir.path().join("store"))?;
    let model = Arc::new(Leaky {
        shared: shared.clone(),
        core: std::sync::OnceLock::new(),
    });
    let providers = [(
        cfg.model.provider.clone(),
        model.clone() as Arc<dyn theseus_core::provider::Provider>,
    )]
    .into_iter()
    .collect();
    let core = Core::build(theseus_core::rpc::Parts {
        cfg,
        providers,
        store,
        secrets: theseus_core::secrets::SecretBoard::empty(),
        startup_log: Arc::default(),
        telemetry: Some(theseus_core::telemetry::Telemetry::disabled()),
        scrubber: Arc::new(theseus_core::scrub::Scrubber::default()),
        launcher: Arc::new(theseus_core::toolrun::InlineLauncher),
        config_gate: theseus_core::config_gate::ConfigGate::file("sim"),
        toollets: vec![
            Arc::new(Fetch {
                shared: shared.clone(),
            }),
            Arc::new(Run {
                shared: shared.clone(),
            }),
        ],
        cpu_cores: None,
    })?;
    let _ = model.core.set(Arc::downgrade(&core));
    core.outbox.warm();
    Ok(Rig { core, _dir: dir })
}
