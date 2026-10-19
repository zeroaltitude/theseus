//! A machine origin's writes to the ontology, and their undo (onto-core,
//! theseus-e3mjc): Jev, the sweep and the dream file categories and lists
//! beside the operator, and each write is one frame whose rows carry what
//! takes it back (`fact::ontology::Undoable`). The undo is the operator's
//! act: it writes the inverse records and an `ontology.undo` row in one
//! frame. The methods that call these are later rows' (R3's RPC and CLI).

use anyhow::{anyhow, bail, Result};
use serde::Deserialize;
use theseus_ontology::{Origin, Record};
use theseus_protocol::now_unix_ms;

use super::Core;
use crate::fact::{self, ontology::Undoable};
use crate::ledger::LedgerRow;

/// The scope of one machine write's rows, and of its undo's: an undo reads
/// the write back by one scan of it.
pub fn scope(write: &str) -> String {
    format!("ontology.write:{write}")
}

/// What a row of a machine write holds for its undo.
#[derive(Deserialize)]
struct Held {
    key: String,
    wrote: serde_json::Value,
    before: serde_json::Value,
}

impl Core {
    /// `by`'s write, which has the id `write` (a judgment's, a sweep's, a
    /// dream's): `records` (categories and lists) checked as `by`'s and
    /// written in one frame, each with its ledger row carrying the record as
    /// it was, so [`Core::ontology_undo`] can restore it.
    pub fn ontology_machine_write(
        &self,
        by: Origin,
        write: &str,
        records: Vec<Record>,
    ) -> Result<()> {
        if !by.is_machine() {
            bail!("a machine write is Jev's, the sweep's or the dream's, not `{by}`'s");
        }
        let o = self.runner.ontology.snapshot(&self.store)?;
        let before = o.replaced(&records);
        let who = format!("{by}:{write}");
        let mut undos = Vec::with_capacity(records.len());
        for (r, b) in records.iter().zip(&before) {
            if !matches!(r, Record::Category(_) | Record::Members(_)) {
                bail!(
                    "a machine write of a kind or guidance is not built: `{}` (kinds come with the \
                     sweep's row)",
                    r.key()
                );
            }
            undos.push(Undoable {
                write: write.to_string(),
                key: r.key(),
                wrote: r.to_value(),
                before: b.as_ref().map_or(serde_json::Value::Null, Record::to_value),
            });
        }
        let (mut cats, mut lists) = (Vec::new(), Vec::new());
        for (r, undo) in records.iter().zip(&undos) {
            match r {
                Record::Category(category) => cats.push(fact::ontology::CategorySet {
                    category,
                    origin: by.name(),
                    who: &who,
                    via: "machine",
                    undo: Some(undo),
                }),
                Record::Members(list) => lists.push(fact::ontology::MembershipSet {
                    list,
                    who: &who,
                    via: "machine",
                    undo: Some(undo),
                }),
                _ => {}
            }
        }
        let scope = scope(write);
        self.runner
            .ontology
            .write(&self.store, records.clone(), by, |now| {
                // The snapshot the write checked against is the one `before`
                // was read from: else the rows would undo another's write.
                if now.replaced(&records) != before {
                    bail!("the ontology changed under the write `{write}`: it is not made");
                }
                let mut rows = Vec::new();
                for f in &cats {
                    rows.push(fact::row(f, None, None)?.scoped(&scope));
                }
                for f in &lists {
                    rows.push(fact::row(f, Some(&f.list.session), None)?.scoped(&scope));
                }
                Ok(rows)
            })?;
        for f in &cats {
            self.rec(None).announce(f);
        }
        for f in &lists {
            self.rec(Some(&f.list.session)).announce(f);
        }
        Ok(())
    }

    /// The machine write `write` taken back, from its rows alone: what each
    /// record replaced is restored byte for byte, and a category it minted is
    /// retired, unless another session's list names it now (`InUse`). One
    /// frame: the inverse records and the `ontology.undo` row (label
    /// `wrong`), written as the operator. A write is undone once.
    pub fn ontology_undo(&self, write: &str, who: &str, via: &str) -> Result<Vec<String>> {
        let scope = scope(write);
        let (mut wrote, mut before) = (Vec::new(), Vec::new());
        for r in self.store.scope_after(&scope, 0)? {
            let Ok(row) = r.decode::<LedgerRow>() else {
                continue;
            };
            if row.kind == theseus_protocol::LedgerKind::OntologyUndo.as_str() {
                bail!("the write {write} was undone already");
            }
            let Some(held) = row.data.get("undo").cloned() else {
                continue;
            };
            let held: Held = serde_json::from_value(held)?;
            wrote.push(Record::decode(&held.key, held.wrote)?);
            before.push(match held.before {
                serde_json::Value::Null => None,
                v => Some(Record::decode(&held.key, v)?),
            });
        }
        if wrote.is_empty() {
            return Err(anyhow!("no machine write {write} to undo"));
        }
        let o = self.runner.ontology.snapshot(&self.store)?;
        let records = o.undo(&wrote, &before, now_unix_ms())?;
        let keys: Vec<String> = records.iter().map(Record::key).collect();
        let row = fact::ontology::OntologyUndo {
            write,
            label: "wrong",
            keys: &keys,
            who,
            via,
        };
        self.runner
            .ontology
            .write(&self.store, records, Origin::Operator, |_| {
                Ok(vec![fact::row(&row, None, None)?.scoped(&scope)])
            })?;
        self.rec(None).announce(&row);
        Ok(keys)
    }
}
