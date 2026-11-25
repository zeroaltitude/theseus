//! What a place shows (theseus-l1y1): its tool lines and its thinking, each
//! unless its binding says otherwise (`show_tools`, `show_thinking` on a
//! `[[channel]]` or a `[[dm]]`, both on by default). The owner wants a lively
//! chat, replete with both; a place for someone who does not can hide
//! either. Both live in a loop's process message (`render/process.rs`), its
//! thinking at the top: off hides that half in that place alone, and a
//! message with neither half is never made. The turn runs as ever, its cards
//! and reply still post, and every other place is as it was.
//!
//! A third word, `steps` (issue 56), says how those process messages show:
//! `folded`, the default, as one line for the turn that a button opens
//! (`render/fold.rs`), or `open`, a message for each loop, as before.

use theseus_protocol::Event;

use crate::bindings::{ChannelBinding, DmBinding, Steps};
use crate::render::fold::{self, Folds};
use crate::render::process::{Process, Shown};
use crate::render::{Buttons, Op, Renderer};

/// A place's three words, its loops' process messages, and its folds.
#[derive(Default)]
pub(crate) struct View {
    tools: bool,
    thinking: bool,
    steps: Steps,
    process: Process,
    folds: Folds,
}

/// A place's words, as its binding gives them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Show {
    pub tools: bool,
    pub thinking: bool,
    pub steps: Steps,
}

impl Show {
    pub(crate) fn of_channel(c: &ChannelBinding) -> Self {
        Self {
            tools: c.show_tools,
            thinking: c.show_thinking,
            steps: c.steps,
        }
    }

    pub(crate) fn of_dm(d: &DmBinding) -> Self {
        Self {
            tools: d.show_tools,
            thinking: d.show_thinking,
            steps: d.steps,
        }
    }
}

impl Default for Show {
    fn default() -> Self {
        Self {
            tools: true,
            thinking: true,
            steps: Steps::default(),
        }
    }
}

impl View {
    pub(crate) fn with_exits(mut self, exits: std::sync::Arc<theseus_core::exits::Exits>) -> Self {
        self.process = self.process.with_exits(exits);
        self
    }

    /// The words this place shows now (a task's thread shows its parent's).
    pub(crate) fn show(&self) -> Show {
        Show {
            tools: self.tools,
            thinking: self.thinking,
            steps: self.steps,
        }
    }

    pub(crate) fn new(s: Show) -> Self {
        Self {
            tools: s.tools,
            thinking: s.thinking,
            steps: s.steps,
            process: Process::default(),
            folds: Folds::default(),
        }
    }

    /// The bindings file changed the place's words (theseus-ocwt).
    pub(crate) fn set(&mut self, s: Show) {
        self.tools = s.tools;
        self.thinking = s.thinking;
        self.steps = s.steps;
    }

    fn shown(&self) -> Shown {
        Shown {
            tools: self.tools,
            thinking: self.thinking,
        }
    }

    /// What one event's thinking shows (its first text, its fold). The
    /// turns are followed either way, so a place shown thinking again holds
    /// the renderer's turns.
    pub(crate) fn on_event(&mut self, e: &Event) -> Vec<Op> {
        let s = self.shown();
        self.process.on_event(e, tokio::time::Instant::now(), s)
    }

    /// The turns the renderer holds now: the thinking and the folds of the
    /// rest go.
    pub(crate) fn hold(&mut self, held: &[String]) {
        self.process.hold(held);
        self.folds.hold(held);
    }

    /// The thinking that grew since the last tick.
    pub(crate) fn tick(&mut self) -> Vec<Op> {
        let s = self.shown();
        self.process.tick(s)
    }

    /// The renderer's ops as the place shows them: a loop's tool lines under
    /// its thinking, each half only under its word, a call's notice embed
    /// only under `show_tools`, everything else always.
    pub(crate) fn fold(&mut self, ops: Vec<Op>) -> Vec<Op> {
        let s = self.shown();
        self.process.fold_ops(ops, s)
    }

    /// What the place writes, last (`Place::apply`): as it was in a place
    /// bound `steps = "open"`; in a folded one, each turn's process messages
    /// as its one fold, closed by its line (`render/fold.rs`).
    pub(crate) fn out(&mut self, ops: Vec<Op>, r: &Renderer) -> Vec<Op> {
        if self.steps == Steps::Open {
            return ops;
        }
        let s = self.shown();
        let Self { process, folds, .. } = self;
        folds.take(ops, |turn| {
            fold::line(r.tally(turn).as_ref(), process.thought(turn), s)
        })
    }

    /// A press on turn `turn`'s fold: open at `page`, or closed. Its lines
    /// are the place's own, or, when it holds no fold for the turn (a
    /// restart), the renderer's for it, as the place would show them. None
    /// when the renderer holds no such turn.
    pub(crate) fn press(
        &mut self,
        turn: &str,
        page: Option<usize>,
        r: &Renderer,
    ) -> Option<(String, Buttons)> {
        if !r.knows(turn) {
            return None;
        }
        let s = self.shown();
        let seed = if self.folds.holds(turn) {
            Vec::new()
        } else {
            self.process.fold_ops(r.tool_messages(turn), s)
        };
        let head = fold::line(r.tally(turn).as_ref(), self.process.thought(turn), s);
        self.folds.press(turn, page, &head, || seed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{Buttons, NoticeCard};
    use theseus_protocol::{ModelDelta, TurnStarted};

    fn up(key: &str) -> Op {
        Op::Upsert {
            key: key.into(),
            content: "x".into(),
            buttons: Buttons::Keep,
        }
    }

    fn notice() -> Op {
        Op::Notice {
            key: "turn_a:notice:tu_1".into(),
            card: NoticeCard {
                title: "🔔".into(),
                color: 0,
                description: String::new(),
                fields: vec![],
                ask: None,
            },
        }
    }

    /// Each word hides its own messages and nothing else; both on, the
    /// default, hides nothing.
    #[test]
    fn each_word_hides_only_its_own_messages() {
        let all = || {
            vec![
                up("turn_a:L0:p0"),
                up("turn_a:L0:tools"),
                up("turn_a:footer"),
                notice(),
                Op::Typing,
            ]
        };
        let shown = |s: Show| -> Vec<Option<String>> {
            let ops = View::new(s).fold(all());
            all()
                .iter()
                .map(|o| {
                    ops.iter()
                        .find(|p| p.key() == o.key())
                        .and_then(|p| p.key().map(str::to_string))
                })
                .collect()
        };
        let keys = |ks: &[Option<&str>]| -> Vec<Option<String>> {
            ks.iter().map(|k| k.map(str::to_string)).collect()
        };
        let every = [
            Some("turn_a:L0:p0"),
            Some("turn_a:L0:tools"),
            Some("turn_a:footer"),
            Some("turn_a:notice:tu_1"),
            None,
        ];
        assert_eq!(shown(Show::default()), keys(&every));
        let no_thinking = Show {
            tools: true,
            thinking: false,
            ..Show::default()
        };
        assert_eq!(shown(no_thinking), keys(&every));
        let no_tools = Show {
            tools: false,
            thinking: true,
            ..Show::default()
        };
        assert_eq!(
            shown(no_tools),
            keys(&[
                Some("turn_a:L0:p0"),
                None,
                Some("turn_a:footer"),
                None,
                None
            ])
        );
        assert!(
            View::new(no_tools).fold(vec![Op::Typing]).len() == 1,
            "typing always"
        );
    }

    /// Thinking off shows none, and on again shows the thinking of the turn
    /// it still follows, at the top of the loop's process message.
    #[test]
    fn thinking_off_makes_none_and_on_again_shows_the_turn() {
        let mut v = View::new(Show {
            tools: true,
            thinking: false,
            ..Show::default()
        });
        let started = Event::TurnStarted(TurnStarted {
            turn_id: "turn_a".into(),
            ..Default::default()
        });
        let think = |t: &str| {
            Event::ModelThinking(ModelDelta {
                turn_id: "turn_a".into(),
                loop_index: 0,
                text: t.into(),
            })
        };
        assert!(v.on_event(&started).is_empty());
        assert!(v.on_event(&think("Tides.")).is_empty());
        assert!(v.tick().is_empty());
        v.set(Show::default());
        assert!(v.on_event(&think(" Low at six.")).is_empty());
        let ops = v.tick();
        assert_eq!(ops.len(), 1, "{ops:?}");
        assert_eq!(ops[0].key(), Some("turn_a:L0:tools"));
    }

    /// A place bound `steps = "open"` writes what it wrote before, op for
    /// op; a folded one, the default, writes a turn's process messages as
    /// its one fold and every other op as it was (issue 56).
    #[test]
    fn open_writes_as_before_and_folded_folds_only_process_messages() {
        let r = Renderer::new(false);
        let all = || {
            vec![
                up("turn_a:L0:p0"),
                up("turn_a:L0:tools"),
                up("turn_a:L1:tools"),
                up("turn_a:footer"),
                notice(),
                Op::Typing,
            ]
        };
        let mut open = View::new(Show {
            steps: Steps::Open,
            ..Show::default()
        });
        assert_eq!(open.out(all(), &r), all());
        let mut folded = View::new(Show::default());
        let keys: Vec<Option<String>> = folded
            .out(all(), &r)
            .iter()
            .map(|o| o.key().map(str::to_string))
            .collect();
        // One fold, where the turn's first process message was.
        assert_eq!(
            keys,
            [
                Some("turn_a:L0:p0".to_string()),
                Some("turn_a:fold".to_string()),
                Some("turn_a:footer".to_string()),
                Some("turn_a:notice:tu_1".to_string()),
                None
            ]
        );
    }
}
