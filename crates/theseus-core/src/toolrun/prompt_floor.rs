//! The prompt files on the floor, kept as the book changes (issue 23).
//!
//! What goes into every prompt is changed only by the daemon, after the
//! owner's yes (`config_change::prompt_file`), so no tool reaches the files:
//! `fs.write`, `fs.edit`, `fs.patch`, `fs.read`, a shell naming one and an L1
//! job each stop at the gate and wait for the owner, whatever the posture.
//! `guarded.rs` says which paths those are. The files `[context] files` names
//! and the two default homes of a soul are known from the config and the
//! layout, and are on the floor from the start. A soul the ontology puts
//! anywhere else is known from the book, which changes while Theseus runs
//! (a persona is added, a soul is moved), and the floor's lists are built
//! once at a start: so the souls are held here, and every question the floor
//! answers asks the book again first (`sync`), at the cost of one comparison
//! when the book is the one it knew.
//!
//! Nothing here is read by a tool, and the daemon's own readers and writers
//! (the context compile, `persona.soul`, `config.propose_file`'s write, the
//! import and the persona migration) are code, not calls, and do not pass the
//! gate.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use super::guarded;

#[derive(Debug, Default)]
struct Held {
    /// The book the souls were read from (held, so that its address is never
    /// another book's); none before the first.
    book: Option<Arc<crate::personas::Book>>,
    /// The files `[context] files` names, and every soul the book names.
    files: Vec<PathBuf>,
    /// The default homes of a soul: whole directories.
    homes: Vec<PathBuf>,
    /// Souls the book named since L1's views were last told (`take_unhidden`).
    unhidden: Vec<PathBuf>,
}

/// The prompt files the floor holds, shared by the gate, the tools' context
/// and the L1 view. A clone is another handle on the same list.
#[derive(Debug, Clone, Default)]
pub struct PromptFloor {
    held: Arc<Mutex<Held>>,
    /// The core, once built, held weakly: the book is read through it.
    core: Arc<OnceLock<Weak<crate::rpc::Core>>>,
}

impl PromptFloor {
    /// What the config and the layout name; the souls follow from the book.
    pub fn of(cfg: &crate::Config, state: &Path) -> Self {
        let p = guarded::prompt_static(cfg, state);
        Self {
            held: Arc::new(Mutex::new(Held {
                book: None,
                files: p.files,
                homes: p.homes,
                unhidden: Vec::new(),
            })),
            core: Arc::default(),
        }
    }

    /// The core, once built: from here every question this floor answers asks
    /// the book first, so every decider (the gate, `policy.explain`, the
    /// language servers, a job's view) sees the souls as they are now.
    pub fn attach(&self, core: &Arc<crate::rpc::Core>) {
        let _ = self.core.set(Arc::downgrade(core));
    }

    /// Read the book again if it is not the one last read.
    fn sync(&self) {
        let Some(core) = self.core.get().and_then(Weak::upgrade) else {
            return;
        };
        let state = guarded::state_dir_in_use(&core.cfg);
        let new = self.refresh(&core.cfg, &state, &core.personas());
        self.held.lock().unwrap().unhidden.extend(new);
    }

    /// The souls newly named since the last call, for L1's views to hide.
    pub fn take_unhidden(&self) -> Vec<PathBuf> {
        std::mem::take(&mut self.held.lock().unwrap().unhidden)
    }

    /// Whether `p` is a prompt file, a copy beside one, or in a soul's home.
    pub fn holds(&self, p: &Path) -> bool {
        self.sync();
        let h = self.held.lock().unwrap();
        h.files
            .iter()
            .chain(&h.homes)
            .any(|f| theseus_tools::paths::within(p, f))
            || guarded::beside(p, &h.files)
    }

    /// Every path held, for a tool's own context.
    pub fn paths(&self) -> Vec<PathBuf> {
        self.sync();
        let h = self.held.lock().unwrap();
        h.files.iter().chain(&h.homes).cloned().collect()
    }

    /// Read the souls from `book`; the paths newly held when it is a book not
    /// seen before, none when it is the one already known.
    pub fn refresh(
        &self,
        cfg: &crate::Config,
        state: &Path,
        book: &Arc<crate::personas::Book>,
    ) -> Vec<PathBuf> {
        let mut h = self.held.lock().unwrap();
        if h.book.as_ref().is_some_and(|b| Arc::ptr_eq(b, book)) {
            return Vec::new();
        }
        h.book = Some(book.clone());
        let mut files: Vec<PathBuf> = Vec::new();
        for f in guarded::prompt_static(cfg, state)
            .files
            .into_iter()
            .chain(guarded::prompt_souls(cfg, book))
        {
            if !files.contains(&f) {
                files.push(f);
            }
        }
        let new = files
            .iter()
            .filter(|f| !h.files.contains(f))
            .cloned()
            .collect();
        h.files = files;
        new
    }
}

impl super::ToolRuntime {
    /// Ask the book again before a call is judged, and hide from L1's views
    /// any soul it newly names.
    pub(crate) fn refresh_prompt_floor(&self) {
        self.policy.prompt.sync();
        for p in self.policy.prompt.take_unhidden() {
            self.sandbox.hide(p);
        }
    }
}

#[cfg(test)]
mod tests;
