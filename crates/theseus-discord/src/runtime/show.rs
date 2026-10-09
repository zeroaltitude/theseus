//! What a place shows (theseus-l1y1): its tool lines and its thinking, each
//! unless its binding says otherwise (`show_tools`, `show_thinking` on a
//! `[[channel]]` or a `[[dm]]`, both on by default). The owner wants a lively
//! chat, replete with both; a place for someone who does not can hide
//! either. Both live in a loop's process message (`render/process.rs`), its
//! thinking at the top: off hides that half in that place alone, and a
//! message with neither half is never made. The turn runs as ever, its cards
//! and reply still post, and every other place is as it was.

use theseus_protocol::Event;

use crate::bindings::{ChannelBinding, DmBinding};
use crate::render::process::{Process, Shown};
use crate::render::Op;

/// A place's two words, and its loops' process messages.
#[derive(Default)]
pub(crate) struct View {
    tools: bool,
    thinking: bool,
    process: Process,
}

/// A place's words, as its binding gives them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Show {
    pub tools: bool,
    pub thinking: bool,
}

impl Show {
    pub(crate) fn of_channel(c: &ChannelBinding) -> Self {
        Self {
            tools: c.show_tools,
            thinking: c.show_thinking,
        }
    }

    pub(crate) fn of_dm(d: &DmBinding) -> Self {
        Self {
            tools: d.show_tools,
            thinking: d.show_thinking,
        }
    }
}

impl Default for Show {
    fn default() -> Self {
        Self {
            tools: true,
            thinking: true,
        }
    }
}

impl View {
    pub(crate) fn new(s: Show) -> Self {
        Self {
            tools: s.tools,
            thinking: s.thinking,
            process: Process::default(),
        }
    }

    /// The bindings file changed the place's words (theseus-ocwt).
    pub(crate) fn set(&mut self, s: Show) {
        self.tools = s.tools;
        self.thinking = s.thinking;
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
        };
        assert_eq!(shown(no_thinking), keys(&every));
        let no_tools = Show {
            tools: false,
            thinking: true,
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
}
