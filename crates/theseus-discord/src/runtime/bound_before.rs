//! The channels a thread was bound to before threads were retired (issue 44):
//! a Discord thread the binding made for one, or a channel `/thread bind`
//! bound, that the bindings file does not name. A store keeps them in each
//! live thread record's `surfaces`. Each runs as an ordinary channel again,
//! with the binding it had then (private by the owner's word, the owners its
//! users, no ceiling), so its messages still find a place and its pending
//! posts still go out. Nothing is adopted, written or changed in Discord. A
//! child of `runtime`.

use std::sync::Arc;

use theseus_core::places::BoundPlace;
use theseus_protocol::threads::{surface_channel, ThreadState};

use super::Shared;
use crate::bindings::ChannelBinding;

impl Shared {
    /// Each such channel and its label, from the live thread records.
    fn bound_before(&self) -> Vec<(u64, String)> {
        let board = self.core.threads();
        board
            .all()
            .filter(|t| t.state != ThreadState::Done)
            .filter_map(|t| {
                let c = t.surfaces.iter().find_map(|s| surface_channel(s))?;
                let title: String = t.title.chars().take(60).collect();
                Some((c, format!("#{title}")))
            })
            .collect()
    }

    /// The binding such a channel ran under: private by the owner's word,
    /// driven by the owners.
    fn binding_before(&self, channel: u64, label: &str) -> ChannelBinding {
        let owners = self.core.runner.place_rule.owners(&self.core.cfg);
        let users = owners
            .iter()
            .filter_map(|o| o.strip_prefix("discord:"))
            .map(str::to_string)
            .collect();
        ChannelBinding {
            guild: String::new(),
            id: channel.to_string(),
            name: Some(label.to_string()),
            users,
            mention_only: false,
            private: Some(true),
            voice: false,
            ceiling: None,
            show_tools: true,
            show_thinking: true,
            silent: None,
            ping_window_secs: None,
            voice_status: None,
            persona: None,
            forum: false,
            task_threads: false,
            // Only the bindings file names the announcement place (theseus-nuqut).
            notify: false,
        }
    }

    /// Its lane, and the core told the place is private, as it was.
    fn lane_before(self: &Arc<Self>, channel: u64, label: &str) -> anyhow::Result<ChannelBinding> {
        let b = self.binding_before(channel, label);
        let target = format!("discord:channel:{channel}");
        if self.lane(&target).is_none() {
            self.start_channel_lane(&b)?;
        }
        self.core.runner.place_rule.bind_more(BoundPlace {
            target,
            name: b.label(),
            private: true,
            guild: None,
            ceiling: None,
            persona: None,
        });
        Ok(b)
    }

    /// The lanes, with the file's own, before the start refuses the posts of
    /// places it has no lane for. A channel the file names keeps its own.
    pub(super) fn revive_bound_before(self: &Arc<Self>) {
        for (channel, label) in self.bound_before() {
            if self.lane(&format!("discord:channel:{channel}")).is_some() {
                continue;
            }
            if let Err(e) = self.lane_before(channel, &label) {
                self.board.error("old bound place", None, format!("{e:#}"));
            }
        }
    }

    /// The places, after the file's own: each runs as itself, with its own
    /// session, opened by its next message.
    pub(super) async fn start_bound_before(self: &Arc<Self>) {
        for (channel, label) in self.bound_before() {
            let listed = self
                .routes
                .lock()
                .unwrap()
                .by_channel
                .contains_key(&channel);
            if listed {
                continue;
            }
            let started = match self.lane_before(channel, &label) {
                Ok(b) => self.start_channel(&b).await,
                Err(e) => Err(e),
            };
            if let Err(e) = started {
                self.board.error("old bound place", None, format!("{e:#}"));
            }
        }
    }
}
