//! The deterministic half of a whole-turn comparison (own-benchmark part (c)):
//! what the two runs did, side by side, where the turn has something to
//! compare. `compare.v1` reads the words; this reads the facts, and the row
//! carries both.
//!
//! - **tests**: the last program run that tests something, passed or failed
//!   (exit 0 or not); `None` when neither side ran one. A run "tests
//!   something" by its words (`is_test`): a part of its argv is `test`,
//!   `tests`, `nextest`, `pytest`, `check`, `verify` or `spec`. That is a
//!   label for the row, not a reading of what the program does: the row lists
//!   every run's argv and exit, so the owner sees what was compared.
//! - **files**: the paths the two changed with `fs.write` and `fs.edit`, as
//!   sets of paths relative to the directory.
//! - **exits**: the distinct exit codes of the programs each ran, as sets.

use std::collections::BTreeSet;

use theseus_protocol::own_bench::{TestRun, WholeSame};

/// What one side did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    pub changed: BTreeSet<String>,
    /// Every program run, in order.
    pub runs: Vec<TestRun>,
}

/// Words in a program's argv that make it a test run for the row.
const TEST_WORDS: [&str; 7] = [
    "test", "tests", "nextest", "pytest", "check", "verify", "spec",
];

/// Whether a program's words say it tests something.
pub fn is_test(argv: &[String]) -> bool {
    argv.iter().any(|a| {
        let base = a.rsplit('/').next().unwrap_or(a);
        TEST_WORDS.contains(&base)
    })
}

impl Facts {
    /// The runs that test something, in order.
    pub fn tests(&self) -> Vec<TestRun> {
        self.runs
            .iter()
            .filter(|r| is_test(&r.argv))
            .cloned()
            .collect()
    }

    fn last_test_passed(&self) -> Option<bool> {
        self.tests().last().map(|r| r.exit == Some(0))
    }

    fn exits(&self) -> BTreeSet<Option<i32>> {
        self.runs.iter().map(|r| r.exit).collect()
    }
}

/// The two sides compared.
pub fn same(original: &Facts, replay: &Facts) -> WholeSame {
    let tests = match (original.last_test_passed(), replay.last_test_passed()) {
        (None, None) => None,
        (a, b) => Some(a == b),
    };
    WholeSame {
        tests,
        files: original.changed == replay.changed,
        exits: original.exits() == replay.exits(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(argv: &[&str], exit: i32) -> TestRun {
        TestRun {
            argv: argv.iter().map(|s| (*s).into()).collect(),
            exit: Some(exit),
        }
    }

    fn facts(paths: &[&str], runs: Vec<TestRun>) -> Facts {
        Facts {
            changed: paths.iter().map(|s| (*s).into()).collect(),
            runs,
        }
    }

    /// A test run is told by its words, whatever the program, and `cargo
    /// build` is not one.
    #[test]
    fn a_test_run_is_told_by_its_words() {
        let a = |v: &[&str]| v.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
        assert!(is_test(&a(&["cargo", "test", "-p", "x"])));
        assert!(is_test(&a(&["cargo", "nextest", "run"])));
        assert!(is_test(&a(&["/usr/bin/pytest", "-q"])));
        assert!(is_test(&a(&["npm", "test"])));
        assert!(!is_test(&a(&["cargo", "build"])));
        assert!(!is_test(&a(&["ls", "-la"])));
    }

    /// The same last test result, the same paths and the same exits are all
    /// same; each differs alone.
    #[test]
    fn the_sides_agree_on_each_fact_alone() {
        let o = facts(&["a.rs"], vec![run(&["cargo", "test"], 0), run(&["ls"], 0)]);
        let r = facts(
            &["a.rs"],
            vec![run(&["ls"], 0), run(&["cargo", "test", "-q"], 0)],
        );
        assert_eq!(
            same(&o, &r),
            WholeSame {
                tests: Some(true),
                files: true,
                exits: true
            }
        );
        // The replay's tests fail: tests differ, the rest is the same.
        let failing = facts(
            &["a.rs"],
            vec![run(&["cargo", "test"], 101), run(&["ls"], 0)],
        );
        let s = same(&o, &failing);
        assert_eq!(s.tests, Some(false));
        assert!(s.files);
        assert!(!s.exits);
        // Another path.
        let other = facts(&["b.rs"], o.runs.clone());
        let s = same(&o, &other);
        assert!(!s.files);
        assert_eq!(s.tests, Some(true));
        // A replay that never tested what the original did.
        let none = facts(&["a.rs"], vec![run(&["ls"], 0)]);
        assert_eq!(same(&o, &none).tests, Some(false));
        // Neither tested: no test fact.
        assert_eq!(same(&none, &none).tests, None);
    }
}
