//! The one-time cleanup of entries Theseus made by mistake (issue 9; the
//! owner's yes of 2026-11-14 to the rule below).
//!
//! The self-improvement loop's own instruction to its builder was read as a
//! message from a person, so entries were made for throwaway build
//! directories, with the loop's own conversations filed under them. The
//! writes that stop it happening are `Inbound.own` and the state directory
//! rule; this retires what was already made, and the daemon does it itself,
//! after serving, in the sweep:
//!
//! - **Which entries.** An entry of the kind that holds folders and files
//!   (`resource`), that a machine made (`added_by` `jev:…`), with no guidance
//!   (the ontology's own rule), and with at least one conversation filed under
//!   it, **every one of which is a conversation of the self-improvement loop's
//!   own** (a task whose origin is the `self` principal), each filed by a
//!   machine. An entry the operator or an import made, or one any other
//!   conversation is filed under, is never touched.
//! - **One write each.** The membership removals and the retirement are one
//!   machine write of the sweep's (`sweep-<ms>-<n>-tidy`), so
//!   `theseus ontology undo <id>` puts the entry and its filings back exactly.
//! - **Once.** A marker in the store (`onto.tidied`) says it ran, so a later
//!   night does not look again. A store with nothing to retire writes no
//!   marker (the sweep writes nothing it need not), and its later nights'
//!   look is one read of the ontology.
//! - **Said in the morning note**: one line a write, with its undo id
//!   (`digest.rs`).

use std::collections::BTreeSet;

use theseus_ontology::{MemberList, Origin, Record};

use super::sweep::Swept;
use crate::rpc::Core;

/// The META key whose presence says the cleanup ran.
const DONE: &str = "onto.tidied";
/// The kind that holds folders and files.
const KIND: &str = "resource";

impl Core {
    /// The sessions that are the self-improvement loop's own: a task whose
    /// origin is the self principal.
    fn self_loop_sessions(&self) -> BTreeSet<String> {
        crate::task_graph::all(&self.store)
            .unwrap_or_default()
            .into_iter()
            .filter(|t| t.origin.principal == crate::rsi::SELF_PRINCIPAL)
            .filter_map(|t| t.session)
            .collect()
    }

    /// The cleanup, once: each entry the rule names is one write of the
    /// sweep's, `n` counting from `first`.
    pub(super) fn onto_tidy(
        &self,
        now_ms: u64,
        first: usize,
        out: &mut Swept,
    ) -> anyhow::Result<()> {
        if self.store.get_meta::<u64>(DONE)?.is_some() {
            return Ok(());
        }
        let o = self.runner.ontology.snapshot(&self.store)?;
        let own = self.self_loop_sessions();
        let mut n = 0;
        for c in o
            .categories()
            .filter(|c| c.kind() == KIND && !c.is_gone() && c.added_by.starts_with("jev:"))
        {
            let lists: Vec<&MemberList> = o
                .lists()
                .filter(|l| l.kind == KIND && l.members.iter().any(|m| m.category == c.id))
                .collect();
            let all_own = lists.iter().all(|l| {
                own.contains(&l.session)
                    && l.members
                        .iter()
                        .filter(|m| m.category == c.id)
                        .all(|m| m.origin.is_machine())
            });
            if lists.is_empty() || !all_own {
                continue;
            }
            let mut records: Vec<Record> = lists
                .iter()
                .map(|l| {
                    Record::Members(MemberList {
                        session: l.session.clone(),
                        kind: l.kind.clone(),
                        members: (l.members.iter())
                            .filter(|m| m.category != c.id)
                            .cloned()
                            .collect(),
                    })
                })
                .collect();
            records.push(Record::Category(c.retired(now_ms)));
            let write = format!("sweep-{now_ms}-{}-tidy", first + n);
            n += 1;
            match self.ontology_machine_write(Origin::Sweep, &write, records) {
                Ok(()) => {
                    out.tidied += 1;
                    out.writes.push(write);
                }
                Err(e) => {
                    out.failed += 1;
                    tracing::warn!(write, error = %format!("{e:#}"), "ontology: an entry was not retired");
                }
            }
        }
        // Said only once something was retired: a store with nothing to retire
        // writes nothing, and a later night's look is one read of the ontology.
        if out.tidied > 0 {
            self.store.put_meta(DONE, &now_ms)?;
        }
        Ok(())
    }
}
