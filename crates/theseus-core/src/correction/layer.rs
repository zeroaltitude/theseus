//! The live correction layer (theseus-q31l): the owner's corrections of
//! routing, kept in memory so a new message close to a corrected one runs
//! where the owner said, ahead of the route pack's verdict.
//!
//! - **Close** is word overlap: the share of content words two messages have
//!   in common (Jaccard, over each message's distinct words of three letters
//!   or more, the commonest English words left out), at or above
//!   `[routing.corrections] similarity`. It is read in memory, never from the
//!   index tender, so the turn waits on no socket.
//! - **Bounded**: `[routing.corrections] max_entries`, the oldest out first.
//! - **Retired with its pack.** An entry follows the route pack version that
//!   judged the corrected message; once another version acts (a new
//!   compiled-in version, or one the learning loop rewrote and placed), the
//!   labels are that version's to have learned, and the entry retires.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

/// Where an entry steers a close message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Steer {
    /// The mode's first usable profile, as a verdict of that mode would.
    Mode(String),
    /// The profile itself.
    Profile(String),
}

/// One correction in the layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// The correction's id (`rcx_…`), its `route.corrected` row's key.
    pub id: String,
    /// The owner label it wrote, when the corrected turn had a judgment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The route pack version that judged the corrected message.
    pub pack: String,
    pub session: String,
    /// The corrected turn.
    pub turn: String,
    /// The corrected message's content words, sorted and distinct.
    pub words: Vec<String>,
    pub steer: Steer,
    pub at_ms: u64,
}

/// The most words a message keeps for the layer.
pub const MAX_WORDS: usize = 96;

/// Words too common to tell two messages apart.
const COMMON: [&str; 48] = [
    "the", "and", "for", "that", "this", "with", "you", "your", "are", "was", "were", "have",
    "has", "had", "not", "but", "can", "could", "would", "should", "will", "what", "when", "where",
    "which", "who", "how", "why", "there", "their", "them", "then", "than", "into", "from",
    "about", "just", "also", "some", "any", "all", "its", "it's", "our", "out", "please", "let",
    "lets",
];

/// A message's content words: lower case, three letters or more, the
/// commonest left out, sorted and distinct, at most [`MAX_WORDS`].
pub fn words(text: &str) -> Vec<String> {
    let mut out: Vec<String> = text
        .to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|w| w.chars().count() >= 3 && !COMMON.contains(w))
        .map(str::to_string)
        .collect();
    out.sort_unstable();
    out.dedup();
    out.truncate(MAX_WORDS);
    out
}

/// The share of words two sorted, distinct lists have in common (0 when
/// either is empty).
pub fn similarity(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let (mut i, mut j, mut both) = (0, 0, 0usize);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                both += 1;
                i += 1;
                j += 1;
            }
        }
    }
    both as f64 / (a.len() + b.len() - both) as f64
}

/// The layer: entries oldest first.
#[derive(Debug, Default)]
pub struct Layer {
    entries: VecDeque<Entry>,
    /// Entries retired since the start (their pack no longer acts).
    pub retired: u64,
}

impl Layer {
    /// Add an entry, the oldest out past `max`. A later correction of the
    /// same turn replaces the earlier one.
    pub fn add(&mut self, e: Entry, max: usize) {
        self.entries.retain(|x| x.turn != e.turn);
        self.entries.push_back(e);
        while self.entries.len() > max.max(1) {
            self.entries.pop_front();
        }
    }

    /// Retire every entry of a pack version other than `acting`.
    pub fn retire(&mut self, acting: &str) -> usize {
        let before = self.entries.len();
        self.entries.retain(|e| e.pack == acting);
        let n = before - self.entries.len();
        self.retired += n as u64;
        n
    }

    /// The entry closest to `words` at `threshold` or above, the newest of
    /// equal closeness, after retiring any `acting` has replaced.
    pub fn nearest(
        &mut self,
        words: &[String],
        acting: &str,
        threshold: f64,
    ) -> Option<(Entry, f64)> {
        if self.entries.is_empty() {
            return None;
        }
        self.retire(acting);
        let mut best: Option<(&Entry, f64)> = None;
        for e in self.entries.iter().rev() {
            let s = similarity(words, &e.words);
            if s >= threshold && best.is_none_or(|(_, b)| s > b) {
                best = Some((e, s));
            }
        }
        best.map(|(e, s)| (e.clone(), s))
    }

    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(turn: &str, text: &str, pack: &str) -> Entry {
        Entry {
            id: format!("rcx_{turn}"),
            label: Some(format!("lbl_{turn}")),
            pack: pack.into(),
            session: "ses_a".into(),
            turn: turn.into(),
            words: words(text),
            steer: Steer::Profile("fable".into()),
            at_ms: 1,
        }
    }

    #[test]
    fn close_is_the_share_of_content_words_in_common() {
        let a = words("Write a parser for the lighthouse log format in Rust");
        assert_eq!(
            a,
            ["format", "lighthouse", "log", "parser", "rust", "write"]
        );
        let b = words("write a parser for the harbour log format in rust");
        assert!((similarity(&a, &b) - 5.0 / 7.0).abs() < 1e-9);
        assert_eq!(similarity(&a, &a), 1.0);
        assert_eq!(similarity(&a, &words("thanks!")), 0.0);
        assert_eq!(similarity(&a, &[]), 0.0);
    }

    #[test]
    fn the_nearest_entry_at_the_threshold_steers() {
        let mut l = Layer::default();
        l.add(
            entry(
                "t1",
                "design the cache eviction policy for the tide tables",
                "route.v2",
            ),
            8,
        );
        l.add(entry("t2", "summarize the harbour minutes", "route.v2"), 8);
        let w = words("design the eviction policy for the tide tables cache");
        let (e, s) = l.nearest(&w, "route.v2", 0.5).unwrap();
        assert_eq!((e.turn.as_str(), s), ("t1", 1.0));
        assert!(l
            .nearest(&words("what time is low tide"), "route.v2", 0.5)
            .is_none());
        // At the bar, and just under it.
        let w = words("design cache eviction policy");
        let s = similarity(&w, &l.entries().next().unwrap().words);
        assert!(l.nearest(&w, "route.v2", s).is_some());
        assert!(l.nearest(&w, "route.v2", s + 1e-9).is_none());
    }

    /// Bounded: the oldest out; and a later correction of the same turn
    /// replaces its earlier one.
    #[test]
    fn the_layer_keeps_its_bound_the_oldest_out() {
        let mut l = Layer::default();
        for i in 0..5 {
            l.add(
                entry(&format!("t{i}"), &format!("message number{i}"), "route.v2"),
                3,
            );
        }
        let turns: Vec<_> = l.entries().map(|e| e.turn.clone()).collect();
        assert_eq!(turns, ["t2", "t3", "t4"]);
        l.add(entry("t3", "message number3 again", "route.v2"), 3);
        let turns: Vec<_> = l.entries().map(|e| e.turn.clone()).collect();
        assert_eq!(turns, ["t2", "t4", "t3"]);
    }

    /// Once another version of the route pack acts, the old version's
    /// entries retire, and steer nothing.
    #[test]
    fn an_entry_retires_when_its_pack_no_longer_acts() {
        let mut l = Layer::default();
        l.add(
            entry("t1", "plan the ferry timetable migration", "route.v2"),
            8,
        );
        let w = words("plan the ferry timetable migration");
        let (e, _) = l.nearest(&w, "route.v2", 0.5).unwrap();
        assert_eq!(e.turn, "t1");
        assert!(
            l.nearest(&w, "route.v3", 0.5).is_none(),
            "t1's pack no longer acts"
        );
        assert_eq!((l.len(), l.retired), (0, 1));
        l.add(
            entry("t2", "plan the ferry timetable rollout", "route.v3"),
            8,
        );
        let (e, _) = l.nearest(&w, "route.v3", 0.5).unwrap();
        assert_eq!(e.turn, "t2");
    }

    /// The lookup a turn adds: a full layer of the default bound, each
    /// entry at its word cap, read for one message.
    #[test]
    #[ignore = "a measurement, run by hand: cargo test -p theseus-core layer::tests::the_lookup -- --ignored --nocapture"]
    fn the_lookup_on_a_full_layer() {
        let mut l = Layer::default();
        for i in 0..64 {
            let text: Vec<String> = (0..MAX_WORDS).map(|k| format!("word{i}x{k}")).collect();
            l.add(entry(&format!("t{i}"), &text.join(" "), "route.v2"), 64);
        }
        let msg: Vec<String> = (0..MAX_WORDS).map(|k| format!("word7x{k}")).collect();
        let msg = msg.join(" ");
        let n = 1_000;
        let t0 = std::time::Instant::now();
        for _ in 0..n {
            let w = words(&msg);
            assert!(l.nearest(&w, "route.v2", 0.5).is_some());
        }
        let each = t0.elapsed() / n;
        println!("a lookup on a full layer (64 entries, {MAX_WORDS} words each): {each:?}");
    }
}
