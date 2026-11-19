//! `config.history`: the last changes the owner approved to Theseus's own
//! settings, newest first (issue 22, undo).
//!
//! **Built by the daemon, from what it keeps**, as the card is: the ledger's
//! `config.changed` rows say when, which settings, who asked and who approved
//! (never a value); the kept copies (`write.rs`) say the values. A change's
//! "before" is the copy kept when it was made, and only if its digest is the
//! one the row recorded; its "after" is the next copy that digest names, or
//! the file as it is now. A change whose copy is gone (only the last
//! [`write::KEEP`] are kept) or was altered is listed with its settings and
//! without values, and says so. A value is never taken from anything else.
//!
//! The copies are on the floor and are read by the daemon only: here (and
//! by `undo.rs` through [`entries`]) and by the restart's way back. A tool of
//! a turn gets only the words this makes, and only the owner's own turn gets
//! them (`wall.rs`): a copy holds every setting, and the names of every
//! secret among them.

use std::path::Path;

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_store::pages::ledger_kind;
use theseus_store::{kinds, Page};
use theseus_tools::{Backend, Plan, Retry, Tool, ToolClass, ToolCtx};

use super::{diff, edit, wall, write};
use crate::config_copy::sha256;
use crate::ledger::LedgerRow;
use crate::toolrun::{ToolRuntime, TurnCtx};

pub const HISTORY: &str = "config.history";
/// The most changes listed: one for each copy kept.
pub const SHOWN: usize = write::KEEP;
/// The most rows of one kind read from the ledger.
const ROWS: usize = 500;

/// One setting of a change: its value before and after (`None`: not set, so
/// its default).
#[derive(Debug, Clone, PartialEq)]
pub struct Shown {
    pub path: Vec<String>,
    pub before: Option<Value>,
    pub after: Option<Value>,
}

/// One approved change.
#[derive(Debug, Clone)]
pub struct Entry {
    /// The number of the copy kept when it was made: how a change is named.
    pub number: u32,
    /// The question the owner answered.
    pub question: String,
    pub at_ms: u64,
    pub paths: Vec<Vec<String>>,
    /// Where the turn that asked ran; none for the terminal or the web UI.
    pub place: Option<String>,
    pub approved_by: String,
    pub via: String,
    pub restart: bool,
    /// The whole file before it, when its kept copy is still the one the row
    /// recorded.
    pub before_doc: Option<Value>,
    /// The whole file after it, when some kept copy, or the file, still is.
    pub after_doc: Option<Value>,
    /// Why the restart that followed it went back, if it did.
    pub rolled_back: Option<String>,
    /// The number of the change that undid it, if one did.
    pub undone_by: Option<u32>,
    /// The number of the change this one undid, if it is an undo.
    pub undoes: Option<u32>,
    /// The prompt file it changed, when it changed one and no setting
    /// (issue 23).
    pub file: Option<super::prompt_file::history::FileEntry>,
}

impl Entry {
    /// Each setting with its value before and after; none when the copy
    /// before the change is no longer kept.
    pub fn settings(&self) -> Option<Vec<Shown>> {
        let before = self.before_doc.as_ref()?;
        Some(
            self.paths
                .iter()
                .map(|p| Shown {
                    path: p.clone(),
                    before: value_at(before, p),
                    after: self.after_doc.as_ref().and_then(|d| value_at(d, p)),
                })
                .collect(),
        )
    }
}

/// The value at `path` in a parsed settings file, whatever it is (a table
/// too), or none when it is not set.
pub fn value_at(doc: &Value, path: &[String]) -> Option<Value> {
    let mut v = doc;
    for k in path {
        v = v.get(k)?;
    }
    Some(v.clone())
}

/// A settings file's text as a tree; none when it does not parse.
pub fn doc_of(text: &str) -> Option<Value> {
    serde_json::to_value(toml::from_str::<toml::Value>(text).ok()?).ok()
}

/// The newest rows of `kind`, oldest first: through the index, or by a scan
/// of the tail while it is built.
pub(crate) fn of_kind(store: &crate::store::Store, kind: LedgerKind) -> Vec<LedgerRow> {
    let page = Page {
        kind: kinds::LEDGER,
        tags: vec![ledger_kind(kind.as_str())],
        limit: ROWS,
        ..Page::default()
    };
    match store.ledger_page(&page) {
        Ok(Some(got)) => got.records.iter().filter_map(|r| r.decode().ok()).collect(),
        _ => store
            .ledger_tail::<LedgerRow>(20_000)
            .map(|rows| {
                rows.into_iter()
                    .map(|(_, r)| r)
                    .filter(|r| r.kind == kind.as_str())
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// The rows history reads: changes, rollbacks and undos, oldest first.
pub fn rows(store: &crate::store::Store) -> Vec<LedgerRow> {
    let mut out: Vec<LedgerRow> = [
        LedgerKind::ConfigChanged,
        LedgerKind::ConfigRolledBack,
        LedgerKind::ConfigUndone,
    ]
    .into_iter()
    .flat_map(|k| of_kind(store, k))
    .collect();
    out.sort_by_key(|r| r.at_unix_ms);
    out
}

fn text(d: &Value, k: &str) -> Option<String> {
    d.get(k).and_then(Value::as_str).map(str::to_string)
}

/// The paths a row names, each parsed back from its dotted form.
pub fn paths_of(d: &Value) -> Vec<Vec<String>> {
    d["paths"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| edit::parse_path(p.as_str()?).ok())
        .collect()
}

/// The changes the daemon itself made on the owner's word (the rows a config read
/// from the vault writes have another shape), oldest first. A plain restart
/// the owner approved writes a row of this kind with no settings
/// (`restart_only`): it is not a change to any setting, so it is neither listed
/// nor something an undo can name. Its kept copy still serves as the "after"
/// of the change before it.
fn changes(rows: &[LedgerRow]) -> Vec<&LedgerRow> {
    rows.iter()
        .filter(|r| r.kind == LedgerKind::ConfigChanged.as_str())
        .filter(|r| r.data["source"] == "owner" && r.data["previous_copy"].is_u64())
        .filter(|r| r.data["restart_only"] != true)
        .collect()
}

/// The changes as history lists them, newest first, at most `limit`.
pub fn entries(rows: &[LedgerRow], state: &Path, file: Option<&Path>, limit: usize) -> Vec<Entry> {
    let all = changes(rows);
    // Which copies exist is the ledger's record (every `config.changed` row
    // names the copy it kept, a plain restart's included), never a directory
    // listing: a file the daemon has no row for is not read.
    let kept_by = |file: bool| -> Vec<u32> {
        rows.iter()
            .filter(|r| r.kind == LedgerKind::ConfigChanged.as_str())
            .filter(|r| r.data["source"] == "owner")
            .filter(|r| r.data["file"].is_object() == file)
            .filter_map(|r| r.data["previous_copy"].as_u64())
            .map(|n| n as u32)
            .collect()
    };
    let (kept, kept_files) = (kept_by(false), kept_by(true));
    let now = file.and_then(|f| std::fs::read_to_string(f).ok());
    let number = |r: &LedgerRow| r.data["previous_copy"].as_u64().unwrap_or_default() as u32;
    let by_question = |q: &str| {
        all.iter()
            .find(|r| r.data["question"] == q)
            .map(|r| number(r))
    };
    let mut out = Vec::new();
    for r in all.iter().rev().take(limit) {
        let d = &r.data;
        let n = number(r);
        let question = text(d, "question").unwrap_or_default();
        let before = write::copy(state, n)
            .filter(|t| Some(sha256(t)) == text(d, "digest_before"))
            .and_then(|t| doc_of(&t));
        let want_after = text(d, "digest_after");
        let after = kept
            .iter()
            .filter(|c| **c > n)
            .take(write::KEEP)
            .filter_map(|c| write::copy(state, *c))
            .chain(now.clone())
            .find(|t| Some(sha256(t)) == want_after)
            .and_then(|t| doc_of(&t));
        let said = |kind: LedgerKind, key: &str| -> Vec<&LedgerRow> {
            rows.iter()
                .filter(|x| x.kind == kind.as_str() && x.data[key] == question.as_str())
                .collect()
        };
        let undone_by = said(LedgerKind::ConfigUndone, "undone_question")
            .into_iter()
            .find_map(|x| by_question(x.data["question"].as_str()?));
        let undoes = said(LedgerKind::ConfigUndone, "question")
            .into_iter()
            .find_map(|x| x.data["undone"].as_u64())
            .map(|u| u as u32);
        out.push(Entry {
            number: n,
            at_ms: r.at_unix_ms,
            paths: paths_of(d),
            place: d["asked_by"]["place"].as_str().map(str::to_string),
            approved_by: text(d, "approved_by").unwrap_or_default(),
            via: text(d, "via").unwrap_or_default(),
            restart: d["restart"].as_bool().unwrap_or(true),
            before_doc: before,
            after_doc: after,
            file: super::prompt_file::history::entry_of(d, state, &kept_files),
            rolled_back: said(LedgerKind::ConfigRolledBack, "question")
                .into_iter()
                .find_map(|x| text(&x.data, "why")),
            undone_by,
            undoes,
            question,
        });
    }
    out
}

/// What history says, for the owner: one block for each change.
pub fn words(entries: &[Entry], now_ms: u64) -> String {
    if entries.is_empty() {
        return "No change to Theseus's settings has been made through Theseus yet.".into();
    }
    let mut out = format!(
        "The last {} to Theseus's settings, newest first:\n",
        crate::narrative::count(entries.len() as u64, "change", "changes")
    );
    for e in entries {
        out.push('\n');
        out.push_str(&block(e, now_ms));
    }
    out.push_str(
        "\nTo put one change's settings back, ask Theseus to undo it by its number; the \
         other settings stay as they are now.\n",
    );
    out
}

fn block(e: &Entry, now_ms: u64) -> String {
    let asked = match &e.place {
        Some(p) => format!("asked in {p}"),
        None => "asked in a conversation in the terminal or the web UI".into(),
    };
    let approved = match e.via == e.approved_by || e.via.is_empty() {
        true => format!("approved by {}", e.approved_by),
        false => format!("approved by {} through {}", e.approved_by, e.via),
    };
    let restart = match e.restart {
        true => "it needed a restart",
        false => "no restart was needed",
    };
    let mut out = format!(
        "#{} ({}): {asked}; {approved}; {restart}.\n",
        e.number,
        theseus_protocol::human_time::ago(now_ms, e.at_ms)
    );
    if let Some(f) = &e.file {
        out.push_str(&super::prompt_file::history::lines(f));
    } else {
        out.push_str(&settings_lines(e));
    }
    if let Some(n) = e.undoes {
        out.push_str(&format!("This put back the settings change #{n} made.\n"));
    }
    if let Some(n) = e.undone_by {
        out.push_str(&format!("Undone since, by change #{n}.\n"));
    }
    if let Some(why) = &e.rolled_back {
        out.push_str(&format!(
            "Theseus went back to the settings before it after the restart: {why}.\n"
        ));
    }
    out
}

/// The settings a change set, each before and after.
fn settings_lines(e: &Entry) -> String {
    let mut out = String::new();
    match e.settings() {
        Some(list) => {
            for s in list {
                let after = match (&e.after_doc, &s.after) {
                    (None, _) => "(its value is no longer kept)".to_string(),
                    (_, a) => diff::words(a.as_ref()),
                };
                out.push_str(&format!(
                    "- {}: {} → {after}\n",
                    edit::dotted(&s.path),
                    diff::words(s.before.as_ref())
                ));
            }
        }
        None => {
            let names: Vec<String> = e.paths.iter().map(|p| edit::dotted(p)).collect();
            out.push_str(&format!(
                "- {} (the copy from before this change is no longer kept, so the values are not \
                 shown, and it cannot be undone)\n",
                names.join(", ")
            ));
        }
    }
    out
}

/// `config.history`'s input: how many changes, at most [`SHOWN`].
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    #[serde(default)]
    limit: Option<usize>,
}

/// How many changes an input asks for.
pub fn limit_of(input: &Value) -> Result<usize, String> {
    let i: Input = theseus_tools::parse(input)?;
    Ok(i.limit.unwrap_or(SHOWN).clamp(1, SHOWN))
}

/// Run `config.history` in the turn `tc`: the owner's own turn gets the words,
/// anything else is refused in its own words.
pub fn run(rt: &ToolRuntime, tc: &TurnCtx<'_>, input: &Value) -> Result<(String, Value), String> {
    if let Some(why) = wall::refusal_for(tc, HISTORY) {
        return Err(why);
    }
    let limit = limit_of(input)?;
    let w = &rt.config_change;
    let list = entries(&rows(tc.store), &w.state, w.file.as_deref(), limit);
    let said = words(&list, theseus_protocol::now_unix_ms());
    Ok((said, json!({"changes": list.len()})))
}

/// `config.history`.
pub struct History;

impl Tool for History {
    fn name(&self) -> &str {
        HISTORY
    }

    fn description(&self) -> &str {
        "List the last changes the owner approved to Theseus's own settings, newest first: when, \
         each setting with its value before and after, who asked, who approved and from where, \
         whether it needed a restart, and whether it was put back since. Each has a number, which \
         `config.undo` takes. Changes nothing. Offered only to the owner, in a private place."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "limit": {"type": "integer", "description": "How many changes, newest first; at most 10."}
            },
            "additionalProperties": false
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Read
    }

    fn backend(&self) -> Backend {
        Backend::Harness
    }

    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        limit_of(input)?;
        Ok(Plan {
            summary: "read the history of changes to Theseus's settings".into(),
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests;
