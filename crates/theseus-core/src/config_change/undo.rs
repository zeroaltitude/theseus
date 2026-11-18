//! `config.undo`: put back the settings ONE change made, to their values
//! before it (issue 22, undo).
//!
//! **It is `config.propose` in disguise.** What it builds is a list of
//! changes (this setting back to what it was, that one removed again), and
//! the same code takes it from there (`mod.rs`, `put`): the candidate is the
//! file as it is now with those changes made on its own text, loaded as a
//! start loads it; the card is built by the daemon from the two parsed
//! files; a sensitive setting is judged by the same list and the same seam
//! (undoing a sensitive change is a sensitive change); the yes is the same
//! question, the same write of one step, the same restart and the same way
//! back. Nothing here writes a file; it reads only the settings file and,
//! through `history.rs`, the kept copies.
//!
//! **It is not "write the old file back".** The file before the change is
//! read only for the values of the settings that change touched. A setting
//! the change did not touch keeps the value it has now, so what was changed
//! since, to other settings, stays. A setting that was changed again since
//! is put back too, and the card says so and shows both values.
//!
//! The answer's yes writes a `config.undone` row beside `config.changed`,
//! naming the change it undid and the settings that had been changed again.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;

use super::edit::{self, Change};
use super::history::{self, Entry};
use super::wall;
use crate::ledger::LedgerRow;
use crate::toolrun::{ToolRuntime, TurnCtx};

pub const UNDO: &str = "config.undo";

/// A setting that is put back though it no longer holds what the change set.
#[derive(Debug, Clone, PartialEq)]
pub struct Since {
    pub path: Vec<String>,
    /// What the change set it to; none when that is no longer kept.
    pub made: Option<Option<Value>>,
    pub now: Option<Value>,
    pub back: Option<Value>,
}

/// What an undo was built from.
#[derive(Debug, Clone)]
pub struct Undoes {
    pub number: u32,
    pub question: String,
    pub at_ms: u64,
    pub place: Option<String>,
    /// The settings put back that were changed since, or may have been.
    pub since: Vec<Since>,
    /// The settings that already hold their value from before the change.
    pub already: Vec<Vec<String>>,
}

/// An undo, planned: the changes to make and what the card says of them.
pub struct Planned {
    pub changes: Vec<Change>,
    pub undoes: Undoes,
}

/// `config.undo`'s input.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    #[serde(default)]
    change: Option<u32>,
    #[serde(default)]
    reason: String,
}

/// The change an input names (default: the newest) and the owner's reason.
pub fn input_of(input: &Value) -> Result<(Option<u32>, String), String> {
    let i: Input = theseus_tools::parse(input)?;
    Ok((i.change, super::card::reason_line(&i.reason)))
}

pub(super) fn target(entries: &[Entry], number: Option<u32>) -> Result<&Entry, String> {
    let e = match number {
        Some(n) => entries.iter().find(|e| e.number == n).ok_or_else(|| {
            format!("there is no change #{n} in the history: ask for the history to see the numbers")
        })?,
        None => entries
            .first()
            .ok_or("no change to Theseus's settings has been made through Theseus yet, so there is nothing to undo")?,
    };
    if let Some(by) = e.undone_by {
        return Err(format!(
            "change #{} was already undone, by change #{by}: to have it back, undo #{by}",
            e.number
        ));
    }
    Ok(e)
}

/// The undo of change `number` (default: the newest) on the file `base`, from
/// the history's `entries`.
pub fn plan(entries: &[Entry], number: Option<u32>, base: &str) -> Result<Planned, String> {
    let e = target(entries, number)?;
    let before = e.before_doc.as_ref().ok_or_else(|| {
        format!(
            "the settings from before change #{} are no longer kept (only the last {} changes' \
             are), so it cannot be undone",
            e.number,
            super::write::KEEP
        )
    })?;
    let now = history::doc_of(base).ok_or("the settings file as it is now does not parse")?;
    let mut changes = Vec::new();
    let mut since = Vec::new();
    let mut already = Vec::new();
    for p in &e.paths {
        let (was, is) = (history::value_at(before, p), history::value_at(&now, p));
        if was == is {
            already.push(p.clone());
            continue;
        }
        changes.push(match &was {
            Some(v) => Change::Set(p.clone(), v.clone()),
            None => Change::Remove(p.clone()),
        });
        let made = e.after_doc.as_ref().map(|d| history::value_at(d, p));
        if made.as_ref() != Some(&is) {
            since.push(Since {
                path: p.clone(),
                made,
                now: is,
                back: was,
            });
        }
    }
    if changes.is_empty() {
        return Err(format!(
            "there is nothing to undo: every setting change #{} touched already has the value it \
             had before it{}",
            e.number,
            match &e.rolled_back {
                Some(_) => " (Theseus went back to those settings after its restart)",
                None => "",
            }
        ));
    }
    Ok(Planned {
        changes,
        undoes: Undoes {
            number: e.number,
            question: e.question.clone(),
            at_ms: e.at_ms,
            place: e.place.clone(),
            since,
            already,
        },
    })
}

impl Undoes {
    /// What the question carries, so the yes can write its row: the change
    /// undone and the settings that were changed again.
    pub fn args(&self) -> Value {
        json!({
            "number": self.number,
            "question": self.question,
            "since": self.since.iter().map(|s| edit::dotted(&s.path)).collect::<Vec<_>>(),
        })
    }
}

/// The card's own lines for an undo: what it does, and what it overwrites.
pub fn preface(u: &Undoes, now_ms: u64) -> String {
    let when = theseus_protocol::human_time::ago(now_ms, u.at_ms);
    let mut out = format!(
        "This puts back the settings change #{} made ({when}) to their values before it. \
         Everything else stays as it is now, including changes made since to other settings.\n",
        u.number
    );
    if !u.already.is_empty() {
        let names: Vec<String> = u.already.iter().map(|p| edit::dotted(p)).collect();
        out.push_str(&format!(
            "Already as they were before it, so not listed: {}.\n",
            names.join(", ")
        ));
    }
    if !u.since.is_empty() {
        out.push_str("Changed again since, so this overwrites a later change:\n");
        for s in &u.since {
            let made = match &s.made {
                Some(m) => format!("it set {}", super::diff::words(m.as_ref())),
                None => "what it set is no longer kept".into(),
            };
            out.push_str(&format!(
                "- {}: {made}; it is now {}; this puts back {}\n",
                edit::dotted(&s.path),
                super::diff::words(s.now.as_ref()),
                super::diff::words(s.back.as_ref())
            ));
        }
    }
    out
}

/// The `config.undone` row a yes writes, beside `config.changed`.
pub fn undone_row(
    session: &str,
    question: &str,
    undoes: &Value,
    args: &Value,
    who: &Value,
    by: &str,
    via: &str,
) -> LedgerRow {
    LedgerRow::new(
        LedgerKind::ConfigUndone,
        Some(session),
        None,
        json!({
            "question": question, "undone": undoes["number"],
            "undone_question": undoes["question"], "paths": args["paths"],
            "changed_since": undoes["since"], "asked_by": who,
            "approved_by": by, "via": via,
        }),
    )
}

/// Run `config.undo` for the call `correlation_id` of the turn `tc`: plan the
/// undo from the history, then put it to the owner exactly as a proposal is.
pub fn run(
    rt: &ToolRuntime,
    tc: &TurnCtx<'_>,
    input: &Value,
    correlation_id: &str,
) -> Result<(String, Value), String> {
    if let Some(why) = wall::refusal_for(tc, UNDO) {
        return Err(why);
    }
    // Run again after a restart, it answers with what the first run asked.
    if let Ok(done) = super::proposed_by_call(tc, correlation_id) {
        return Ok(done);
    }
    let (number, reason) = input_of(input)?;
    let base = super::read_base(rt)?;
    let w = &rt.config_change;
    let rows = history::rows(tc.store);
    let list = history::entries(&rows, &w.state, w.file.as_deref(), history::SHOWN);
    // A change to a prompt file is undone by its own text (issue 23).
    let e = target(&list, number)?;
    if let Some(f) = e.file.as_ref() {
        let core = w
            .core()
            .ok_or("Theseus is not ready to undo: try again in a moment")?;
        return super::prompt_file::undo::run(rt, tc, &core, correlation_id, e, f, &reason);
    }
    let planned = plan(&list, number, &base)?;
    let undoes = Some(&planned.undoes);
    super::put(
        rt,
        tc,
        correlation_id,
        &base,
        &planned.changes,
        &reason,
        undoes,
    )
}

/// `config.undo`.
pub struct Undo;

impl theseus_tools::Tool for Undo {
    fn name(&self) -> &str {
        UNDO
    }

    fn description(&self) -> &str {
        "Propose putting back the settings ONE earlier change made, to their values before it, on \
         the owner's word. `change` is the number `config.history` gives (default: the newest \
         change). Settings that change did not touch keep their values now. The owner gets the \
         same card as for `config.propose`, with each setting before and after, and a line for \
         any setting that was changed again since; only their yes changes anything, and the \
         daemon writes it itself, with the same restart and way back. A change to a setting that \
         governs what Theseus may do needs a second proof as well. Offered only to the owner, in \
         a private place."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "change": {"type": "integer", "description": "The change's number, from the history; the newest when left out."},
                "reason": {"type": "string", "description": "One line, in the owner's words: why they want it back."}
            },
            "additionalProperties": false
        })
    }

    fn class(&self) -> theseus_tools::ToolClass {
        theseus_tools::ToolClass::Run
    }

    fn backend(&self) -> theseus_tools::Backend {
        theseus_tools::Backend::Harness
    }

    fn retry(&self) -> theseus_tools::Retry {
        // Run again after a restart, it answers with what it asked.
        theseus_tools::Retry::NonRepeatable
    }

    fn plan(
        &self,
        input: &Value,
        _ctx: &theseus_tools::ToolCtx,
    ) -> Result<theseus_tools::Plan, String> {
        let (n, _) = input_of(input)?;
        Ok(theseus_tools::Plan {
            summary: match n {
                Some(n) => format!("propose undoing settings change #{n}"),
                None => "propose undoing the newest settings change".into(),
            },
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests;
