//! The turns a renderer holds, and what goes with a turn it drops
//! (theseus-6809, theseus-whb0). A child of `render`, so it sees `Renderer`'s
//! fields.
//!
//! A renderer holds the last `RECENT_TURNS` turns, so a late event (a
//! background job's result, a parked call approved in a later turn) still
//! finds its tool line. A turn older than that stays while one of its calls
//! is in the background or waiting for an answer (`pending`), at most
//! `PINNED_TURNS` of them, so a job's end shows however many turns came
//! between (theseus-9zif). Its per-key maps hold only those turns' keys: when a
//! turn is dropped, its keys leave `emitted` and `menus`, and its calls'
//! notice cards leave `notices`. The place's actor tells its lane the turns
//! held (`held`) whenever they change, and the lane keeps their messages'
//! ids out of its recency bound until then.
//!
//! A turn's keys all begin `<turn_id>:`: its stream's `<turn>:L<n>:p<i>` and
//! `<turn>:L<n>:tools` (at most `STEP_LOOPS` of those, the later loops
//! sharing the step's, theseus-8wm), its reply's `<turn>:footer`, and the
//! notice card of each call whose line it holds, `<turn>:notice:<tool_use_id>`.

use std::collections::HashSet;

use super::{Renderer, ToolState, TurnView, RECENT_TURNS};

/// The most turns past the recent ones held for a call still waiting: a job
/// that never ends does not hold every turn that started it.
const PINNED_TURNS: usize = RECENT_TURNS;

/// Does a call of this turn still wait for its end, a background job's late
/// result or an answer to its approval? Its end names the call, not the turn
/// it comes in, and finds the line by the call (`update_tool`), so the line's
/// turn must outlive the wait (theseus-9zif).
fn pending(t: &TurnView) -> bool {
    t.loops
        .values()
        .flat_map(|lv| &lv.tools)
        .any(|l| matches!(l.state, ToolState::Background | ToolState::Waiting))
}

impl Renderer {
    /// The turns it holds, oldest first: what the place's lane keeps whole.
    pub fn held(&self) -> Vec<String> {
        self.turns.iter().map(|t| t.turn_id.clone()).collect()
    }

    /// The renderer has seen turn `turn_id` start.
    pub fn knows(&self, turn_id: &str) -> bool {
        self.turns.iter().any(|t| t.turn_id == turn_id)
    }

    /// Drop the turns past `RECENT_TURNS`, oldest first, with their keys,
    /// but keep an older turn while one of its calls still waits for an
    /// end (`pending`), up to `PINNED_TURNS` of them, the newest.
    pub(super) fn drop_past_recent(&mut self) {
        let older = self.turns.len().saturating_sub(RECENT_TURNS);
        let mut room = PINNED_TURNS;
        let mut keep = vec![true; self.turns.len()];
        for (i, t) in self.turns.iter().enumerate().take(older).rev() {
            if pending(t) && room > 0 {
                room -= 1;
            } else {
                keep[i] = false;
            }
        }
        let mut dropped = vec![];
        let mut at = 0;
        self.turns.retain(|t| {
            at += 1;
            if !keep[at - 1] {
                dropped.push(format!("{}:", t.turn_id));
            }
            keep[at - 1]
        });
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
    use super::PINNED_TURNS;

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

    /// A background job's call, as the place sees it up to its turn's end:
    /// proposed, notified, started, handed to the background, turn ended.
    fn parked(r: &mut Renderer, turn: &str, use_id: &str) {
        r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": turn}));
        r.on_notification("tool.proposed", &json!({"turn_id": turn, "tool_use_id": use_id, "tool": "proc.run",
            "input": {"argv": ["sleep", "75"]}, "gate": {"result": {"gate": "allow"},
            "decision": {"mode": "allow", "posture": "notify", "notify": {"kind": "notify", "setting": "enforcement = notify", "rule": "proc.run — notify (enforcement = notify)"}}}}));
        r.on_notification("policy.notified", &json!({"session_id": "s", "turn_id": turn, "tool_use_id": use_id,
            "tool": "proc.run", "input": {"argv": ["sleep", "75"]}, "summary": "run `sleep 75` in /w",
            "kind": "notify", "setting": "enforcement = notify", "rule": "proc.run — notify (enforcement = notify)"}));
        r.on_notification(
            "tool.started",
            &json!({"turn_id": turn, "tool_use_id": use_id}),
        );
        r.on_notification("tool.ended", &json!({"turn_id": turn, "tool_use_id": use_id, "status": "background", "duration_ms": 2000}));
        r.on_notification("turn.ended", &json!({"session_id": "s", "turn_id": turn}));
        r.tick();
    }

    /// An ordinary turn that calls nothing.
    fn plain(r: &mut Renderer, turn: &str) {
        r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": turn}));
        r.on_notification("turn.ended", &json!({"session_id": "s", "turn_id": turn}));
        r.tick();
    }

    /// The job's late result: its wake is a turn of its own, whose start
    /// comes first, and whose events name the job's call.
    fn late_end(r: &mut Renderer, wake: &str, use_id: &str) -> Vec<Op> {
        r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": wake}));
        let mut ops = r.on_notification(
            "tool.ended",
            &json!({"turn_id": wake, "tool_use_id": use_id,
            "status": "ok", "duration_ms": 75001}),
        );
        ops.extend(r.tick());
        ops
    }

    /// theseus-9zif: a job that outlives more turns than the renderer holds
    /// (it held eight, and the job's wake was the ninth, whose start dropped
    /// the job's turn) still ends its line and its card: the line says `✅`
    /// and the milliseconds, and the card's Outcome says the same.
    #[test]
    fn a_background_job_that_outlives_the_held_turns_shows_its_end() {
        for embeds in [false, true] {
            let mut r = Renderer::new(embeds);
            parked(&mut r, "turn_job", "use_job");
            for n in 0..RECENT_TURNS {
                plain(&mut r, &format!("turn_{n}"));
            }
            let ops = late_end(&mut r, "turn_wake", "use_job");
            let lines: Vec<&String> = ops
                .iter()
                .filter_map(|o| match o {
                    Op::Upsert { key, content, .. } if key == "turn_job:L0:tools" => Some(content),
                    _ => None,
                })
                .collect();
            assert_eq!(
                lines.len(),
                1,
                "{embeds}: one edit of the job's line: {ops:?}"
            );
            assert!(
                lines[0].contains("✅ `proc.run`") && lines[0].contains("75001 ms"),
                "{embeds}: {}",
                lines[0]
            );
            assert!(!lines[0].contains("running in the background"), "{embeds}");
            let cards: Vec<&Op> = ops
                .iter()
                .filter(|o| matches!(o, Op::Notice { .. }))
                .collect();
            if embeds {
                assert_eq!(cards.len(), 1, "{ops:?}");
                let Op::Notice { key, card } = cards[0] else {
                    unreachable!()
                };
                assert_eq!(key, "turn_job:notice:use_job");
                let outcome = card.fields.iter().find(|(n, _)| n == "Outcome");
                assert_eq!(outcome.map(|f| f.1.as_str()), Some("✅ ok · 75001 ms"));
            } else {
                assert!(cards.is_empty(), "{embeds}: no card without embeds");
            }
        }
    }

    /// The job's turn is held only while the job has not ended: once its end
    /// shows, the turn goes at the next start like any other past the recent
    /// ones, and what it held with it, and no turn is held for a call that
    /// is not waiting.
    #[test]
    fn a_turn_held_for_its_job_goes_once_the_job_ended() {
        let mut r = Renderer::new(true);
        parked(&mut r, "turn_job", "use_job");
        for n in 0..RECENT_TURNS {
            plain(&mut r, &format!("turn_{n}"));
        }
        assert!(r.knows("turn_job"), "held for its job");
        assert_eq!(r.held().len(), RECENT_TURNS + 1);
        late_end(&mut r, "turn_wake", "use_job");
        r.on_notification(
            "turn.ended",
            &json!({"session_id": "s", "turn_id": "turn_wake"}),
        );
        plain(&mut r, "turn_next");
        assert!(!r.knows("turn_job"), "{:?}", r.held());
        assert_eq!(r.held().len(), RECENT_TURNS);
        assert!(r.emitted.keys().all(|k| !k.starts_with("turn_job:")));
        assert!(r.notices.is_empty() || !r.notices.contains_key("use_job"));
    }

    /// Past the cap the oldest job's turn goes anyway: a job that never ends
    /// does not hold every turn it ever started.
    #[test]
    fn the_turns_held_for_jobs_are_capped() {
        let mut r = Renderer::new(false);
        for n in 0..3 * PINNED_TURNS {
            parked(&mut r, &format!("job_{n}"), &format!("use_{n}"));
        }
        assert_eq!(r.held().len(), RECENT_TURNS + PINNED_TURNS);
        let first = 3 * PINNED_TURNS - (RECENT_TURNS + PINNED_TURNS);
        assert_eq!(r.held()[0], format!("job_{first}"));
        assert!(r.emitted.keys().all(|k| !k.starts_with("job_0:")));
    }
}
