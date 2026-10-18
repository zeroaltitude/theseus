//! The dry replay on real temp repositories: recorded facts are made by
//! running the flow once, so "reproduces" is a statement about the replay
//! and not about a fixture's luck, and a planted deviation fails it.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

use super::dry::{dry_replay, Deviation, Facts, Instance, Remote};
use super::fixtures::{Fixtures, GitFixtures};
use super::tests_walk::land_flow;
use super::walk::{walk, StepResult, Steps};
use crate::flow::class;
use crate::flow::spec::Spec;

/// A test's own runtime for the walker's futures.
pub(crate) fn block_on<F: Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(f)
}

/// Steps run for real in a directory, `proc.run` only, at L0: a test's
/// stand-in for the whole-turn hands, which run a program in L1.
pub(crate) struct Local {
    dir: PathBuf,
    /// Extra writes a planted bug makes after each step.
    pub plant: Option<(&'static str, &'static str)>,
}

impl Local {
    pub fn new(dir: &Path) -> Self {
        Self {
            dir: dir.into(),
            plant: None,
        }
    }
}

impl Steps for Local {
    async fn run(&mut self, _step: &str, tool: &str, args: &Value) -> StepResult {
        assert_eq!(tool, "proc.run", "the stand-in runs programs only");
        let argv: Vec<String> = args["argv"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a.as_str().unwrap().to_string())
            .collect();
        let out = Command::new(&argv[0])
            .args(&argv[1..])
            .current_dir(&self.dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_EDITOR", "true")
            .env("GIT_AUTHOR_NAME", "fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
            .env("GIT_COMMITTER_NAME", "fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
            .output()
            .unwrap();
        if let Some((path, text)) = self.plant {
            std::fs::write(self.dir.join(path), text).unwrap();
        }
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        StepResult {
            ok: out.status.success(),
            exit: out.status.code().map(i64::from),
            text,
        }
    }
}

/// A recorded instance: the fixture for `on`, with the facts of running the
/// flow on it once, which is what a recording of that run would hold.
fn recorded(spec: &Spec, on: &str, expect: &str, base: &Path, remote: bool) -> Instance {
    let fx = GitFixtures.make(on, expect, spec, base).expect("a fixture");
    let before = super::dry::snapshot(&fx.dir);
    let copy = base.join("copy");
    copy_dir(&fx.dir, &copy);
    let mut steps = Local::new(&fx.dir);
    let w = block_on(walk(spec, &fx.inputs, &mut steps)).unwrap();
    let changed = super::dry::changed(&before, &super::dry::snapshot(&fx.dir));
    let tests = w
        .ran
        .iter()
        .rev()
        .find(|r| class::test_runner(&r.argv))
        .map(|r| r.exit == Some(0));
    let remote = if remote {
        vec![Remote {
            argv: vec!["git".into(), "push".into()],
            exit: Some(0),
            text: "recorded".into(),
        }]
    } else {
        vec![]
    };
    Instance {
        id: format!("{on}@{}", base.display()),
        dir: copy,
        inputs: fx.inputs,
        recorded: Facts {
            exit: w.exit.clone(),
            ran: w.ran.iter().map(|r| (r.id.clone(), r.exit)).collect(),
            tests,
            changed: Some(changed),
        },
        remote,
    }
}

fn copy_dir(from: &Path, to: &Path) {
    assert!(Command::new("cp")
        .arg("-a")
        .arg(from)
        .arg(to)
        .status()
        .unwrap()
        .success());
}

/// The land flow's check half: clean, resolved, left, red.
fn instances(spec: &Spec, keep: &mut Vec<tempfile::TempDir>) -> Vec<Instance> {
    let arms = [
        ("ok", "ok"),
        ("err:1+conflict+rerere-resolved", "ok"),
        ("err:1+conflict+rerere-left", "handback:conflicts-left"),
        ("err:1+test-fail", "tests-red"),
    ];
    arms.iter()
        .map(|(on, expect)| {
            let d = tempfile::tempdir().unwrap();
            // The push is a remote step: stubbed with its recorded result,
            // so it is not run again against the copy's upstream.
            let i = recorded(spec, on, expect, d.path(), true);
            keep.push(d);
            i
        })
        .collect()
}

#[test]
fn the_dry_replay_reproduces_recorded_facts() {
    let spec = land_flow();
    let mut keep = Vec::new();
    let inst = instances(&spec, &mut keep);
    let r = block_on(dry_replay(&spec, &inst, |i| Local::new(&i.dir)));
    assert!(r.pass, "{r:?}");
    assert_eq!((r.instances, r.reproduced), (4, 4));
    assert!(r.deviations.is_empty(), "{r:?}");
}

#[test]
fn a_deviation_an_extra_write_fails_it() {
    let spec = land_flow();
    let mut keep = Vec::new();
    let inst = instances(&spec, &mut keep);
    // A tool that also writes a file the recording never wrote.
    let r = block_on(dry_replay(&spec, &inst, |i| {
        let mut l = Local::new(&i.dir);
        l.plant = Some(("stray.txt", "x"));
        l
    }));
    assert!(!r.pass, "{r:?}");
    assert!(
        r.deviations.contains(&Deviation::Write("stray.txt".into())),
        "{r:?}"
    );
    assert!(r.why.contains("deviation"), "{}", r.why);
}

#[test]
fn a_step_the_recording_never_ran_is_a_deviation() {
    let spec = land_flow();
    let mut keep = Vec::new();
    let mut inst = instances(&spec, &mut keep);
    // The recording of the clean land holds no retest, but the tool ran one.
    inst[0].recorded.ran.retain(|(s, _)| s != "retest");
    let r = block_on(dry_replay(&spec, &inst, |i| Local::new(&i.dir)));
    assert!(!r.pass, "{r:?}");
    assert!(
        r.deviations
            .contains(&Deviation::StepNeverRun("retest".into())),
        "{r:?}"
    );
}

#[test]
fn an_exit_the_recording_never_reached_is_a_deviation() {
    let spec = land_flow();
    let mut keep = Vec::new();
    let mut inst = instances(&spec, &mut keep);
    // No recording handed back, so the tool handing back is an exit the
    // recordings never reached.
    inst.retain(|i| i.recorded.exit != "handback:conflicts-left");
    let d = tempfile::tempdir().unwrap();
    let left = recorded(
        &spec,
        "err:1+conflict+rerere-left",
        "handback:conflicts-left",
        d.path(),
        true,
    );
    inst[0].dir = left.dir;
    let r = block_on(dry_replay(&spec, &inst, |i| Local::new(&i.dir)));
    assert!(!r.pass);
    assert!(
        r.deviations.contains(&Deviation::ExitNeverReached(
            "handback:conflicts-left".into()
        )),
        "{r:?}"
    );
    drop(d);
}

#[test]
fn under_ninety_percent_reproducing_fails_without_any_deviation() {
    let spec = land_flow();
    let mut keep = Vec::new();
    let mut inst = instances(&spec, &mut keep);
    // One of four recordings holds another exit code for the same steps: the
    // replay does not reproduce it, yet it writes nothing it should not and
    // runs nothing the recording did not.
    inst[0].recorded.ran[0].1 = Some(7);
    let r = block_on(dry_replay(&spec, &inst, |i| Local::new(&i.dir)));
    assert!(r.deviations.is_empty(), "{r:?}");
    assert_eq!((r.instances, r.reproduced), (4, 3));
    assert!(!r.pass && r.why.contains("75%"), "{r:?}");
}

#[test]
fn fewer_than_three_instances_cannot_pass() {
    let spec = land_flow();
    let mut keep = Vec::new();
    let inst: Vec<Instance> = instances(&spec, &mut keep).into_iter().take(2).collect();
    let r = block_on(dry_replay(&spec, &inst, |i| Local::new(&i.dir)));
    assert!(!r.pass && r.deviations.is_empty(), "{r:?}");
    assert!(r.why.contains("at least 3"), "{}", r.why);
}

#[test]
fn a_remote_step_runs_with_its_recorded_result_not_for_real() {
    let spec = land_flow();
    let dirs: Vec<_> = (0..3).map(|_| tempfile::tempdir().unwrap()).collect();
    let inst: Vec<Instance> = dirs
        .iter()
        .map(|d| {
            let mut i = recorded(&spec, "ok", "ok", d.path(), true);
            // The recorded push failed; the replay must see that, not run a push.
            i.remote[0].exit = Some(1);
            i.remote[0].text = "rejected".into();
            i.recorded.ran.last_mut().unwrap().1 = Some(1);
            i.recorded.exit = "handback:step-failed".into();
            i
        })
        .collect();
    let r = block_on(dry_replay(&spec, &inst, |i| Local::new(&i.dir)));
    assert_eq!(r.reproduced, 3, "{r:?}");
}

#[test]
fn the_recorded_inputs_are_values_held_in_memory_and_never_in_a_report() {
    let spec = land_flow();
    let mut keep = Vec::new();
    let inst = instances(&spec, &mut keep);
    let r = block_on(dry_replay(&spec, &inst, |i| Local::new(&i.dir)));
    let text = serde_json::to_string(&r).unwrap();
    assert!(!text.contains("feature"), "{text}");
    let _ = json!(null);
}
