//! The held calls the owner can still let run (theseus-p5kln): the store's
//! side of the override route.
//!
//! **Why the hold is kept at all.** `undo.v1`'s `irreversible_reshapeable`
//! verdict holds a call *before* the order runs, so the call leaves no parked
//! action and no card: that is the whole point of the reshapeable arm, and
//! the no-card assertion is not to be touched. But the owner's word of
//! 2026-11-12 asks for the power to say the original was right — *"that i
//! have the power to say it's OK to run what was originally suggested"* — and
//! there is nothing to say it *about* unless the call as first proposed is
//! written down. This record is that. It is an offer of an override, not the
//! record of what happened: the `tool.held` row is the record and does not
//! move.
//!
//! **One META record, a map by id**, as a repository's grants are
//! (`project_context::trust::Grants`): [`RECORD`], read with one keyed read
//! rather than a prefix scan, which matters because the gate reads it on its
//! own path. It is a new META key and a new type, so no stored record gained
//! a field and `MANIFEST_FORMAT` does not move.
//!
//! **Why the allowance is keyed to the call's own bytes.** [`Holds::spend`]
//! matches the tool *and* the input, exactly as the model first proposed it.
//! So the override lets the ORIGINAL call run and nothing else: a re-planned
//! call — the thing the model was told to produce, with different arguments —
//! is a different input and is held as it was. That is the shape the owner's
//! decision names, and it is also what makes the route an override of the
//! hold rather than a way past the classifier.
//!
//! The map is capped at [`KEEP`], oldest forgotten first, so a session that
//! proposes a hundred held calls cannot grow one record without bound.

use std::collections::BTreeMap;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use theseus_protocol::work::undo::{Allowance, HeldCall, KEEP};
use theseus_store::{kinds, NewRecord};

/// The META key the holds live under.
pub const RECORD: &str = "undo.holds";

/// Every held call the owner may still allow, by id.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Holds {
    pub held: BTreeMap<String, HeldCall>,
}

impl Holds {
    pub fn read(store: &crate::store::Store) -> Result<Self> {
        Ok(store.get_meta::<Self>(RECORD)?.unwrap_or_default())
    }

    pub fn record(&self) -> Result<NewRecord> {
        NewRecord::json(kinds::META, Some(RECORD), self)
    }

    /// The holds newest first; `session` narrows to one conversation and
    /// `all` keeps the ones an override has already spent.
    pub fn list(&self, session: Option<&str>, all: bool) -> Vec<HeldCall> {
        let mut out: Vec<HeldCall> = self
            .held
            .values()
            .filter(|h| session.is_none_or(|s| h.session_id == s))
            .filter(|h| all || h.ran_ms.is_none())
            .cloned()
            .collect();
        out.sort_by(|a, b| b.held_ms.cmp(&a.held_ms).then(a.id.cmp(&b.id)));
        out
    }

    /// Keep `h`, forgetting the oldest once [`KEEP`] are held. A spent hold
    /// is forgotten before an unspent one, so an offer outlives history.
    pub fn keep(&mut self, h: HeldCall) {
        self.held.insert(h.id.clone(), h);
        while self.held.len() > KEEP {
            let Some(id) = self.oldest() else { break };
            self.held.remove(&id);
        }
    }

    /// The id of the hold to forget next: one with no allowance waiting on it
    /// before one that has, and the oldest of each. An offer the owner has
    /// already made outlives a hold nobody has answered, however old it is:
    /// losing it would lose his word.
    fn oldest(&self) -> Option<String> {
        self.held
            .values()
            .min_by_key(|h| (h.waiting(), h.held_ms))
            .map(|h| h.id.clone())
    }

    /// Record the owner's allowance on the hold `id`. `None`: no such hold.
    pub fn allow(&mut self, id: &str, by: Allowance) -> Option<&HeldCall> {
        let h = self.held.get_mut(id)?;
        h.allowed = Some(by);
        h.ran_ms = None;
        Some(h)
    }

    /// The waiting allowance for this exact call, if there is one: the id of
    /// the hold whose tool and input are the call's own bytes. The newest
    /// such hold, so a call held twice and allowed twice spends the later
    /// allowance first.
    pub fn allowance_for(&self, session: &str, tool: &str, input: &Value) -> Option<String> {
        self.held
            .values()
            .filter(|h| h.waiting() && h.session_id == session && h.tool == tool)
            .filter(|h| &h.input == input)
            .max_by_key(|h| h.held_ms)
            .map(|h| h.id.clone())
    }

    /// Spend the allowance on `id`: the hold is marked run, so one allowance
    /// admits one call. `false` when it was already spent or is unknown.
    pub fn spend(&mut self, id: &str, now_ms: u64) -> bool {
        let Some(h) = self.held.get_mut(id) else {
            return false;
        };
        if !h.waiting() {
            return false;
        }
        h.ran_ms = Some(now_ms);
        true
    }
}

/// A hold's id: `hld_` and eight hex characters of the call's own names, so
/// the same call in the same turn is the same id and a retry of the hold does
/// not fill the map. Eight hex is short enough to type and is nothing a name
/// scrub reads as an identifier of a person or a place.
pub fn id_of(session: &str, turn: &str, call: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(session.as_bytes());
    h.update([0]);
    h.update(turn.as_bytes());
    h.update([0]);
    h.update(call.as_bytes());
    format!("hld_{}", &hex::encode(h.finalize())[..8])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn held(id: &str, at: u64) -> HeldCall {
        HeldCall {
            id: id.into(),
            session_id: "ses_1".into(),
            turn_id: Some("turn_1".into()),
            tool: "fs.write".into(),
            input: json!({"path": "/w/notes.txt", "content": "new"}),
            summary: "write notes.txt".into(),
            judgment: "jdg_1".into(),
            shape: Some("move_aside".into()),
            held_ms: at,
            allowed: None,
            ran_ms: None,
        }
    }

    fn by() -> Allowance {
        Allowance {
            by: "the owner".into(),
            via: "cli".into(),
            at_ms: 9,
        }
    }

    #[test]
    fn an_allowance_admits_the_original_call_and_nothing_else() {
        let mut hs = Holds::default();
        let h = held("hld_1a2b3c4d", 5);
        let original = h.input.clone();
        hs.keep(h);
        // Nothing is admitted before the owner allows it.
        assert_eq!(hs.allowance_for("ses_1", "fs.write", &original), None);
        assert!(hs.allow("hld_1a2b3c4d", by()).is_some());
        assert_eq!(
            hs.allowance_for("ses_1", "fs.write", &original).as_deref(),
            Some("hld_1a2b3c4d"),
            "the original call, as first proposed"
        );
        // The re-planned call: the same tool, different arguments.
        let replanned = json!({"path": "/w/notes.txt.new", "content": "new"});
        assert_eq!(
            hs.allowance_for("ses_1", "fs.write", &replanned),
            None,
            "a re-planned call is not what the owner allowed"
        );
        // Another tool, and another session, are not it either.
        assert_eq!(hs.allowance_for("ses_1", "fs.patch", &original), None);
        assert_eq!(hs.allowance_for("ses_2", "fs.write", &original), None);
    }

    #[test]
    fn one_allowance_admits_one_call() {
        let mut hs = Holds::default();
        let h = held("hld_1a2b3c4d", 5);
        let input = h.input.clone();
        hs.keep(h);
        hs.allow("hld_1a2b3c4d", by());
        assert!(hs.spend("hld_1a2b3c4d", 11), "the first call runs");
        assert!(!hs.spend("hld_1a2b3c4d", 12), "the second does not");
        assert_eq!(
            hs.allowance_for("ses_1", "fs.write", &input),
            None,
            "a spent allowance admits nothing"
        );
        assert!(
            !hs.spend("hld_nosuch", 13),
            "an unknown hold spends nothing"
        );
    }

    #[test]
    fn the_map_is_capped_and_forgets_a_spent_hold_before_a_waiting_one() {
        let mut hs = Holds::default();
        // A waiting allowance on the OLDEST hold, which the cap must keep.
        let mut old = held("hld_00000000", 0);
        old.allowed = Some(by());
        hs.keep(old);
        for i in 1..=KEEP as u64 + 4 {
            hs.keep(held(&format!("hld_{i:08}"), i));
        }
        assert_eq!(hs.held.len(), KEEP, "capped");
        assert!(
            hs.held.contains_key("hld_00000000"),
            "an offer outlives history: the waiting hold is kept"
        );
        assert_eq!(hs.list(None, true).len(), KEEP, "and all of them list");
    }

    #[test]
    fn the_list_is_newest_first_and_hides_what_ran_unless_asked() {
        let mut hs = Holds::default();
        hs.keep(held("hld_aaaaaaaa", 1));
        hs.keep(held("hld_bbbbbbbb", 2));
        let mut ran = held("hld_cccccccc", 3);
        ran.allowed = Some(by());
        ran.ran_ms = Some(4);
        hs.keep(ran);
        let open = hs.list(None, false);
        let ids: Vec<&str> = open.iter().map(|h| h.id.as_str()).collect();
        assert_eq!(ids, ["hld_bbbbbbbb", "hld_aaaaaaaa"], "newest first");
        assert_eq!(hs.list(None, true).len(), 3, "and `all` keeps what ran");
        assert_eq!(hs.list(Some("ses_2"), true).len(), 0, "another session's");
        assert_eq!(hs.list(Some("ses_1"), true).len(), 3);
    }

    #[test]
    fn a_holds_id_is_the_same_for_the_same_call_and_different_otherwise() {
        let a = id_of("ses_1", "turn_1", "c1");
        assert_eq!(a, id_of("ses_1", "turn_1", "c1"), "the same call");
        assert_ne!(a, id_of("ses_1", "turn_1", "c2"));
        assert_ne!(a, id_of("ses_1", "turn_2", "c1"));
        assert_ne!(a, id_of("ses_2", "turn_1", "c1"));
        assert!(a.starts_with("hld_") && a.len() == 12, "{a}");
    }
}
