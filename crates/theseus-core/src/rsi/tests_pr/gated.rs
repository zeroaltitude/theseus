//! What the attempt makes of the gate's outcome (theseus-tsfkm): a stop is a
//! halt and a gate that could not run is a failure. The stop is a value, so
//! nothing that rewraps an error on the way out can turn the one into the other.

use super::*;

fn input() -> Input {
    Input {
        item: "x1".into(),
        issue: "theseus-sail1".into(),
        why: "Trim the sails.".into(),
    }
}

fn open(i: &Input) -> Attempt<'_> {
    Attempt {
        i,
        tree: Tree {
            slug: "trim".into(),
            branch: "self/20261027-trim".into(),
            dir: PathBuf::from("/nowhere/tree"),
            target: PathBuf::from("/nowhere/target"),
        },
        head: None,
        gate: None,
        report: None,
        pushed: None,
        stopped_at: None,
        verdict: "",
        diff: (0, 0, 0),
    }
}

/// KEY: a build stopped on purpose is halted at the gate, with the reason.
#[test]
fn a_stopped_gate_is_a_halt_at_the_gate() {
    let i = input();
    let mut a = open(&i);
    let out = gate_ran(&mut a, Ok(tree::Gated::Stopped("the owner's stop".into())))
        .expect_err("a stop ends the attempt");
    let said = out.expect_err("and is the model's error, not a result");
    assert!(
        said.contains("Stopped before the gate") && said.contains("the owner's stop"),
        "{said}"
    );
    assert_eq!((a.verdict, a.stopped_at), ("halted", Some(GATE)));
}

/// A gate that did not run, however its error is wrapped, is a failure and not
/// a stop: the two never read as each other.
#[test]
fn a_gate_that_did_not_run_is_failed_even_when_wrapped() {
    use anyhow::Context;
    let i = input();
    let mut a = open(&i);
    let wrapped = Err::<tree::Gated, _>(anyhow::anyhow!("git could not start"))
        .context("the gate could not start")
        .context("a wrapper someone added later");
    let out = gate_ran(&mut a, wrapped).expect_err("no run, so the attempt ends");
    let said = out.expect_err("with the model's error");
    assert!(said.starts_with("The gate did not run"), "{said}");
    assert_eq!((a.verdict, a.stopped_at), ("failed", None));
}

/// A gate that ran hands the attempt its run untouched.
#[test]
fn a_gate_that_ran_goes_on() {
    let i = input();
    let mut a = open(&i);
    let run = tree::GateRun {
        verdict: Verdict::Red,
        exit: Some(1),
        blob: String::new(),
        log: PathBuf::from("/nowhere/log"),
        secs: 3,
        tail: String::new(),
    };
    let got = gate_ran(&mut a, Ok(tree::Gated::Ran(run))).expect("the run goes on");
    assert_eq!((got.verdict, got.exit), (Verdict::Red, Some(1)));
    assert_eq!(a.verdict, "", "the caller names the verdict from the run");
}
