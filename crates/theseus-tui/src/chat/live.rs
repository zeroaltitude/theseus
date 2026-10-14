//! What is happening now, which only the dock shows: the turn that runs and
//! the calls in flight. A call is a dock row while it runs and one line of
//! the scrollback once it ends; nothing here is ever drawn twice.

use serde_json::Value;

/// What the turn is doing, in the dock's word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Thinking,
    Writing,
    /// The turn parked on a question for the operator.
    Waiting,
}

impl Phase {
    pub fn word(self) -> &'static str {
        match self {
            Phase::Thinking => "thinking",
            Phase::Writing => "writing",
            Phase::Waiting => "waiting for you",
        }
    }
}

#[derive(Debug, Clone)]
pub struct TurnLive {
    pub id: String,
    pub since_ms: u64,
    pub phase: Phase,
}

/// One call in flight (or a background job still running).
#[derive(Debug, Clone)]
pub struct LiveTool {
    pub id: String,
    pub tool: String,
    pub input: Value,
    pub since_ms: u64,
    /// The last line of its output so far, when something supplies one.
    pub last: Option<String>,
    /// It went to the background: its turn is on, the job is not.
    pub background: bool,
}

#[derive(Debug, Default)]
pub struct Live {
    pub turn: Option<TurnLive>,
    pub tools: Vec<LiveTool>,
}

impl Live {
    /// Whether anything moves (and so the dock needs its clock).
    pub fn active(&self) -> bool {
        self.turn.is_some() || !self.tools.is_empty()
    }
}

/// `0:03`, `12:07`, `1:02:09`: an elapsed time as a person reads a timer.
pub fn elapsed(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// The spinner's frame at `now_ms`: four a second, so the dock moves each
/// time its clock ticks.
pub fn spinner(now_ms: u64) -> char {
    ['◐', '◓', '◑', '◒'][(now_ms / 100 % 4) as usize]
}

/// `1.2 s`, `340 ms`, `2 m 3 s`: a duration in words, for a turn's line.
pub fn took(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms} ms")
    } else if ms < 60_000 {
        format!("{:.1} s", ms as f64 / 1000.0)
    } else {
        format!("{} m {} s", ms / 60_000, ms / 1000 % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_timer_reads_as_a_timer() {
        assert_eq!(elapsed(3_400), "0:03");
        assert_eq!(elapsed(34_000), "0:34");
        assert_eq!(elapsed(727_000), "12:07");
        assert_eq!(elapsed(3_729_000), "1:02:09");
    }

    #[test]
    fn a_spinner_has_four_frames_and_a_duration_has_words() {
        let frames: std::collections::HashSet<char> = (0..4).map(|i| spinner(i * 100)).collect();
        assert_eq!(frames.len(), 4);
        assert_eq!(took(340), "340 ms");
        assert_eq!(took(1234), "1.2 s");
        assert_eq!(took(123_000), "2 m 3 s");
    }
}
