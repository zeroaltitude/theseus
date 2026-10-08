//! Learned pack versions (M5 25f; design §2.17): a version the learning
//! loop wrote at run time lives in the store, not the binary.
//!
//! - **A row each.** A `pack.version` row (keyed `pv_<name>`, scoped
//!   `judge.learn:<id>` beside the lineage's `judge.proposal` rows) holds the
//!   version's whole TOML, its sha256, its parent, its root (the compiled-in
//!   version at the head of its lineage), and the proposal that made it.
//!   Every version, compiled or learned, loads through `Pack::parse`.
//! - **A file each, derived.** `<state dir>/packs/<name>.toml` is written
//!   from the row, after serving and as each row lands, as 25c's
//!   `learning/<date>.json` is: the rows rebuild it, and nothing reads it.
//! - **One version per role.** Each point names its root as a constant;
//!   [`JudgeService::placed`] gives the version of its lineage that stands
//!   in its place: the newest learned one the ladder placed (a `pack.mode`
//!   row of `shadow`, `canary`, or `live`; a canary's control arm skips it),
//!   else the root. A learned version with no row, or one rolled back or
//!   turned `off`, stands nowhere, so a rollback gives the place back.
//! - **Read after serving** (`Core::warm_ladder`), or by an RPC or the
//!   learning loop, then kept, as the ladder is. A point never reads it
//!   (theseus-289c): until the warm read, [`JudgeService::placed`] answers
//!   the root.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::Result;
use serde_json::{json, Value};
use theseus_judge::Pack;
use theseus_protocol::LedgerKind;

use super::ladder::{self, Rung};
use crate::config::PackMode;
use crate::ledger::LedgerRow;
use crate::store::Store;

/// A lineage's scope: its learned versions and its proposals.
pub fn scope(pack: &str) -> String {
    format!("judge.learn:{}", ladder::id_of(pack))
}

/// A version's row key.
pub fn key(name: &str) -> String {
    format!("pv_{name}")
}

/// The roots the learning loop rewrites: every wired version but those the
/// step leaves alone (the route pack, every version, and rerank.v1, live
/// with their own rollback rules; the memory pass's two).
pub const LEFT_ALONE: [&str; 6] = [
    "route.v1",
    "route.v2",
    "route.v3",
    "rerank.v1",
    "memory.v1",
    "attribution.v1",
];

/// One learned version, read back.
#[derive(Debug, Clone)]
pub struct Learned {
    pub pack: Arc<Pack>,
    pub text: String,
    pub parent: String,
    pub root: String,
    pub proposal: String,
    pub at_ms: u64,
}

impl Learned {
    pub fn name(&self) -> String {
        self.pack.name()
    }

    /// Its row's data.
    pub fn data(&self) -> Value {
        json!({
            "name": self.name(), "id": self.pack.id, "version": self.pack.version,
            "sha256": self.pack.sha256, "parent": self.parent, "root": self.root,
            "proposal": self.proposal, "text": self.text,
        })
    }
}

#[derive(Default)]
pub struct Lineage {
    loaded: Mutex<Option<BTreeMap<String, Learned>>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Every learned version in the store, by name: each lineage's
/// `pack.version` rows, each text through `Pack::parse`. A row whose text
/// does not load (a later build's rules) is skipped, and said.
pub fn read(store: &Store) -> Result<BTreeMap<String, Learned>> {
    let mut out = BTreeMap::new();
    for id in crate::learning::pack_ids() {
        for r in store.scope_after(&scope(&id), 0)? {
            let Ok(row) = r.decode::<LedgerRow>() else {
                continue;
            };
            if row.kind != LedgerKind::PackVersion.as_str() {
                continue;
            }
            let d = &row.data;
            let s = |k: &str| d[k].as_str().unwrap_or_default().to_string();
            match Pack::parse(&s("text")) {
                Ok(p) => {
                    let l = Learned {
                        pack: Arc::new(p),
                        text: s("text"),
                        parent: s("parent"),
                        root: s("root"),
                        proposal: s("proposal"),
                        at_ms: row.at_unix_ms,
                    };
                    out.insert(l.name(), l);
                }
                Err(e) => {
                    tracing::warn!(name = %s("name"), error = %e, "judge: a learned version does not load; it stands nowhere")
                }
            }
        }
    }
    Ok(out)
}

impl Lineage {
    /// Run `f` on what is loaded, loading it first. A store that cannot be
    /// read leaves it unloaded, and `f` sees no learned version.
    fn with<T>(&self, store: &Store, f: impl FnOnce(&BTreeMap<String, Learned>) -> T) -> T {
        let mut g = lock(&self.loaded);
        if g.is_none() {
            match read(store) {
                Ok(m) => *g = Some(m),
                Err(e) => {
                    tracing::warn!(error = %format!("{e:#}"), "judge: learned versions cannot be read; every root stands");
                    return f(&BTreeMap::new());
                }
            }
        }
        f(g.as_ref().expect("loaded"))
    }

    /// Whether the learned versions have been read.
    pub fn is_loaded(&self) -> bool {
        lock(&self.loaded).is_some()
    }

    /// Read them now, unless they are (the warm read's).
    pub fn read(&self, store: &Store) {
        self.with(store, |_| ());
    }

    pub fn get(&self, store: &Store, name: &str) -> Option<Learned> {
        self.with(store, |m| m.get(name).cloned())
    }

    /// Every learned version, oldest name first.
    pub fn all(&self, store: &Store) -> Vec<Learned> {
        self.with(store, |m| m.values().cloned().collect())
    }

    /// A root's learned versions' names, newest (highest) first: what each
    /// point reads, without the versions' texts.
    pub fn names_of_root(&self, store: &Store, root: &str) -> Vec<String> {
        let mut v: Vec<(u32, String)> = self.with(store, |m| {
            m.values()
                .filter(|l| l.root == root)
                .map(|l| (l.pack.version, l.name()))
                .collect()
        });
        v.sort_by_key(|(n, _)| std::cmp::Reverse(*n));
        v.into_iter().map(|(_, name)| name).collect()
    }

    /// Keep a version whose row was just written.
    pub fn add(&self, l: Learned) {
        if let Some(m) = lock(&self.loaded).as_mut() {
            m.insert(l.name(), l);
        }
    }

    /// Drop what is loaded: the next read is the store's (a restart's).
    pub fn forget(&self) {
        *lock(&self.loaded) = None;
    }
}

/// The state dir the store lives in: the store's own directory's parent.
/// The daemon's `--state-dir` moves the store but not `[server] state_dir`,
/// so `Config::state_dir` is not where a daemon so started keeps its state.
pub fn state_of(store: &Store) -> std::path::PathBuf {
    let dir = store.dir();
    dir.parent().unwrap_or(dir).to_path_buf()
}

/// `<state dir>/packs/`.
pub fn dir(state: &Path) -> std::path::PathBuf {
    state.join("packs")
}

/// Write each version's file from its row where it is missing or differs,
/// whole and renamed into place. Returns the files written.
pub fn write_files(state: &Path, versions: &[Learned]) -> Result<usize> {
    let dir = dir(state);
    let mut n = 0;
    for l in versions {
        let path = dir.join(format!("{}.toml", l.name()));
        if std::fs::read_to_string(&path).is_ok_and(|t| t == l.text) {
            continue;
        }
        std::fs::create_dir_all(&dir)?;
        let tmp = dir.join(format!(".{}.toml.tmp", l.name()));
        std::fs::write(&tmp, &l.text)?;
        std::fs::rename(&tmp, &path)?;
        n += 1;
    }
    Ok(n)
}

/// Whether a learned version stands in its root's place: its latest row
/// (declined ones skipped) puts it in shadow, a canary, or live.
pub fn is_placed(rung: Rung, has_row: bool) -> bool {
    has_row && matches!(rung, Rung::Shadow | Rung::Canary | Rung::Live)
}

impl super::JudgeService {
    /// A pack version by name: compiled in, else learned.
    pub fn pack(&self, name: &str) -> Option<Arc<Pack>> {
        theseus_judge::pack::by_name(name)
            .or_else(|| self.lineage.get(&self.store, name).map(|l| l.pack))
    }

    /// The compiled-in version at the head of `name`'s lineage (itself for
    /// one).
    pub fn root_of(&self, name: &str) -> String {
        if theseus_judge::pack::by_name(name).is_some() {
            return name.to_string();
        }
        self.lineage
            .get(&self.store, name)
            .map_or_else(|| name.to_string(), |l| l.root)
    }

    pub fn lineage(&self) -> &Lineage {
        &self.lineage
    }

    /// The version standing in `root`'s place for `session`: the newest
    /// learned one the ladder placed (a canary's only in its canary arm),
    /// else the root. A root with no learned version reads nothing more.
    /// Before the warm read it is the root, read from nothing
    /// (theseus-289c): a point never reads the lineage or the ladder.
    pub fn placed(&self, root: &str, session: &str) -> String {
        if !self.cfg.enabled || !self.ladder_read() {
            return root.to_string();
        }
        for name in self.lineage.names_of_root(&self.store, root) {
            let has_row = !self.ladder().rows_of(&name).iter().all(|r| r.declined);
            let s = self.ladder().standing(&name);
            if !is_placed(s.rung, has_row) {
                continue;
            }
            if s.rung == Rung::Canary
                && theseus_judge::learn::arm(session, &name, s.share.unwrap_or(0.0))
                    == theseus_judge::learn::Arm::Control
            {
                continue;
            }
            return name;
        }
        root.to_string()
    }

    /// [`JudgeService::placed`] for a reader off every point's path (the
    /// learning loop, `pack.list`): the ladder and the lineage read first,
    /// when they are not.
    pub fn placed_read(&self, root: &str, session: &str) -> String {
        if self.cfg.enabled {
            self.read_ladder();
        }
        self.placed(root, session)
    }

    /// The root's config line caps a learned version: `[judge.packs."<root>"]`
    /// and `max_mode` bind its whole lineage.
    pub(super) fn capped_by_root(&self, pack: &str, given: ladder::Given) -> ladder::Given {
        let root = self.root_of(pack);
        if root == pack {
            return given;
        }
        match self.cfg.mode_of(&root, PackMode::Live) {
            PackMode::Off => ladder::Given::OFF,
            PackMode::Shadow if given.mode == PackMode::Live => ladder::Given {
                mode: PackMode::Shadow,
                arm: given.arm,
            },
            _ => given,
        }
    }

    /// After serving: each learned version's file, from its row.
    pub fn write_pack_files(&self, state: &Path) {
        let all = self.lineage.all(&self.store);
        if let Err(e) = write_files(state, &all) {
            tracing::warn!(error = %format!("{e:#}"), "judge: the learned packs' files were not written; their rows hold them");
        }
    }
}
