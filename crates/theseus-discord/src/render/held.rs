//! The turns a renderer holds, and what goes with a turn it drops
//! (theseus-6809, theseus-whb0). A child of `render`, so it sees `Renderer`'s
//! fields.
//!
//! A renderer holds the last `RECENT_TURNS` turns, so a late event (a
//! background job's result, a parked call approved in a later turn) still
//! finds its tool line. Its per-key maps hold only those turns' keys: when a
//! turn is dropped, its keys leave `emitted` and `menus`, and its calls'
//! notice cards leave `notices`. The place's actor tells its lane the turns
//! held (`held`) whenever they change, and the lane keeps their messages'
//! ids out of its recency bound until then.
//!
//! A turn's keys all begin `<turn_id>:`: its stream's `<turn>:L<n>:p<i>` and
//! `<turn>:L<n>:tools`, its reply's `<turn>:footer`, and the notice card of
//! each call whose line it holds, `<turn>:notice:<tool_use_id>`.

use std::collections::HashSet;

use super::{Renderer, RECENT_TURNS};

impl Renderer {
    /// The turns it holds, oldest first: what the place's lane keeps whole.
    pub fn held(&self) -> Vec<String> {
        self.turns.iter().map(|t| t.turn_id.clone()).collect()
    }

    /// Drop the turns past `RECENT_TURNS`, oldest first, with their keys.
    pub(super) fn drop_past_recent(&mut self) {
        let mut dropped = vec![];
        while self.turns.len() > RECENT_TURNS {
            if let Some(t) = self.turns.pop_front() {
                dropped.push(format!("{}:", t.turn_id));
            }
        }
        if dropped.is_empty() {
            return;
        }
        let ours = |k: &String| dropped.iter().any(|p| k.starts_with(p.as_str()));
        self.emitted.retain(|k, _| !ours(k));
        self.menus.retain(|k, _| !ours(k));
        // A card stays while a held turn holds its call's line.
        let calls: HashSet<&str> = self
            .turns
            .iter()
            .flat_map(|t| t.loops.values())
            .flat_map(|lv| lv.tools.iter().map(|l| l.tool_use_id.as_str()))
            .collect();
        self.notices.retain(|id, _| calls.contains(id.as_str()));
    }

    /// A call's notice card's key: under the turn that holds its line, so it
    /// is kept and forgotten with that turn. A call no held turn shows keeps
    /// the lane's recency bound, and its card leaves at the next drop.
    pub(super) fn notice_key(&self, tool_use_id: &str) -> String {
        let holds = |t: &&super::TurnView| {
            t.loops
                .values()
                .any(|lv| lv.tools.iter().any(|l| l.tool_use_id == tool_use_id))
        };
        match self.turns.iter().rev().find(holds) {
            Some(t) => format!("{}:notice:{tool_use_id}", t.turn_id),
            None => format!("notice:{tool_use_id}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::{Op, Renderer, RECENT_TURNS};

    /// theseus-whb0: a renderer put through three times the turns it holds,
    /// each streaming a text part and a tool line with a notice card, keeps
    /// in `emitted`, `menus` and `notices` only the keys of the turns it
    /// still holds.
    #[test]
    fn a_renderers_maps_hold_only_its_held_turns_keys() {
        for embeds in [false, true] {
            let mut r = Renderer::new(embeds);
            let mut notice_keys = vec![];
            for n in 0..3 * RECENT_TURNS {
                let turn = format!("turn_{n}");
                let use_id = format!("use_{n}");
                r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": turn}));
                r.on_notification(
                    "model.delta",
                    &json!({"session_id": "s", "turn_id": turn, "loop_index": 0, "text": "Looking."}),
                );
                r.on_notification("tool.proposed", &json!({"turn_id": turn, "tool_use_id": use_id, "tool": "proc.run",
                    "input": {"argv": ["cargo", "test"]}, "gate": {"result": {"gate": "allow"},
                    "decision": {"mode": "allow", "posture": "notify", "notify": {"kind": "notify", "setting": "enforcement = notify", "rule": "proc.run — notify (enforcement = notify)"}}}}));
                let ops = r.on_notification("policy.notified", &json!({"session_id": "s", "turn_id": turn, "tool_use_id": use_id,
                    "tool": "proc.run", "input": {"argv": ["cargo", "test"]}, "summary": "run `cargo test` in /w",
                    "kind": "notify", "setting": "enforcement = notify", "rule": "proc.run — notify (enforcement = notify)"}));
                notice_keys.extend(ops.iter().filter_map(|o| match o {
                    Op::Notice { key, .. } => Some(key.clone()),
                    _ => None,
                }));
                r.tick();
            }
            let kept: Vec<String> = (2 * RECENT_TURNS..3 * RECENT_TURNS)
                .map(|n| format!("turn_{n}"))
                .collect();
            assert_eq!(r.held(), kept);
            let ours = |k: &str| kept.iter().any(|t| k.starts_with(&format!("{t}:")));
            assert!(
                r.emitted.keys().all(|k| ours(k)),
                "{embeds}: {:?}",
                r.emitted.keys()
            );
            // Each kept turn's text part and tool line.
            assert_eq!(r.emitted.len(), 2 * RECENT_TURNS, "{embeds}");
            assert!(
                r.menus.keys().all(|k| ours(k)),
                "{embeds}: {:?}",
                r.menus.keys()
            );
            if embeds {
                assert_eq!(notice_keys.len(), 3 * RECENT_TURNS);
                assert_eq!(notice_keys[0], "turn_0:notice:use_0");
                let mut ids: Vec<&String> = r.notices.keys().collect();
                ids.sort();
                let mut want: Vec<String> = (2 * RECENT_TURNS..3 * RECENT_TURNS)
                    .map(|n| format!("use_{n}"))
                    .collect();
                want.sort();
                assert_eq!(ids, want.iter().collect::<Vec<_>>());
            } else {
                assert_eq!(r.menus.len(), RECENT_TURNS, "a menu per kept tool line");
                assert!(r.notices.is_empty());
            }
        }
    }
}
