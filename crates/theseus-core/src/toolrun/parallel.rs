//! Which calls of one response run together (theseus-d1hi), and what a
//! call that ran in a group says of it (theseus-da46).
//!
//! In response order, a gated call joins the open group when it has the
//! group's class and touches nothing a call already in it touches; otherwise
//! the group runs and a new one opens:
//! - reads with reads, always;
//! - writes with writes whose paths differ; a write to a path already in the
//!   group, or one whose plan names no path, starts a group;
//! - programs with programs, each its own job: two sends to one terminal,
//!   two calls to one MCP server, or two programs whose argvs name one file
//!   (`gen.py`, then `cat gen.py`'s output) stay in order. A harness verb
//!   (`task.create`, `extend.propose`) and an AWS hands group run alone.
//!
//! A class change is a barrier, as before: a program after a write starts
//! once the write has ended, and a read after a program reads its effect.
//! The owner's call (theseus-d1hi, 2026-10-09 09:07): programs run together,
//! with insurance that dependent calls are not flattened.

use std::collections::BTreeSet;

use serde_json::Value;
use theseus_tools::{Access, Backend, Plan, Tool, ToolClass};

tokio::task_local! {
    /// How many other calls of its group ran at once with the call being
    /// run, set around each call of a group of writes or programs: its
    /// result says so, and the model learns what it may put in one response.
    pub(super) static BESIDE: usize;
}

/// What a gated call touches, for the grouping rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Touch {
    pub(super) class: ToolClass,
    /// It runs in a group of its own.
    pub(super) alone: bool,
    /// What a later call of its group must not touch too.
    pub(super) keys: BTreeSet<String>,
}

impl Touch {
    /// `tool`'s call of `input`, by its gated `plan`.
    pub(super) fn of(tool: &dyn Tool, input: &Value, plan: &Plan) -> Self {
        let class = plan.class.unwrap_or(tool.class());
        let mut keys = BTreeSet::new();
        let alone = match class {
            ToolClass::Read => false,
            ToolClass::Write => {
                keys.extend(
                    plan.resources
                        .iter()
                        .filter(|r| r.access == Access::Write)
                        .map(|r| format!("path:{}", r.path.display())),
                );
                keys.is_empty()
            }
            ToolClass::Run => {
                let name = tool.name();
                if let Some(server) = name.strip_prefix("mcp:").and_then(|s| s.split('/').next()) {
                    keys.insert(format!("mcp:{server}"));
                }
                if let Some(t) = input.get("terminal").and_then(Value::as_str) {
                    keys.insert(format!("term:{t}"));
                }
                // A program's own path is not a file it works on: two `sleep`s
                // share `/usr/bin/sleep`.
                let argvs = plan.argv.iter().chain(plan.steps.iter().flatten());
                let args = argvs.flat_map(|a| a.iter().skip(1));
                keys.extend(
                    args.flat_map(|a| file_names(a))
                        .map(|n| format!("name:{n}")),
                );
                keys.extend(
                    plan.resources
                        .iter()
                        .filter(|r| r.access != Access::Exec)
                        .filter_map(|r| r.path.file_name())
                        .map(|n| format!("name:{}", n.to_string_lossy())),
                );
                tool.backend() == Backend::Harness || name == crate::aws::hands::RUN
            }
        };
        Self { class, alone, keys }
    }
}

/// The open group: its class, and what its calls touch.
#[derive(Default)]
pub(super) struct Open {
    class: Option<ToolClass>,
    keys: BTreeSet<String>,
}

impl Open {
    /// Whether `t` joins the group; when it does not, the group is over and
    /// `t` opens the next (a call that runs alone closes it behind itself).
    /// Returns true when `t` joins.
    pub(super) fn admit(&mut self, t: &Touch) -> bool {
        let joins = !t.alone && self.class == Some(t.class) && self.keys.is_disjoint(&t.keys);
        if !joins {
            self.keys.clear();
        }
        self.class = (!t.alone).then_some(t.class);
        self.keys.extend(t.keys.iter().cloned());
        joins
    }
}

/// The file names an argument names: the last part of each word in it that
/// holds a `/` (a path or a URL), or that ends in an extension (`gen.py`).
/// A shell script's words are split at its operators and quotes, and an
/// option's value at its `=`. A device (`/dev/null`) is no file. It errs toward a name: two programs that share
/// one only wait for each other.
pub(super) fn file_names(arg: &str) -> Vec<String> {
    let words = arg.split(|c: char| c.is_whitespace() || "'\";|&<>()`=,$".contains(c));
    words
        .filter_map(|w| {
            // `2>/dev/null` names no file a program shares.
            if w.starts_with("/dev/") {
                return None;
            }
            let last = w.trim_end_matches('/').rsplit('/').next()?;
            let named = w.contains('/') || has_extension(last);
            (named && !last.is_empty() && last != "." && last != "..").then(|| last.to_string())
        })
        .collect()
}

/// `name.ext`, its extension 1 to 8 letters and digits, starting with a letter.
fn has_extension(w: &str) -> bool {
    match w.rsplit_once('.') {
        Some((stem, ext)) => {
            !stem.is_empty()
                && (1..=8).contains(&ext.len())
                && ext.starts_with(|c: char| c.is_ascii_alphabetic())
                && ext.chars().all(|c| c.is_ascii_alphanumeric())
        }
        None => false,
    }
}

/// The line a result of a group of writes or programs ends with: how many
/// calls of the response ran at once with it. None outside such a group.
pub(super) fn beside_line() -> Option<String> {
    let n = BESIDE.try_with(|n| *n).ok().filter(|n| *n > 0)?;
    let s = if n == 1 { "" } else { "s" };
    Some(format!(
        "[ran at once with {n} other call{s} of the same response]"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_names_a_file_by_its_path_or_its_extension() {
        assert_eq!(file_names("sleep"), Vec::<String>::new());
        assert_eq!(file_names("2"), Vec::<String>::new());
        assert_eq!(file_names("1.5"), Vec::<String>::new());
        assert_eq!(file_names("gen.py"), ["gen.py"]);
        assert_eq!(file_names("src/lib.rs"), ["lib.rs"]);
        assert_eq!(file_names("out/"), ["out"]);
        assert_eq!(file_names("--out=build/app.tar"), ["app.tar"]);
        assert_eq!(
            file_names("python3 gen.py > out.txt && cat 'out.txt'"),
            ["gen.py", "out.txt", "out.txt"]
        );
        assert_eq!(file_names("./ .. ."), Vec::<String>::new());
        assert_eq!(file_names("-n 1,20p"), Vec::<String>::new());
        assert_eq!(file_names("make 2>/dev/null"), Vec::<String>::new());
    }

    fn touch(class: ToolClass, alone: bool, keys: &[&str]) -> Touch {
        Touch {
            class,
            alone,
            keys: keys.iter().map(|k| k.to_string()).collect(),
        }
    }

    /// The rule, call by call: whether each joins the open group.
    fn joins(calls: &[Touch]) -> Vec<bool> {
        let mut open = Open::default();
        calls.iter().map(|t| open.admit(t)).collect()
    }

    #[test]
    fn programs_join_programs_until_one_names_what_another_does() {
        let run = |k: &[&str]| touch(ToolClass::Run, false, k);
        let got = joins(&[
            run(&[]),
            run(&[]),
            run(&["name:gen.py"]),
            run(&["name:gen.py", "name:out.txt"]),
            run(&["term:t1"]),
            run(&["term:t1"]),
            run(&["term:t2"]),
        ]);
        assert_eq!(got, [false, true, true, false, true, false, true]);
    }

    #[test]
    fn a_class_change_and_a_call_alone_end_the_group() {
        let got = joins(&[
            touch(ToolClass::Write, false, &["path:/w/a"]),
            touch(ToolClass::Write, false, &["path:/w/b"]),
            touch(ToolClass::Write, false, &["path:/w/a"]),
            touch(ToolClass::Run, false, &[]),
            touch(ToolClass::Run, true, &[]),
            touch(ToolClass::Run, false, &[]),
            touch(ToolClass::Read, false, &[]),
            touch(ToolClass::Read, false, &[]),
            touch(ToolClass::Write, true, &[]),
            touch(ToolClass::Write, false, &["path:/w/c"]),
        ]);
        assert_eq!(
            got,
            [false, true, false, false, false, false, false, true, false, false]
        );
    }
}
