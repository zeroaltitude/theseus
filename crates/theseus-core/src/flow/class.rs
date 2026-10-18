//! Result classes: what a step's result is, by small extractors over its
//! text, exit code and status. A flow's `on` table is keyed by these.
//!
//! `classes` is the one function that names them, most specific first and
//! ending in `ok` or `err`, so a spec that maps `conflict` and `err` both
//! sends a merge conflict to the first. The pattern miner (R5) and the mint
//! (R7) call this instead of keeping their own readings.
//!
//! - `conflict`: a failed `git` merge, pull, cherry-pick, rebase, revert or
//!   apply whose text says `CONFLICT` or `fix conflicts`.
//! - `test-fail`: a failed run of a test runner (`cargo test|nextest`,
//!   `pytest`, `go test`, `npm|yarn|pnpm test`, `make test|check`, and the like).
//! - `rerere-left`, `rerere-resolved`: a succeeded `git rerere status` or
//!   `remaining` that printed paths, or printed none.
//! - `ok`, `err`: the result's own status (and exit code, for a program).

use std::path::Path;

use crate::node::ResultStatus;

/// Every class a spec may map in `on`.
pub const CLASSES: &[&str] = &[
    "ok",
    "err",
    "conflict",
    "rerere-left",
    "rerere-resolved",
    "test-fail",
];

/// What a step's result shows.
pub struct Seen<'a> {
    /// The tool's canonical name (`proc.run`).
    pub tool: &'a str,
    /// The program's argv, for `proc.run`.
    pub argv: &'a [String],
    pub status: ResultStatus,
    /// The program's exit code, when the result has one.
    pub exit: Option<i64>,
    pub text: &'a str,
}

impl Seen<'_> {
    /// Whether the step succeeded: an ok result, and exit 0 for a program.
    pub fn succeeded(&self) -> bool {
        self.status == ResultStatus::Ok && self.exit.unwrap_or(0) == 0
    }
}

/// The classes of a result, most specific first.
pub fn classes(s: &Seen<'_>) -> Vec<&'static str> {
    let mut out = Vec::new();
    let program = (s.tool == "proc.run").then(|| base(s.argv.first()));
    let sub = s.argv.get(1).map(String::as_str);
    if s.succeeded() {
        if program.as_deref() == Some("git")
            && sub == Some("rerere")
            && matches!(
                s.argv.get(2).map(String::as_str),
                Some("status" | "remaining")
            )
        {
            out.push(if listed_paths(s.text).is_empty() {
                "rerere-resolved"
            } else {
                "rerere-left"
            });
        }
        out.push("ok");
        return out;
    }
    if program.as_deref() == Some("git")
        && matches!(
            sub,
            Some("merge" | "pull" | "cherry-pick" | "rebase" | "revert" | "apply" | "stash")
        )
        && (s.text.contains("CONFLICT") || s.text.contains("fix conflicts"))
    {
        out.push("conflict");
    }
    if program.is_some() && test_runner(s.argv) {
        out.push("test-fail");
    }
    out.push("err");
    out
}

/// A program's name without its directory.
fn base(program: Option<&String>) -> String {
    program
        .and_then(|p| Path::new(p).file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Whether `argv` runs a test suite.
pub fn test_runner(argv: &[String]) -> bool {
    let words: Vec<&str> = argv.iter().map(String::as_str).collect();
    let program = base(argv.first());
    let has = |w: &str| words.iter().skip(1).any(|x| *x == w);
    match program.as_str() {
        "cargo" => has("test") || has("nextest"),
        "go" => has("test"),
        "npm" | "yarn" | "pnpm" | "bun" => has("test"),
        "make" | "just" | "ninja" => has("test") || has("check"),
        "pytest" | "py.test" | "jest" | "mocha" | "vitest" | "ctest" | "tox" | "rspec" => true,
        "python" | "python3" => has("pytest") || has("unittest"),
        "mvn" | "gradle" | "gradlew" => has("test"),
        _ => false,
    }
}

/// What a program printed, from the text of its `proc.run` result: the
/// header lines the runtime puts before it (`[exit code 1 · in /w]`, a
/// timeout's, a cancel's) and its "(no output)" stand-in left off.
pub fn program_output(text: &str) -> &str {
    let mut rest = text;
    for _ in 0..4 {
        match rest.split_once('\n') {
            Some((line, tail)) if line.starts_with('[') && line.ends_with(']') => rest = tail,
            _ => break,
        }
    }
    match rest.trim_end() {
        "(no output)" => "",
        _ => rest,
    }
}

/// The paths a command printed, one a line: blank lines left out.
pub fn listed_paths(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('['))
        .map(str::to_string)
        .collect()
}

/// The files a merge reported in conflict: `CONFLICT (content): Merge
/// conflict in <path>` and the other forms that end `in <path>` or begin
/// `CONFLICT (...): <path> ...`.
pub fn conflict_files(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines().filter(|l| l.contains("CONFLICT")) {
        let path = line
            .split_once("Merge conflict in ")
            .map(|(_, p)| p)
            .or_else(|| {
                line.split_once("): ")
                    .map(|(_, rest)| rest.split(" deleted in ").next().unwrap_or(rest))
                    .and_then(|r| r.split(" added in ").next())
                    .and_then(|r| r.split(" renamed ").next())
            })
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty());
        if let Some(p) = path {
            if !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen<'a>(argv: &'a [String], status: ResultStatus, exit: i64, text: &'a str) -> Seen<'a> {
        Seen {
            tool: "proc.run",
            argv,
            status,
            exit: Some(exit),
            text,
        }
    }

    fn argv(s: &str) -> Vec<String> {
        s.split(' ').map(str::to_string).collect()
    }

    #[test]
    fn a_failed_merge_with_conflicts_is_a_conflict_then_an_error() {
        let a = argv("git merge --no-ff topic");
        let text = "Auto-merging a.txt\nCONFLICT (content): Merge conflict in a.txt\nAutomatic merge failed; fix conflicts and then commit the result.";
        let s = seen(&a, ResultStatus::Error, 1, text);
        assert_eq!(classes(&s), ["conflict", "err"]);
        assert_eq!(conflict_files(text), ["a.txt"]);
    }

    #[test]
    fn a_failed_test_run_is_a_test_failure_and_a_failed_other_program_is_not() {
        let t = argv("cargo nextest run");
        assert_eq!(
            classes(&seen(&t, ResultStatus::Error, 100, "")),
            ["test-fail", "err"]
        );
        let g = argv("git push");
        assert_eq!(
            classes(&seen(&g, ResultStatus::Error, 1, "CONFLICT")),
            ["err"]
        );
    }

    #[test]
    fn rerere_status_is_left_when_it_prints_paths_and_resolved_when_it_prints_none() {
        let a = argv("git rerere status");
        assert_eq!(
            classes(&seen(&a, ResultStatus::Ok, 0, "a.txt\n")),
            ["rerere-left", "ok"]
        );
        assert_eq!(
            classes(&seen(&a, ResultStatus::Ok, 0, "")),
            ["rerere-resolved", "ok"]
        );
        let o = argv("git status");
        assert_eq!(classes(&seen(&o, ResultStatus::Ok, 0, "a.txt")), ["ok"]);
    }

    #[test]
    fn a_programs_output_is_what_follows_the_runtimes_header() {
        assert_eq!(program_output("[exit code 0 · in /w]\n(no output)"), "");
        assert_eq!(
            program_output("[exit code 1 · in /w]\nCONFLICT x\nmore\n"),
            "CONFLICT x\nmore\n"
        );
        assert_eq!(program_output("plain\n"), "plain\n");
        assert_eq!(
            program_output("[1/3] building\ndone"),
            "[1/3] building\ndone"
        );
    }

    #[test]
    fn other_tools_are_ok_or_err_by_status() {
        let s = Seen {
            tool: "fs.read",
            argv: &[],
            status: ResultStatus::Error,
            exit: None,
            text: "no such file",
        };
        assert_eq!(classes(&s), ["err"]);
    }
}
