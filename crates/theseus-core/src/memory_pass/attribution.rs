//! Attribution (M6 §2.6, step 31a): whether a recalled item was used, and
//! later how it turned out, deterministically. Pure: the pass reads the
//! turn's reply and calls, the item's excerpt, and the entities the tender
//! named, and hands them here.
//!
//! - **Used** when one of the item's entities is in the reply's entities or
//!   a tool call's input's, or a run of [`RUN_WORDS`] of its excerpt's words
//!   is in the reply's.
//! - **Its outcome**, once the session's next turn has its input:
//!   `corrected` when the operator's next message is a correction that
//!   overlaps the item (an entity in common, or [`OVERLAP_WORDS`] content
//!   words); `ok` when the operator's next message goes on otherwise;
//!   `unknown` when the next turn brings no operator message (a wake, a
//!   task's report).

use std::collections::BTreeSet;

use serde::Serialize;

use super::labels::{is_correction, words};

/// The words of a run that make an item used.
pub const RUN_WORDS: usize = 8;
/// The content words in common that make a correction overlap an item.
pub const OVERLAP_WORDS: usize = 2;
/// A content word has at least this many characters, or digits in it.
pub const CONTENT_CHARS: usize = 4;

/// What the turn did with its recall: its reply's text and entities, and
/// its calls' inputs' entities.
pub struct Turned<'a> {
    pub reply: &'a str,
    pub reply_entities: &'a BTreeSet<String>,
    pub call_entities: &'a BTreeSet<String>,
}

/// Why an item counts as used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Use {
    pub used: bool,
    /// `entity:<term>` (the reply's or a call's: `in`), or `run`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub by: Vec<String>,
}

/// Whether the item (its excerpt, and its entities) was used in `t`.
pub fn used(excerpt: &str, entities: &BTreeSet<String>, t: &Turned<'_>) -> Use {
    let mut by: Vec<String> = Vec::new();
    for e in entities {
        if t.reply_entities.contains(e) {
            by.push(format!("entity:{e} in reply"));
        } else if t.call_entities.contains(e) {
            by.push(format!("entity:{e} in a call"));
        }
    }
    if shares_a_run(excerpt, t.reply) {
        by.push("run".into());
    }
    Use {
        used: !by.is_empty(),
        by,
    }
}

/// Whether `reply` holds a run of [`RUN_WORDS`] of `excerpt`'s words.
pub fn shares_a_run(excerpt: &str, reply: &str) -> bool {
    let a = words(excerpt);
    let b = words(reply);
    if a.len() < RUN_WORDS || b.len() < RUN_WORDS {
        return false;
    }
    let runs: BTreeSet<&[String]> = b.windows(RUN_WORDS).collect();
    a.windows(RUN_WORDS).any(|w| runs.contains(w))
}

/// How a used item turned out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Ok,
    Corrected,
    Unknown,
}

/// The session's next input after the turn that used the item.
pub enum Next<'a> {
    /// The operator's message, and its entities.
    Operator(&'a str, &'a BTreeSet<String>),
    /// A turn with no operator message (a wake, a task's report).
    Other,
}

/// The outcome of a used item, from the session's next input.
pub fn outcome(excerpt: &str, entities: &BTreeSet<String>, next: &Next<'_>) -> Outcome {
    match next {
        Next::Other => Outcome::Unknown,
        Next::Operator(text, theirs) => {
            if is_correction(text) && overlaps(excerpt, entities, text, theirs) {
                Outcome::Corrected
            } else {
                Outcome::Ok
            }
        }
    }
}

/// Whether a message overlaps an item: an entity in common, or
/// [`OVERLAP_WORDS`] content words.
pub fn overlaps(
    excerpt: &str,
    entities: &BTreeSet<String>,
    text: &str,
    theirs: &BTreeSet<String>,
) -> bool {
    if entities.intersection(theirs).next().is_some() {
        return true;
    }
    let content = |t: &str| -> BTreeSet<String> {
        words(t)
            .into_iter()
            .filter(|w| w.chars().count() >= CONTENT_CHARS || w.chars().any(|c| c.is_ascii_digit()))
            .collect()
    };
    content(excerpt).intersection(&content(text)).count() >= OVERLAP_WORDS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(xs: &[&str]) -> BTreeSet<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }

    /// Attribution on fixtures: an entity in the reply or a call's input,
    /// an 8-word run of the excerpt in the reply, or neither.
    #[test]
    fn an_item_is_used_by_an_entity_or_a_run_of_its_words() {
        let excerpt = "The staging port of the Larkspur service is 8082 since the move to crates/heron/src/main.rs";
        let item = set(&["path:crates/heron/src/main.rs", "file:main.rs"]);
        let none = BTreeSet::new();
        let reply_e = set(&["file:main.rs"]);
        let t = Turned {
            reply: "It lives in main.rs.",
            reply_entities: &reply_e,
            call_entities: &none,
        };
        let u = used(excerpt, &item, &t);
        assert!(u.used);
        assert_eq!(u.by, ["entity:file:main.rs in reply"]);
        let call_e = set(&["path:crates/heron/src/main.rs"]);
        let t = Turned {
            reply: "Reading it.",
            reply_entities: &none,
            call_entities: &call_e,
        };
        assert_eq!(
            used(excerpt, &item, &t).by,
            ["entity:path:crates/heron/src/main.rs in a call"]
        );
        let t = Turned {
            reply: "As noted, the staging port of the Larkspur service is 8082 today.",
            reply_entities: &none,
            call_entities: &none,
        };
        assert_eq!(used(excerpt, &item, &t).by, ["run"]);
        // Seven words in a row are not a run.
        let t = Turned {
            reply: "the staging port of the Larkspur service was moved",
            reply_entities: &none,
            call_entities: &none,
        };
        assert!(!used(excerpt, &item, &t).used);
    }

    /// The outcome: a correction that overlaps is `corrected`; one that does
    /// not, or any other message, `ok`; no operator message, `unknown`.
    #[test]
    fn a_used_items_outcome_follows_the_next_message() {
        let excerpt = "Larkspur's staging port is 8082";
        let item = BTreeSet::new();
        let none = BTreeSet::new();
        let fix = Next::Operator("Correction: Larkspur's staging port is 8083 now.", &none);
        assert_eq!(outcome(excerpt, &item, &fix), Outcome::Corrected);
        let elsewhere = Next::Operator("Actually, lunch is at noon.", &none);
        assert_eq!(outcome(excerpt, &item, &elsewhere), Outcome::Ok);
        let thanks = Next::Operator("thanks, that worked", &none);
        assert_eq!(outcome(excerpt, &item, &thanks), Outcome::Ok);
        assert_eq!(outcome(excerpt, &item, &Next::Other), Outcome::Unknown);
        // An entity in common is overlap enough.
        let item = BTreeSet::from(["commit:abc1234".to_string()]);
        let theirs = BTreeSet::from(["commit:abc1234".to_string()]);
        let fix = Next::Operator("No, that was wrong: revert abc1234 first.", &theirs);
        assert_eq!(
            outcome("merged as abc1234", &item, &fix),
            Outcome::Corrected
        );
    }
}
