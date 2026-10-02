//! Notices (design `stage2` §2.9, "Seen state and notices"): on transitions
//! only. A session going into needs you says "needs you"; one out of focus
//! going from working to ready or idle says "finished". Each waits out a
//! second and is checked again against the latest view before it fires, and
//! none is given for the session in focus while the terminal has focus. How
//! it reaches the operator is `--notify`'s: the bell (the default), OSC 9,
//! OSC 777, or off.

use std::collections::HashMap;

use theseus_protocol::{ExecutionView, Level};

/// How long a transition must hold before its notice fires.
pub const DEBOUNCE_MS: u64 = 1_000;

/// How a notice reaches the operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Delivery {
    /// The terminal's bell.
    #[default]
    Bell,
    /// OSC 9, a desktop notification (iTerm2, Windows Terminal, kitty).
    Osc9,
    /// OSC 777, a desktop notification with a title (rxvt, foot, Ghostty).
    Osc777,
    Off,
}

impl Delivery {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "bell" => Delivery::Bell,
            "osc9" => Delivery::Osc9,
            "osc777" => Delivery::Osc777,
            "off" => Delivery::Off,
            _ => return None,
        })
    }

    /// The bytes that deliver `text`: none when off. No control character of
    /// the text reaches the terminal's sequence.
    pub fn bytes(self, text: &str) -> Vec<u8> {
        let clean: String = text.chars().filter(|c| !c.is_control()).collect();
        match self {
            Delivery::Bell => b"\x07".to_vec(),
            Delivery::Osc9 => format!("\x1b]9;theseus: {clean}\x07").into_bytes(),
            Delivery::Osc777 => format!("\x1b]777;notify;theseus;{clean}\x07").into_bytes(),
            Delivery::Off => Vec::new(),
        }
    }
}

/// What a notice says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    NeedsYou,
    Finished,
}

impl Kind {
    pub fn words(self) -> &'static str {
        match self {
            Kind::NeedsYou => "needs you",
            Kind::Finished => "finished",
        }
    }

    /// Whether the latest view still says what the notice says.
    fn holds(self, v: &ExecutionView) -> bool {
        match self {
            Kind::NeedsYou => v.attention.level == Level::NeedsYou,
            Kind::Finished => {
                matches!(v.attention.level, Level::Ready | Level::Idle) && v.state != "cancelled"
            }
        }
    }
}

/// The notices waiting out their second, by session.
#[derive(Debug, Default)]
pub struct Notices {
    due: HashMap<String, (Kind, u64)>,
}

impl Notices {
    /// A session's level moved. A notice waiting for the same session is
    /// replaced, and its second starts again; a move that says nothing
    /// leaves it to be checked against the view when its second is up.
    pub fn moved(&mut self, sid: &str, before: Option<Level>, v: &ExecutionView, now_ms: u64) {
        let after = v.attention.level;
        let kind = match (before, after) {
            (Some(Level::NeedsYou), Level::NeedsYou) => None,
            (_, Level::NeedsYou) => Some(Kind::NeedsYou),
            (Some(Level::Working), Level::Ready | Level::Idle) if v.state != "cancelled" => {
                Some(Kind::Finished)
            }
            _ => None,
        };
        if let Some(k) = kind {
            self.due.insert(sid.to_string(), (k, now_ms + DEBOUNCE_MS));
        }
    }

    /// The soonest time a notice is due.
    pub fn next_due(&self) -> Option<u64> {
        self.due.values().map(|(_, at)| *at).min()
    }

    /// The notices due by `now_ms` that still hold: each checked against the
    /// session's latest view (`view`), and dropped for a session the operator
    /// is looking at (`watched`). Each fires once.
    pub fn take_due(
        &mut self,
        now_ms: u64,
        view: &dyn Fn(&str) -> Option<ExecutionView>,
        watched: &dyn Fn(&str) -> bool,
    ) -> Vec<(String, Kind)> {
        let due: Vec<String> = self
            .due
            .iter()
            .filter(|(_, (_, at))| *at <= now_ms)
            .map(|(sid, _)| sid.clone())
            .collect();
        let mut out = Vec::new();
        for sid in due {
            let Some((kind, _)) = self.due.remove(&sid) else {
                continue;
            };
            if !watched(&sid) && view(&sid).is_some_and(|v| kind.holds(&v)) {
                out.push((sid, kind));
            }
        }
        out.sort();
        out
    }
}
