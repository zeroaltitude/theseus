//! The dry replay (§2.5 tier 2, §3.1): for each check-half occurrence the
//! directory is rebuilt as it was at the stretch's start, the tool is run on
//! that copy with the recorded inputs bound and **no model**, and its facts at
//! the end are compared with the recorded ones: the exit it reached, the steps
//! it ran with their exit codes, the last test result, and the paths it
//! changed. A remote step (pull, push, a `gh` call) is not run: its recorded
//! result stands in, and the fixtures prove its behaviour.
//!
//! **Pass** (§3.1): at least [`MIN_INSTANCES`] instances, at least 90% of them
//! reproduce the recorded facts, and **no deviation**. A deviation is a write
//! outside the recorded paths, a step the recording never ran, or an exit the
//! recording never reached (in any check-half instance).
//!
//! What it cannot see: a recording's paths are the files its edit tools wrote.
//! A program's own writes (a merge's files) are in no record, so an instance
//! whose recording ran programs carries `changed: None` and its paths are
//! counted as unverified (`DryReport::unverified`), never as a pass.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::walk::{walk, StepResult, Steps};
use crate::flow::class;
use crate::flow::spec::Spec;

/// The fewest instances a dry replay may pass on.
pub const MIN_INSTANCES: usize = 3;
/// The share of instances that must reproduce.
pub const MIN_REPRODUCED: f64 = 0.9;

/// Commands that reach another machine: stubbed with their recorded result.
const REMOTE: &[&[&str]] = &[
    &["git", "push"],
    &["git", "pull"],
    &["git", "fetch"],
    &["git", "clone"],
    &["gh"],
];

/// Whether a program's argv reaches another machine.
pub fn is_remote(argv: &[String]) -> bool {
    REMOTE
        .iter()
        .any(|p| argv.len() >= p.len() && p.iter().zip(argv).all(|(a, b)| a == b))
}

/// A recorded remote call's result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Remote {
    pub argv: Vec<String>,
    pub exit: Option<i64>,
    pub text: String,
}

/// What a run ended with, recorded or replayed.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Facts {
    /// `ok`, an exit's name, or `handback:<name>`.
    pub exit: String,
    /// Each step run: its id and exit code.
    pub ran: Vec<(String, Option<i64>)>,
    /// The last test-runner step passed (true) or failed (false).
    pub tests: Option<bool>,
    /// The paths changed, relative to the directory; `None` where the
    /// recording cannot say (it ran programs).
    pub changed: Option<BTreeSet<String>>,
}

/// One check-half occurrence, ready to replay.
#[derive(Debug, Clone)]
pub struct Instance {
    pub id: String,
    /// The directory as it was at the stretch's start (a copy to run in).
    pub dir: PathBuf,
    /// The recorded inputs, in memory only.
    pub inputs: Value,
    pub recorded: Facts,
    pub remote: Vec<Remote>,
}

/// What a dry replay found wrong with the tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "what", rename_all = "snake_case")]
pub enum Deviation {
    /// The tool wrote a path the recording did not.
    Write(String),
    /// The tool ran a step the recording never ran.
    StepNeverRun(String),
    /// The tool ended in an exit the recording never reached.
    ExitNeverReached(String),
}

/// The dry replay's result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DryReport {
    pub instances: usize,
    pub reproduced: usize,
    pub deviations: Vec<Deviation>,
    /// Instances whose paths could not be compared (the recording ran programs).
    pub unverified: usize,
    pub pass: bool,
    pub why: String,
}

/// The files under `dir` (but `.git`) and their digests.
pub fn snapshot(dir: &Path) -> BTreeMap<String, [u8; 32]> {
    fn go(root: &Path, dir: &Path, out: &mut BTreeMap<String, [u8; 32]>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.file_name().is_some_and(|n| n == ".git") {
                continue;
            }
            match e.file_type() {
                Ok(t) if t.is_dir() => go(root, &p, out),
                Ok(t) if t.is_file() => {
                    let bytes = std::fs::read(&p).unwrap_or_default();
                    let rel = p.strip_prefix(root).unwrap_or(&p).display().to_string();
                    out.insert(rel, Sha256::digest(&bytes).into());
                }
                _ => {}
            }
        }
    }
    let mut out = BTreeMap::new();
    go(dir, dir, &mut out);
    out
}

/// The paths that differ between two snapshots.
pub fn changed(
    before: &BTreeMap<String, [u8; 32]>,
    after: &BTreeMap<String, [u8; 32]>,
) -> BTreeSet<String> {
    before
        .keys()
        .chain(after.keys())
        .filter(|k| before.get(*k) != after.get(*k))
        .cloned()
        .collect()
}

/// A runner that answers a remote step with its recorded result and passes
/// every other step to `inner`.
pub struct Stubbed<S> {
    pub inner: S,
    pub remote: Vec<Remote>,
}

impl<S: Steps> Steps for Stubbed<S> {
    async fn run(&mut self, step: &str, tool: &str, args: &Value) -> StepResult {
        let argv: Vec<String> = args
            .get("argv")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        if tool == "proc.run" && is_remote(&argv) {
            let at = self
                .remote
                .iter()
                .position(|r| argv.starts_with(&r.argv) || r.argv.starts_with(&argv));
            return match at {
                Some(i) => {
                    let r = self.remote.remove(i);
                    StepResult {
                        ok: r.exit.unwrap_or(1) == 0,
                        exit: r.exit,
                        text: r.text,
                    }
                }
                None => StepResult {
                    ok: false,
                    exit: None,
                    text: "no recorded result for this remote step".into(),
                },
            };
        }
        self.inner.run(step, tool, args).await
    }
}

/// The facts of a replayed walk and the paths it changed.
fn facts_of(w: &super::walk::Walk, changed: BTreeSet<String>) -> Facts {
    let tests = w
        .ran
        .iter()
        .rev()
        .find(|r| class::test_runner(&r.argv))
        .map(|r| r.exit == Some(0));
    Facts {
        exit: canonical(&w.exit),
        ran: w.ran.iter().map(|r| (r.id.clone(), r.exit)).collect(),
        tests,
        changed: Some(changed),
    }
}

/// An exit's name as a flow ends it: `exit:x` and `x` are one.
pub fn canonical(word: &str) -> String {
    word.strip_prefix("exit:").unwrap_or(word).to_string()
}

/// Replay `instances` of `spec`. `make` builds the step runner for one
/// instance (production: the whole-turn replay's hands over the instance's
/// directory, which runs a program in L1).
pub async fn dry_replay<S: Steps>(
    spec: &Spec,
    instances: &[Instance],
    mut make: impl FnMut(&Instance) -> S,
) -> DryReport {
    let reached: BTreeSet<String> = instances
        .iter()
        .map(|i| canonical(&i.recorded.exit))
        .collect();
    let (mut reproduced, mut unverified) = (0, 0);
    let mut deviations: Vec<Deviation> = Vec::new();
    for inst in instances {
        let before = snapshot(&inst.dir);
        let mut runner = Stubbed {
            inner: make(inst),
            remote: inst.remote.clone(),
        };
        let walked = walk(spec, &inst.inputs, &mut runner).await;
        let after = snapshot(&inst.dir);
        let Ok(w) = walked else { continue };
        let got = facts_of(&w, changed(&before, &after));
        let rec = &inst.recorded;
        let recorded_steps: BTreeSet<&str> = rec.ran.iter().map(|(s, _)| s.as_str()).collect();
        let mut dev: Vec<Deviation> = Vec::new();
        for (s, _) in &got.ran {
            if !recorded_steps.contains(s.as_str()) {
                dev.push(Deviation::StepNeverRun(s.clone()));
            }
        }
        if !reached.contains(&got.exit) {
            dev.push(Deviation::ExitNeverReached(got.exit.clone()));
        }
        match (&rec.changed, &got.changed) {
            (Some(r), Some(g)) => dev.extend(g.difference(r).map(|p| Deviation::Write(p.clone()))),
            _ => unverified += 1,
        }
        let paths_agree = match (&rec.changed, &got.changed) {
            (Some(r), Some(g)) => r == g,
            _ => true,
        };
        let same = got.exit == canonical(&rec.exit)
            && got.ran == rec.ran
            && got.tests == rec.tests
            && paths_agree;
        if same && dev.is_empty() {
            reproduced += 1;
        }
        dev.dedup();
        deviations.extend(dev);
    }
    let n = instances.len();
    let share = if n == 0 {
        0.0
    } else {
        reproduced as f64 / n as f64
    };
    let (pass, why) = if n < MIN_INSTANCES {
        (
            false,
            format!("{n} instances: at least {MIN_INSTANCES} are needed"),
        )
    } else if !deviations.is_empty() {
        (false, format!("{} deviations", deviations.len()))
    } else if share < MIN_REPRODUCED {
        (
            false,
            format!(
                "{reproduced} of {n} reproduced the recorded facts: {:.0}% is under {:.0}%",
                share * 100.0,
                MIN_REPRODUCED * 100.0
            ),
        )
    } else {
        (
            true,
            format!("{reproduced} of {n} reproduced the recorded facts, no deviation"),
        )
    };
    DryReport {
        instances: n,
        reproduced,
        deviations,
        unverified,
        pass,
        why,
    }
}
