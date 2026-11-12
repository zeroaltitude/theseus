//! A large tool payload, flagged where it enters and compiled out of the
//! context to a marker (theseus-czpp, row B4).
//!
//! A `proc.run` that prints a megabyte used to eat the window twice over:
//! the result cap ([`crate::config::ToolsConfig::result_max_chars`], 30,000
//! characters by default) already keeps the whole output at
//! `<state>/outputs/<session>/<call>.out` and shows the model a head and a
//! tail of it (theseus-v73m), but 30,000 characters of head and tail is
//! still about 8,000 tokens of a 30,000-token bound, and most of it is
//! scrollback the model will never cite. So an output whose *whole* size
//! passes `[tools] large_payload_chars` is a **large payload**: it is given a
//! guid where it enters, its bytes stay where the cap already keeps them,
//! and every compile of its session renders it as one marker line instead of
//! the capped text.
//!
//! **Explicit rehydration, never automatic re-inclusion** (the owner,
//! 2026-11-12 14:33:10, point 1: "part a explicit rehydration with a
//! model-informative rehydration description the model can reason about",
//! and at 14:33:44: "a model-informative rehydration marker next to the
//! guid, so that the model can know about how to decide whether to examine
//! it and when"). Nothing puts the bytes back in a request on its own. The
//! marker carries the guid **and** a description written for the model to
//! decide on: what kind of payload it is, how big it is, what tool it came
//! from, what its first and last lines are, and when it would be worth
//! reading. The rehydration verb is the one that already reads a kept
//! output, `fs_read` of the path the marker names, which
//! [`crate::outputs::Outputs::own_read`] already judges as a read inside the
//! roots — so this row adds no tool, no schema and no new approval path.
//!
//! **The marker's bytes are frozen.** [`Marked::line`] is a pure function of
//! the stored record, so every compile of a session renders the same bytes
//! and a payload in the cached prefix never moves the cache.
//!
//! The bytes live as long as any kept output does: the outputs sweep
//! (`[tools] outputs_keep_days`, `outputs_max_bytes`, and a retired
//! session's copies) still bounds them, so a marker can outlive its bytes.
//! The marker says the path and the model learns that the read failed; a
//! store that outlives the sweep is a follow-up, not this row.

use serde::{Deserialize, Serialize};

use crate::narrative::thousands;

/// How many characters of the payload's first and last line the marker shows.
const EDGE_CHARS: usize = 160;

/// What kind of thing a large payload is, which decides the words that say
/// when it is worth reading. From the tool that produced it, never guessed
/// from the bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PayloadKind {
    /// A program's output: `proc.run`, a batch's step.
    Program,
    /// A terminal's screen: `term.read`.
    Screen,
    /// A file's or a page's text: every other tool.
    Text,
}

impl PayloadKind {
    /// The kind of a result of `tool`.
    pub fn of_tool(tool: &str) -> Self {
        match tool {
            "proc.run" => Self::Program,
            crate::term::READ => Self::Screen,
            _ => Self::Text,
        }
    }

    /// What the payload is, in the marker's own words.
    fn noun(self) -> &'static str {
        match self {
            Self::Program => "a program's whole output",
            Self::Screen => "a terminal's whole screen",
            Self::Text => "the whole text a tool returned",
        }
    }

    /// When it would be worth reading, which is the part of the marker the
    /// model is meant to reason about.
    fn worth(self) -> &'static str {
        match self {
            Self::Program => {
                "Read it when you need a name, a figure or an error from inside the run; a \
                 program usually says how it ended in its last lines, so read from the end \
                 first, and search it before you read it whole"
            }
            Self::Screen => {
                "Read it when you need something the screen scrolled past; the newest output is \
                 at the end"
            }
            Self::Text => {
                "Read it when you need a passage of it; read the range you need rather than the \
                 whole, and search it first when you are looking for one thing"
            }
        }
    }
}

/// A large payload, as the result node records it (store format 59). Minted
/// by [`crate::toolrun::ToolRuntime::capped`] where the output enters and
/// never written again: the marker's bytes are this record's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marked {
    /// The payload's guid: `pay_` and 32 hex characters, as every id here is.
    pub guid: String,
    pub kind: PayloadKind,
    /// The tool whose call produced it, as the model named it.
    pub tool: String,
    /// Characters of the whole payload.
    pub chars: u64,
    /// Lines of the whole payload.
    pub lines: u64,
    /// Where its bytes are kept, which `fs_read` reads.
    pub at: String,
    /// Its first line, cut to [`EDGE_CHARS`].
    pub first: String,
    /// Its last non-empty line, cut to [`EDGE_CHARS`].
    pub last: String,
}

impl Marked {
    /// A payload of `text` from a call of `tool`, kept at `at`.
    pub fn of(tool: &str, text: &str, at: &str) -> Self {
        let lines: Vec<&str> = text.lines().collect();
        let first = lines.iter().find(|l| !l.trim().is_empty()).unwrap_or(&"");
        let last = lines
            .iter()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or(&"");
        Self {
            guid: crate::new_id("pay"),
            kind: PayloadKind::of_tool(tool),
            tool: tool.to_string(),
            chars: text.chars().count() as u64,
            lines: lines.len() as u64,
            at: at.to_string(),
            first: edge(first),
            last: edge(last),
        }
    }

    /// The marker the compile renders in the payload's place: the guid, then
    /// the description beside it. A pure function of this record, so a
    /// session's every compile renders the same bytes.
    pub fn line(&self) -> String {
        let same = self.lines <= 1 || self.first == self.last;
        let edges = match (self.first.is_empty(), same) {
            (true, _) => String::new(),
            (false, true) => format!(" Its one line reads: \"{}\".", self.first),
            (false, false) => format!(
                " It begins \"{}\" and ends \"{}\".",
                self.first, self.last
            ),
        };
        format!(
            "[Large output held out of this request. Its id is {guid}: {noun} from your \
             {tool} call, {chars} characters over {lines} lines, kept whole at {at}.{edges} \
             {worth}. Nothing of it is in this request: `fs_read` of {at} with `offset` and \
             `limit` reads it in ranges, and `proc_run` of a search over that path finds a line \
             in it without reading it all.]",
            guid = self.guid,
            noun = self.kind.noun(),
            tool = self.tool,
            chars = thousands(self.chars),
            lines = thousands(self.lines),
            at = self.at,
            edges = edges,
            worth = self.kind.worth(),
        )
    }
}

/// `line` cut to [`EDGE_CHARS`] characters, with its whitespace trimmed and
/// its quotes and newlines made safe to sit inside the marker's own quotes.
fn edge(line: &str) -> String {
    let t = line.trim();
    let cut = match t.char_indices().nth(EDGE_CHARS) {
        Some((i, _)) => format!("{}…", &t[..i]),
        None => t.to_string(),
    };
    cut.replace(['\n', '\r'], " ").replace('"', "'")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The marker carries the guid and every part of the description the
    /// owner asked for: what it is, how big it is, where it came from, what
    /// it likely holds, and when to read it.
    #[test]
    fn the_marker_carries_its_guid_and_its_description() {
        let text = format!("first line\n{}\nlast line\n", "middle\n".repeat(100));
        let m = Marked::of("proc.run", &text, "/s/outputs/ses_x/toolu_1.out");
        let line = m.line();
        assert!(m.guid.starts_with("pay_"), "{}", m.guid);
        assert!(line.contains(&m.guid), "no guid: {line}");
        assert!(line.contains("a program's whole output"), "no kind: {line}");
        assert!(line.contains("proc.run"), "no origin: {line}");
        assert!(line.contains("characters over"), "no size: {line}");
        assert!(line.contains("first line"), "no first line: {line}");
        assert!(line.contains("last line"), "no last line: {line}");
        assert!(line.contains("Read it when you need"), "no worth: {line}");
        assert!(
            line.contains("/s/outputs/ses_x/toolu_1.out"),
            "no path: {line}"
        );
        assert!(line.contains("fs_read"), "no rehydration verb: {line}");
    }

    /// The marker is short: it is the whole point of the row that a megabyte
    /// costs a line.
    #[test]
    fn a_megabyte_renders_as_under_a_thousand_characters() {
        let text = "x".repeat(1024 * 1024);
        let m = Marked::of("proc.run", &text, "/s/outputs/ses_x/toolu_1.out");
        assert_eq!(m.chars, 1024 * 1024);
        let n = m.line().chars().count();
        assert!(n < 1_000, "the marker is {n} characters: {}", m.line());
    }

    /// The bytes are frozen: the same record renders the same line.
    #[test]
    fn the_markers_bytes_are_the_records() {
        let m = Marked::of("term.read", "a\nb\nc\n", "/s/outputs/ses_x/t.out");
        assert_eq!(m.line(), m.line());
        assert_eq!(m.kind, PayloadKind::Screen);
        let back: Marked = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
        assert_eq!(back.line(), m.line());
    }

    /// A one-line payload says so rather than quoting itself twice.
    #[test]
    fn a_one_line_payload_is_quoted_once() {
        let m = Marked::of("fs.read", "the whole thing on one line", "/s/o/x.out");
        let line = m.line();
        assert!(line.contains("Its one line reads:"), "{line}");
        assert_eq!(m.kind, PayloadKind::Text);
    }

    /// A line long enough to be a payload of its own is cut, and a quote in
    /// it cannot close the marker's own.
    #[test]
    fn a_long_first_line_is_cut_and_its_quotes_are_made_safe() {
        let text = format!("{}\"and a quote\n second\n", "y".repeat(400));
        let m = Marked::of("proc.run", &text, "/s/o/x.out");
        assert!(m.first.ends_with('…'), "{}", m.first);
        assert_eq!(m.first.chars().count(), EDGE_CHARS + 1);
        assert!(!m.first.contains('"'), "{}", m.first);
    }
}
