//! Proposed standing records (theseus-a8n7): the feedback `persona.import`
//! finds in an agent's notes, kept as META records `standing.proposal:<id>`
//! until the owner answers. A proposal reaches no prompt: only a keep writes
//! a standing record (scoped to the persona), and a dismissed or kept
//! proposal is never proposed again. State changes are new versions of the
//! key (append-only), each in one frame with its ledger row.

use anyhow::{anyhow, bail, Result};
use theseus_protocol::{ProposedStanding, StandingProposal};
use theseus_store::{kinds, NewRecord, Store as _};

use crate::store::Store;

/// The META key prefix of a proposal.
pub const PREFIX: &str = "standing.proposal:";

pub fn key(id: &str) -> String {
    format!("{PREFIX}{id}")
}

/// Every proposal, newest version of each, oldest first. A read for the
/// owner's commands, never a turn's path.
pub fn all(store: &Store) -> Result<Vec<StandingProposal>> {
    let rows = theseus_store::blocking(|| store.inner().latest_with_prefix(kinds::META, PREFIX))?;
    let mut out: Vec<StandingProposal> = rows
        .into_iter()
        .filter_map(|r| match r.decode::<StandingProposal>() {
            Ok(p) => Some(p),
            Err(e) => {
                tracing::warn!(error = %e, "a standing proposal does not read: it is skipped");
                None
            }
        })
        .collect();
    out.sort_by(|a, b| a.proposed_ms.cmp(&b.proposed_ms).then(a.id.cmp(&b.id)));
    Ok(out)
}

/// The proposals `found` that are new, as records for the caller's frame,
/// and how many were known already (open, kept or dismissed).
pub fn new_records(
    store: &Store,
    persona: &str,
    found: &[ProposedStanding],
    now_ms: u64,
) -> Result<(Vec<NewRecord>, Vec<StandingProposal>, u32)> {
    let known: std::collections::BTreeSet<String> = all(store)?.into_iter().map(|p| p.id).collect();
    let mut records = Vec::new();
    let mut fresh = Vec::new();
    let mut seen = 0;
    for f in found {
        if known.contains(&f.id) {
            seen += 1;
            continue;
        }
        let p = StandingProposal {
            id: f.id.clone(),
            persona: persona.to_string(),
            kind: f.kind,
            text: f.text.clone(),
            from: f.from.clone(),
            proposed_ms: now_ms,
            state: "open".into(),
            record: None,
        };
        records.push(NewRecord::json(kinds::META, Some(&key(&p.id)), &p)?);
        fresh.push(p);
    }
    Ok((records, fresh, seen))
}

/// The open proposal `id` names: a whole id, or the start of one open
/// proposal's.
pub fn resolve(all: &[StandingProposal], id: &str) -> Result<StandingProposal> {
    let id = id.trim();
    if id.is_empty() {
        bail!("name a proposal: `theseus standing proposals` lists them");
    }
    let open: Vec<&StandingProposal> = all.iter().filter(|p| p.state == "open").collect();
    if let Some(p) = open.iter().find(|p| p.id == id) {
        return Ok((*p).clone());
    }
    let starts: Vec<&&StandingProposal> = open.iter().filter(|p| p.id.starts_with(id)).collect();
    match starts.as_slice() {
        [one] => Ok((***one).clone()),
        [] => match all.iter().find(|p| p.id == id) {
            Some(p) => Err(anyhow!("proposal {} is already {}", p.id, p.state)),
            None => Err(anyhow!(
                "no open proposal {id}: `theseus standing proposals` lists them"
            )),
        },
        many => Err(anyhow!(
            "{id} starts {} open proposals ({}): give more of the id",
            many.len(),
            many.iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}
