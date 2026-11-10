//! The in-loop check for a model thanking the owner for his own standing
//! record (theseus-ectzg, ask 3's second half).
//!
//! `standing::FRAMING` tells the model the records are carried on every
//! request and are never to be acknowledged. A model that still opens with
//! "Noted: in group chats I speak as myself…" is caught here, on the answer
//! the loop just got, with no model call: a prefix test, and a comparison of
//! words with the records the request carried.
//!
//! **Strip or flag.** The paragraph is stripped only when all of these hold,
//! and flagged (recorded, left in place) otherwise:
//!
//! - it is the answer's *first* paragraph and opens with `Noted` or
//!   `Understood` (the two openings the owner saw, no others);
//! - it is short (at most [`MAX_CHARS`]) and most of its content words
//!   ([`RESTATES`]) are words of one or more records the request showed, at
//!   least [`MIN_SHARED`] of them: "nothing in it but a restatement" is
//!   decided by that share, not by meaning, so a paragraph that also says
//!   something else falls under the share and is left alone entirely;
//! - the owner's last message does not carry those shared words (fewer than
//!   [`MIN_SHARED`] of them): if he just said it, "Noted" is a reply;
//! - something follows it. An answer that is only the acknowledgement is
//!   flagged, never emptied.
//!
//! Either way the caller records the ids and the paragraph, so the original
//! words are never lost and the owner can see how often it happens.

use std::collections::BTreeSet;

/// The longest paragraph the check will strip.
pub const MAX_CHARS: usize = 400;
/// The fewest content words a paragraph must share with the records.
pub const MIN_SHARED: usize = 3;
/// The share of a paragraph's content words that are record words, at least,
/// for it to count as a restatement (per mille).
pub const RESTATES: usize = 400;

/// The openings the check knows: the two the owner saw, and no others.
pub(crate) const MARKERS: [&str; 2] = ["Noted", "Understood"];

/// The cheap pre-filter in front of [`check`]: the answer opens with one of
/// [`MARKERS`]. The step iterates the one list through this, so a marker added
/// to it is never rejected before `check` runs.
pub(crate) fn opens_with_marker(answer: &str) -> bool {
    MARKERS.iter().any(|m| answer.trim_start().starts_with(m))
}

/// Words too common to show a restatement.
const STOP: &[&str] = &[
    "this", "that", "with", "from", "have", "will", "your", "here", "there", "then", "than",
    "they", "them", "what", "when", "where", "which", "while", "would", "could", "should", "about",
    "into", "also", "just", "been", "being", "does", "only", "more", "some", "such", "very",
    "keep",
];

/// What the check made of an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// The record ids the paragraph restates, in the order given.
    pub ids: Vec<String>,
    /// The leading paragraph, as written.
    pub paragraph: String,
    /// The answer without it, when it was stripped; none when only flagged.
    pub stripped: Option<String>,
}

fn words(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !c.is_alphabetic())
        .map(str::to_lowercase)
        .filter(|w| w.chars().count() >= 4 && !STOP.contains(&w.as_str()))
        .map(|w| {
            let w = w.strip_suffix('s').unwrap_or(&w).to_string();
            w.chars().take(5).collect()
        })
        .collect()
}

/// The leading paragraph and the rest, when the answer opens with a marker.
fn leading(answer: &str) -> Option<(&str, &str)> {
    let t = answer.trim_start();
    let m = MARKERS.iter().find(|m| {
        t.strip_prefix(**m)
            .is_some_and(|r| r.starts_with([':', '.', ',', '!', ' ', '\u{2014}']))
    })?;
    let _ = m;
    let (first, rest) = match t.find("\n\n") {
        Some(i) => (&t[..i], t[i..].trim_start()),
        None => (t, ""),
    };
    Some((first, rest))
}

/// Check `answer` against the `records` (id, text) the request showed and
/// the `owner` text just sent. `None` when nothing is restated.
pub fn check(answer: &str, records: &[(String, String)], owner: &str) -> Option<Found> {
    let (first, rest) = leading(answer)?;
    if records.is_empty() {
        return None;
    }
    let p = words(first);
    if p.is_empty() {
        return None;
    }
    let mut shared: BTreeSet<&String> = BTreeSet::new();
    let mut ids = Vec::new();
    for (id, text) in records {
        let r = words(text);
        let hit: Vec<&String> = p.iter().filter(|w| r.contains(*w)).collect();
        if hit.len() >= MIN_SHARED {
            ids.push(id.clone());
        }
        shared.extend(hit);
    }
    if ids.is_empty() || shared.len() < MIN_SHARED || shared.len() * 1000 < p.len() * RESTATES {
        return None;
    }
    // He just said it: "Noted" answers him.
    let o = words(owner);
    if shared.iter().filter(|w| o.contains(**w)).count() >= MIN_SHARED {
        return None;
    }
    let stripped =
        (!rest.is_empty() && first.chars().count() <= MAX_CHARS).then(|| rest.to_string());
    Some(Found {
        ids,
        paragraph: first.to_string(),
        stripped,
    })
}

/// The records a standing block lists, `(id, text)`, read from its lines
/// (`- <id>: <text>`) for the ids it showed.
pub fn records_of(texts: &[&str], shown: &[&str]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for text in texts {
        for line in text.lines() {
            let Some(rest) = line.strip_prefix("- ") else {
                continue;
            };
            let Some((id, body)) = rest.split_once(": ") else {
                continue;
            };
            if shown.contains(&id) {
                out.push((id.to_string(), body.to_string()));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recs() -> Vec<(String, String)> {
        vec![(
            "1f2de1b3".into(),
            "In group chats, speak as yourself (Theseus)\u{2026} when something needs his voice \
             or decision, say so and leave it to him"
                .into(),
        )]
    }

    const NOTED: &str = "Noted: in group chats I speak as myself, and anything that needs \
        your voice or decision goes back to you. This is a DM, so nothing changes here.";

    #[test]
    fn a_leading_acknowledgement_of_a_record_the_owner_did_not_just_send_is_stripped() {
        let a = format!("{NOTED}\n\nThe disk is at 41%.");
        let f = check(&a, &recs(), "how full is the disk?").expect("found");
        assert_eq!(f.ids, ["1f2de1b3"]);
        assert_eq!(f.stripped.as_deref(), Some("The disk is at 41%."));
        assert!(f.paragraph.starts_with("Noted: in group chats"));
    }

    #[test]
    fn an_answer_that_is_only_the_acknowledgement_is_flagged_not_emptied() {
        let f = check(NOTED, &recs(), "hi").expect("found");
        assert_eq!(f.stripped, None);
    }

    #[test]
    fn a_noted_that_answers_what_the_owner_just_said_is_left_alone() {
        let owner = "In group chats, speak as yourself; if it needs my voice or decision, say so.";
        let a = format!("{NOTED}\n\nDone.");
        assert_eq!(check(&a, &recs(), owner), None);
        // The port case: nothing to do with any record.
        assert_eq!(check("Noted: 8082.", &recs(), "the port is 8082"), None);
        assert_eq!(check("Noted: 8082.\n\nMore.", &recs(), "x"), None);
    }

    #[test]
    fn only_a_leading_marker_counts() {
        let a = format!("The disk is at 41%.\n\n{NOTED}");
        assert_eq!(check(&a, &recs(), "x"), None);
        assert_eq!(check("The disk is at 41%.", &recs(), "x"), None);
        assert_eq!(
            check("Notedly, group chats speak voice decision.", &recs(), "x"),
            None
        );
    }

    #[test]
    fn a_paragraph_with_something_else_in_it_is_not_a_restatement() {
        let a = "Noted: group chats, speak, voice. The deploy failed at step nine because the \
                 certificate expired, and the retry budget is spent.\n\nMore.";
        assert_eq!(check(a, &recs(), "x"), None);
    }

    #[test]
    fn records_are_read_from_the_blocks_lines() {
        let block = "Standing: head\nPreferences:\n- aa11: one\n- bb22: two";
        let r = records_of(&[block], &["bb22"]);
        assert_eq!(r, [("bb22".to_string(), "two".to_string())]);
    }
}
