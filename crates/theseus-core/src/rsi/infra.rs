//! A gate that never started is no verdict on the code (theseus-otkwu).
//!
//! Between 2026-10-20 and 10-25 every self build died with `exit 127` and
//! `gate_secs 0` before its gate ran, and the pipeline read each as a red
//! branch: it re-filed the same cause and regenerated the same patch every
//! night. An attempt is one of three things: **green** and **red** (the gate
//! ran and judged the code) and **infrastructure** (the gate never started,
//! so the code is unjudged and nothing may be concluded about it).
//!
//! [`of_attempt`] and [`of_row`] are the one classifier, over a finished
//! attempt and over a `self.branch.built` row. [`watch`] halts self builds,
//! through the existing kill switch ([`crate::rpc::Core::self_halt`]), after
//! [`HALT_AFTER`] infrastructure failures in a row, read from those rows, and
//! tells the owner once, through [`tell`]. [`unjudged_items`] names the items
//! whose newest attempt was infrastructure, which the backlog does not re-file.

use std::collections::HashSet;

use serde_json::Value;
use theseus_protocol::rsi::SelfHaltParams;

use super::tree::{GateRun, Notify, Place};
use crate::approval::{Answerer, Surface};
use crate::ledger::LedgerRow;
use crate::rpc::Core;

/// Infrastructure failures in a row that halt self builds: one is noise, the
/// second is a pattern, the third is money.
pub const HALT_AFTER: usize = 2;
/// The shell's "command not found".
const NOT_FOUND: i32 = 127;

/// What an attempt shows about the code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Green,
    /// The gate ran and judged the code (the keel guard's finding too).
    Red,
    /// The gate never started: the code is unjudged.
    Infrastructure,
}

/// The one rule, over the row's own words. `gate` is the attempt's verdict
/// name; `None` is an attempt that says nothing about a gate (halted between
/// phases, empty): it neither counts nor resets.
///
/// - `green` is green, even at `exit 0` with 0 seconds: the gate said so.
/// - `keel` is judged: the guard ran and found something, however fast.
/// - `failed` (the gate did not run) is infrastructure.
/// - `red` is infrastructure at `exit 127` whatever the seconds (a command the
///   gate needs is missing), and at 0 seconds (nothing ran); otherwise red,
///   including a gate cut off with no exit after running for a while.
fn judge(gate: &str, exit: Option<i64>, secs: Option<u64>) -> Option<Outcome> {
    match gate {
        "green" => Some(Outcome::Green),
        "keel" => Some(Outcome::Red),
        "failed" => Some(Outcome::Infrastructure),
        "red" if exit == Some(i64::from(NOT_FOUND)) || secs.unwrap_or(0) == 0 => {
            Some(Outcome::Infrastructure)
        }
        "red" => Some(Outcome::Red),
        _ => None,
    }
}

/// A finished attempt: its verdict name and its gate run, if one was made.
pub fn of_attempt(verdict: &str, gate: Option<&GateRun>) -> Option<Outcome> {
    judge(
        verdict,
        gate.and_then(|g| g.exit).map(i64::from),
        gate.map(|g| g.secs),
    )
}

/// A `self.branch.built` row's data.
pub fn of_row(d: &Value) -> Option<Outcome> {
    judge(
        d["gate"].as_str()?,
        d["numbers"]["exit"].as_i64(),
        d["numbers"]["gate_secs"].as_u64(),
    )
}

/// An infrastructure failure, as the owner is told of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failed {
    pub branch: String,
    pub exit: Option<i64>,
}

/// The infrastructure failures at the end of `rows` (oldest first), newest
/// first, up to `max`; an attempt that says nothing of a gate is passed over,
/// and any judged attempt ends the run.
pub fn streak(rows: &[LedgerRow], max: usize) -> Vec<Failed> {
    let mut out = Vec::new();
    for r in rows
        .iter()
        .rev()
        .filter(|r| r.kind.as_str() == "self.branch.built")
    {
        match of_row(&r.data) {
            Some(Outcome::Infrastructure) => {
                out.push(Failed {
                    branch: r.data["branch"].as_str().unwrap_or("unknown").to_string(),
                    exit: r.data["numbers"]["exit"].as_i64(),
                });
                if out.len() == max {
                    break;
                }
            }
            Some(_) => break,
            None => {}
        }
    }
    out
}

/// The items whose newest attempt that says anything of a gate was
/// infrastructure: unjudged, so no evidence of anything.
pub fn unjudged_items(rows: &[LedgerRow]) -> HashSet<String> {
    let mut newest: std::collections::HashMap<&str, Outcome> = std::collections::HashMap::new();
    for r in rows
        .iter()
        .filter(|r| r.kind.as_str() == "self.branch.built")
    {
        if let (Some(item), Some(o)) = (r.data["item"].as_str(), of_row(&r.data)) {
            newest.insert(item, o);
        }
    }
    newest
        .into_iter()
        .filter(|(_, o)| *o == Outcome::Infrastructure)
        .map(|(i, _)| i.to_string())
        .collect()
}

/// After an attempt, before its row is written: when it is the second
/// infrastructure failure in a row, halt self builds and tell the owner. A
/// switch already halted is left alone and nothing is told twice.
pub(super) fn watch(
    core: &Core,
    place: &Place,
    branch: &str,
    now: Option<Outcome>,
    exit: Option<i32>,
) {
    if now != Some(Outcome::Infrastructure) {
        return;
    }
    let rows =
        theseus_store::blocking(|| super::join::rows_of(&core.store, &["self.branch.built"]));
    let rows = match rows {
        Ok(Some(rows)) => rows,
        Ok(None) => return,
        Err(e) => {
            tracing::warn!(error = %format!("{e:#}"), "self build: the history was not read");
            return;
        }
    };
    let mut run = vec![Failed {
        branch: branch.to_string(),
        exit: exit.map(i64::from),
    }];
    run.extend(streak(&rows, HALT_AFTER - 1));
    if run.len() < HALT_AFTER {
        return;
    }
    let who = Answerer {
        label: "the self build watch".into(),
        surface: Surface::Unnamed,
        discord: None,
        agent: None,
    };
    let params = SelfHaltParams {
        why: Some(format!(
            "the gate did not start on {HALT_AFTER} self builds in a row"
        )),
        ..SelfHaltParams::default()
    };
    match theseus_store::blocking(|| core.self_halt(&params, &who)) {
        Ok(r) if r.changed => tell(place.notify.as_ref(), &run),
        Ok(_) => {}
        Err(e) => tracing::warn!(error = %format!("{e:#}"), "self build: the halt was not written"),
    }
}

/// The one telling (the JOIN SEAM): the owner's notice that self builds have
/// halted, through the place's sink; none in a test's place.
pub(super) fn tell(notify: Option<&Notify>, run: &[Failed]) {
    let Some(n) = notify else { return };
    let attempts: Vec<String> = run
        .iter()
        .map(|f| {
            format!(
                "{} (exit {})",
                f.branch,
                f.exit.map_or_else(|| "none".to_string(), |e| e.to_string())
            )
        })
        .collect();
    n.send_halt(format!(
        "Self builds are halted, and they need you. The gate did not start on the last {} \
         attempts: {}. That is the build environment, not the code: the code was never tested, \
         so nothing was marked red and no cause was filed from it. Fix the gate's environment, \
         then release the halt with `theseus self resume`, or just say `resume self` here.",
        run.len(),
        attempts.join("; ")
    ));
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn row(gate: &str, exit: Value, secs: Value) -> Value {
        json!({"gate": gate, "branch": "self/b", "item": "x1",
               "numbers": {"exit": exit, "gate_secs": secs}})
    }

    fn of(gate: &str, exit: Value, secs: Value) -> Option<Outcome> {
        of_row(&row(gate, exit, secs))
    }

    #[test]
    fn a_gate_that_never_started_is_infrastructure() {
        use Outcome::Infrastructure as I;
        // no gate at all: the attempt's gate did not run
        assert_eq!(of("failed", Value::Null, Value::Null), Some(I));
        // the shell's "command not found", the week's shape
        assert_eq!(of("red", json!(127), json!(0)), Some(I));
        // a missing command late in a gate that ran for a while is the same
        assert_eq!(of("red", json!(127), json!(41)), Some(I));
        // no seconds at all, or none elapsed, with a non-zero exit
        assert_eq!(of("red", json!(2), json!(0)), Some(I));
        assert_eq!(of("red", json!(1), Value::Null), Some(I));
        assert_eq!(of("red", Value::Null, json!(0)), Some(I));
    }

    #[test]
    fn a_gate_that_ran_judged_the_code() {
        use Outcome::{Green, Red};
        assert_eq!(of("red", json!(1), json!(340)), Some(Red));
        // cut off after running a long while, with no exit: it started
        assert_eq!(of("red", Value::Null, json!(7200)), Some(Red));
        // green is the gate's own word, even at exit 0 in under a second
        assert_eq!(of("green", json!(0), json!(0)), Some(Green));
        assert_eq!(of("green", json!(0), json!(400)), Some(Green));
        // the keel guard ran and found something, however fast
        assert_eq!(of("keel", json!(1), json!(0)), Some(Red));
    }

    #[test]
    fn an_attempt_that_says_nothing_of_a_gate_is_no_outcome() {
        assert_eq!(of("halted", Value::Null, Value::Null), None);
        assert_eq!(of("empty", Value::Null, Value::Null), None);
        assert_eq!(of_row(&json!({"item": "x1"})), None);
    }

    #[test]
    fn a_finished_attempt_reads_as_its_row_does() {
        use crate::rsi::tree::Verdict;
        let run = |exit, secs| GateRun {
            verdict: Verdict::Red,
            exit,
            blob: String::new(),
            log: std::path::PathBuf::new(),
            secs,
            tail: String::new(),
        };
        let infra = Some(Outcome::Infrastructure);
        assert_eq!(of_attempt("failed", None), infra);
        assert_eq!(of_attempt("red", Some(&run(Some(127), 0))), infra);
        assert_eq!(
            of_attempt("red", Some(&run(Some(1), 90))),
            Some(Outcome::Red)
        );
        assert_eq!(of_attempt("halted", None), None);
    }

    fn built(gate: &str, exit: Value, secs: Value, branch: &str, item: &str) -> LedgerRow {
        let mut d = row(gate, exit, secs);
        d["branch"] = json!(branch);
        d["item"] = json!(item);
        LedgerRow::named("self.branch.built", None, None, d)
    }

    #[test]
    fn the_streak_counts_infrastructure_failures_at_the_end_and_a_judged_run_ends_it() {
        let i = |b: &str| built("red", json!(127), json!(0), b, "x1");
        let red = built("red", json!(1), json!(300), "self/r", "x1");
        let halted = built("halted", Value::Null, Value::Null, "self/h", "x1");
        let rows = vec![i("self/a"), red, i("self/b"), halted, i("self/c")];
        let s = streak(&rows, 5);
        assert_eq!(
            s.iter().map(|f| f.branch.as_str()).collect::<Vec<_>>(),
            ["self/c", "self/b"],
            "a halted attempt is passed over; the red one ends the run"
        );
        assert_eq!(s[0].exit, Some(127));
        assert_eq!(streak(&rows, 1).len(), 1);
        assert!(streak(&rows[..2], 5).is_empty());
    }

    #[test]
    fn an_item_is_unjudged_while_its_newest_attempt_is_infrastructure() {
        let rows = vec![
            built("red", json!(127), json!(0), "self/a", "x1"),
            built("red", json!(127), json!(0), "self/b", "x2"),
            built("red", json!(1), json!(300), "self/c", "x2"),
            built("green", json!(0), json!(300), "self/d", "x3"),
            built("failed", Value::Null, Value::Null, "self/e", "x4"),
            built("halted", Value::Null, Value::Null, "self/f", "x4"),
        ];
        let mut got: Vec<String> = unjudged_items(&rows).into_iter().collect();
        got.sort();
        assert_eq!(got, ["x1", "x4"]);
    }
}
