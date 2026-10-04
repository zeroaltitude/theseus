//! Loaded extensions on the board (M7 43b, design §2.7): an acked
//! extension's server, `ext-<name>`, tended as a configured server is (the
//! same start, crash backoff, stored list, and stop), but loaded and revoked
//! while the daemon runs.
//!
//! - **Load** puts the server beside the configured ones and rebuilds the
//!   catalog, so its tools are offered from the next turn's start: a turn's
//!   request spec, fixed at its start, keeps the tools it began with. After
//!   serving it starts at once; before, `start` starts it with the rest.
//! - **A new version** of a loaded name replaces the old one in one step:
//!   the old server runs until the new one is put in its place, then stops.
//! - **Revoke** takes it off the board, rebuilds the catalog, ends its tending,
//!   and sends SIGTERM to its process group: in L1 the `mcp-sandbox` role,
//!   whose namespace ends with it.

use std::sync::{Arc, PoisonError};

use tokio::task::AbortHandle;

use super::{McpBoard, Server, StoredList};
use crate::config::McpServerConfig;

/// A loaded extension's server, and the task that tends it once started.
pub struct Loaded {
    server: Arc<Server>,
    tending: Option<AbortHandle>,
}

impl Loaded {
    /// Its tending ends, and its process gets SIGTERM, never waited for.
    fn end(self) {
        if let Some(t) = self.tending {
            t.abort();
        }
        if let Some(c) = self.server.live().client.take() {
            c.terminate();
        }
        self.server.set(|l| l.pid = None);
    }
}

impl McpBoard {
    fn loaded(&self) -> std::sync::MutexGuard<'_, std::collections::BTreeMap<String, Loaded>> {
        self.extensions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// The loaded extensions' servers, by name.
    pub(super) fn loaded_servers(&self) -> Vec<Arc<Server>> {
        self.loaded().values().map(|l| l.server.clone()).collect()
    }

    pub(super) fn loaded_server(&self, name: &str) -> Option<Arc<Server>> {
        self.loaded().get(name).map(|l| l.server.clone())
    }

    /// Whether `name` is a loaded extension's server.
    pub fn is_loaded(&self, name: &str) -> bool {
        self.loaded().contains_key(name)
    }

    /// Load the extension server `name` (`ext-<name>`) of `cfg`, offering
    /// `stored` at once (the tools its trial listed, or its last list), and
    /// start it if the board has started. A server of that name already
    /// loaded (an older version) is replaced, then stopped.
    pub fn load_extension(
        self: &Arc<Self>,
        name: &str,
        cfg: McpServerConfig,
        stored: Option<StoredList>,
    ) {
        let server = Arc::new(Server::new(name, &cfg, stored));
        // Under the list's lock, so `start` either sees it or has started.
        let old = {
            let mut loaded = self.loaded();
            let started = self.started.load(std::sync::atomic::Ordering::SeqCst);
            // After serving it starts now; before, `start` starts it.
            let tending = match tokio::runtime::Handle::try_current() {
                Ok(rt) if started => {
                    Some(rt.spawn(self.clone().tend(server.clone())).abort_handle())
                }
                _ => None,
            };
            loaded.insert(name.to_string(), Loaded { server, tending })
        };
        self.rebuild();
        if let Some(old) = old {
            old.end();
        }
    }

    /// Revoke the extension server `name`: off the board, its tools out of
    /// the catalog, its process stopped. False when none is loaded.
    pub fn unload_extension(&self, name: &str) -> bool {
        let Some(gone) = self.loaded().remove(name) else {
            return false;
        };
        self.rebuild();
        gone.end();
        true
    }

    /// At `start`: tend each loaded extension not tended yet.
    pub(super) fn start_extensions(self: &Arc<Self>) {
        let mut loaded = self.loaded();
        for l in loaded.values_mut().filter(|l| l.tending.is_none()) {
            l.tending = Some(tokio::spawn(self.clone().tend(l.server.clone())).abort_handle());
        }
    }
}
