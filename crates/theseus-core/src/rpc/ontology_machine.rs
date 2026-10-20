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

/// A machine write, checked and ready: its records, and what each replaced
/// (`before`, read from the snapshot the write was checked against).
pub struct Machine {
    by: Origin,
    write: String,
    records: Vec<Record>,
    before: Vec<Option<Record>>,
    undos: Vec<Undoable>,
}

impl Machine {
    /// `by`'s write `write` of `records` (categories and lists), against the
    /// snapshot `o`: each record's row will carry the record as it was.
    pub fn new(
        o: &theseus_ontology::Ontology,
        by: Origin,
        write: &str,
        records: Vec<Record>,
    ) -> Result<Self> {
        if !by.is_machine() {
            bail!("a machine write is Jev's, the sweep's or the dream's, not `{by}`'s");
        }
        let before = o.replaced(&records);
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
        Ok(Self {
            by,
            write: write.to_string(),
            records,
            before,
            undos,
        })
    }

    /// The records to write.
    pub fn records(&self) -> &[Record] {
        &self.records
    }

    /// Whether `now` holds what the write was checked against: else another
    /// write came between, and its rows would undo the wrong thing.
    pub fn holds_on(&self, now: &theseus_ontology::Ontology) -> bool {
        now.replaced(&self.records) == self.before
    }

    fn who(&self) -> String {
        format!("{}:{}", self.by, self.write)
    }

    /// The ledger row of each record, with what undoes it.
    pub fn rows(&self) -> Result<Vec<theseus_store::NewRecord>> {
        let (who, scope) = (self.who(), scope(&self.write));
        let mut rows = Vec::new();
        for (r, undo) in self.records.iter().zip(&self.undos) {
            match r {
                Record::Category(category) => {
                    let f = fact::ontology::CategorySet {
                        category,
                        origin: self.by.name(),
                        who: &who,
                        via: "machine",
                        undo: Some(undo),
                    };
                    rows.push(fact::row(&f, None, None)?.scoped(&scope));
                }
                Record::Members(list) => {
                    let f = fact::ontology::MembershipSet {
                        list,
                        who: &who,
                        via: "machine",
                        undo: Some(undo),
                    };
                    rows.push(fact::row(&f, Some(&list.session), None)?.scoped(&scope));
                }
                _ => {}
            }
        }
        Ok(rows)
    }

    /// Say each written record, once its frame is.
    pub fn announce(&self, core: &Core) {
        let who = self.who();
        for (r, undo) in self.records.iter().zip(&self.undos) {
            match r {
                Record::Category(category) => {
                    core.rec(None).announce(&fact::ontology::CategorySet {
                        category,
                        origin: self.by.name(),
                        who: &who,
                        via: "machine",
                        undo: Some(undo),
                    })
                }
                Record::Members(list) => {
                    core.rec(Some(&list.session))
                        .announce(&fact::ontology::MembershipSet {
                            list,
                            who: &who,
                            via: "machine",
                            undo: Some(undo),
                        })
                }
                _ => {}
            }
        }
    }
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
        let o = self.runner.ontology.snapshot(&self.store)?;
        let m = Machine::new(&o, by, write, records)?;
        self.runner
            .ontology
            .write(&self.store, m.records().to_vec(), by, |now| {
                // The snapshot the write checked against is the one `before`
                // was read from: else the rows would undo another's write.
                if !m.holds_on(now) {
                    bail!("the ontology changed under the write `{write}`: it is not made");
                }
                m.rows()
            })?;
        m.announce(self);
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
