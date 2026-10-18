//! The fixture tier (§2.5 tier 1, §3.1): one fixture for each branch arm the
//! motif shows, at least, and the tool must end each in the exit that arm
//! ended in. A fixture is a temp directory the tool runs in (production: in
//! L1, through the whole-turn hands), built by a [`Fixtures`] catalog.
//!
//! [`GitFixtures`] is the catalog for repository flows, a temp git repository
//! with an upstream and a feature branch:
//! - `clean`: a merge with no conflict, tests green;
//! - `known`: a conflict rerere has a resolution for;
//! - `unknown`: a conflict it has none for, which must hand back;
//! - `red`: a merge with no conflict and tests red.
//!
//! A repository's test runner is `./pytest`, a script in it (a program name
//! `flow::class::test_runner` reads as a test run), so a red fixture is a
//! failing test in the flow's own reading. An arm with no fixture fails the
//! tier by name: a tool is never passed on arms it was not tried on.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::dry::canonical;
use super::walk::{walk, StepResult, Steps};
use crate::flow::spec::{InputType, Spec};
use crate::rsi::mine::Motif;

/// One fixture: a directory, the inputs to bind, and the exit expected.
#[derive(Debug, Clone)]
pub struct Fixture {
    pub arm: String,
    pub dir: PathBuf,
    pub inputs: Value,
    pub expect: String,
}

/// Builds the fixture an arm needs, or says it cannot.
pub trait Fixtures: Send + Sync {
    /// A fixture under `base` for the arm whose result class is `on` and
    /// which ends in `expect`.
    fn make(&self, on: &str, expect: &str, spec: &Spec, base: &Path) -> Option<Fixture>;

    /// The program a step runs in a fixture, for a program the fixture stands
    /// in for (a test runner): the argv as it is, by default.
    fn argv(&self, argv: &[String]) -> Vec<String> {
        argv.to_vec()
    }
}

/// Steps with each `proc.run` argv put through the catalog's `argv`.
struct Mapped<'a, S> {
    inner: S,
    catalog: &'a dyn Fixtures,
}

impl<S: Steps> Steps for Mapped<'_, S> {
    async fn run(&mut self, step: &str, tool: &str, args: &Value) -> StepResult {
        let mut args = args.clone();
        if tool == "proc.run" {
            let argv: Option<Vec<String>> = args.get("argv").and_then(Value::as_array).map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            });
            if let Some(argv) = argv {
                args["argv"] = json!(self.catalog.argv(&argv));
            }
        }
        self.inner.run(step, tool, &args).await
    }
}

/// The fixture tier's result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FixtureReport {
    pub arms: usize,
    pub passed: usize,
    /// One line for each arm that did not pass.
    pub failed: Vec<String>,
    pub pass: bool,
}

/// The arms to try: each branch arm as (class, expected exit), or one clean
/// run when the motif has no branch.
pub fn arms_of(m: &Motif) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = m
        .branches
        .iter()
        .flat_map(|b| b.arms.iter())
        .map(|a| (a.on.clone(), canonical(&a.exit.word())))
        .collect();
    if out.is_empty() {
        out.push(("ok".into(), "ok".into()));
    }
    out
}

/// Run `spec` on a fixture for each arm of `m`, in `base`, over the steps
/// `make_steps` builds for a directory.
pub async fn run_fixtures<S: Steps>(
    arms: &[(String, String)],
    spec: &Spec,
    catalog: &dyn Fixtures,
    base: &Path,
    mut make_steps: impl FnMut(&Path) -> S,
) -> FixtureReport {
    let mut failed = Vec::new();
    let mut passed = 0;
    for (i, (on, expect)) in arms.iter().enumerate() {
        let dir = base.join(format!("arm{i}"));
        let _ = std::fs::create_dir_all(&dir);
        let Some(fx) = catalog.make(on, expect, spec, &dir) else {
            failed.push(format!(
                "{on}: no fixture builds this arm, so the tool was not tried on it"
            ));
            continue;
        };
        let mut steps = Mapped {
            inner: make_steps(&fx.dir),
            catalog,
        };
        match walk(spec, &fx.inputs, &mut steps).await {
            Ok(w) if canonical(&w.exit) == fx.expect => passed += 1,
            Ok(w) => failed.push(format!("{on}: ended {}, expected {}", w.exit, fx.expect)),
            Err(e) => failed.push(format!("{on}: did not start: {e}")),
        }
    }
    FixtureReport {
        arms: arms.len(),
        passed,
        pass: failed.is_empty(),
        failed,
    }
}

/// The git catalog.
pub struct GitFixtures;

fn git(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .args([
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@example.invalid",
        ])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn write(dir: &Path, name: &str, text: &str) {
    let _ = std::fs::write(dir.join(name), text);
}

/// `./pytest`: a script that exits `code`.
fn runner(dir: &Path, code: i32) {
    use std::os::unix::fs::PermissionsExt;
    write(dir, "pytest", &format!("#!/bin/sh\nexit {code}\n"));
    let _ = std::fs::set_permissions(dir.join("pytest"), std::fs::Permissions::from_mode(0o755));
}

impl GitFixtures {
    /// A clone of a bare upstream with one commit, rerere on, `feature` ready
    /// to merge: it changes `a.txt` (`feature_line`) and main changes it
    /// (`main_line`), or neither when both are empty.
    fn repo(dir: &Path, main_line: &str, feature_line: &str, tests: i32) -> bool {
        let up = dir.join("upstream.git");
        let work = dir.join("work");
        let _ = std::fs::create_dir_all(&work);
        let ok = Command::new("git")
            .args(["init", "-q", "--bare", "-b", "main"])
            .arg(&up)
            .output()
            .is_ok_and(|o| o.status.success())
            && git(&work, &["init", "-q", "-b", "main"])
            && git(&work, &["config", "rerere.enabled", "true"])
            && git(&work, &["config", "rerere.autoupdate", "true"])
            && git(
                &work,
                &["remote", "add", "origin", &up.display().to_string()],
            );
        if !ok {
            return false;
        }
        write(&work, "a.txt", "base\n");
        write(&work, ".gitignore", "pytest\n");
        runner(&work, tests);
        git(&work, &["add", "-A"])
            && git(&work, &["commit", "-q", "-m", "base"])
            && git(&work, &["push", "-q", "-u", "origin", "main"])
            && git(&work, &["checkout", "-q", "-b", "feature"])
            && {
                write(
                    &work,
                    if feature_line.is_empty() {
                        "b.txt"
                    } else {
                        "a.txt"
                    },
                    &format!(
                        "{feature_line}{}\n",
                        if feature_line.is_empty() {
                            "feature"
                        } else {
                            ""
                        }
                    ),
                );
                git(&work, &["add", "-A"]) && git(&work, &["commit", "-q", "-m", "feature"])
            }
            && git(&work, &["checkout", "-q", "main"])
            && (main_line.is_empty() || {
                write(&work, "a.txt", &format!("{main_line}\n"));
                git(&work, &["add", "-A"]) && git(&work, &["commit", "-q", "-am", "main"])
            })
    }

    /// Teach rerere the conflict: merge, resolve, commit, then go back to
    /// before the merge.
    fn teach(work: &Path) -> bool {
        git(work, &["merge", "--no-ff", "feature"]) || {
            write(work, "a.txt", "resolved\n");
            git(work, &["add", "a.txt"])
                && git(work, &["commit", "-q", "--no-edit"])
                && git(work, &["reset", "-q", "--hard", "HEAD~1"])
        }
    }

    /// The inputs a spec takes, from what a repository fixture has.
    fn inputs(spec: &Spec) -> Option<Value> {
        let mut m = Map::new();
        for (n, i) in &spec.inputs {
            let v = match i.ty {
                InputType::Ref => json!("feature"),
                InputType::Argv => json!(["./pytest"]),
                InputType::Path => json!("a.txt"),
                InputType::Str => json!("fixture"),
                InputType::Num => json!(1),
            };
            m.insert(n.clone(), v);
        }
        Some(Value::Object(m))
    }
}

/// Which repository an arm needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scenario {
    Clean,
    Known,
    Unknown,
    Red,
}

fn scenario(on: &str, expect: &str) -> Scenario {
    let has = |w: &str| on.split('+').any(|x| x == w);
    if has("rerere-left") {
        Scenario::Unknown
    } else if has("rerere-resolved") {
        Scenario::Known
    } else if has("conflict") {
        // The arm that ends handing the conflict back is the unknown one.
        if expect.starts_with("handback:") {
            Scenario::Unknown
        } else {
            Scenario::Known
        }
    } else if has("test-fail") || (on.starts_with("err") && expect != "ok") {
        Scenario::Red
    } else {
        Scenario::Clean
    }
}

impl Fixtures for GitFixtures {
    fn make(&self, on: &str, expect: &str, spec: &Spec, base: &Path) -> Option<Fixture> {
        let sc = scenario(on, expect);
        let ok = match sc {
            Scenario::Clean => Self::repo(base, "", "", 0),
            Scenario::Red => Self::repo(base, "", "", 1),
            Scenario::Known | Scenario::Unknown => {
                Self::repo(base, "main change", "feature change", 0)
            }
        };
        let work = base.join("work");
        if !ok || (sc == Scenario::Known && !Self::teach(&work)) {
            return None;
        }
        Some(Fixture {
            arm: on.into(),
            dir: work,
            inputs: Self::inputs(spec)?,
            expect: expect.into(),
        })
    }

    /// A test run in a repository fixture is the repository's own `./pytest`,
    /// so a red fixture is red whatever command the motif's tests used.
    fn argv(&self, argv: &[String]) -> Vec<String> {
        if crate::flow::class::test_runner(argv) {
            vec!["./pytest".into()]
        } else {
            argv.to_vec()
        }
    }
}
