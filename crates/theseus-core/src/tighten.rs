//! "Should have asked" (theseus-sgh, spec §3.9 "Notices and records"): one
//! press on a notice makes that tool ask first from then on. It is how the
//! operator corrects the posture from what actually ran, with nothing to edit
//! in the vault.
//!
//! A tightening lives in the store, never in the config: the config is the
//! vault's, static, and agents cannot write it. Every tightening is one
//! `meta` record, written in the same WAL frame as the ledger row that
//! records the change, and read once when the core starts. The gate applies
//! a tightening after the config's posture, and the stricter one wins
//! (`ToolPolicy::posture_now`), so a tightening can never loosen anything.
//! An undo removes it, and the tool returns to what the config says.

use std::collections::BTreeMap;
use std::sync::RwLock;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use theseus_protocol::Tightening;
use theseus_store::{kinds, NewRecord};

use crate::ledger::LedgerRow;
use crate::policy::{Posture, Tightened};
use crate::store::Store;

/// The `meta` key that holds every tightening.
pub const META_KEY: &str = "policy.tightenings";

/// The record under `META_KEY`.
#[derive(Serialize, Deserialize)]
struct Stored {
    schema: u16,
    /// Tool name → its tightening.
    tools: BTreeMap<String, Tightening>,
}

/// The tightenings in force, as the store holds them.
#[derive(Default)]
pub struct Tightenings {
    map: RwLock<BTreeMap<String, Tightening>>,
}

impl Tightenings {
    /// Read them from the store: one keyed read.
    pub fn load(&self, store: &Store) -> Result<()> {
        let stored = store.get_meta::<Stored>(META_KEY)?;
        *self.map.write().unwrap() = stored.map(|s| s.tools).unwrap_or_default();
        Ok(())
    }

    pub fn get(&self, tool: &str) -> Option<Tightening> {
        self.map.read().unwrap().get(tool).cloned()
    }

    /// Every tightening, oldest first.
    pub fn all(&self) -> Vec<Tightening> {
        let mut v: Vec<Tightening> = self.map.read().unwrap().values().cloned().collect();
        v.sort_by_key(|t| t.at_ms);
        v
    }

    /// Record a tightening and its ledger row in one frame. False, with
    /// nothing written, when the tool is tightened already.
    pub fn insert(&self, store: &Store, t: Tightening, row: &LedgerRow) -> Result<bool> {
        let mut map = self.map.write().unwrap();
        if map.contains_key(&t.tool) {
            return Ok(false);
        }
        let mut next = map.clone();
        next.insert(t.tool.clone(), t);
        write(store, &next, row)?;
        *map = next;
        Ok(true)
    }

    /// Remove a tool's tightening, with its ledger row, in one frame. False,
    /// with nothing written, when the tool is not tightened.
    pub fn remove(&self, store: &Store, tool: &str, row: &LedgerRow) -> Result<bool> {
        let mut map = self.map.write().unwrap();
        if !map.contains_key(tool) {
            return Ok(false);
        }
        let mut next = map.clone();
        next.remove(tool);
        write(store, &next, row)?;
        *map = next;
        Ok(true)
    }
}

/// The whole set and the row that changed it, as one WAL frame.
fn write(store: &Store, tools: &BTreeMap<String, Tightening>, row: &LedgerRow) -> Result<()> {
    let stored = Stored {
        schema: 1,
        tools: tools.clone(),
    };
    store.append(&[
        NewRecord::json(kinds::META, Some(META_KEY), &stored)?,
        NewRecord::json(kinds::LEDGER, None, row)?,
    ])?;
    Ok(())
}

/// A stored tightening as the gate reads it. A posture this build does not
/// know reads as `approve`, the strictest.
pub fn as_tightened(t: &Tightening) -> Tightened<'_> {
    Tightened {
        posture: Posture::parse(&t.posture).unwrap_or(Posture::Approve),
        by: &t.by,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tightening(tool: &str, at_ms: u64) -> Tightening {
        Tightening {
            tool: tool.into(),
            posture: "approve".into(),
            by: "sock#1".into(),
            who: "sock#1".into(),
            via: "cli".into(),
            at_ms,
            ..Default::default()
        }
    }

    /// A tightening is in the store: a second open of the same directory
    /// reads it back, and the change and its ledger row are one frame.
    #[test]
    fn a_tightening_is_stored_with_its_row_and_read_back() {
        let d = tempfile::tempdir().unwrap();
        let store = Store::open(&d.path().join("store")).unwrap();
        let t = Tightenings::default();
        t.load(&store).unwrap();
        assert!(t.all().is_empty());
        let frames = store.stats().unwrap().frames_appended;
        let row = LedgerRow::new("policy.tightened", None, None, json!({"tool": "proc.run"}));
        assert!(t.insert(&store, tightening("proc.run", 2), &row).unwrap());
        assert!(t.insert(&store, tightening("fs.write", 1), &row).unwrap());
        assert_eq!(store.stats().unwrap().frames_appended - frames, 2);
        assert!(
            !t.insert(&store, tightening("proc.run", 9), &row).unwrap(),
            "a second press records nothing"
        );
        assert_eq!(store.stats().unwrap().frames_appended - frames, 2);
        let tools: Vec<String> = t.all().into_iter().map(|x| x.tool).collect();
        assert_eq!(tools, ["fs.write", "proc.run"], "oldest first");
        let undo = LedgerRow::new(
            "policy.untightened",
            None,
            None,
            json!({"tool": "fs.write"}),
        );
        assert!(t.remove(&store, "fs.write", &undo).unwrap());
        assert!(!t.remove(&store, "fs.write", &undo).unwrap());
        drop(store);

        let store = Store::open(&d.path().join("store")).unwrap();
        let again = Tightenings::default();
        again.load(&store).unwrap();
        assert_eq!(again.get("proc.run"), Some(tightening("proc.run", 2)));
        assert_eq!(again.get("fs.write"), None);
        let kinds: Vec<String> = store
            .ledger_tail::<LedgerRow>(10)
            .unwrap()
            .into_iter()
            .map(|(_, r)| r.kind)
            .collect();
        assert_eq!(
            kinds,
            ["policy.tightened", "policy.tightened", "policy.untightened"]
        );
        let unknown = Tightening {
            posture: "someday".into(),
            ..tightening("x", 0)
        };
        assert_eq!(as_tightened(&unknown).posture, Posture::Approve);
    }
}
