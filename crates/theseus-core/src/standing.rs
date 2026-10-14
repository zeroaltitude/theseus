//! Standing (theseus-a7vi; the killer-features plan's R7 with the personality
//! plan's row 4): the owner's standing orders, preferences and facts, one
//! record each, rendered in every prompt of their scope.
//!
//! - **Records** are META keys under `standing:` (store format 29), each
//!   `protocol::StandingRecord`. Append-only: a revoke, or the edit of one, is
//!   a new record whose `ends` names the one it ends. The set is read from the
//!   store once and kept in memory (`Board`), so a compile reads memory.
//! - **The block** (`Block::of`) is the last thing in the system's first
//!   layer, the header's (theseus-h4a7): no block of its own, so it takes no
//!   cache breakpoint (the layout uses 3 of the provider's 4). The header
//!   block, which every session shares, never carries it. It sits in the
//!   system, outside the ring, so no compaction drops it. A change of the
//!   records changes the system digest, so the next compile is a
//!   `system_changed` recompile and the prefix is otherwise byte-stable.
//! - **Privacy**: a shared place gets the orders of its scope with their text,
//!   and for the preferences and facts only a line saying they are withheld.
//! - **Refused if dropped**: the situation check (`missing`) fails a compile
//!   whose system lacks a record of the turn's scope it should show.
//!   Tests: `tests_standing.rs`.

use std::sync::{Arc, Mutex, RwLock};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use theseus_protocol::{StandingKind, StandingRecord, StandingScope};
use theseus_store::{kinds, NewRecord, Store as _};

use crate::compiler::situation::Unadmitted;
use crate::compiler::{Compiled, RequestSpec};
use crate::places::PlaceClass;
use crate::store::Store;

/// The META key prefix of a record.
pub const PREFIX: &str = "standing:";

/// A test's plant: the compile leaves the block out, as a bug in it would, so
/// the check's refusal can be seen end to end.
#[cfg(test)]
pub(crate) static DROP_THE_BLOCK: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// The longest text of a record, in chars, and the most live records one
/// scope may hold: the block is a few hundred tokens in a cached prefix.
pub const MAX_TEXT: usize = 1000;
pub const MAX_LIVE: usize = 64;

/// What the turn's scope is: its session, and its place's target.
#[derive(Debug, Clone, Copy)]
pub struct Scope<'a> {
    pub session: &'a str,
    pub place: Option<&'a str>,
}

impl Scope<'_> {
    pub fn holds(&self, r: &StandingRecord) -> bool {
        match &r.scope {
            StandingScope::Everywhere => true,
            StandingScope::Session(s) => s == self.session,
            StandingScope::Place(p) => self.place == Some(p.as_str()),
        }
    }
}

pub fn key(id: &str) -> String {
    format!("{PREFIX}{id}")
}

/// A record's state at `now`: `live`, `revoked`, `replaced`, `expired`, or
/// `ending` for a revoke record itself (it has no text and shows nowhere).
pub fn state(all: &[StandingRecord], r: &StandingRecord, now_ms: u64) -> &'static str {
    if r.text.is_empty() {
        return "ending";
    }
    if let Some(e) = all.iter().find(|e| e.ends.as_deref() == Some(&r.id)) {
        return if e.text.is_empty() {
            "revoked"
        } else {
            "replaced"
        };
    }
    match r.until_ms {
        Some(u) if u <= now_ms => "expired",
        _ if r.since_ms > now_ms => "expired",
        _ => "live",
    }
}

/// The live records of `scope` at `now`, oldest first.
pub fn live<'a>(
    all: &'a [StandingRecord],
    scope: Option<Scope<'_>>,
    now_ms: u64,
) -> Vec<&'a StandingRecord> {
    let mut out: Vec<&StandingRecord> = all
        .iter()
        .filter(|r| state(all, r, now_ms) == "live" && scope.is_none_or(|s| s.holds(r)))
        .collect();
    out.sort_by(|a, b| (a.since_ms, &a.id).cmp(&(b.since_ms, &b.id)));
    out
}

/// A record the block listed, as a manifest names it (store format 29).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StandingUsed {
    pub id: String,
    /// Its text was in the block; false in a shared place for a preference or
    /// a fact, whose text is private.
    pub shown: bool,
}

/// What a turn's compile carries of the records: the block's text, and what
/// the manifest and the check read of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub text: String,
    pub used: Vec<StandingUsed>,
    /// The line a shared place's block carries for the private ones it
    /// withholds; the check wants it when any were.
    pub withheld: bool,
}

const HEAD: &str = "Standing: what the owner has told you to hold to, in force in every turn of its scope until they revoke it. An order is to be followed; a preference is to be honoured where it can be; a fact is what the owner has told you about themself. Each line names its record.";
pub const WITHHELD: &str =
    "The owner's preferences and facts are private to them and are not shown in this place.";

impl Block {
    /// The block for `scope` in a place of `class`; none while no record is
    /// live there. A shared place's lists the orders alone, with their text.
    pub fn of(
        all: &[StandingRecord],
        scope: Scope<'_>,
        class: PlaceClass,
        now_ms: u64,
    ) -> Option<Block> {
        let held = live(all, Some(scope), now_ms);
        if held.is_empty() {
            return None;
        }
        let shared = class == PlaceClass::Shared;
        let mut text = String::from(HEAD);
        let mut used = Vec::new();
        let mut withheld = false;
        for (kind, title) in [
            (StandingKind::Order, "Orders"),
            (StandingKind::Preference, "Preferences"),
            (StandingKind::Fact, "Facts about the owner"),
        ] {
            let of_kind: Vec<&&StandingRecord> = held.iter().filter(|r| r.kind == kind).collect();
            if of_kind.is_empty() {
                continue;
            }
            let hidden = shared && kind != StandingKind::Order;
            used.extend(of_kind.iter().map(|r| StandingUsed {
                id: r.id.clone(),
                shown: !hidden,
            }));
            if hidden {
                withheld = true;
                continue;
            }
            text.push_str(&format!("\n{title}:"));
            for r in of_kind {
                text.push_str(&format!("\n- {}: {}", r.id, one_line(&r.text)));
            }
        }
        if withheld {
            text.push_str(&format!("\n{WITHHELD}"));
        }
        Some(Block {
            text,
            used,
            withheld,
        })
    }

    /// Put the block in the spec's header layer, after the header's own text
    /// (the first layer of `compiler::layers`: a place class's sessions share
    /// it).
    pub fn apply(&self, spec: &mut RequestSpec) {
        #[cfg(test)]
        if DROP_THE_BLOCK.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        spec.system_text = match spec.system_text.is_empty() {
            true => self.text.clone(),
            false => format!("{}\n\n{}", spec.system_text, self.text),
        };
    }
}

/// A text on one line: a record is a line of the block, so a newline in its
/// text could pass for another record.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The check after the compile (`turn::situation_step`): the system carries
/// every record the turn's scope holds that it should show, and the line
/// about the withheld ones when it withholds any. A compile that drops one
/// is not sent.
pub fn missing(compiled: &Compiled, block: Option<&Block>) -> Result<(), Unadmitted> {
    let Some(block) = block else { return Ok(()) };
    let system: String = compiled
        .request
        .system
        .iter()
        .filter_map(|b| b["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let gone = |piece: String, detail: String| {
        Err(Unadmitted {
            why: "standing_missing",
            piece,
            detail,
        })
    };
    for u in block.used.iter().filter(|u| u.shown) {
        if !system.contains(&format!("- {}: ", u.id)) {
            return gone(
                format!("standing {} missing", u.id),
                format!(
                    "the owner's standing record {} is live in this turn's scope and is not in the system",
                    u.id
                ),
            );
        }
    }
    if block.withheld && !system.contains(WITHHELD) {
        return gone(
            "standing withheld line missing".into(),
            "the system lacks the line that says the owner's private records are withheld here"
                .into(),
        );
    }
    Ok(())
}

/// The records in memory, and the lock their writers take.
#[derive(Default)]
pub struct Board {
    records: RwLock<Option<Arc<Vec<StandingRecord>>>>,
    writes: Mutex<()>,
}

impl Board {
    /// Every record: memory once built; built now, by one META prefix scan,
    /// when nothing has built it yet. A record that does not read is left
    /// out and warned of.
    pub fn all(&self, store: &Store) -> Result<Arc<Vec<StandingRecord>>> {
        if let Some(r) = self.held() {
            return Ok(r);
        }
        theseus_store::blocking(|| {
            let _w = self.writes.lock().unwrap();
            if let Some(r) = self.held() {
                return Ok(r);
            }
            let mut records = Vec::new();
            for r in store.inner().latest_with_prefix(kinds::META, PREFIX)? {
                match r.decode::<StandingRecord>() {
                    Ok(rec) => records.push(rec),
                    Err(e) => tracing::warn!(key = ?r.key, error = %e,
                        "a standing record does not read: left out"),
                }
            }
            records.sort_by(|a, b| (a.since_ms, &a.id).cmp(&(b.since_ms, &b.id)));
            let all = Arc::new(records);
            *self.records.write().unwrap() = Some(all.clone());
            Ok(all)
        })
    }

    pub fn held(&self) -> Option<Arc<Vec<StandingRecord>>> {
        self.records.read().unwrap().clone()
    }

    /// Write `records` in one frame with `rows`, which `rows` makes from the
    /// set as it was before; then hold the new set. `check` runs under the
    /// lock against that set, and a refusal writes nothing.
    pub fn write(
        &self,
        store: &Store,
        check: impl FnOnce(&[StandingRecord]) -> Result<Vec<StandingRecord>>,
        rows: impl FnOnce(&[StandingRecord], &[StandingRecord]) -> Result<Vec<NewRecord>>,
    ) -> Result<Vec<StandingRecord>> {
        let now = self.all(store)?;
        theseus_store::blocking(|| {
            let _w = self.writes.lock().unwrap();
            let now = self.held().unwrap_or(now);
            let records = check(&now)?;
            let mut frame = Vec::with_capacity(records.len() + 1);
            for r in &records {
                frame.push(NewRecord::json(kinds::META, Some(&key(&r.id)), r)?);
            }
            frame.extend(rows(&now, &records)?);
            store.append(&frame)?;
            let mut next = (*now).clone();
            next.extend(records.iter().cloned());
            *self.records.write().unwrap() = Some(Arc::new(next));
            Ok(records)
        })
    }
}

/// An id no record holds: `std_` and eight hex digits of a UUIDv7's random
/// part, short enough to type and to read in a prompt.
pub fn mint(all: &[StandingRecord]) -> String {
    loop {
        let u = crate::new_id("std");
        let id = format!("std_{}", &u[u.len() - 8..]);
        if all.iter().all(|r| r.id != id) {
            return id;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(id: &str, kind: StandingKind, text: &str, since: u64) -> StandingRecord {
        StandingRecord {
            id: id.into(),
            kind,
            text: text.into(),
            scope: StandingScope::Everywhere,
            since_ms: since,
            until_ms: None,
            source: "the CLI".into(),
            ends: None,
        }
    }

    /// A record is live until a later one ends it or its time passes; a
    /// revoke names what it ends and shows nowhere; an edit ends the old.
    #[test]
    fn a_record_is_live_until_it_is_ended_or_expires() {
        let mut lapses = rec("std_b", StandingKind::Preference, "short answers", 20);
        lapses.until_ms = Some(500);
        let mut revoke = rec("std_r", StandingKind::Order, "", 40);
        revoke.ends = Some("std_a".into());
        let mut edit = rec("std_e", StandingKind::Fact, "keeps bees", 50);
        edit.ends = Some("std_c".into());
        let all = vec![
            rec("std_a", StandingKind::Order, "never push to main", 10),
            lapses,
            rec("std_c", StandingKind::Fact, "keeps hens", 30),
            revoke,
            edit,
        ];
        let st = |now| -> Vec<(&str, &str)> {
            all.iter()
                .map(|r| (r.id.as_str(), state(&all, r, now)))
                .collect()
        };
        assert_eq!(
            st(100),
            [
                ("std_a", "revoked"),
                ("std_b", "live"),
                ("std_c", "replaced"),
                ("std_r", "ending"),
                ("std_e", "live")
            ]
        );
        assert_eq!(st(500)[1], ("std_b", "expired"), "until is exclusive");
        let live_ids = |now| -> Vec<&str> {
            live(&all, None, now)
                .iter()
                .map(|r| r.id.as_str())
                .collect()
        };
        assert_eq!(live_ids(100), ["std_b", "std_e"]);
        assert_eq!(live_ids(600), ["std_e"]);
    }

    /// The block lists the live records of a scope by kind, each with its id;
    /// a shared place's lists the orders' text and names the rest withheld,
    /// holding none of their text.
    #[test]
    fn the_block_lists_by_kind_and_a_shared_place_withholds_the_private_ones() {
        let mut here = rec(
            "std_p",
            StandingKind::Preference,
            "short  answers\nplease",
            20,
        );
        here.scope = StandingScope::Place("discord:dm:u1".into());
        let mut other = rec("std_x", StandingKind::Order, "not for this session", 15);
        other.scope = StandingScope::Session("ses_other".into());
        let all = vec![
            rec("std_a", StandingKind::Order, "never push to main", 10),
            here,
            other,
            rec("std_f", StandingKind::Fact, "keeps a garden", 30),
        ];
        let scope = Scope {
            session: "ses_me",
            place: Some("discord:dm:u1"),
        };
        let b = Block::of(&all, scope, PlaceClass::Private, 100).unwrap();
        assert!(
            b.text.contains("\nOrders:\n- std_a: never push to main"),
            "{}",
            b.text
        );
        assert!(
            b.text
                .contains("\nPreferences:\n- std_p: short answers please"),
            "{}",
            b.text
        );
        assert!(
            b.text
                .contains("\nFacts about the owner:\n- std_f: keeps a garden"),
            "{}",
            b.text
        );
        assert!(!b.text.contains("std_x") && !b.withheld);
        assert!(b.used.iter().all(|u| u.shown) && b.used.len() == 3);

        let b = Block::of(&all, scope, PlaceClass::Shared, 100).unwrap();
        assert!(b.text.contains("- std_a: never push to main"), "{}", b.text);
        assert!(b.text.ends_with(WITHHELD), "{}", b.text);
        for hidden in ["short", "garden", "std_p", "std_f"] {
            assert!(!b.text.contains(hidden), "{hidden} leaked: {}", b.text);
        }
        assert_eq!(
            b.used
                .iter()
                .map(|u| (u.id.as_str(), u.shown))
                .collect::<Vec<_>>(),
            [("std_a", true), ("std_p", false), ("std_f", false)]
        );
        assert!(Block::of(
            &all,
            Scope {
                session: "s",
                place: None
            },
            PlaceClass::Private,
            5
        )
        .is_none());
    }
}
