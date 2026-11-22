//! The owner is told when the self loop cannot start anything (issue 41). A hold on the slots that has lasted
//! [`TELL_AFTER`] is said where the owner looks: the morning notice's "Waits for you" and the Ops page (`health`'s
//! `self_held`). Before, the reason was only in `theseus self pipeline`, and nine hours went by unseen.
//!
//! It is information, not a verdict: three hours on the slots can be a healthy install (a build's watchdog is on
//! its silence, not its length). So it says, per tree, what the tree IS, and prescribes nothing that destroys work:
//! - **opened by this daemon and still in use** (its mark, and this process opened it): live, "it is working",
//!   with its phase and how long; an install under way is said as one (`installing`);
//! - **the loop's mark, not this daemon's**: the start's sweep closes those, so this should not happen; it says so;
//! - **an item's tree**: the item and its queue, as the pipeline's view names them; what ends it is the owner's own
//!   verbs, described, never removing the tree;
//! - **anything else**: not opened by the loop, "look before removing it", with no command.

use std::time::Duration;

use theseus_protocol::rsi::SelfMode;

use crate::rpc::Core;
use crate::rsi::capacity;
use crate::rsi::join::{guard, rows_of};
use crate::rsi::tree::{self, Place};

/// How long a hold on the slots lasts before the owner is told. Information only: no length of a hold proves that
/// a tree is abandoned.
pub const TELL_AFTER: Duration = Duration::from_secs(3 * 3600);

/// `ms` as hours and minutes.
fn span(ms: u64) -> String {
    let m = ms / 60_000;
    match m / 60 {
        0 => format!("{m} min"),
        h => format!("{h} h {} min", m % 60),
    }
}

/// An item's tree: its item, title and queue, as the pipeline's view names them.
type ItemOf = (String, String, &'static str);

impl Core {
    /// The line for the owner when the loop has been held on its slots for [`TELL_AFTER`] or more.
    pub fn self_held(&self) -> Option<String> {
        self.self_held_at(theseus_protocol::now_unix_ms())
    }

    /// The same, at `now_ms`. None with the mode off, with no `[self] repo`, while the slots are not all taken by
    /// trees, while the last thing the pipeline said is not a hold on the slots, or before [`TELL_AFTER`] has gone
    /// by since it said so.
    pub(crate) fn self_held_at(&self, now_ms: u64) -> Option<String> {
        let cfg = &self.cfg.self_improve;
        if cfg.mode != SelfMode::Act {
            return None;
        }
        let place = self.tools.self_pr.place(cfg.repo.as_deref()?);
        let open = capacity::trees_named(&place.trees());
        if open.is_empty() || open.len() < cfg.pipeline_builds as usize {
            return None;
        }
        let rows = rows_of(
            &self.store,
            &["self.pipeline.held", "self.pipeline.started"],
        )
        .ok()??;
        let last = rows.iter().max_by_key(|r| r.at_unix_ms)?;
        if last.kind != "self.pipeline.held" || last.data["hold"] != "slots" {
            return None;
        }
        let held = now_ms.saturating_sub(last.at_unix_ms);
        if held < TELL_AFTER.as_millis() as u64 {
            return None;
        }
        let items = self.items_by_tree(&place);
        let mut parts: Vec<String> = Vec::new();
        if let Some(i) = self.self_installing() {
            parts.push(format!(
                "the loop is {}: it is working",
                super::installing::said(&i)
            ));
        }
        parts.extend(open.iter().map(|t| tree_line(&place, t, &items, now_ms)));
        Some(format!(
            "the self loop has started nothing for {}: every slot is taken by an open tree. {}",
            span(held),
            parts.join("; ")
        ))
    }

    /// The items whose trees the pipeline knows, by tree name.
    fn items_by_tree(&self, place: &Place) -> Vec<(String, ItemOf)> {
        let Ok(Some(q)) = self.pipeline_queues(place, super::GRACE) else {
            return Vec::new();
        };
        let queues = [
            ("ready", &q.ready),
            ("building", &q.building),
            ("reviewing", &q.reviewing),
            ("joining", &q.joining),
            ("held for the owner", &q.held),
            ("stale", &q.stale),
            ("ended", &q.ended),
            ("dropped", &q.dropped),
            ("joined", &q.joined),
        ];
        queues
            .into_iter()
            .flat_map(|(name, es)| {
                es.iter()
                    .map(move |e| (tree::slug(&e.item), (e.item.clone(), e.title.clone(), name)))
            })
            .collect()
    }
}

/// What `name` is, in a few words, and what (if anything) the owner might do.
fn tree_line(place: &Place, name: &str, items: &[(String, ItemOf)], now_ms: u64) -> String {
    if let Some(m) = guard::mark_of(place, name) {
        let dir = std::fs::canonicalize(place.trees())
            .map(|t| t.join(name))
            .unwrap_or_default();
        let age = span(now_ms.saturating_sub(m.at_ms));
        return match guard::is_opened_here(&dir) {
            true => format!(
                "{name}: the loop's {} tree, opened by this daemon {age} ago and still in use: it is \
                 working, nothing to do",
                m.kind
            ),
            false => format!(
                "{name}: the loop's {} tree, opened {age} ago and not by this daemon: the start's sweep \
                 closes such a tree, so this should not be; look before removing it",
                m.kind
            ),
        };
    }
    if let Some((_, (item, title, queue))) = items.iter().find(|(slug, _)| slug == name) {
        return format!(
            "{name}: the tree of item {item} \u{ab}{title}\u{bb} ({queue}), the pipeline's own; it goes with \
             its item, and the owner's own verbs end that work (`theseus self halt` stops every build; \
             dropping the item cancels its build)"
        );
    }
    format!("{name}: not opened by the loop: look before removing it")
}
