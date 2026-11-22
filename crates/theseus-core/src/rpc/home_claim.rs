//! The undo of a home claim (theseus-98bz.6): `thread.unclaim`.
//!
//! Until 2026-11-21 a turn the router ran at a place's home, whose files all fell in one live
//! thread's working set, was claimed for that thread: each of its nodes was keyed
//! `moved.of.<node>` = `MovedTo { thread, split: "claim_…" }`, with the claim's record (META
//! `thread.claim.<id>`) and its `thread.claimed` row. The owner removed routing by a guess that
//! day, and the claim with it: nothing claims a turn now. The claims already in a store still
//! read (the lens and the history read the keys as they read a split's), and this is their undo:
//!
//! - **`thread.unclaim`** writes each key back to null where it still holds this claim's value (a
//!   later split's is kept), marks the record undone and writes `thread.unclaimed`.
//! - **A split beside the undo** (theseus-l4n39). The undo reads the nodes' keys and writes in one
//!   frame under the home session's execution lock, the lock a split's frame (`write_moved`, and an
//!   unsplit's) holds while it writes its keys: a split lands wholly before the read or after the
//!   write.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use theseus_protocol::threads::{ThreadUnclaimParams, ThreadUnclaimResult};
use theseus_protocol::{error_code, method};
use theseus_store::{kinds, NewRecord};

use super::server::RpcFailure;
use super::Core;
use crate::approval::Answerer;
use crate::fact;
use crate::fact::thread::ThreadUnclaimed;
use crate::moved::{self, MovedTo};

/// The META key of a claim's record, which `thread.unclaim` reads.
pub fn claim_key(id: &str) -> String {
    format!("thread.claim.{id}")
}

/// What a claim wrote (an earlier build), for its undo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimRecord {
    pub id: String,
    pub thread: String,
    /// The home session the turn ran in, and the turn.
    pub session: String,
    pub turn: String,
    /// The nodes keyed to the thread.
    pub nodes: Vec<String>,
    /// The turn's paths the thread holds.
    pub paths: Vec<String>,
    pub at_ms: u64,
    #[serde(default)]
    pub undone: bool,
}

impl Core {
    /// `build`'s records written in one frame under `exec`'s lock (the home session's execution),
    /// `build` reading what it needs inside it; with no execution (no turn and no split can hold
    /// one), a plain append. Whether anything was written.
    fn under_exec(
        &self,
        exec: Option<&str>,
        build: impl FnOnce(&crate::store::Store) -> Result<Option<Vec<NewRecord>>>,
    ) -> Result<bool> {
        match exec {
            Some(e) => self.kernel.frame(&[e], |k| match build(&self.store)? {
                Some(records) => {
                    k.stage(&records)?;
                    Ok(true)
                }
                None => Ok(false),
            }),
            None => match build(&self.store)? {
                Some(records) => {
                    self.store.append(&records)?;
                    Ok(true)
                }
                None => Ok(false),
            },
        }
    }

    /// `thread.unclaim`: the claim `p.claim` undone. `restored` counts the nodes whose key still
    /// held this claim's value, now null; `kept` those whose key holds another claim's or a split's
    /// value, left as it is. A node whose key is null already (an unsplit over the claim) is
    /// neither.
    pub fn thread_unclaim(
        &self,
        p: &ThreadUnclaimParams,
        who: impl Into<Answerer>,
    ) -> Result<ThreadUnclaimResult, RpcFailure> {
        let who = who.into();
        let id = p.claim.trim();
        let rec: ClaimRecord = self
            .store
            .get_meta(&claim_key(id))
            .map_err(RpcFailure::invalid)?
            .ok_or_else(|| {
                RpcFailure::new(
                    error_code::NOT_FOUND,
                    format!(
                        "no claim {id}: `thread.unclaim`'s `claim` is a claim's id (`claim_…`), \
                         as `theseus history` and the `thread.claimed` row name it"
                    ),
                )
            })?;
        self.judge_thread(&who, method::THREAD_UNCLAIM, &rec.thread)?;
        if rec.undone {
            return Err(RpcFailure::invalid(anyhow::anyhow!(
                "the claim {id} was undone already: `thread.unclaim`'s `claim` names one that holds"
            )));
        }
        let mine = MovedTo {
            thread: rec.thread.clone(),
            split: rec.id.clone(),
        };
        let (who_s, via) = (who.who(), who.via());
        let (mut restored, mut kept) = (0, 0);
        let exec = self
            .store
            .get_session::<crate::session::SessionRecord>(&rec.session)
            .map_err(RpcFailure::invalid)?
            .and_then(|s| s.execution_id);
        // Under the home's execution lock, as the claim was written: a split's keys land wholly
        // before or after this read.
        self.under_exec(exec.as_deref(), |store| {
            let mut records = Vec::new();
            for node in &rec.nodes {
                let key = moved::moved_of_key(node);
                let to: Option<Option<MovedTo>> = store.get_meta(&key)?;
                match to.flatten() {
                    Some(m) if m == mine => {
                        records.push(NewRecord::json(kinds::META, Some(&key), &None::<MovedTo>)?);
                        restored += 1;
                    }
                    // Another claim's or a split's value: kept.
                    Some(_) => kept += 1,
                    // Nulled already (an unsplit over the claim): neither.
                    None => {}
                }
            }
            let fact = ThreadUnclaimed {
                claim: &rec.id,
                thread: &rec.thread,
                session: &rec.session,
                restored,
                kept,
                who: &who_s,
                via: &via,
            };
            let done = ClaimRecord {
                undone: true,
                ..rec.clone()
            };
            records.push(NewRecord::json(
                kinds::META,
                Some(&claim_key(&rec.id)),
                &done,
            )?);
            records.push(fact::row(&fact, Some(&rec.session), None)?);
            Ok(Some(records))
        })
        .map_err(RpcFailure::invalid)?;
        let fact = ThreadUnclaimed {
            claim: &rec.id,
            thread: &rec.thread,
            session: &rec.session,
            restored,
            kept,
            who: &who_s,
            via: &via,
        };
        self.rec(Some(&rec.session)).announce(&fact);
        Ok(ThreadUnclaimResult {
            claim: rec.id,
            thread: rec.thread,
            session: rec.session,
            restored,
            kept,
        })
    }
}
