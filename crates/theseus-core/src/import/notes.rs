//! The durable-versus-run-state rule for an agent's own notes
//! (`b4-theseus-holds-its-own-notes`, row B4): which of them enters Theseus's
//! memory and which stays out. The rule in words, its two numbers, what it
//! gets wrong and what it leaves open are `docs/design/agent-notes-rule.md`;
//! this module is its one implementation, and the document is the thing the
//! owner reads before the bulk pass runs.
//!
//! **A note enters memory when what it says is still true after the work that
//! produced it is finished.** It stays out when reading it later tells you
//! only what one run did.
//!
//! Pure: a note's name and its text in, a [`Reading`] out. No model, no
//! store, no clock and no spend, so one note always reads the same way and a
//! reading can be shown to a person before anything is written. [`read`] asks
//! three questions and the first that answers wins:
//!
//! 1. **A dated log** (the name holds a date, or a whole name part is a log
//!    word) is run state whatever it says: tomorrow's log supersedes it, so
//!    the series and not the file is the memory, and the rules file the log
//!    was written against is in the same directory.
//! 2. **A note with [`MIN_RULES`] rule lines or more** ([`rule_line`]: an
//!    imperative or a normative line of at least [`MIN_WORDS`] words that
//!    holds for more than one task and names no single run) is durable,
//!    and the reading says how many it found.
//! 3. **Anything else** is run state, and the reading says which run mark
//!    fired, or that the note states no rule.
//!
//! Who enforces it: [`Gate`], read from `[import.notes]` by
//! `write::import_batch_gated` at `import.episodes`, in the daemon. Nothing
//! a client sends goes round it, and the gateless call sites get
//! [`Gate::off`], so a caller that says nothing writes no note.

use crate::card::evidence::one_task;
use crate::config::import::NotesImportConfig;

use super::episode::{Episode, NOTES_SOURCE};

/// How many rule lines make a note durable.
///
/// Judgement, not measurement, and `docs/design/agent-notes-rule.md` says so:
/// three is the smallest count a run log does not reach by accident. A log's
/// imperative lines are its own next steps ("push it", "rerun the gate"),
/// which [`MIN_WORDS`] takes most of out, but one or two survive a long log.
/// One rule line is not a rules document; three is. Do not tune it against
/// the documents it is about to classify (§"choose the rule before the data").
pub const MIN_RULES: usize = 3;

/// A line's words at fewest, so a heading or a bullet of two or three words
/// counts for nothing. It is `card::evidence::MIN_WORDS`'s own number and its
/// own counting (whitespace-separated words, `evidence.rs:313`), taken from
/// there on purpose: the two rules about an agent's own writing agree on what
/// a thin line is.
pub const MIN_WORDS: usize = crate::card::evidence::MIN_WORDS;

/// A whole name part that makes a note a dated log, beside a date.
const LOG_WORDS: &[&str] = &[
    "log", "logs", "journal", "journals", "daily", "diary", "standup", "worklog",
];

/// The bare verbs a rule line may open with. Closed, and short on purpose: a
/// wider list reads a log's own narration ("ran the gate") as an order.
const IMPERATIVES: &[&str] = &[
    "read", "run", "keep", "use", "make", "write", "never", "always", "avoid", "prefer", "follow",
    "check", "ask", "tell", "put", "leave", "name", "say", "treat", "hold", "pin", "chain",
    "commit", "push", "merge", "rebase", "claim", "measure", "count", "extract", "split", "state",
    "quote", "report", "declare", "do", "don't", "stop", "start", "ensure", "refuse", "require",
];

/// What makes a line normative wherever it stands in the line.
const NORMATIVE: &[&str] = &[
    "must",
    "must not",
    "never",
    "always",
    "only",
    "should",
    "do not",
    "don't",
    "cannot",
    "can't",
    "by default",
    "is required",
    "are required",
    "every",
    "each",
    "prefer",
    "the rule is",
];

/// What a note is read as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// What it says is still true after the work that produced it: it enters.
    Durable,
    /// It tells you what one run did: it stays out.
    RunState,
}

impl Verdict {
    /// The word a refusal and a dry run print.
    pub fn word(self) -> &'static str {
        match self {
            Verdict::Durable => "durable",
            Verdict::RunState => "run state",
        }
    }
}

/// One note, read: the verdict, the reason in words, and the rule lines
/// counted (so a reader can tell a note that reached the floor from one that
/// passed it widely).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reading {
    pub verdict: Verdict,
    /// Why, in the words every surface prints.
    pub why: String,
    /// The rule lines found, counted even for a dated log, so a reading never
    /// hides that a log held rules.
    pub rules: usize,
}

impl Reading {
    /// The one line a refusal and a dry run both carry.
    pub fn line(&self) -> String {
        format!("{} ({})", self.verdict.word(), self.why)
    }
}

/// Whether `part` is a date, or begins with one (`2026-11-09`,
/// `2026-11-09-1432`): ten characters of `YYYY-MM-DD` shape.
fn dated(part: &str) -> bool {
    let b = part.as_bytes();
    b.len() >= 10
        && b[..10].iter().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                *c == b'-'
            } else {
                c.is_ascii_digit()
            }
        })
}

/// Whether a note's name makes it a dated log: any whole part of its path is
/// a date or a log word. Whole parts only, so `logging.md` and `dialogue.md`
/// are not logs.
pub fn dated_log(name: &str) -> Option<String> {
    for part in name.split(['/', '\\']).filter(|p| !p.is_empty()) {
        let stem = part.split('.').next().unwrap_or("").to_lowercase();
        if dated(&stem) {
            return Some(format!("a dated log: {stem}"));
        }
        if LOG_WORDS.contains(&stem.as_str()) {
            return Some(format!("a log: {stem}"));
        }
    }
    None
}

/// Whether `phrase` stands in `lower` as whole words. `card::evidence` has
/// the same predicate for the owner card's lines; it is private there and
/// this is three lines, so it is written again rather than the other file
/// widened for a second reader.
fn has_phrase(lower: &str, phrase: &str) -> bool {
    lower.match_indices(phrase).any(|(i, _)| {
        let before = lower[..i].chars().next_back();
        let after = lower[i + phrase.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

/// A token of a line, lower case, punctuation off its ends.
fn words(line: &str) -> Vec<String> {
    line.split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'' && c != '/' && c != '#')
                .to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

/// Whether a line names one run of one task: a branch-shaped path, an issue
/// or pull-request number, a session or execution id, a commit-shaped hex
/// token, or a date. A rule holds past the run it was written in, so a line
/// that names one is evidence about that run and not a rule.
pub fn names_a_run(line: &str) -> bool {
    for w in words(line) {
        if dated(&w) {
            return true;
        }
        if w.starts_with("lane/") || w.starts_with("cloud/") || w.starts_with("wt/") {
            return true;
        }
        if let Some(n) = w.strip_prefix('#') {
            if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) {
                return true;
            }
        }
        if w.starts_with("ses_") || w.starts_with("exe_") || w.starts_with("ep_") {
            return true;
        }
        // A commit-shaped token: eight hex digits or more, and at least one
        // of them not a decimal digit, so a plain number is not one.
        if w.len() >= 8
            && w.chars().all(|c| c.is_ascii_hexdigit())
            && w.chars().any(|c| c.is_ascii_alphabetic())
        {
            return true;
        }
    }
    false
}

/// Whether a line tells you what to do or what holds, past one run: an
/// imperative or a normative line of [`MIN_WORDS`] words or more that holds
/// for more than one task and names no single run.
pub fn rule_line(line: &str) -> bool {
    let trimmed = line.trim().trim_start_matches(['-', '*', '>', '#', ' ']);
    let ws = words(trimmed);
    if ws.len() < MIN_WORDS {
        return false;
    }
    let lower = trimmed.to_lowercase();
    let first = ws.first().map(String::as_str).unwrap_or("");
    let normative = IMPERATIVES.contains(&first) || NORMATIVE.iter().any(|p| has_phrase(&lower, p));
    normative && !one_task(trimmed) && !names_a_run(trimmed)
}

/// One note, read: its name and its whole text.
pub fn read(name: &str, text: &str) -> Reading {
    let rules = text.lines().filter(|l| rule_line(l)).count();
    if let Some(why) = dated_log(name) {
        return Reading {
            verdict: Verdict::RunState,
            why,
            rules,
        };
    }
    if rules >= MIN_RULES {
        return Reading {
            verdict: Verdict::Durable,
            why: format!("{rules} rule lines"),
            rules,
        };
    }
    let why = if rules == 0 {
        "it states no rule".to_string()
    } else {
        format!("{rules} rule lines, under the {MIN_RULES} a rules note needs")
    };
    Reading {
        verdict: Verdict::RunState,
        why,
        rules,
    }
}

/// The design document the refusals name, so the owner reading one knows
/// where the rule is written down.
pub const RULE_DOC: &str = "docs/design/agent-notes-rule.md";

/// `[import.notes]`, as the import reads it: whether a note is written at
/// all, and whether a run-state one is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Gate {
    enabled: bool,
    keep_run_state: bool,
}

impl Gate {
    /// Both switches off: every `agent-notes` episode is refused with its
    /// reading, which is the dry run. It is what a caller that names no
    /// config gets, so the safe state is the one nobody has to remember.
    pub fn off() -> Self {
        Self::default()
    }

    /// The owner's own switches.
    pub fn of(cfg: &NotesImportConfig) -> Self {
        Self {
            enabled: cfg.enabled,
            keep_run_state: cfg.keep_run_state,
        }
    }
}

/// What a notes episode's note is, for the rule: its name is its place's
/// (`place.kind` is `file` and `place.name` the note's path, as the exporter
/// writes it), and its text is its messages' text in order. An episode with
/// no place name is read by its id, which no rule line can match, so it
/// reads as run state rather than as a note that states rules.
pub fn note_of(e: &Episode) -> (String, String) {
    let name = e.place.name.clone().unwrap_or_else(|| e.episode_id.clone());
    let mut text = String::new();
    for m in &e.messages {
        text.push_str(&m.text);
        text.push('\n');
    }
    (name, text)
}

/// Why `e` is not written now, or `None` when it may be. Only an
/// `agent-notes` episode is this rule's business: every other source passes
/// untouched, so the switch cannot hold up the OpenClaw sync or a harness
/// import.
pub fn refused(e: &Episode, gate: &Gate) -> Option<String> {
    if e.source != NOTES_SOURCE {
        return None;
    }
    let (name, text) = note_of(e);
    let r = read(&name, &text);
    if !gate.enabled {
        return Some(format!(
            "[import.notes] enabled is off, so no agent note is written: the import waits on the \
             owner's sight of the durable-versus-run-state rule ({RULE_DOC}). This note reads as \
             {}, and nothing was written.",
            r.line()
        ));
    }
    if r.verdict == Verdict::RunState && !gate.keep_run_state {
        return Some(format!(
            "the durable-versus-run-state rule ({RULE_DOC}) reads this note as {}, and \
             [import.notes] keep_run_state is off.",
            r.line()
        ));
    }
    None
}

#[cfg(test)]
mod tests;
