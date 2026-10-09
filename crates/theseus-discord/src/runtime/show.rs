//! What a place shows (theseus-l1y1): its tool messages and its thinking
//! messages, each unless its binding says otherwise (`show_tools`,
//! `show_thinking` on a `[[channel]]` or a `[[dm]]`, both on by default).
//! The owner wants a lively chat, replete with both; a place for someone who
//! does not can hide either. Off hides those messages in that place alone:
//! the turn runs as ever, its cards and reply still post, and every other
//! place is as it was. What is hidden is never sent to the lane, so no
//! message is made for it.

use theseus_protocol::Event;

use crate::bindings::{ChannelBinding, DmBinding};
use crate::policy::THINK_SUFFIX;
use crate::render::thinking::Thinking;
use crate::render::Op;

/// A place's two words, and its thinking messages while it shows them.
#[derive(Default)]
pub(crate) struct View {
    tools: bool,
    thinking: bool,
    think: Thinking,
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
            think: Thinking::default(),
        }
    }

    /// The bindings file changed the place's words (theseus-ocwt).
    pub(crate) fn set(&mut self, s: Show) {
        self.tools = s.tools;
        self.thinking = s.thinking;
    }

    /// The thinking messages one event makes, while the place shows them.
    /// The turns are followed either way, so a place shown thinking again
    /// holds the renderer's turns.
    pub(crate) fn on_event(&mut self, e: &Event) -> Vec<Op> {
        let ops = self.think.on_event(e);
        if self.thinking {
            ops
        } else {
            vec![]
        }
    }

    /// The thinking that grew since the last tick.
    pub(crate) fn tick(&mut self) -> Vec<Op> {
        let ops = self.think.tick();
        if self.thinking {
            ops
        } else {
            vec![]
        }
    }

    /// Does the place show `op`? A tool line (`<turn>:L<n>:tools`) and a
    /// call's notice embed only under `show_tools`, a thinking message only
    /// under `show_thinking`; everything else always.
    pub(crate) fn shows(&self, op: &Op) -> bool {
        match op {
            Op::Typing => true,
            Op::Notice { .. } => self.tools,
            Op::Upsert { key, .. } if key.ends_with(":tools") => self.tools,
            Op::Upsert { key, .. } if key.ends_with(THINK_SUFFIX) => self.thinking,
            Op::Upsert { .. } => true,
        }
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
        let all = [
            up("turn_a:L0:p0"),
            up("turn_a:L0:tools"),
            up("turn_a:L0:think"),
            up("turn_a:footer"),
            notice(),
            Op::Typing,
        ];
        let shown = |v: &View| -> Vec<bool> { all.iter().map(|o| v.shows(o)).collect() };
        let on = View::new(Show::default());
        assert_eq!(shown(&on), [true; 6]);
        let no_tools = View::new(Show {
            tools: false,
            thinking: true,
        });
        assert_eq!(shown(&no_tools), [true, false, true, true, false, true]);
        let no_thinking = View::new(Show {
            tools: true,
            thinking: false,
        });
        assert_eq!(shown(&no_thinking), [true, true, false, true, true, true]);
    }

    /// Thinking off makes no thinking message, and on again shows the
    /// thinking of the turn it still follows.
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
        assert_eq!(ops[0].key(), Some("turn_a:L0:think"));
    }
}
