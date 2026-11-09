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
//! uses it). The walk reads every spawn call under the `src/` of the three
//! daemon-resident crates (theseus-core, theseus-kernel, theseusd). A file
//! with a spawn call that is not reached through `children::spawn(` (on the
//! call's own line or within the two lines above it) must be in [`BARE`], with
//! the reason it is safe. The table lists **files**, not lines, so a line
//! move does not churn it; the failure carries the lines.
//!
//! **The blind spot, named.** `crate::spawn::spawn` (theseus-kernel's
//! `job.rs`, `spawn_l0`) starts a child by `clone3`/posix_spawn and returns a
//! raw pid, not a `Child`. This walk cannot see it; it runs in the job
//! wrapper, and the census does not claim it.

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
        "theseus-core/src/blobs.rs",
        "test-only: a writer killed mid-batch",
    ),
    (
        "theseus-core/src/peer.rs",
        "test-only: a stand-in peer process",
    ),
    ("theseus-core/src/telemetry/tests_cancel.rs", "test-only"),
    ("theseus-core/src/tests_m3.rs", "test-only"),
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
        "test-only: a job's process found by its socket",
    ),
];

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
    /// Per file with a bare spawn: its lines.
    bare: BTreeMap<String, String>,
}

/// Walk each root (a crate's `src/`), naming files relative to the root's
/// parent's parent (`crates/`), so `theseus-kernel/src/job.rs`.
fn walk(roots: &[PathBuf]) -> Walk {
    let mut w = Walk {
        files: 0,
        total: 0,
        registered: 0,
        bare: BTreeMap::new(),
    };
    // Built at run time so this file does not match its own walk.
    let needle = [".spawn", "()"].concat();
    for root in roots {
        let base = root.parent().and_then(Path::parent).unwrap_or(root);
        let files = rust_files(root);
        w.files += files.len();
        for (path, text) in files {
            let lines: Vec<&str> = text.lines().collect();
            let mut at = Vec::new();
            for (i, l) in lines.iter().enumerate() {
                if !l.contains(needle.as_str()) {
                    continue;
                }
                w.total += 1;
                let from = i.saturating_sub(2);
                if lines[from..=i]
                    .iter()
                    .any(|x| x.contains("children::spawn("))
                {
                    w.registered += 1;
                } else {
                    at.push((i + 1).to_string());
                }
            }
            if !at.is_empty() {
                let name = path
                    .strip_prefix(base)
                    .unwrap_or(&path)
                    .display()
                    .to_string();
                w.bare
                    .insert(name, format!("bare spawn at line {}", at.join(", ")));
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
        world: "the walk of every spawn call under the three daemon crates' src/",
        add: "register the child with theseus_kernel::children::spawn and the \
              right Kind (Owned if its spawner waits for it), or table the file \
              in BARE with the reason it is safe: a process that never runs the \
              serve path's SIGCHLD sweep, or test-only code. An unregistered \
              child its spawner waits for loses a race to that sweep about 2% \
              of the time (wait: No child processes)",
        drop: "a file's last bare spawn went (or became registered): delete its \
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
        "the walk found {} spawn sites, {} of them through children::spawn: \
         the matcher stopped matching, so it holds nothing",
        w.total,
        w.registered
    );
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
            "fn g(x: &mut std::process::Command) {{ let _c = x.spawn{}.unwrap(); }}\n",
            "()"
        ),
    )
    .unwrap();
    let w = walk(&[src]);
    assert_eq!(w.bare.len(), 2, "both planted files are bare");
    crate::complete::check(
        &crate::complete::MustBeComplete { least: 1, ..spec() },
        std::iter::empty(),
        w.bare.iter().map(|(f, at)| (f.clone(), at.clone())),
    );
}
