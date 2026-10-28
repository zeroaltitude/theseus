//! A consolidation finding, said first (theseus-slvl.9): after the nightly
//! pass wrote notes, a chatty persona tells the owner what it found, in its
//! voice (`Proactive::Observation`: `normal` and `quiet` say nothing, and the
//! pass's notes are in the memory either way). A task of the runtime, off the
//! pass: its result returns before the message is written.

use std::sync::Arc;

use theseus_protocol::memory::MemoryConsolidateResult;
use theseus_protocol::Proactive;

use super::lines::Line;
use super::speak::Facts;
use crate::rpc::Core;

/// The most notes the message's facts name, and each one's first line cut.
const NAMED: usize = 3;
const LINE_CHARS: usize = 160;

impl Core {
    /// Tell the owner what the nightly pass found, when the persona's level
    /// lets an observation through and the pass wrote something.
    pub fn say_consolidation(self: &Arc<Self>, r: &MemoryConsolidateResult) {
        let kept: Vec<&str> = r
            .clusters
            .iter()
            .filter(|c| c.outcome == "supported" || c.outcome == "unchecked")
            .filter_map(|c| c.text.as_deref())
            .collect();
        if kept.is_empty()
            || !self
                .personas()
                .persona_for(None, None)
                .initiative
                .lets(Proactive::Observation)
            || !self.runner.place_rule.owner_dm_bound()
        {
            return;
        }
        let n = kept.len() as u64;
        let named = kept
            .iter()
            .take(NAMED)
            .map(|t| {
                let first = t.lines().next().unwrap_or("").trim();
                format!("- {}", first.chars().take(LINE_CHARS).collect::<String>())
            })
            .collect::<Vec<_>>()
            .join("\n");
        let brief = format!("Tonight's memory pass wrote {n} new notes. The first:\n{named}");
        let core = Arc::downgrade(self);
        let pace = 1000 * self.cfg.notify.unprompted_pace_secs;
        tokio::spawn(async move {
            // The one pacing rule (`pacer.rs`): the finding waits for the
            // window before its model call.
            let core = loop {
                let Some(core) = core.upgrade() else { return };
                let now = theseus_protocol::now_unix_ms();
                if core.outbox.pacer.open(now, pace) {
                    break core;
                }
                let wait = (core.outbox.pacer.last_ms() + pace).saturating_sub(now);
                drop(core);
                tokio::time::sleep(std::time::Duration::from_millis(wait.max(1_000))).await;
            };
            let spoken = core
                .initiative_speak(&Facts {
                    kind: "consolidation",
                    brief: &brief,
                    line: Line::Consolidation,
                    n,
                })
                .await;
            if let Err(e) = core.initiative_post("consolidation", &spoken) {
                tracing::warn!(error = %format!("{e:#}"), "the consolidation finding was not written");
            }
        });
    }
}
