//! A question in plain words (theseus-cudar), worded once by the core so that
//! every surface (the Discord card and its settle, `theseus answer`, the TUI)
//! says the same thing: who asks, what the call does, why, how risky it is,
//! and what Yes and No do. Absent from a daemon that does not say, and from a
//! request the core could not word; a surface then falls back to `reason`.

use serde::{Deserialize, Serialize};

/// The words of a question the owner is asked.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PlainWords {
    /// Who asks: a self-repair item's title, a task's title, or a short tag.
    pub who: String,
    /// What the call does: a verb and a friendly object, never an elided path.
    pub what: String,
    /// The model's own one-line reason for the call, when it gave one.
    #[serde(default)]
    pub why: String,
    /// Why the gate holds the call, in words (the rule that asks).
    #[serde(default)]
    pub held: String,
    /// The risk badge: `read-only`, `writes`, `runs a program`, `network`,
    /// `spends` or `touches secrets`.
    pub risk: String,
    /// What Yes does.
    pub yes: String,
    /// What No does.
    pub no: String,
    /// The daemon's own whole words for a question that is more than a
    /// call: a settings card, with each setting's value before and after.
    /// Empty for an ordinary approval. A surface that has it shows it whole,
    /// never cut, instead of the raw arguments. Left out of the wire when
    /// empty, so an ordinary approval's record is as it was.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

impl PlainWords {
    /// The lines a text surface prints: who and what, the risk and the
    /// model's reason, then what Yes and No do. `held` is left to the
    /// surface that wants the rule.
    pub fn lines(&self) -> Vec<String> {
        let mut out = vec![format!("{} wants to {}", self.who, self.what)];
        let mut badge = format!("risk: {}", self.risk);
        if !self.why.is_empty() {
            badge.push_str(&format!(" · why: {}", self.why));
        }
        out.push(badge);
        out.push(format!("Yes: {} No: {}", self.yes, self.no));
        out
    }

    /// The lines of [`Self::detail`], whole, for a text surface.
    pub fn detail_lines(&self) -> Vec<String> {
        self.detail.lines().map(str::to_string).collect()
    }

    /// One sentence that repeats the whole question: the settle's text keeps
    /// it, so a closed card still says what it was.
    pub fn line(&self) -> String {
        let mut s = format!("{} wanted to {} · {}", self.who, self.what, self.risk);
        if !self.why.is_empty() {
            s.push_str(&format!(" · why: {}", self.why));
        }
        s
    }
}
