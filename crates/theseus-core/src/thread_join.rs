//! Merging threads and splitting one (theseus-vt9k, T8; the owner, 2026-10-09:
//! separable paths of thought, each addressable). The pure parts: how a card
//! folds into another, the unions of a working set and of topics, the record
//! a merge keeps so it can be undone, and the notes a join leaves in a
//! session. The methods are `rpc/thread_join.rs`; the split's turns and its
//! tool are `thread_split.rs`.
//!
//! Nothing is deleted. A merge retires thread A (`merged_into = B`) and
//! quotes its card at the end of B's; A's session and nodes stay as they
//! were, and recall draws on them with their own provenance. The META record
//! `thread.merge.<A>` keeps what the merge changed about A (its state, its
//! surfaces, the session the merge retired) in the merge's frame, so
//! `thread.unmerge` puts A back as it was. A split quotes turns of A into a
//! new thread's first compile (an arrangement) and moves nothing.

use serde::{Deserialize, Serialize};
use theseus_protocol::term::words::clip;
use theseus_protocol::threads::{
    ThreadInfo, ThreadState, WorkingSet, MAX_CARD, MAX_LINE_CHARS, MAX_TOPICS,
};

/// The most entries one list of a working set holds, as `thread.edit` holds.
pub const MAX_SET: usize = 64;
/// The most of a card's room a quoted card may take, header included, when
/// the receiving thread has lines of its own.
const MAX_QUOTE_LINES: usize = 6;

/// The META key of the record a merge keeps for the thread it retired.
pub fn merge_key(slug: &str) -> String {
    format!("thread.merge.{slug}")
}

/// What a merge changed about the thread it retired: enough to put it back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeRecord {
    pub into: String,
    /// The state the thread was in before the merge.
    pub was: ThreadState,
    /// Its surfaces before the merge (the retired thread keeps none).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub surfaces: Vec<String>,
    /// The session the merge retired, when the thread's was open: the undo
    /// opens it again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retired_session: Option<String>,
    pub at_ms: u64,
    /// The merge was undone.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub undone: bool,
}

/// A date as `2026-10-14`.
pub fn day(ms: u64) -> String {
    crate::arrangement::utc(ms).chars().take(10).collect()
}

/// `into`'s card with `from`'s quoted at its end as an evidence block:
/// a header, `merged from «a», 2026-10-14:`, and `from`'s lines each
/// marked `> `. The whole stays within `MAX_CARD`: the quoted block takes at
/// most `MAX_QUOTE_LINES` lines (header included) while `into` has lines of
/// its own to keep, or the room `into`'s lines leave when that is more;
/// `into`'s oldest lines go first, then `from`'s oldest. `from`'s whole card
/// is in the merge's ledger row and in `from`'s own record either way.
pub fn merged_card(into: &[String], from: &ThreadInfo, at_ms: u64) -> Vec<String> {
    let header = format!("merged from «{}», {}:", from.slug, day(at_ms));
    let room = MAX_CARD.saturating_sub(into.len().min(MAX_CARD - MAX_QUOTE_LINES));
    let take = room.saturating_sub(1).min(from.card.len());
    let quoted = from.card[from.card.len() - take..]
        .iter()
        .map(|l| format!("> {}", clip(l, MAX_LINE_CHARS - 2)));
    let block: Vec<String> = std::iter::once(header).chain(quoted).collect();
    let keep = MAX_CARD.saturating_sub(block.len()).min(into.len());
    into[into.len() - keep..]
        .iter()
        .cloned()
        .chain(block)
        .collect()
}

/// `card` with one line more, the newest `MAX_CARD` kept.
pub fn with_line(card: &[String], line: &str) -> Vec<String> {
    let mut next = card.to_vec();
    next.push(clip(line, MAX_LINE_CHARS));
    let over = next.len().saturating_sub(MAX_CARD);
    next.drain(..over);
    next
}

fn union_list(a: &[String], b: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for x in a.iter().chain(b) {
        if !out.contains(x) {
            out.push(x.clone());
        }
    }
    out.truncate(MAX_SET);
    out
}

/// Both working sets, `a`'s entries first, none twice, each list within
/// `MAX_SET`.
pub fn union_set(a: &WorkingSet, b: &WorkingSet) -> WorkingSet {
    WorkingSet {
        roots: union_list(&a.roots, &b.roots),
        repos: union_list(&a.repos, &b.repos),
        branches: union_list(&a.branches, &b.branches),
        refs: union_list(&a.refs, &b.refs),
        files: union_list(&a.files, &b.files),
        hosts: union_list(&a.hosts, &b.hosts),
    }
}

/// Both threads' topics, `a`'s first, at most `MAX_TOPICS`; the rest are
/// returned for the owner to pick among.
pub fn union_topics(a: &[String], b: &[String]) -> (Vec<String>, Vec<String>) {
    let mut all: Vec<String> = Vec::new();
    for t in a.iter().chain(b) {
        if !all.contains(t) {
            all.push(t.clone());
        }
    }
    let left_out = all.split_off(all.len().min(MAX_TOPICS));
    (all, left_out)
}

/// The note a merge leaves at the tail of the retired thread's session.
pub fn merged_note(from: &str, into: &str, at_ms: u64) -> String {
    format!(
        "Thread {from} was merged into {into} on {}. Its card is quoted in {into}'s; this \
         session and everything in it stay, and `theseus thread unmerge {from}` opens the \
         thread again.",
        day(at_ms)
    )
}

/// The note a split leaves at the tail of the source thread's session (and
/// the result of the tool that made it): the new thread, the positions, and
/// that nothing here moved.
pub fn split_note(
    from: &str,
    new: &str,
    title: &str,
    quoted: u32,
    first: u64,
    last: u64,
) -> String {
    format!(
        "Split to «{new}» ({title}) at @{first} to @{last}: {} of this thread, {from}, are \
         quoted there, on {}. Nothing here was moved or removed; `theseus thread done {new}` \
         ends the new thread.",
        crate::narrative::count(quoted as u64, "message", "messages"),
        day(theseus_protocol::now_unix_ms())
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread(slug: &str, card: &[&str]) -> ThreadInfo {
        ThreadInfo {
            slug: slug.into(),
            title: slug.into(),
            goal: String::new(),
            topics: vec![],
            working_set: WorkingSet::default(),
            card: card.iter().map(|s| s.to_string()).collect(),
            persona: None,
            state: ThreadState::Active,
            sealed: None,
            seal_pending: None,
            surfaces: vec![],
            parent: None,
            merged_into: None,
            split_from: None,
            session_id: None,
            last_active_ms: 0,
            created_at_ms: 0,
        }
    }

    fn lines(n: usize, tag: &str) -> Vec<String> {
        (0..n).map(|i| format!("{tag}{i}")).collect()
    }

    #[test]
    fn a_merged_card_quotes_the_other_card_under_a_dated_header() {
        // 2026-10-14 12:00 UTC.
        let at = 1_791_979_200_000;
        let c = merged_card(&["b0".into()], &thread("io", &["a0", "a1"]), at);
        assert_eq!(c, ["b0", "merged from «io», 2026-10-14:", "> a0", "> a1"]);
    }

    #[test]
    fn a_merged_card_stays_within_the_cards_limit_and_keeps_the_newest() {
        let from = thread(
            "io",
            &lines(12, "a")
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        );
        // A full card on each side: the quote takes six lines, B keeps six.
        let c = merged_card(&lines(12, "b"), &from, 0);
        assert_eq!(c.len(), MAX_CARD, "{c:?}");
        assert_eq!(c[0], "b6");
        assert!(c[6].starts_with("merged from «io»"), "{c:?}");
        assert_eq!(c[11], "> a11");
        // An empty card on B leaves the quote the room.
        let c = merged_card(&[], &from, 0);
        assert_eq!(c.len(), MAX_CARD, "{c:?}");
        assert_eq!(c[1], "> a1");
        // Nothing to quote: the header alone says where it came from.
        let c = merged_card(&["b0".into()], &thread("io", &[]), 0);
        assert_eq!(c.len(), 2, "{c:?}");
    }

    #[test]
    fn working_sets_join_without_repeats_and_topics_stop_at_three() {
        let a = WorkingSet {
            files: vec!["x.rs".into(), "y.rs".into()],
            ..Default::default()
        };
        let b = WorkingSet {
            files: vec!["y.rs".into(), "z.rs".into()],
            hosts: vec!["h1".into()],
            ..Default::default()
        };
        let u = union_set(&a, &b);
        assert_eq!(u.files, ["x.rs", "y.rs", "z.rs"]);
        assert_eq!(u.hosts, ["h1"]);
        let (kept, out) = union_topics(
            &["t1".into(), "t2".into()],
            &["t2".into(), "t3".into(), "t4".into()],
        );
        assert_eq!(kept, ["t1", "t2", "t3"]);
        assert_eq!(out, ["t4"]);
    }

    #[test]
    fn a_line_added_to_a_full_card_pushes_the_oldest_out() {
        let c = with_line(&lines(12, "c"), "new");
        assert_eq!(c.len(), MAX_CARD);
        assert_eq!((c[0].as_str(), c[11].as_str()), ("c1", "new"));
    }
}
