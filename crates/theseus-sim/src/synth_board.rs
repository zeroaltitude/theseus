//! A synthetic people-and-topics board (theseus-mem-daemon-rest): parked
//! sessions of `synth`, and the ontology's records for them, of the owner's
//! shape (414 categories, about 71,000 memberships over 51,000 sessions), for
//! the memory the daemon holds for the board. Names are invented; the same
//! arguments write the same store.

use std::path::Path;
use std::time::Instant;

use anyhow::Result;
use theseus_core::store::Store;
use theseus_ontology::{Category, CategoryId, MemberList, Membership, Origin, Record};
use theseus_store::{kinds, NewRecord};

use crate::synth::{write_sessions, Generated};

/// An invented, repeatable base time for the memberships' `as_of_ms`.
const AS_OF_MS: u64 = 1_790_000_000_000;
/// Records in one frame.
const PER_FRAME: usize = 500;

/// Every sixth category is a person, the rest topics, as a board of people
/// and topics has them.
fn category(k: u64) -> Result<Category> {
    let (kind, name) = match k % 6 {
        5 => ("person", format!("Person {k}")),
        _ => ("topic", format!("Topic {k}")),
    };
    let id = CategoryId::new(kind, &format!("synth-{k}"))?;
    Ok(Category::new(id, name, Origin::Import.name()))
}

/// Sessions' parked records, the board's categories, and `memberships` of
/// them spread over the sessions, a topic list and a person list each at most
/// (a topic list holds at most three entries, a category once). Which
/// category a membership names is a cube-law draw, so a few are held by many
/// sessions and most by few, as a real board's are.
pub fn generate(dir: &Path, sessions: u64, categories: u64, memberships: u64) -> Result<Generated> {
    let t0 = Instant::now();
    let store = Store::open(dir)?;
    let (sids, mut records, mut frames) = write_sessions(&store, sessions)?;
    let cats = (0..categories)
        .map(category)
        .collect::<Result<Vec<Category>>>()?;
    let mut out: Vec<NewRecord> = Vec::new();
    for c in &cats {
        let r = Record::Category(c.clone());
        out.push(NewRecord::json(kinds::META, Some(&r.key()), &r.to_value())?);
    }
    let draw = |salt: u64| -> usize {
        let h = salt.wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 40;
        let u = (h % 1_000_003) as f64 / 1_000_003.0;
        ((u * u * u) * cats.len() as f64) as usize % cats.len()
    };
    let n = sids.len() as u64;
    for (k, sid) in sids.iter().enumerate() {
        let k = k as u64;
        // Spread `memberships` evenly: session k holds the difference of the
        // running totals.
        let want =
            (memberships * (k + 1)).div_ceil(n.max(1)) - (memberships * k).div_ceil(n.max(1));
        let (mut topics, mut people): (Vec<Membership>, Vec<Membership>) = (vec![], vec![]);
        for j in 0..want {
            let pick = draw(k * 8 + j);
            // The first category of the right shape that this list lacks.
            for step in 0..cats.len() {
                let c = &cats[(pick + step) % cats.len()];
                let list = if c.kind() == "person" {
                    &mut people
                } else {
                    &mut topics
                };
                if list.iter().all(|m| m.category != c.id)
                    && (c.kind() == "person" || list.len() < 3)
                {
                    list.push(Membership {
                        category: c.id.clone(),
                        origin: Origin::Import,
                        confidence: None,
                        as_of_ms: AS_OF_MS + k,
                    });
                    break;
                }
            }
        }
        for (kind, members) in [("topic", topics), ("person", people)] {
            if members.is_empty() {
                continue;
            }
            let r = Record::Members(MemberList {
                session: sid.clone(),
                kind: kind.to_string(),
                members,
            });
            out.push(NewRecord::json(kinds::META, Some(&r.key()), &r.to_value())?);
        }
    }
    for chunk in out.chunks(PER_FRAME) {
        records += chunk.len() as u64;
        store.append(chunk)?;
        frames += 1;
    }
    store.checkpoint()?;
    let wal_bytes = store.stats()?.wal_bytes;
    Ok(Generated {
        sessions,
        records,
        frames,
        wal_bytes,
        ms: t0.elapsed().as_secs_f64() * 1000.0,
    })
}
