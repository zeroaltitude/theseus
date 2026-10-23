//! Jev offers a link once (theseus-98bz.11). When Jev admits notes of another live thread into a
//! thread's recall (`recall_cross.v1`, `turn/cross_step.rs`), the pair is counted once a recall;
//! at its second recall, one sentence to the owner's DM offers `theseus thread link`. Nothing is
//! linked by it, a pair is asked at most once whatever the owner does, and the count is written
//! in a task of its own after the recall, never before the request leaves.
//!
//! The count is the META record `thread.link_seen.<a>.<b>` (the slugs sorted): `{recalls, asked}`.
//! The post is a `link_card` (theseus-98bz.12) naming the two slugs: Discord draws it with **Link
//! these threads** and **Not needed**; its last line says the CLI form for a DM with no buttons.

use std::sync::{Mutex, PoisonError};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use theseus_store::{kinds, NewRecord};

use crate::fact;
use crate::fact::thread::ThreadLinkProposed;
use crate::outbox::Outbox;
use crate::store::Store;
use crate::turn::TurnRunner;

/// The recalls of a pair that earn its proposal.
const RECALLS: u32 = 2;

/// Counts read and written one at a time: two turns may admit the same pair at once.
static COUNTING: Mutex<()> = Mutex::new(());

/// What Jev's yes has joined, for one pair of threads.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Seen {
    recalls: u32,
    asked: bool,
}

/// A thread asked, and another whose notes answered it: slug and title of each.
#[derive(Debug, Clone, PartialEq)]
pub struct Pair {
    pub here: (String, String),
    pub other: (String, String),
}

impl Pair {
    /// The META key of the pair's count, the slugs in order.
    pub fn key(&self) -> String {
        let (a, b) = (&self.here.0, &self.other.0);
        let (x, y) = if a <= b { (a, b) } else { (b, a) };
        format!("thread.link_seen.{x}.{y}")
    }

    pub(crate) fn text(&self) -> String {
        crate::owner_note::note(
            crate::owner_note::Kind::Decision,
            &format!("Link threads «{}» and «{}»?", self.here.1, self.other.1),
            &format!(
                "Notes of «{}» answered questions in «{}» twice.",
                self.other.1, self.here.1
            ),
            &format!("theseus thread link {} {}", self.here.0, self.other.0),
        )
    }
}

/// One recall of `pair`: counted, and proposed at its second when `dm` can take it.
fn count(store: &Store, outbox: &Outbox, pair: &Pair, text: &str, dm: bool) -> Result<()> {
    let _one = COUNTING.lock().unwrap_or_else(PoisonError::into_inner);
    let key = pair.key();
    let mut seen: Seen = store.get_meta(&key)?.unwrap_or_default();
    if seen.asked {
        return Ok(());
    }
    seen.recalls += 1;
    if seen.recalls < RECALLS || !dm {
        return store.put_meta(&key, &seen);
    }
    seen.asked = true;
    let body = json!({
        "kind": "link_card", "text": text, "here": pair.here.0, "other": pair.other.0,
    });
    let (post, more) = outbox.stage_to_operator(None, body)?;
    let mut frame = vec![
        NewRecord::json(kinds::META, Some(&key), &seen)?,
        fact::row(
            &ThreadLinkProposed {
                thread: &pair.here.0,
                other: &pair.other.0,
                recalls: seen.recalls,
            },
            None,
            None,
        )?,
    ];
    frame.extend(more);
    store.append(&frame)?;
    outbox.posted(&post);
    Ok(())
}

impl TurnRunner {
    /// Count each pair once for the recall that just admitted notes across it. Spawned: the write
    /// and the post belong to no turn's frames, and nothing waits for them.
    pub(crate) fn link_seen(&self, pairs: Vec<Pair>) {
        if pairs.is_empty() || !self.cfg.threads.link_proposals {
            return;
        }
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let (store, outbox) = (self.store.clone(), self.outbox.clone());
        let dm = self.place_rule.owner_dm_bound();
        let texts: Vec<String> = pairs
            .iter()
            .map(|p| self.tools.scrubber.scrub(&p.text()).0)
            .collect();
        rt.spawn_blocking(move || {
            for (pair, text) in pairs.iter().zip(&texts) {
                if let Err(e) = count(&store, &outbox, pair, text, dm) {
                    tracing::warn!(error = %format!("{e:#}"), "a link proposal's count was not written");
                }
            }
        });
    }
}
