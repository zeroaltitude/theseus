//! A loop's thinking as a message of its own (theseus-l1y1): the model's
//! thinking, as the provider streams it (`model.thinking`; Anthropic's
//! summarized thinking by default, `[profiles.*] thinking_display`), under
//! `<turn>:L<loop>:think`, before the loop's text. Before this, the binding
//! dropped thinking: the CLI and the TUI showed it, Discord never did.
//!
//! One message per loop, edited as the thinking grows at the place's edit
//! tick, its first text at once, as a loop's text is (theseus-ck0n). It shows
//! the start of the thinking, up to `SHOWN` characters, and says how much it
//! left out: the whole of it is the session's history. Live progress, best
//! effort, never a post. A place shows it unless its binding says
//! `show_thinking = false` (`runtime/show.rs`). It holds the turns the
//! place's renderer holds (`RECENT_TURNS`, by the same `turn.started`), so
//! the lane keeps and forgets its keys with theirs.

use std::collections::{BTreeMap, VecDeque};

use theseus_protocol::Event;

use super::{Buttons, Op, RECENT_TURNS};
use crate::policy::THINK_SUFFIX;

/// The most of a loop's thinking its message shows, in characters: with its
/// header and the line that says what it left out, under Discord's 2,000.
const SHOWN: usize = 1800;

#[derive(Default)]
pub struct Thinking {
    turns: VecDeque<(String, BTreeMap<u32, Think>)>,
}

#[derive(Default)]
struct Think {
    text: String,
    /// Grew since Discord last got it.
    dirty: bool,
}

impl Thinking {
    /// One event of the place's session. Thinking's first text shows at
    /// once; any other event of its turn (the loop's text, a call, its end)
    /// shows what it holds, so the thinking lands before what follows it.
    pub fn on_event(&mut self, e: &Event) -> Vec<Op> {
        let Some(turn) = e.turn_id() else {
            return vec![];
        };
        match e {
            Event::TurnStarted(_) => {
                self.turns.push_back((turn.to_string(), BTreeMap::new()));
                while self.turns.len() > RECENT_TURNS {
                    self.turns.pop_front();
                }
                vec![]
            }
            Event::ModelThinking(d) => {
                let Some((_, loops)) = self.turns.iter_mut().find(|(t, _)| t == turn) else {
                    return vec![];
                };
                let th = loops.entry(d.loop_index).or_default();
                let first = th.text.trim().is_empty();
                th.text.push_str(&d.text);
                th.dirty = true;
                if first && !th.text.trim().is_empty() {
                    return self.tick();
                }
                vec![]
            }
            _ => self.tick(),
        }
    }

    /// Each loop whose thinking grew since Discord last got it, as an upsert.
    pub fn tick(&mut self) -> Vec<Op> {
        let mut ops = Vec::new();
        for (turn, loops) in &mut self.turns {
            for (li, th) in loops.iter_mut().filter(|(_, th)| th.dirty) {
                th.dirty = false;
                if th.text.trim().is_empty() {
                    continue;
                }
                ops.push(Op::Upsert {
                    key: format!("{turn}:L{li}{THINK_SUFFIX}"),
                    content: content(&th.text),
                    buttons: Buttons::Keep,
                });
            }
        }
        ops
    }
}

/// A loop's thinking as its message says it: a header, then the thinking
/// quoted, cut at `SHOWN` characters with what was left out said.
fn content(text: &str) -> String {
    let text = text.trim();
    let n = text.chars().count();
    let mut shown: String = text.chars().take(SHOWN).collect();
    if n > SHOWN {
        shown.push_str(&format!("… ({} more characters)", n - SHOWN));
    }
    format!("-# 💭 thinking\n>>> {shown}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::{ModelDelta, TurnStarted};

    fn started(turn: &str) -> Event {
        Event::TurnStarted(TurnStarted {
            turn_id: turn.into(),
            ..Default::default()
        })
    }

    fn thinking(turn: &str, li: u32, text: &str) -> Event {
        Event::ModelThinking(ModelDelta {
            turn_id: turn.into(),
            loop_index: li,
            text: text.into(),
        })
    }

    fn delta(turn: &str, li: u32, text: &str) -> Event {
        Event::ModelDelta(ModelDelta {
            turn_id: turn.into(),
            loop_index: li,
            text: text.into(),
        })
    }

    fn upserts(ops: &[Op]) -> Vec<(String, String)> {
        ops.iter()
            .filter_map(|o| match o {
                Op::Upsert { key, content, .. } => Some((key.clone(), content.clone())),
                _ => None,
            })
            .collect()
    }

    /// Its first text shows at once, the rest at the tick, and the loop's
    /// text that follows shows what the thinking holds first.
    #[test]
    fn a_loops_thinking_shows_at_once_then_at_the_tick() {
        let mut t = Thinking::default();
        assert!(t.on_event(&started("turn_a")).is_empty());
        let got = upserts(&t.on_event(&thinking("turn_a", 0, "The tide chart")));
        assert_eq!(
            got,
            [(
                "turn_a:L0:think".to_string(),
                "-# 💭 thinking\n>>> The tide chart".to_string()
            )]
        );
        assert!(t
            .on_event(&thinking("turn_a", 0, " is in work/."))
            .is_empty());
        let got = upserts(&t.on_event(&delta("turn_a", 0, "Reading it.")));
        assert_eq!(got.len(), 1);
        assert!(got[0].1.ends_with("is in work/."), "{got:?}");
        assert!(t.tick().is_empty(), "nothing grew since");
        // The next loop's thinking is a message of its own.
        let got = upserts(&t.on_event(&thinking("turn_a", 1, "Low tide.")));
        assert_eq!(got[0].0, "turn_a:L1:think");
    }

    /// Long thinking is cut, saying how much it left out; a turn it does not
    /// hold shows nothing; and it holds the renderer's recent turns alone.
    #[test]
    fn long_thinking_is_cut_and_says_so_and_old_turns_are_dropped() {
        let mut t = Thinking::default();
        t.on_event(&started("turn_a"));
        let long = "x".repeat(SHOWN + 25);
        let got = upserts(&t.on_event(&thinking("turn_a", 0, &long)));
        assert!(got[0].1.ends_with("… (25 more characters)"), "{got:?}");
        assert!(got[0].1.chars().count() < 2000);
        assert!(t.on_event(&thinking("turn_x", 0, "unseen")).is_empty());
        for i in 0..RECENT_TURNS {
            t.on_event(&started(&format!("turn_{i}")));
        }
        assert_eq!(t.turns.len(), RECENT_TURNS);
        assert!(t.turns.iter().all(|(id, _)| id != "turn_a"));
    }
}
