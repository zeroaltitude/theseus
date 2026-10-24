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
//!   two calls to one MCP server, two programs whose argvs name one file
//!   (`gen.py`, then `cat gen.py`'s output), or two that share an operand
//!   stay in order. A harness verb (`task.create`, `extend.propose`) and an
//!   AWS hands group run alone.
//!
//! **The dependency rule for programs** (theseus-0hufe): an operand is a
//! non-flag argument that names a thing (a service, unit, container,
//! process, host, port, URL, branch, database, directory). Two programs of
//! one response that share one stay in order, the later starting once the
//! earlier has ended: `systemctl --user start web` then `systemctl --user
//! status web`, `docker run --name db …` then `docker exec db psql …`, `git
//! worktree add ../wt-a` then `make -C ../wt-a`. Left out are the program
//! itself, bare numbers, a few words every call of a tool shares (`json`),
//! and the leading subcommand words both calls begin with, so `npm test`
//! twice, `pytest tests/a.py` beside `pytest tests/b.py`, two `curl`s to
//! different URLs and two builds in different directories run together.
//! `operands.rs` has the words; the rule keys on no tool's name. It does not
//! reach `kubectl apply -f api.yaml` then `kubectl rollout status deploy/api`:
//! the file `api.yaml` and the resource `deploy/api` share no whole word, and
//! splitting words apart would join every `pytest tests/…` pair too. A call
//! held back says so (`HELD`): "started after call 2, which names `web` too".
//! A script's `cd /srv/app` names where it runs, as a call's own `cwd` does:
//! two calls that change into one directory share nothing by that alone
//! (`cd /srv/app && load-part 2` beside `… load-part 3`, theseus-1kr2g), but
//! one that names that directory as a file or argument still waits for it.
//!
//! `[tools] parallel_reads`, `parallel_writes` and `parallel_runs` cap how
//! many calls of a group of that class start at once; the rest of the group
//! waits for a slot, and the results keep their order.
//!
//! A class change is a barrier, as before: a program after a write starts
//! once the write has ended, and a read after a program reads its effect.
//! The owner's call (theseus-d1hi, 2026-10-09 09:07): programs run together,
//! with insurance that dependent calls are not flattened.

use std::collections::BTreeSet;

use serde_json::Value;
use theseus_tools::{Access, Backend, Plan, Tool, ToolClass};

use super::operands;

tokio::task_local! {
    /// How many other calls of its group ran at once with the call being
    /// run, set around each call of a group of writes or programs: its
    /// result says so, and the model learns what it may put in one response.
    pub(super) static BESIDE: usize;

    /// Why the call being run started after the calls before it: the
    /// operand (or file, terminal) it shares with one of them
    /// (theseus-0hufe). Set around the first call of the group it opened.
    pub(super) static HELD: String;
}

/// What a gated call touches, for the grouping rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Touch {
    pub(super) class: ToolClass,
    /// It runs in a group of its own.
    pub(super) alone: bool,
    /// What a later call of its group must not touch too.
    pub(super) keys: BTreeSet<String>,
    /// The operands of a program's commands (`operands.rs`).
    pub(super) ops: Vec<operands::Invocation>,
}

impl Touch {
    /// `tool`'s call of `input`, by its gated `plan`.
    pub(super) fn of(tool: &dyn Tool, input: &Value, plan: &Plan) -> Self {
        let class = plan.class.unwrap_or(tool.class());
        let mut keys = BTreeSet::new();
        let mut ops = Vec::new();
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
                let args = argvs.clone().flat_map(|a| a.iter().skip(1));
                keys.extend(
                    args.clone()
                        .flat_map(|a| file_names(a))
                        .map(|n| format!("name:{n}")),
                );
                keys.extend(args.flat_map(|a| dir_names(a)).map(|n| format!("dir:{n}")));
                keys.extend(
                    plan.resources
                        .iter()
                        .filter(|r| r.access != Access::Exec)
                        .filter_map(|r| r.path.file_name())
                        .map(|n| format!("name:{}", n.to_string_lossy())),
                );
                ops = operands::of_argvs(argvs);
                tool.backend() == Backend::Harness || name == crate::aws::hands::RUN
            }
        };
        Self {
            class,
            alone,
            keys,
            ops,
        }
    }
}

/// The open group: its class, and its calls with what each touches; and
/// the calls of the groups before it.
#[derive(Default)]
pub(super) struct Open {
    class: Option<ToolClass>,
    members: Vec<(usize, Touch)>,
    earlier: Vec<(usize, Touch)>,
}

/// What a call joining the open group came to.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Verdict {
    /// It joins the open group; otherwise the group is over and the call
    /// opens the next.
    pub(super) joins: bool,
    /// The earlier call of the response it shares something with (the first
    /// in call order), and the thing: it starts once that call has ended, and
    /// its result says so.
    pub(super) held: Option<(usize, String)>,
}

impl Open {
    /// Whether `t`, the response's call `index`, joins the group; when it
    /// does not, the group is over and `t` opens the next (a call that runs
    /// alone closes it behind itself).
    pub(super) fn admit(&mut self, index: usize, t: &Touch) -> Verdict {
        let shares = |(i, m): &(usize, Touch)| shared(m, t).map(|on| (*i, on));
        let in_group = self.members.iter().find_map(shares);
        let joins = !t.alone && self.class == Some(t.class) && in_group.is_none();
        if !joins {
            self.earlier.append(&mut self.members);
        }
        let held = in_group.or_else(|| self.earlier.iter().find_map(shares));
        self.class = (!t.alone).then_some(t.class);
        self.members.push((index, t.clone()));
        Verdict { joins, held }
    }
}

/// What two calls share: a key (a path, terminal, server or file name), else
/// an operand.
fn shared(m: &Touch, t: &Touch) -> Option<String> {
    let key = m.keys.iter().find(|k| meets(k, &t.keys)).map(|k| shown(k));
    key.or_else(|| operands::shared(&m.ops, &t.ops))
}

/// Whether one call's `key` meets a key of the other's: the same key, except
/// that a directory both change into (`dir:`) is only where both run; a file
/// name of the other's that names it (`name:`) meets it.
fn meets(key: &str, other: &BTreeSet<String>) -> bool {
    let crossed = |from: &str, to: &str| {
        key.strip_prefix(from)
            .is_some_and(|n| other.contains(&format!("{to}{n}")))
    };
    if key.starts_with("dir:") {
        crossed("dir:", "name:")
    } else {
        other.contains(key) || crossed("name:", "dir:")
    }
}

/// A shared key as the model reads it: the name, path or terminal.
fn shown(key: &str) -> String {
    key.split_once(':')
        .map_or(key, |(_, rest)| rest)
        .to_string()
}

/// The line a call that started after another says why, with the call's
/// number in the response (1-based) and what both name.
pub(super) fn held_note(by: usize, on: &str) -> String {
    format!("started after call {}, which names `{on}` too", by + 1)
}

/// The file names an argument names: the last part of each word in it that
/// holds a `/` (a path or a URL), or that ends in an extension (`gen.py`).
/// A shell script's words are split at its operators and quotes, and an
/// option's value at its `=`. A device (`/dev/null`) is no file. It errs toward a name: two programs that share
/// one only wait for each other. A script's `cd` and `pushd` name a directory,
/// not a file (`dir_names`).
pub(super) fn file_names(arg: &str) -> Vec<String> {
    commands(arg)
        .filter(|c| !changes_dir(c))
        .flat_map(names)
        .collect()
}

/// The directories a script's `cd` and `pushd` change into, named as
/// `file_names` names a file: where the script runs, not a thing it works on.
pub(super) fn dir_names(arg: &str) -> Vec<String> {
    commands(arg)
        .filter(|c| changes_dir(c))
        .flat_map(names)
        .collect()
}

/// A script's simple commands, split at its operators.
fn commands(arg: &str) -> impl Iterator<Item = &str> {
    arg.split(|c: char| ";|&\n()".contains(c))
}

/// Whether a simple command only changes the script's directory.
fn changes_dir(command: &str) -> bool {
    matches!(command.split_whitespace().next(), Some("cd" | "pushd"))
}

/// The file names one simple command's words name.
fn names(arg: &str) -> Vec<String> {
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
    let beside = BESIDE.try_with(|n| *n).ok().filter(|n| *n > 0).map(|n| {
        let s = if n == 1 { "" } else { "s" };
        format!("ran at once with {n} other call{s} of the same response")
    });
    let held = HELD.try_with(|h| h.clone()).ok();
    let lines: Vec<_> = beside
        .into_iter()
        .chain(held)
        .map(|l| format!("[{l}]"))
        .collect();
    (!lines.is_empty()).then(|| lines.join("\n"))
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
            ops: Vec::new(),
        }
    }

    /// The rule, call by call: whether each joins the open group.
    fn joins(calls: &[Touch]) -> Vec<bool> {
        let mut open = Open::default();
        calls
            .iter()
            .enumerate()
            .map(|(i, t)| open.admit(i, t).joins)
            .collect()
    }

    fn ops(line: &str) -> Touch {
        let argv: Vec<String> = line.split(' ').map(String::from).collect();
        Touch {
            ops: operands::of_argvs([&argv]),
            ..touch(ToolClass::Run, false, &[])
        }
    }

    #[test]
    fn a_program_sharing_an_operand_is_held_and_says_after_which_call() {
        let mut open = Open::default();
        let v = |open: &mut Open, i, line: &str| open.admit(i, &ops(line));
        v(&mut open, 0, "sleep 2");
        let first = v(&mut open, 1, "systemctl --user start web");
        assert!(first.joins && first.held.is_none());
        assert!(v(&mut open, 2, "pytest tests/a.py").joins);
        let held = v(&mut open, 3, "systemctl --user status web");
        assert_eq!(held.held, Some((1, "web".to_string())));
        assert!(!held.joins);
        assert_eq!(
            held_note(1, "web"),
            "started after call 2, which names `web` too"
        );
        // The group it opened is the open one now.
        assert!(v(&mut open, 4, "pytest tests/b.py").joins);
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

    /// A `proc.run` call as the grouping reads it, through its gated plan:
    /// `$ line` a `bash -c` line, else an argv split at its spaces.
    fn proc_run(call: &str) -> Touch {
        let argv: Vec<String> = match call.strip_prefix("$ ") {
            Some(line) => vec!["bash".into(), "-c".into(), line.into()],
            None => call.split(' ').map(String::from).collect(),
        };
        let plan = Plan {
            argv: Some(argv),
            ..Plan::default()
        };
        Touch::of(&theseus_tools::proc::Run, &Value::Null, &plan)
    }

    /// The whole rule, keys and operands, on the shapes a model writes a
    /// fan-out of independent parts in (theseus-1kr2g): none shares a thing,
    /// so all run together; what truly depends on another still waits.
    #[test]
    fn a_fan_out_of_parts_shares_nothing_and_a_true_share_still_waits() {
        let independent = [
            ("load-part 2", "load-part 3"),
            ("load-part part-2", "load-part part-3"),
            ("$ load-part 2", "$ load-part 3"),
            (
                "$ cd /srv/app && load-part 2",
                "$ cd /srv/app && load-part 3",
            ),
            (
                "$ pushd /srv/app; load-part 2",
                "$ pushd /srv/app; load-part 3",
            ),
            (
                "$ load-part 2; echo \"exit $?\"",
                "$ load-part 3; echo \"exit $?\"",
            ),
            (
                "$ load-part 2 2>&1 | tee part-2.log",
                "$ load-part 3 2>&1 | tee part-3.log",
            ),
            ("$ timeout 600 load-part 2", "$ timeout 600 load-part 3"),
            ("timeout 600 load-part 2", "timeout 600 load-part 3"),
            (
                "$ until load-part 2; do sleep 1; done",
                "$ until load-part 3; do sleep 1; done",
            ),
            (
                "$ cd /srv/app && load-part 2 2>&1 | tee /tmp/p2.log; echo \"exit=$?\"",
                "$ cd /srv/app && load-part 3 2>&1 | tee /tmp/p3.log; echo \"exit=$?\"",
            ),
            ("$ cd /srv/app && load-part 2", "load-part /srv/app/3"),
        ];
        for (a, b) in independent {
            assert_eq!(shared(&proc_run(a), &proc_run(b)), None, "{a} / {b}");
            assert_eq!(shared(&proc_run(b), &proc_run(a)), None, "{b} / {a}");
        }
        let depends = [
            (
                "$ cd /srv/app && load-part 2",
                "load-part 3 --out /srv/app",
                "app",
            ),
            (
                "$ load-part 2 > /srv/app/p2.out",
                "$ cat /srv/app/p2.out",
                "p2.out",
            ),
            (
                "$ mkdir -p /srv/wt && load-part 2",
                "$ cd /srv/wt && make",
                "wt",
            ),
            ("git worktree add ../wt-a", "$ cd ../wt-a && make", "wt-a"),
        ];
        for (a, b, on) in depends {
            let (a, b) = (proc_run(a), proc_run(b));
            assert_eq!(shared(&a, &b).as_deref(), Some(on), "{a:?} / {b:?}");
            assert!(shared(&b, &a).is_some(), "{b:?} / {a:?}");
        }
    }
}
