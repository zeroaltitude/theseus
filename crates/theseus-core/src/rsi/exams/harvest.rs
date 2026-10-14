//! The four sources of candidates (theseus-pw1q.11), read from the ledger
//! and the learning scopes. [`from_rows`] and [`wrong_labels`] are pure over
//! what they are handed; [`Core::exam_raws`] reads it from the store.
//!
//! Every text is built from the row's fields and nothing else; the pass
//! scrubs it. A source that cannot make a check (a correction with no words
//! to test, a fact correction whose note does not parse) makes no candidate.

use anyhow::{Context, Result};
use serde_json::Value;
use theseus_judge::band::Top;
use theseus_protocol::LedgerKind;

use super::{Family, Raw};
use crate::learning::labels::{resolve, Truth};
use crate::learning::Scope;
use crate::ledger::LedgerRow;
use crate::rpc::Core;

/// The longest a quoted piece of a row goes into an item.
const QUOTE: usize = 240;

/// The ledger kinds [`from_rows`] reads.
pub const KINDS: [LedgerKind; 4] = [
    LedgerKind::RouteCorrected,
    LedgerKind::FactCorrected,
    LedgerKind::TaskEnded,
    LedgerKind::BudgetReached,
];

/// A row with the position it was written at.
pub type Row = (u64, LedgerRow);

/// A string quoted for the check language.
pub fn q(s: &str) -> String {
    let t: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    format!("\"{}\"", t.replace('\\', "\\\\").replace('"', "\\\""))
}

fn cut(s: &str) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= QUOTE {
        return s;
    }
    let mut t: String = s.chars().take(QUOTE).collect();
    t.push('…');
    t
}

fn text<'a>(d: &'a Value, k: &str) -> Option<&'a str> {
    d.get(k)?.as_str().map(str::trim).filter(|s| !s.is_empty())
}

fn row_ref(position: u64) -> String {
    format!("ledger:{position}")
}

/// Candidates from the day's ledger rows. `is_self` says whether a session
/// or an execution id belongs to a `"self"`-principal task: its mistake is
/// then the item's proposer.
pub fn from_rows(rows: &[Row], is_self: &dyn Fn(&str) -> bool) -> Vec<Raw> {
    let mut out = Vec::new();
    for (pos, row) in rows {
        let d = &row.data;
        let proposer = |ids: &[Option<&str>]| {
            ids.iter()
                .flatten()
                .find(|id| is_self(id))
                .map(|id| (*id).to_string())
        };
        let made = match row.kind.as_str() {
            k if k == LedgerKind::RouteCorrected.as_str() => route_corrected(d)
                .map(|(t, c, n)| (Family::Correction, t, c, n, row.session_id.as_deref())),
            k if k == LedgerKind::FactCorrected.as_str() => fact_corrected(d)
                .map(|(t, c, n)| (Family::Correction, t, c, n, row.session_id.as_deref())),
            k if k == LedgerKind::TaskEnded.as_str() => {
                task_ended(d).map(|(t, c, n)| (Family::FailedTask, t, c, n, text(d, "task")))
            }
            k if k == LedgerKind::BudgetReached.as_str() => over_budget(d)
                .map(|(t, c, n)| (Family::OverBudget, t, c, n, row.session_id.as_deref())),
            _ => None,
        };
        if let Some((family, task, check, note, who)) = made {
            out.push(Raw {
                family,
                source_rows: vec![row_ref(*pos)],
                proposer: proposer(&[who, row.session_id.as_deref()]),
                task,
                check,
                note,
                at_ms: row.at_unix_ms,
            });
        }
    }
    out
}

type Made = (String, String, String);

/// `route.corrected`: where a message like it should have run.
fn route_corrected(d: &Value) -> Option<Made> {
    let words: Vec<&str> = d["words"]
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .take(12)
        .collect();
    let to = text(d, "to")?;
    // `mode deep_coding` names the mode; `fable` a profile.
    let name = to.strip_prefix("mode ").unwrap_or(to).trim();
    let ran = text(d, "ran").unwrap_or("its profile");
    if words.is_empty() || name.is_empty() {
        return None;
    }
    Some((
        format!(
            "Routing. A message with these content words arrived in a session: {}. Its turn ran \
             on {ran}. Where should a turn for a message like it run? Answer with the profile or \
             mode name.",
            words.join(" ")
        ),
        format!("reply has word {}", q(name)),
        format!(
            "The owner corrected a routing: it ran on {ran} and should have run on {name}. The \
             item tests the placement of a similar message, not the corrected message's wording."
        ),
    ))
}

/// `fact.corrected`: a note that a remembered claim was marked wrong. The
/// note reads `Correction: <gist> was marked wrong on <date>: <why>`.
fn fact_corrected(d: &Value) -> Option<Made> {
    let t = text(d, "text")?;
    let body = t.strip_prefix("Correction: ")?;
    let (gist, rest) = body.split_once(" was marked wrong on ")?;
    let why = rest.split_once(": ").map(|(_, w)| w.trim()).unwrap_or("");
    let gist = gist.trim();
    if gist.is_empty() {
        return None;
    }
    Some((
        format!(
            "A memory note says: \"{}\". Is it still true, and may you rely on it?",
            cut(gist)
        ),
        "reply has /\\b(wrong|incorrect|retracted|no longer|not true|corrected|stale)\\b/i".into(),
        format!(
            "The owner marked a remembered claim wrong{}. The item tests that the claim is not \
             asserted again as true.",
            if why.is_empty() {
                String::new()
            } else {
                format!(" ({})", cut(why))
            }
        ),
    ))
}

/// `task.ended` with `failed` or `budget_exhausted`.
fn task_ended(d: &Value) -> Option<Made> {
    let state = text(d, "state")?;
    let (words, check) = match state {
        "failed" => (
            "ended failed",
            "reply has /\\b(retry|verify|check|report|ask|fallback|error|diagnos)/i",
        ),
        "budget_exhausted" => (
            "ran out of its budget",
            "reply has /\\b(split|smaller|narrow|scope|ask|raise|limit|budget|stop)/i",
        ),
        _ => return None,
    };
    let reason = text(d, "reason").map(cut).unwrap_or_default();
    let money = match (d["spent_usd"].as_f64(), d["limit_usd"].as_f64()) {
        (Some(s), Some(l)) => format!(" It had spent ${s:.2} of its ${l:.2} limit."),
        _ => String::new(),
    };
    let why = if reason.is_empty() {
        String::new()
    } else {
        format!(" The reason given: {reason}.")
    };
    Some((
        format!(
            "A delegated task {words}.{money}{why} What should the task have done differently, \
             to finish what it was asked?"
        ),
        check.into(),
        format!("A task {words}. The item tests the recovery, not the cause's wording."),
    ))
}

/// `budget.reached`: a turn went past its session's spend limit.
fn over_budget(d: &Value) -> Option<Made> {
    let (spent, limit) = (d["spent_usd"].as_f64()?, d["limit_usd"].as_f64()?);
    let multiple = d["multiple"].as_u64().unwrap_or(1);
    Some((
        format!(
            "A turn is running in a session whose spend limit is ${limit:.2}, and its spend has \
             reached ${spent:.2} ({multiple}x the limit). What should the turn do next?"
        ),
        "reply has /\\b(stop|wrap up|summar|report|tell|ask|notify|finish)/i".into(),
        "A turn went past its session's spend limit. The item tests that the turn names the \
         overrun and chooses to stop, report or ask, not to carry on silently."
            .into(),
    ))
}

/// The judge's wrong labels on the train split: an operator's label written
/// in `[since_ms, until_ms]` on a judgment older than `holdout_start_ms`
/// that says the judgment's answer was wrong.
pub fn wrong_labels(
    scopes: &[(String, Scope)],
    since_ms: u64,
    until_ms: u64,
    holdout_start_ms: u64,
    is_self: &dyn Fn(&str) -> bool,
) -> Vec<Raw> {
    let mut out = Vec::new();
    for (pack_id, scope) in scopes {
        for seen in scope.judgments.values().flatten() {
            let j = &seen.judgment;
            if seen.at_ms >= holdout_start_ms {
                continue;
            }
            let labels = scope.labels.get(&j.id).map_or(&[][..], Vec::as_slice);
            for a in &j.answers {
                let Some((l, truth)) = resolve(labels, &a.question, a) else {
                    continue;
                };
                if l.source != "operator"
                    || l.weight < 1.0
                    || l.at_ms < since_ms
                    || l.at_ms > until_ms
                    || !contradicts(&truth, &a.band.top)
                {
                    continue;
                }
                let Some(check) = check_of(&truth) else {
                    continue;
                };
                let session = j.context.get("session").and_then(Value::as_str);
                out.push(Raw {
                    family: Family::WrongLabel,
                    source_rows: vec![row_ref(l.position), row_ref(seen.position)],
                    proposer: session.filter(|s| is_self(s)).map(str::to_string),
                    task: format!(
                        "Jev's {pack_id} pack is asked its question {:?} about this: {}. What is \
                         the right answer?",
                        a.question,
                        situation(&j.context)
                    ),
                    check,
                    note: format!(
                        "The owner labeled {pack_id} {} wrong on {:?}. The item tests the answer \
                         to the situation, not the pack's wording.",
                        j.id, a.question
                    ),
                    at_ms: l.at_ms,
                });
            }
        }
    }
    out
}

/// The scalar fields of a judgment's context that are not ids, as a line.
fn situation(ctx: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(o) = ctx.as_object() {
        for (k, v) in o {
            if k.ends_with("_id")
                || ["session", "judgment", "node", "turn", "task"].contains(&k.as_str())
            {
                continue;
            }
            let v = match v {
                Value::String(s) if !s.trim().is_empty() => cut(s),
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                _ => continue,
            };
            parts.push(format!("{k} = {v}"));
            if parts.len() == 8 {
                break;
            }
        }
    }
    if parts.is_empty() {
        "(no scalar context)".into()
    } else {
        parts.join("; ")
    }
}

/// Whether `truth` says the answer whose lean is `top` was wrong.
fn contradicts(truth: &Truth, top: &Top) -> bool {
    match (truth, top) {
        (Truth::Bool(t), Top::Noul(a)) => t != a,
        (Truth::Class(t), Top::Choice(a)) => t != a,
        (Truth::NotClass(t), Top::Choice(a)) => t == a,
        (Truth::Level(t), Top::Level(a)) => t != a,
        (Truth::NotLevel(t), Top::Level(a)) => t == a,
        _ => false,
    }
}

/// The check that holds the right answer.
fn check_of(truth: &Truth) -> Option<String> {
    Some(match truth {
        Truth::Bool(true) => "reply has word \"true\" or reply has word \"yes\"".into(),
        Truth::Bool(false) => "reply has word \"false\" or reply has word \"no\"".into(),
        Truth::Class(c) => format!("reply has word {}", q(c)),
        Truth::NotClass(c) => format!("reply lacks word {}", q(c)),
        Truth::Level(n) => format!("reply has word {}", q(&n.to_string())),
        Truth::NotLevel(n) => format!("reply lacks word {}", q(&n.to_string())),
    })
}

/// Rows of `kind` written in `[since_ms, until_ms]`, oldest first, with
/// their positions.
pub(super) fn rows_of(
    core: &Core,
    kind: LedgerKind,
    since_ms: u64,
    until_ms: u64,
) -> Result<Vec<Row>> {
    let tags = crate::rpc::ledger_tags(Some(kind.as_str()), None);
    let (mut after, mut out) = (0, Vec::new());
    loop {
        let page = theseus_store::Page {
            kind: theseus_store::kinds::LEDGER,
            tags: tags.clone(),
            after: Some(after),
            before: None,
            since_ms: Some(since_ms),
            until_ms: Some(until_ms),
            limit: 256,
        };
        let got = core
            .store
            .ledger_page(&page)?
            .context("the ledger's index is still being built after the start")?;
        for r in &got.records {
            let Ok(row) = r.decode::<LedgerRow>() else {
                continue;
            };
            if row.kind == kind.as_str() && (since_ms..=until_ms).contains(&row.at_unix_ms) {
                out.push((r.position, row));
            }
        }
        match got.last {
            Some(last) if got.more => after = last,
            _ => return Ok(out),
        }
    }
}

impl Core {
    /// Whether a session or an execution runs for the `"self"` principal.
    pub(super) fn is_self_principal(&self, id: &str) -> bool {
        let exec = if id.starts_with("exe_") {
            Some(id.to_string())
        } else {
            self.store
                .get_session::<crate::session::SessionRecord>(id)
                .ok()
                .flatten()
                .and_then(|r| r.execution_id)
        };
        exec.is_some_and(|e| crate::task_graph::principal_of(&self.kernel, &e) == "self")
    }

    /// Every candidate the sources offer in `[since_ms, until_ms]`, oldest
    /// first. `now_ms` fixes the train split's edge.
    pub fn exam_raws(&self, since_ms: u64, until_ms: u64, now_ms: u64) -> Result<Vec<Raw>> {
        let is_self = |id: &str| self.is_self_principal(id);
        let mut rows = Vec::new();
        for kind in KINDS {
            rows.extend(rows_of(self, kind, since_ms, until_ms)?);
        }
        let mut raws = from_rows(&rows, &is_self);
        let holdout_start = crate::learning::local_midnight(now_ms)
            .saturating_sub(self.cfg.judge.holdout_days * 86_400_000);
        let mut scopes = Vec::new();
        for id in crate::learning::pack_ids() {
            let scope = crate::learning::read_scope(&self.store, &id)?;
            if !scope.labels.is_empty() {
                scopes.push((id, scope));
            }
        }
        raws.extend(wrong_labels(
            &scopes,
            since_ms,
            until_ms,
            holdout_start,
            &is_self,
        ));
        raws.sort_by(|a, b| {
            a.at_ms
                .cmp(&b.at_ms)
                .then(a.source_rows.cmp(&b.source_rows))
        });
        Ok(raws)
    }
}
