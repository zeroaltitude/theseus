//! Freeze, load and retire (theseus-pw1q.21.4). A flow is frozen at
//! `<state>/self/mint/<name>/<digest>/spec.json`, read-only, beside the
//! extensions' freeze in kind: what runs is that copy, checked against its
//! digest each time it is read, never the file it came from. The loaded set
//! is one META record, `flows`, written with its `flow.loaded` row in one
//! frame (no store format change: a key of its own), read once after the
//! start. Retiring writes the record without it and `flow.retired`; the
//! frozen copy is never deleted.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError, RwLock};

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use theseus_protocol::flow::FlowInfo;
use theseus_store::{kinds, NewRecord};
use theseus_tools::Tool;

use super::spec::{canonical, Spec};
use super::tool::{FlowTool, PREFIX, WIRE_PREFIX};
use crate::fact::flow::{FlowLoaded, FlowRetired};
use crate::rpc::Core;

/// The META key of the loaded flows.
pub const RECORD: &str = "flows";
/// The largest spec a load reads.
const MAX_BYTES: u64 = 64 * 1024;

/// What the record keeps of one loaded flow: the frozen copy is the rest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stored {
    pub name: String,
    /// Hex, without `sha256:`.
    pub digest: String,
    pub by: String,
    pub via: String,
    pub at_ms: u64,
}

/// The META record `flows`: every loaded flow, by name.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LoadedSet {
    pub loaded: BTreeMap<String, Stored>,
}

impl LoadedSet {
    pub fn read(store: &crate::store::Store) -> Result<Self> {
        Ok(store.get_meta::<Self>(RECORD)?.unwrap_or_default())
    }

    pub fn record(&self) -> Result<NewRecord> {
        NewRecord::json(kinds::META, Some(RECORD), self)
    }
}

/// A flow on the board: its spec as the frozen copy read, and its tool.
pub struct Entry {
    pub spec: Arc<Spec>,
    pub digest: String,
    pub frozen: PathBuf,
    pub stored: Stored,
    pub tool: Arc<FlowTool>,
}

impl Entry {
    pub fn info(&self) -> FlowInfo {
        FlowInfo {
            name: self.spec.name.clone(),
            digest: self.digest.clone(),
            frozen: self.frozen.display().to_string(),
            steps: self.spec.steps.len() as u64,
            repos: self.spec.scope.repos.clone(),
            places: self.spec.scope.places.clone(),
            loaded_by: self.stored.by.clone(),
            loaded_at_ms: self.stored.at_ms,
        }
    }
}

/// The loaded flows, in memory: the tool catalog's flow part. Empty by
/// default, and then it adds nothing to any request.
pub struct Board {
    dir: PathBuf,
    loaded: RwLock<BTreeMap<String, Arc<Entry>>>,
    /// Loads and retires one at a time.
    pub(crate) writes: std::sync::Mutex<()>,
}

impl Board {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            loaded: RwLock::default(),
            writes: std::sync::Mutex::new(()),
        }
    }

    /// Whether any flow is loaded: one read lock, so a request without flows pays that.
    pub fn any(&self) -> bool {
        !self
            .loaded
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty()
    }

    pub fn get(&self, name: &str) -> Option<Arc<Entry>> {
        self.loaded
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(name)
            .cloned()
    }

    pub fn all(&self) -> Vec<Arc<Entry>> {
        self.loaded
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .cloned()
            .collect()
    }

    /// A flow's tool by its canonical name (`flow:<name>`).
    pub fn tool(&self, canonical: &str) -> Option<Arc<dyn Tool>> {
        let e = self.get(canonical.strip_prefix(PREFIX)?)?;
        Some(e.tool.clone())
    }

    /// A flow's tool by the name the model called it (`flow__<name>`).
    pub fn tool_by_wire(&self, wire: &str) -> Option<Arc<dyn Tool>> {
        let e = self.get(wire.strip_prefix(WIRE_PREFIX)?)?;
        Some(e.tool.clone())
    }

    fn put(&self, e: Entry) {
        self.loaded
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(e.spec.name.clone(), Arc::new(e));
    }

    fn drop_flow(&self, name: &str) -> Option<Arc<Entry>> {
        self.loaded
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(name)
    }

    /// Where flows are frozen: `<state>/self/mint`.
    pub fn root(&self) -> &Path {
        &self.dir
    }

    /// `<dir>/<name>/<digest>`.
    pub fn frozen_dir(&self, name: &str, digest: &str) -> PathBuf {
        self.dir.join(name).join(digest)
    }

    /// Freeze `spec` read-only at `<dir>/<name>/<digest>/spec.json`; a copy
    /// already there is read back and must equal it. The copy is written
    /// beside its place and renamed, so no reader sees half of one.
    pub fn freeze(&self, spec: &Spec) -> Result<PathBuf> {
        let digest = spec.digest_hex();
        let dir = self.frozen_dir(&spec.name, &digest);
        let bytes = spec.frozen_bytes();
        if dir.is_dir() {
            if fs::read(dir.join("spec.json")).ok().as_deref() == Some(bytes.as_slice()) {
                return Ok(dir);
            }
            bail!(
                "{} is there and holds other bytes: it is left as it is",
                dir.display()
            );
        }
        let parent = dir.parent().ok_or_else(|| anyhow!("no parent"))?;
        fs::create_dir_all(parent)?;
        let tmp = parent.join(format!(".{digest}.{}", std::process::id()));
        let _ = crate::extend::freeze::make_writable(&tmp);
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir(&tmp)?;
        let file = tmp.join("spec.json");
        fs::write(&file, &bytes)?;
        fs::set_permissions(&file, fs::Permissions::from_mode(0o444))?;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o555))?;
        fs::rename(&tmp, &dir)?;
        Ok(dir)
    }

    /// The spec frozen at `<name>/<digest>`, read and checked: it parses, its
    /// own digest is its content's, and that digest is the directory's.
    pub fn read_frozen(&self, name: &str, digest: &str) -> Result<(Spec, PathBuf)> {
        let dir = self.frozen_dir(name, digest);
        let bytes = fs::read(dir.join("spec.json"))
            .map_err(|e| anyhow!("{}: {e}", dir.join("spec.json").display()))?;
        let spec = Spec::parse(&bytes).map_err(|e| anyhow!("{}: {e}", dir.display()))?;
        if spec.digest_hex() != digest || spec.name != name {
            bail!(
                "{} holds a spec that is not {name} {digest}: the frozen copy changed",
                dir.display()
            );
        }
        Ok((spec, dir))
    }
}

impl Core {
    /// Load the flow spec at `path`, by `by`: read, validated, scrubbed (a
    /// spec the scrubber would change is refused), frozen, and recorded with
    /// `flow.loaded` in one frame. A loaded flow of the same name is replaced.
    /// Offered from the next turn. No wire method reaches this yet: a test,
    /// and the owner's CLI when it is added, call it.
    pub fn flow_load(&self, path: &Path, by: &str, via: &str) -> Result<FlowInfo> {
        self.flow_allowed(by)?;
        let md = fs::metadata(path).map_err(|e| anyhow!("{}: {e}", path.display()))?;
        if md.len() > MAX_BYTES {
            bail!(
                "{} is over {} KiB: not a flow spec",
                path.display(),
                MAX_BYTES >> 10
            );
        }
        let spec = Spec::parse(&fs::read(path)?).map_err(|e| anyhow!(e))?;
        let (_, hidden) = self
            .tools
            .scrubber
            .scrub(&canonical(&serde_json::to_value(&spec)?));
        if hidden > 0 {
            bail!("the spec holds a value the scrubber would hide: a flow's constants are never secrets");
        }
        let flows = &self.tools.flows;
        let _writes = flows.writes.lock().unwrap_or_else(PoisonError::into_inner);
        let frozen = flows.freeze(&spec)?;
        let digest = spec.digest_hex();
        let stored = Stored {
            name: spec.name.clone(),
            digest: digest.clone(),
            by: by.into(),
            via: via.into(),
            at_ms: theseus_protocol::now_unix_ms(),
        };
        let mut set = LoadedSet::read(&self.store)?;
        set.loaded.insert(spec.name.clone(), stored.clone());
        let spec = Arc::new(spec);
        let fact = FlowLoaded {
            name: &spec.name,
            digest: &digest,
            steps: spec.steps.len(),
            repos: &spec.scope.repos,
            places: &spec.scope.places,
            by,
            via,
        };
        let rec = self.rec(None);
        self.store.append(&[rec.row(&fact)?, set.record()?])?;
        let entry = Entry {
            tool: Arc::new(FlowTool::new(spec.clone())),
            spec: spec.clone(),
            digest: digest.clone(),
            frozen,
            stored,
        };
        let info = entry.info();
        flows.put(entry);
        rec.announce(&fact);
        Ok(info)
    }

    /// Retire the loaded flow `name`, by `by`: out of the record with
    /// `flow.retired` in one frame, then off the board. The frozen copy stays.
    pub fn flow_retire(&self, name: &str, by: &str, via: &str) -> Result<FlowInfo> {
        self.flow_allowed(by)?;
        let flows = &self.tools.flows;
        let _writes = flows.writes.lock().unwrap_or_else(PoisonError::into_inner);
        let mut set = LoadedSet::read(&self.store)?;
        let Some(stored) = set.loaded.remove(name) else {
            bail!("no flow named {name:?} is loaded (`theseus self revert flow:<name>` takes a loaded one)");
        };
        let fact = FlowRetired {
            name,
            digest: &stored.digest,
            by,
            via,
        };
        let rec = self.rec(None);
        self.store.append(&[rec.row(&fact)?, set.record()?])?;
        let info = flows
            .drop_flow(name)
            .map(|e| e.info())
            .unwrap_or_else(|| FlowInfo {
                name: name.into(),
                digest: stored.digest.clone(),
                ..Default::default()
            });
        rec.announce(&fact);
        Ok(info)
    }

    /// A self-minted load or retire (`by` is the self principal) is a self
    /// step: it asks `rsi::gate` first and reads and writes nothing while the
    /// mode is off or the kill switch holds. The owner's own act is not one.
    fn flow_allowed(&self, by: &str) -> Result<()> {
        if by != crate::rsi::pr::PRINCIPAL {
            return Ok(());
        }
        match crate::rsi::gate(&self.self_ctx()) {
            crate::rsi::Gate::Allowed => Ok(()),
            g => bail!(
                "self-improvement is {}: a self-minted flow neither loads nor retires",
                g.name()
            ),
        }
    }

    /// The loaded flows.
    pub fn flow_list(&self) -> Vec<FlowInfo> {
        self.tools.flows.all().iter().map(|e| e.info()).collect()
    }

    /// At build: each stored flow read back from its frozen copy and put on
    /// the board. A copy that no longer reads or no longer digests as it did
    /// is not loaded, and says so; the record keeps it for the owner to revert.
    pub(crate) fn seed_flows(&self) -> Result<()> {
        let flows = &self.tools.flows;
        for s in LoadedSet::read(&self.store)?.loaded.values() {
            match flows.read_frozen(&s.name, &s.digest) {
                Ok((spec, frozen)) => {
                    let spec = Arc::new(spec);
                    flows.put(Entry {
                        tool: Arc::new(FlowTool::new(spec.clone())),
                        spec,
                        digest: s.digest.clone(),
                        frozen,
                        stored: s.clone(),
                    });
                }
                Err(e) => {
                    tracing::warn!(flow = %s.name, error = %e, "a loaded flow does not read: not offered")
                }
            }
        }
        Ok(())
    }
}
