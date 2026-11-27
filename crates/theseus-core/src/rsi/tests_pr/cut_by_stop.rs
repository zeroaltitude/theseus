//! A gate cut by the daemon's stop (issue 63) is no verdict. The stop asks its own runs to end (`own_runs`), and a
//! gate that ends after that is the stopped outcome, a halt at the gate, with its consumer still alive to record it:
//! never a red or failed build. The stand-in gate here runs at L0 and ends by itself; the stop that ends a real L1
//! gate is the daemon's (`theseusd`'s `stop_job.rs`). What this holds is what the attempt records.

use std::time::{Duration, Instant};

use theseus_protocol::rsi::SelfMode;

use super::*;
use crate::rsi::tests_tree::fixture_with;

/// A gate that prints its phase, then fails a moment later, as a gate cut in its suite would.
const CUT_GATE: &str =
    "#!/bin/bash\necho 'gate: phase suite'\nsleep 2\necho 'gate: FAILED in suite'\nexit 1\n";

/// KEY: the daemon's stop comes while the gate runs; the attempt that awaits it is alive past the stop's 2 s
/// fallback, and records a halt at the gate, not a red build.
#[tokio::test]
async fn a_gate_the_daemons_stop_cut_is_a_halt_not_a_red_build() {
    let r = rig_on(
        fixture_with(CUT_GATE),
        SelfMode::Act,
        Some("origin"),
        |_| {},
    );
    let sid = r.session("self");
    r.open_and_edit(&sid, "sails.txt").await;
    let state = r.f.place.state.clone();
    let stop = std::thread::spawn(move || {
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_secs(60) {
            if gate_log_in(&state) {
                theseus_kernel::own_runs::end_all(theseus_kernel::job::STOP_GRACE);
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    });
    let said = r.turn(&sid, WIRE, propose()).await;
    assert!(stop.join().unwrap(), "the gate never started");
    assert!(said.contains("Stopped before the gate"), "{said}");
    let rows = r.rows();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["gate"], "halted", "{}", rows[0]);
    assert_eq!(rows[0]["stopped_at"], "gate", "{}", rows[0]);
}

/// Whether a gate's log is anywhere under `dir`: a `.log` in a directory named `gates`.
fn gate_log_in(dir: &std::path::Path) -> bool {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .any(|e| {
            let p = e.path();
            if p.is_dir() {
                return gate_log_in(&p);
            }
            p.extension().is_some_and(|x| x == "log")
                && p.parent()
                    .and_then(|d| d.file_name())
                    .is_some_and(|n| n == "gates")
        })
}
