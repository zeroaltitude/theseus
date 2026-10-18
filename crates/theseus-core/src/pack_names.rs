//! The pack-text check at start (theseus-oejh3): the text of every embedded
//! pack is matched against `[privacy] sensitive_names`, because the packs ship
//! in a public repository and the list lives in the operator's own config. A
//! hit is said once as a warning and once in health's startup phases
//! (`privacy.packs`), naming the pack, its version and how many names hit,
//! never a name. The start goes on. It runs once, after serving, on the
//! blocking pool.

use std::time::Instant;

use serde_json::json;
use theseus_judge::names::packs_naming;

use crate::rpc::Core;

/// The startup phase that carries a hit.
pub const PHASE: &str = "privacy.packs";

impl Core {
    /// Check the packs' text against the list, off the start path.
    pub fn check_pack_text_after_serving(&self) {
        let names = self.cfg.privacy.names();
        if names.is_empty() {
            return;
        }
        let log = self.startup_log.clone();
        tokio::task::spawn_blocking(move || {
            let t0 = Instant::now();
            let found = packs_naming(&names);
            tracing::debug!(
                us = t0.elapsed().as_micros() as u64,
                "pack text checked against the sensitive names"
            );
            if found.is_empty() {
                return;
            }
            let packs: Vec<_> = found
                .iter()
                .map(|(pack, hits)| json!({"pack": pack, "hits": hits}))
                .collect();
            tracing::warn!(
                packs = %found.iter().map(|(p, n)| format!("{p} ({n})")).collect::<Vec<_>>().join(", "),
                "an embedded pack's text names something in [privacy] sensitive_names: \
                 the packs ship in a public repository"
            );
            log.record(PHASE, true, t0, json!({"packs": packs}));
        });
    }
}
