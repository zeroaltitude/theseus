//! Pages of a kind's records through the index (theseus-vm3n.5): reads that
//! cost about their answer, not the history before it.
//!
//! - **Tags.** A record's tags (`tags_of`) go into the index's `tagged` table
//!   with its position, in the transaction that indexes it. A ledger row's
//!   are its kind (`k:`), its session (`s:`), and the two together (`ks:`),
//!   so a read of one kind, one session, or one kind in one session reads
//!   that tag's postings alone.
//! - **Time.** Each kind has a clock: the newest frame time its records have
//!   had. A frame whose time stepped back (a host's clock set back) takes the
//!   clock's time instead, so the clock only grows with position, and a
//!   window of time is one stretch of positions. `bytime` keeps, for each
//!   minute the clock entered, the first position in it: a window's first
//!   position is one lookup and then at most a minute of the kind's records.
//!   A record written while the clock was ahead of the host's counts at the
//!   clock's time, so a window never splits around it, and its own `at` is
//!   what it says.
//! - **Cursors.** `after` pages forward from a position, oldest first, as
//!   the ledger's walk always has; `before` pages back from the newest. Both
//!   are positions, which never move, so a page read while rows are written
//!   neither repeats nor skips one.
//!
//! This is date partitioning at the index's level: no per-day files.

use serde::Deserialize;

use crate::record::{kinds, Record, RecordKind};

/// One page's question.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Page {
    pub kind: RecordKind,
    /// Records with any of these tags; none: every record of the kind.
    pub tags: Vec<String>,
    /// Only positions after this one, the oldest first; without it the page
    /// is the newest.
    pub after: Option<u64>,
    /// Only positions before this one.
    pub before: Option<u64>,
    /// Only records whose kind's clock is at or after this time (unix ms).
    pub since_ms: Option<u64>,
    /// Only records whose kind's clock is at or before this time (unix ms).
    pub until_ms: Option<u64>,
    pub limit: usize,
}

/// One page's answer.
#[derive(Debug, Clone, Default)]
pub struct PageOut {
    /// The page, oldest first. A record whose read is refused (a corrupt
    /// frame) is left out, and counted in the store's stats, as every list
    /// read does.
    pub records: Vec<Record>,
    /// The page's first and last positions, those left out included: the
    /// cursors for the next page either way.
    pub first: Option<u64>,
    pub last: Option<u64>,
    /// Whether more records lie past the page in its direction: older ones
    /// for a newest page, newer ones for an `after` page.
    pub more: bool,
    /// How many records the kind has, from the same snapshot as the page.
    pub count: u64,
}

/// A ledger row's kind tag.
pub fn ledger_kind(kind: &str) -> String {
    format!("k:{kind}")
}

/// A ledger row's session tag.
pub fn ledger_session(session: &str) -> String {
    format!("s:{session}")
}

/// A ledger row's tag for its kind in its session.
pub fn ledger_kind_session(kind: &str, session: &str) -> String {
    format!("ks:{kind}\u{1}{session}")
}

/// A record's tags. Only ledger rows have any: their `kind`, their
/// `session_id`, and the two together. A tag never holds 0x00 (the index
/// ends a tag with one), so a name that does is not tagged.
pub fn tags_of(kind: RecordKind, payload: &[u8]) -> Vec<String> {
    #[derive(Deserialize)]
    struct Row {
        kind: String,
        #[serde(default)]
        session_id: Option<String>,
    }
    if kind != kinds::LEDGER {
        return Vec::new();
    }
    let Ok(row) = serde_json::from_slice::<Row>(payload) else {
        return Vec::new();
    };
    let mut tags = vec![ledger_kind(&row.kind)];
    if let Some(s) = &row.session_id {
        tags.push(ledger_session(s));
        tags.push(ledger_kind_session(&row.kind, s));
    }
    tags.retain(|t| !t.contains('\0'));
    tags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ledger_row_is_tagged_by_its_kind_and_session() {
        let row =
            br#"{"at_unix_ms":1,"kind":"turn.started","session_id":"ses_a","data":{"x":[1,2]}}"#;
        assert_eq!(
            tags_of(kinds::LEDGER, row),
            vec!["k:turn.started", "s:ses_a", "ks:turn.started\u{1}ses_a"]
        );
        let row = br#"{"at_unix_ms":1,"kind":"server.started"}"#;
        assert_eq!(tags_of(kinds::LEDGER, row), vec!["k:server.started"]);
        assert!(tags_of(kinds::LEDGER, b"not json").is_empty());
        assert!(tags_of(kinds::SESSION, row).is_empty());
    }
}
