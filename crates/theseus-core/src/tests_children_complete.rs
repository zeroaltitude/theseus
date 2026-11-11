//! The children registry's invariant, asserted (theseus-3i6d1, B6).
//!
//! `theseus_kernel::children`'s module doc says: *every child the daemon
//! starts goes through `spawn`; one that did not, and was waited for by its
//! spawner, could be reaped by the SIGCHLD sweep as an orphan.* Nothing
//! checked it. A scratch model of the real sweep measured the race at about
//! 2% per start (800 runs per arm: bare spawn, 15 failed `wait: No child
//! processes` and 15 were reaped as orphans; the same child registered
//! `Kind::Owned`, 0 and 0).
//!
//! This is the completeness census the codebase already has a shape for
//! (`crate::complete`, as `outbox::tests::every_operator_call_site_is_accounted_for`
//! uses it). The walk reads every call that forks a child under the `src/` of
//! the three daemon-resident crates (theseus-core, theseus-kernel, theseusd).
//! A file with such a call that is not reached through `children::spawn(` (on
//! the call's own line or within the two lines above it) must be in [`BARE`],
//! with the reason it is safe. The table lists **files**, not lines, so a line
//! move does not churn it; the failure carries the lines.
//!
//! **Three call shapes, not one** (theseus-v4cj, B6). The walk read `spawn()`
//! alone until this step, and `Command::output()` and `Command::status()` fork
//! and wait exactly as `spawn()` does. So the login-shell capture's own
//! `output()` call on its command — a child of the serving daemon, waited for
//! by its spawner, in the shape this census exists to catch — never appeared
//! in [`BARE`], and its branch passed this test green. A census blind to the
//! shape used teaches a reviewer nothing. [`SHAPES`] is now all three.
//!
//! `output()` and `status()` are also plain getters on other types (an HTTP
//! response's, a board's), and they fork only on a `Command`, so the walk
//! reads those two only where `Command` is named in the **function that holds
//! the call** ([`fn_names_a_command`]) — where a command is built and run.
//! `spawn()` is read wherever it appears, as before. Over-reading would fill
//! this table with rows that say "not a command", and a tabled file is a file
//! this census no longer watches.
//!
//! **What the widening measured** (over main `42b9212e`, the walk of 1,622
//! source files): 66 fork sites, 17 of them through `children::spawn` and 34
//! files bare — 36 `spawn()`, 20 `output()`, 10 `status()`. The walk's own
//! figures are a property of the tree and not of a branch, so they move with
//! every landing; the main sha is named so a later reader can tell a
//! historical measurement from a wrong one. Reading `output()` and `status()`
//! everywhere, rather than only in a function that names a `Command`, would
//! have found 241 sites and 89 bare files, nearly all of them an HTTP
//! response's status. Twenty-one files joined [`BARE`] and none left it.
//!
//! **The table moves with the tree, so a red here is attributed by walking the
//! merged tree** (theseus-v4cj, the 18:14 departure of 2026-11-10). The 35th
//! row, `tests_project_context.rs`, is the `git` its fixture repository is
//! built with. It arrived from a different row in the same departure, after
//! this branch's base: each branch passes this test alone, neither touches a
//! file the other touches, and the first tree in which both exist is the
//! departure's merge — which is where this census is meant to catch it. So
//! `git log -- tests_children_complete.rs` names nothing on such a red; walk
//! the merged tree instead.
//!
//! Two of the newly visible sites were not safe and were fixed rather than
//! tabled, since the only reasons this table accepts are "no sweep runs in
//! that process" and "test-only": the capture's own (above), and
//! `rsi::mint::validate::fixtures`, whose `git` runs in the daemon.
//!
//! **The blind spots, named.** `crate::spawn::spawn` (theseus-kernel's
//! `job.rs`, `spawn_l0`) starts a child by `clone3`/posix_spawn and returns a
//! raw pid, not a `Child`. This walk cannot see it; it runs in the job
//! wrapper, and the census does not claim it. Nor does it see a `Command`
//! reached through a field or a call (`self.cmd` then the shape) from a
//! function that names no `Command` of its own.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Every file that starts a child outside `children::spawn`, and why that is
/// safe. The daemon's reaper (`theseusd`'s `reap_children`, its
/// `signal(SignalKind::child())`) runs only on the serve path; the `Hand`
/// and `Wrapper` subcommands exit before it, so no sweep runs in those
/// processes and there is no race to lose. A `#[cfg(test)]` spawn runs in a
/// test process, which has no sweep either.
const BARE: &[(&str, &str)] = &[
    (
        "theseus-core/src/aws/hands/hand.rs",
        "production: runs in the `theseusd hand` process, which exits at main.rs's \
         `Cmd::Hand` arm before `reap_children` installs SIGCHLD; no sweep there",
    ),
    (
        "theseus-kernel/src/job.rs",
        "production, `spawn_l0` (the first, non-test site): runs in the job wrapper \
         process, which returns before the serve path; its test spawns are test-only",
    ),
    (
        "theseusd/src/install/host.rs",
        "production, `System::run` (the account tools of `theseusd install --separate`): \
         `main.rs`'s `Cmd::Install` arm returns before `daemon()` spawns \
         `reap_children`, so no sweep runs in that process",
    ),
    (
        "theseus-core/src/crash.rs",
        "test-only: in the file's own `#[cfg(test)] mod tests`, which runs the \
         binary to see a panic write its crash file",
    ),
    (
        "theseus-core/src/rsi/join/install.rs",
        "test-only: both sites are in the file's own `#[cfg(test)] mod tests`, \
         which runs `scripts/build.sh` and the generated install script",
    ),
    (
        "theseus-core/src/blobs.rs",
        "test-only: a writer killed mid-batch",
    ),
    (
        "theseus-core/src/peer.rs",
        "test-only: a stand-in peer process, and the `kill` that its `Drop` runs; \
         both are under `#[cfg(test)]` in the file",
    ),
    (
        "theseus-core/src/broker/tests.rs",
        "test-only: a hermetic git",
    ),
    ("theseus-core/src/flow/tests_run.rs", "test-only"),
    ("theseus-core/src/judge/people/tests_same.rs", "test-only"),
    ("theseus-core/src/own_bench/whole/tests.rs", "test-only"),
    ("theseus-core/src/rpc/tests_scratch.rs", "test-only"),
    ("theseus-core/src/rpc/tests_task_pause.rs", "test-only"),
    ("theseus-core/src/rsi/join/tests.rs", "test-only"),
    ("theseus-core/src/rsi/join/tests_sealed.rs", "test-only"),
    (
        "theseus-core/src/rsi/mint/validate/tests_dry.rs",
        "test-only",
    ),
    ("theseus-core/src/rsi/revert/tests.rs", "test-only"),
    (
        "theseus-core/src/rsi/revert/tests/fetch_retry.rs",
        "test-only",
    ),
    ("theseus-core/src/rsi/sealed/tests_runner.rs", "test-only"),
    ("theseus-core/src/rsi/tests_fetch_bound.rs", "test-only"),
    ("theseus-core/src/rsi/tests_identity.rs", "test-only"),
    ("theseus-core/src/rsi/tests_pr.rs", "test-only"),
    ("theseus-core/src/rsi/tests_tree.rs", "test-only"),
    ("theseus-core/src/telemetry/tests_cancel.rs", "test-only"),
    ("theseus-core/src/tests_activation_pace.rs", "test-only"),
    ("theseus-core/src/tests_m3.rs", "test-only"),
    (
        "theseus-core/src/tests_project_context.rs",
        "test-only: a hermetic git builds the fixture repository the project \
         context reads",
    ),
    ("theseus-core/src/tests_retry.rs", "test-only"),
    ("theseus-kernel/src/job_wait.rs", "test-only: a pidfd wait"),
    ("theseus-kernel/src/spool.rs", "test-only"),
    ("theseus-kernel/src/tests_enosys.rs", "test-only"),
    ("theseus-kernel/src/tests_gone.rs", "test-only"),
    ("theseus-kernel/src/tests_spool.rs", "test-only"),
    (
        "theseus-kernel/src/tree.rs",
        "test-only: a child is a descendant until reaped",
    ),
    (
        "theseusd/src/mcp/trace.rs",
        "test-only: a job's process found by its socket, and the `kill` that ends it",
    ),
];

/// The `Command` methods that fork a child and wait for it. Each is matched
/// as `.<name>()`, built at run time so this file does not match its own
/// walk. `spawn()` returns the child instead of waiting, but its caller then
/// waits, which is the same race.
const SHAPES: [&str; 3] = ["spawn", "output", "status"];

/// Does this line declare a function? Its trimmed text reaches `fn` through
/// nothing but the words rustc allows before it, so a `fn` inside a comment
/// or an expression is not one.
fn starts_a_fn(line: &str) -> bool {
    let t = line.trim_start();
    let Some(cut) = t.find("fn ") else {
        return false;
    };
    t[..cut].split_whitespace().all(|word| {
        matches!(word, "const" | "async" | "unsafe" | "default" | "extern")
            || word == "pub"
            || word.starts_with("pub(")
            || word.starts_with('"')
    })
}

/// Does the function that holds line `i` name a `Command`? Its bounds are the
/// nearest function declaration at or above the line, and the next one below.
/// This is what tells `Command::output()` from an HTTP response's `status()`:
/// a call that forks is a command's, and a command is built where it is run.
fn fn_names_a_command(lines: &[&str], i: usize) -> bool {
    let start = (0..=i).rev().find(|&j| starts_a_fn(lines[j])).unwrap_or(0);
    let end = (i + 1..lines.len())
        .find(|&j| starts_a_fn(lines[j]))
        .unwrap_or(lines.len());
    lines[start..end].iter().any(|l| l.contains("Command"))
}

/// Every `*.rs` under `dir`, as (path, text).
fn rust_files(dir: &Path) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).expect("read the crate's source") {
            let p = e.expect("a source entry").path();
            if p.is_dir() {
                todo.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                let text = std::fs::read_to_string(&p).expect("read a source file");
                out.push((p, text));
            }
        }
    }
    out
}

/// What a walk found.
struct Walk {
    files: usize,
    total: usize,
    registered: usize,
    /// How many calls of each of [`SHAPES`] the walk read, so a shape that
    /// stops matching is a failure and not a quiet narrowing.
    by_shape: BTreeMap<&'static str, usize>,
    /// Per file with a bare fork: its lines, each with its shape.
    bare: BTreeMap<String, String>,
}

/// Walk each root (a crate's `src/`), naming files relative to the root's
/// parent's parent (`crates/`), so `theseus-kernel/src/job.rs`.
fn walk(roots: &[PathBuf]) -> Walk {
    let mut w = Walk {
        files: 0,
        total: 0,
        registered: 0,
        by_shape: BTreeMap::new(),
        bare: BTreeMap::new(),
    };
    // Built at run time so this file does not match its own walk.
    let shapes: Vec<String> = SHAPES.iter().map(|s| [".", s, "()"].concat()).collect();
    for root in roots {
        let base = root.parent().and_then(Path::parent).unwrap_or(root);
        let files = rust_files(root);
        w.files += files.len();
        for (path, text) in files {
            let lines: Vec<&str> = text.lines().collect();
            let mut at = Vec::new();
            for (i, l) in lines.iter().enumerate() {
                let Some(k) = shapes.iter().position(|s| l.contains(s.as_str())) else {
                    continue;
                };
                // `output()` and `status()` fork only on a `Command`, and are
                // getters on much else: read them where the function that
                // holds the call names one. `spawn()` is read everywhere.
                if SHAPES[k] != "spawn" && !fn_names_a_command(&lines, i) {
                    continue;
                }
                w.total += 1;
                *w.by_shape.entry(SHAPES[k]).or_default() += 1;
                let from = i.saturating_sub(2);
                if lines[from..=i]
                    .iter()
                    .any(|x| x.contains("children::spawn("))
                {
                    w.registered += 1;
                } else {
                    at.push(format!("{}{}", i + 1, shapes[k]));
                }
            }
            if !at.is_empty() {
                let name = path
                    .strip_prefix(base)
                    .unwrap_or(&path)
                    .display()
                    .to_string();
                w.bare
                    .insert(name, format!("a bare fork at line {}", at.join(", ")));
            }
        }
    }
    w
}

fn daemon_roots() -> Vec<PathBuf> {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    ["src", "../theseus-kernel/src", "../theseusd/src"]
        .iter()
        .map(|p| here.join(p))
        .collect()
}

fn spec() -> crate::complete::MustBeComplete<'static> {
    crate::complete::MustBeComplete {
        item: "a file that starts a child outside `children::spawn`",
        list: "tests_children_complete::BARE, the census",
        world: "the walk of every spawn(), output() and status() call under the \
                three daemon crates' src/",
        add: "register the child with theseus_kernel::children::spawn and the \
              right Kind (Owned if its spawner waits for it), or table the file \
              in BARE with the reason it is safe: a process that never runs the \
              serve path's SIGCHLD sweep, or test-only code. An unregistered \
              child its spawner waits for loses a race to that sweep about 2% \
              of the time (wait: No child processes). A reason that is not one \
              of those two is not a reason: fix the call instead",
        drop: "a file's last bare fork went (or became registered): delete its \
               row from BARE",
        least: 10,
        stale_fails: true,
    }
}

#[test]
fn every_daemon_child_is_registered_or_accounted_for() {
    let w = walk(&daemon_roots());
    assert!(
        w.files > 1_000,
        "only {} source files found under theseus-core, theseus-kernel and theseusd: \
         the walk read the wrong tree",
        w.files
    );
    assert!(
        w.total >= 25 && w.registered >= 10,
        "the walk found {} fork sites, {} of them through children::spawn: \
         the matcher stopped matching, so it holds nothing",
        w.total,
        w.registered
    );
    // Each shape must still match something. A shape that quietly stopped
    // matching is how this census went blind to `output()` in the first
    // place, and three counts over one total cannot hide it.
    for shape in SHAPES {
        assert!(
            w.by_shape.get(shape).copied().unwrap_or(0) > 0,
            "the walk read no .{shape}() call at all, so that shape holds \
             nothing: {:?} over {} sites",
            w.by_shape,
            w.total
        );
    }
    crate::complete::check(
        &spec(),
        BARE.iter()
            .map(|(f, why)| ((*f).to_string(), (*why).to_string())),
        w.bare.iter().map(|(f, at)| (f.clone(), at.clone())),
    );
}

/// The red, planted: a daemon source with a bare spawn spliced in is found,
/// and the census names it.
#[test]
#[should_panic(expected = "crates_x/src/planted.rs")]
fn a_planted_bare_spawn_is_named() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("crates_x").join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("planted.rs"),
        format!(
            "fn f(x: &mut std::process::Command) {{\n    let _c = x.spawn{};\n}}\n",
            "()"
        ),
    )
    .unwrap();
    std::fs::write(
        src.join("planted2.rs"),
        format!(
            "fn g(x: &mut std::process::Command) {{ let _c = x.output{}.unwrap(); }}\n",
            "()"
        ),
    )
    .unwrap();
    // And the precision half: `status()` on something that is not a command,
    // in a function that names none, is not a fork and is not read. Without
    // this the walk would table every file that reads an HTTP status, and a
    // tabled file is one this census no longer watches.
    std::fs::write(
        src.join("not_a_command.rs"),
        format!(
            "async fn h(r: Response) -> u16 {{ r.status{}.as_u16() }}\n",
            "()"
        ),
    )
    .unwrap();
    let w = walk(&[src]);
    assert_eq!(
        w.bare.len(),
        2,
        "both planted files are bare and the response's status() is not: {:?}",
        w.bare
    );
    crate::complete::check(
        &crate::complete::MustBeComplete { least: 1, ..spec() },
        std::iter::empty(),
        w.bare.iter().map(|(f, at)| (f.clone(), at.clone())),
    );
}
