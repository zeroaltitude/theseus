//! The words a captured directive keeps, and how two of them compare.
//!
//! `directive.v1` answers choices and probabilities, not text, so the words
//! kept are the owner's own, normalised to one line and cut at a size no
//! standing line should pass. Two lines are the same directive when their
//! content words overlap (words of three letters or more, less a few
//! connectives: most of the shorter line's words are in the other, and a
//! third of all the words are shared): a size for a similarity, not a
//! reading of what a sentence means. The pack decides *that* a message is a directive and of
//! what kind; this decides only whether it repeats one already held.

use std::collections::BTreeSet;

/// The longest line kept, in chars: a record rides every prompt of its scope.
pub const MAX_LINE: usize = 240;

/// The share of the shorter line's content words the other must hold, and
/// the share of all the words the two must have in common, for the same
/// directive said twice.
pub const SAME_OF_SHORTER: f64 = 0.6;
pub const SAME_OF_ALL: f64 = 0.3;

const CONNECTIVES: [&str; 12] = [
    "the", "and", "for", "with", "that", "this", "you", "your", "are", "but", "can", "please",
];

/// The message as one line: its whitespace folded, its ends trimmed. None
/// when it is empty or past [`MAX_LINE`] (a long message is not a line to
/// keep as it stands, and nothing here rewrites the owner's words).
pub fn line(text: &str) -> Option<String> {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let n = line.chars().count();
    (n > 0 && n <= MAX_LINE).then_some(line)
}

fn content(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '\'')
        .map(|w| w.trim_matches('\'').to_lowercase())
        .filter(|w| w.chars().count() >= 3 && !CONNECTIVES.contains(&w.as_str()))
        .collect()
}

/// Whether two lines say the same thing, as far as their words show.
pub fn same(a: &str, b: &str) -> bool {
    let (a, b) = (content(a), content(b));
    let shared = a.intersection(&b).count();
    let all = a.union(&b).count();
    let shorter = a.len().min(b.len());
    if shared == 0 {
        return false;
    }
    shared as f64 / shorter as f64 >= SAME_OF_SHORTER && shared as f64 / all as f64 >= SAME_OF_ALL
}
