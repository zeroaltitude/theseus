//! The halt after two infrastructure failures in a row (theseus-otkwu),
//! through the core's own kill switch and a counting notice sink.

use std::sync::atomic::{AtomicUsize, Ordering};

use theseus_protocol::rsi::SelfMode;

use super::*;
use crate::fact::rsi_pr::SelfBranchBuilt;
use crate::rsi::infra::{of_row, watch};
use crate::rsi::tree::Notify;
use crate::rsi::{gate, Gate};

/// A rig, and a place whose notices are counted and kept.
struct Watch {
    r: Rig,
    place: crate::rsi::tree::Place,
    told: Arc<Mutex<Vec<String>>>,
    n: AtomicUsize,
}

fn watching() -> Watch {
    let r = rig(SelfMode::Act, None);
    let told = Arc::new(Mutex::new(Vec::new()));
    let sink = told.clone();
    let mut place = r.f.place.clone();
    place.notify = Some(Notify::new(move |t| sink.lock().unwrap().push(t)));
    Watch {
        r,
        place,
        told,
        n: AtomicUsize::new(0),
    }
}

impl Watch {
    /// One attempt as `built` writes it: `watch` first, then its row.
    fn attempt(&self, gate: &str, exit: Option<i32>, secs: Option<u64>) {
        let n = self.n.fetch_add(1, Ordering::Relaxed);
        let branch = format!("self/20261026-trim-{n}");
        let outcome = of_row(&json!({"gate": gate, "numbers": {"gate_secs": secs, "exit": exit}}));
        watch(&self.r.core, &self.place, &branch, outcome, exit);
        self.r.core.rec(None).record(&SelfBranchBuilt {
            item: "x1",
            issue: "theseus-sail1",
            branch: &branch,
            head: None,
            gate,
            report: None,
            cost_usd: 1.0,
            stopped_at: None,
            why: "Trim the sails.",
            numbers: json!({"gate_secs": secs, "exit": exit}),
        });
    }

    fn infra(&self) {
        self.attempt("red", Some(127), Some(0));
    }

    fn halted(&self) -> bool {
        matches!(gate(&self.r.core.self_ctx()), Gate::Halted { .. })
    }
}

#[tokio::test]
async fn one_infrastructure_failure_does_not_halt() {
    let w = watching();
    w.infra();
    assert!(!w.halted());
    assert!(w.told.lock().unwrap().is_empty());
}

#[tokio::test]
async fn two_in_a_row_halt_and_tell_the_owner_once() {
    let w = watching();
    w.infra();
    w.infra();
    assert!(w.halted());
    // A third (a build that was already running) halts nothing new and tells nothing.
    w.infra();
    let told = w.told.lock().unwrap();
    assert_eq!(told.len(), 1, "{told:?}");
    let text = &told[0];
    assert!(
        text.contains("halted") && text.contains("did not start") && text.contains("exit 127"),
        "{text}"
    );
    assert!(text.contains("self/20261026-trim-0") && text.contains("self/20261026-trim-1"));
}

#[tokio::test]
async fn a_real_red_between_two_resets_the_run() {
    let w = watching();
    w.infra();
    w.attempt("red", Some(1), Some(310));
    w.infra();
    assert!(
        !w.halted(),
        "the gate ran in between, so the plumbing works"
    );
    assert!(w.told.lock().unwrap().is_empty());
    w.infra();
    assert!(w.halted(), "two in a row since the red");
}

#[tokio::test]
async fn an_attempt_halted_between_phases_neither_counts_nor_resets() {
    let w = watching();
    w.infra();
    w.attempt("halted", None, None);
    w.infra();
    assert!(w.halted());
}

#[tokio::test]
async fn a_judged_attempt_never_halts() {
    let w = watching();
    w.attempt("red", Some(1), Some(310));
    w.attempt("red", Some(1), Some(310));
    w.attempt("green", Some(0), Some(300));
    assert!(!w.halted());
}

/// The halt's notice is its own kind and pings: no `quiet` (theseus-6s7ll).
#[test]
fn the_halt_notice_is_its_own_kind_and_does_not_ask_for_quiet() {
    let b = crate::rsi::pr::notice_body(true, "halted".into());
    assert_eq!(b["kind"], "self_halt");
    assert_eq!(b["text"], "halted");
    assert!(b.get("quiet").is_none(), "{b}");
}

/// The guard on the exception: every other self notice stays a quiet `notice`
/// (passes at the base too; it is not a red-first test).
#[test]
fn an_informational_self_notice_stays_quiet() {
    let b = crate::rsi::pr::notice_body(false, "may build".into());
    assert_eq!(b["kind"], "notice");
    assert_eq!(b["quiet"], true);
}

/// The halt takes the sink's halt line when it has one, else the plain one.
#[test]
fn the_halt_takes_the_halt_sink_and_a_plain_sink_takes_it_too() {
    let plain = Arc::new(Mutex::new(Vec::new()));
    let halts = Arc::new(Mutex::new(Vec::new()));
    let (p, h) = (plain.clone(), halts.clone());
    Notify::new(move |t| p.lock().unwrap().push(t))
        .with_halt(move |t| h.lock().unwrap().push(t))
        .send_halt("h".into());
    assert!(plain.lock().unwrap().is_empty());
    assert_eq!(*halts.lock().unwrap(), ["h"]);
    let only = Arc::new(Mutex::new(Vec::new()));
    let o = only.clone();
    Notify::new(move |t| o.lock().unwrap().push(t)).send_halt("h".into());
    assert_eq!(*only.lock().unwrap(), ["h"]);
}
