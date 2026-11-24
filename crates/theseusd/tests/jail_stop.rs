//! A job stopped at its limit in a daemon's jail names the phase its log was
//! last in (theseus-vv4km; theseus-ccik8 did it for the L0 stop only, and a
//! daemon never takes that one); a log with no phase line says so.
//!
//! `Jail::run_blocking` starts the job's init as `/proc/self/exe job-sandbox`,
//! the running binary (`theseusd`, in a daemon). A libtest binary reads that
//! word as a test filter, runs nothing and exits 0, so the init never reports
//! ("it exited before it started the command, exit status 0") and the stop is
//! never reached. So this is a test binary of its own (`harness = false`),
//! which takes the init's role first, as the sandbox's own test binaries do
//! (`theseus-sandbox/tests/common`). `self_build.rs` cannot hold it: its jobs
//! run through the wrapper, which is `theseusd`.
//!
//! Real L1, unprivileged: as root L1 refuses every job (theseus-pv6i), and the
//! case only checks that refusal.
//!
//! The self-join's count (issue 45) runs main's frozen `scripts/stats.py` the same way, so two more
//! cases hold what that step relies on: `python3` and `git` work in the view, reading the repository
//! through its overlay, and a stopped job leaves nothing of itself running, not even a process that
//! left the job's group.

#[path = "../../theseus-sandbox/tests/common/mod.rs"]
mod common;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{check, Case};
use theseus_core::rsi::jail::{Clocked, Jail, Job, CLOCKED_OUTPUT};
use theseus_core::rsi::tree::{Place, Tree};

fn main() -> ! {
    common::main(
        &[
            Case {
                name: "a_stopped_jailed_job_names_its_phase",
                run: a_stopped_jailed_job_names_its_phase,
            },
            Case {
                name: "the_counter_runs_in_l1_with_python_and_git",
                run: the_counter_runs_in_l1_with_python_and_git,
            },
            Case {
                name: "a_stopped_jailed_job_leaves_nothing_of_itself_running",
                run: a_stopped_jailed_job_leaves_nothing_of_itself_running,
            },
            Case {
                name: "a_clocked_job_that_keeps_printing_is_stopped_at_its_limit",
                run: a_clocked_job_that_keeps_printing_is_stopped_at_its_limit,
            },
            Case {
                name: "a_range_whose_output_was_cut_gives_no_lines",
                run: a_range_whose_output_was_cut_gives_no_lines,
            },
        ],
        &[],
        |_| {},
    )
}

/// A job that outlives its limit is stopped, and the error names its log's
/// last `gate: ` line, or says there is none.
fn a_stopped_jailed_job_names_its_phase() -> Result<(), String> {
    if unsafe { libc::geteuid() } == 0 {
        check(
            theseus_sandbox::refused_here().is_some(),
            "L1 accepted a root daemon's job",
        )?;
        eprintln!(
            "skipped: L1 refuses a root daemon's jobs (theseus-pv6i); run as an ordinary user"
        );
        return Ok(());
    }
    let dir = tempfile::tempdir().map_err(|e| format!("a temp dir: {e}"))?;
    let p = |s: &str| dir.path().join(s);
    for d in ["repo/state", "repo/sealed", "tree", "target"] {
        std::fs::create_dir_all(p(d)).map_err(|e| format!("{d}: {e}"))?;
    }
    let mut cfg = theseus_core::Config::example();
    // The rig's state dir on the config: `sandbox_for` takes no directory
    // (theseus-0h9f3, the owner's word of 2026-11-11 11:34).
    cfg.server.state_dir = p("repo/state").to_string_lossy().into_owned();
    cfg.self_improve.sealed.dir = Some(p("repo/sealed").to_string_lossy().into_owned());
    let sandbox = theseus_core::toolrun::sandbox_for(&cfg);
    let tree = Tree {
        slug: "t".into(),
        branch: "self/t".into(),
        dir: p("tree"),
        target: p("target"),
    };
    let place = Place {
        repo: p("repo"),
        state: p("state"),
        env: Vec::new(),
        jail: None,
        notify: None,
        stop: None,
        owner: None,
    };
    let jail = Jail::new(Arc::new(sandbox));
    for (i, (body, want)) in [
        (
            "echo 'gate: phase suite'; exec sleep 60",
            "gate: phase suite",
        ),
        ("exec sleep 60", "no `gate: ` line"),
    ]
    .into_iter()
    .enumerate()
    {
        let log = p(&format!("stopped-{i}.log"));
        let job = Job {
            tree: &tree,
            ro: Vec::new(),
            argv: ["sh", "-c", body].map(OsString::from).to_vec(),
            env: Vec::new(),
            log: &log,
            limit: Duration::from_secs(3),
        };
        let e = jail
            .run_blocking(&place, &job)
            .err()
            .ok_or("a job that outlived its limit was not stopped")?;
        let said = format!("{e:#}");
        check(said.contains("was stopped"), format!("not a stop: {said}"))?;
        check(said.contains(want), format!("no {want:?} in: {said}"))?;
    }
    Ok(())
}

/// As root L1 refuses every job; then that refusal is all a case checks.
fn refused_as_root() -> Result<bool, String> {
    if unsafe { libc::geteuid() } != 0 {
        return Ok(false);
    }
    check(
        theseus_sandbox::refused_here().is_some(),
        "L1 accepted a root daemon's job",
    )?;
    eprintln!("skipped: L1 refuses a root daemon's jobs (theseus-pv6i); run as an ordinary user");
    Ok(true)
}

/// A jail over `root` (a repository at `repo/`, its tree at `tree/`), as the daemon's place has one,
/// on disk: `/tmp` is a tmpfs of its own inside the view.
fn jailed_rig(root: &Path) -> Result<(Place, Tree, Jail), String> {
    let p = |s: &str| root.join(s);
    for d in ["repo/state", "repo/sealed", "target"] {
        std::fs::create_dir_all(p(d)).map_err(|e| format!("{d}: {e}"))?;
    }
    let mut cfg = theseus_core::Config::example();
    cfg.server.state_dir = p("repo/state").to_string_lossy().into_owned();
    cfg.self_improve.sealed.dir = Some(p("repo/sealed").to_string_lossy().into_owned());
    let sandbox = theseus_core::toolrun::sandbox_for(&cfg);
    let tree = Tree {
        slug: "t".into(),
        branch: "self/t".into(),
        dir: p("tree"),
        target: p("target"),
    };
    let place = Place {
        repo: p("repo"),
        state: p("state"),
        env: Vec::new(),
        jail: None,
        notify: None,
        stop: None,
        owner: None,
    };
    Ok((place, tree, Jail::new(Arc::new(sandbox))))
}

fn git(at: &Path, args: &[&str]) -> Result<String, String> {
    let o = Command::new("git")
        .arg("-C")
        .arg(at)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .map_err(|e| format!("git: {e}"))?;
    check(
        o.status.success(),
        format!("git {args:?}: {}", String::from_utf8_lossy(&o.stderr)),
    )?;
    Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// The join's count, as it runs: main's `scripts/stats.py`, frozen outside the tree and bound
/// read-only, run with `python3 -I` in the view over a detached worktree holding a made merge, on the
/// clock (`Jail::run_clocked`). It counts (git reads the repository through the view's overlay, a
/// private writable copy that ends with the job) and writes STATS.md in the tree, and its `--range`
/// prints the change in lines.
fn the_counter_runs_in_l1_with_python_and_git() -> Result<(), String> {
    if refused_as_root()? {
        return Ok(());
    }
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).map_err(|e| format!("{e}"))?;
    let (place, tree, jail) = jailed_rig(dir.path())?;
    let repo = place.repo.clone();
    std::fs::write(repo.join("a.rs"), "fn a() {}\n").map_err(|e| format!("{e}"))?;
    git(&repo, &["init", "-q", "-b", "main"])?;
    git(&repo, &["add", "a.rs"])?;
    git(&repo, &["commit", "-q", "-m", "base"])?;
    git(&repo, &["checkout", "-q", "-b", "side"])?;
    std::fs::write(repo.join("b.rs"), "fn b() {}\n").map_err(|e| format!("{e}"))?;
    git(&repo, &["add", "b.rs"])?;
    git(&repo, &["commit", "-q", "-m", "side"])?;
    git(&repo, &["checkout", "-q", "main"])?;
    let t = tree.dir.to_string_lossy().into_owned();
    git(&repo, &["worktree", "add", "-q", "--detach", &t, "main"])?;
    git(&tree.dir, &["merge", "-q", "--no-ff", "-m", "join", "side"])?;
    let (main, side) = (
        git(&repo, &["rev-parse", "--short=8", "main"])?,
        git(&repo, &["rev-parse", "--short=8", "side"])?,
    );
    let frozen = dir.path().join("gates").join("stats-frozen.py");
    std::fs::create_dir_all(frozen.parent().unwrap()).map_err(|e| format!("{e}"))?;
    let counter = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/stats.py");
    std::fs::copy(&counter, &frozen).map_err(|e| format!("{}: {e}", counter.display()))?;
    let run = |args: &[&str], log: &PathBuf| -> Result<(), String> {
        let mut argv: Vec<OsString> = ["python3", "-I"].map(OsString::from).to_vec();
        argv.push(frozen.clone().into_os_string());
        argv.extend(args.iter().map(OsString::from));
        argv.extend(["--root".into(), tree.dir.clone().into_os_string()]);
        let job = Job {
            tree: &tree,
            ro: vec![frozen.clone()],
            argv,
            env: Vec::new(),
            log,
            limit: Duration::from_secs(60),
        };
        let code = jail
            .run_clocked(&place, &job)
            .map_err(|e| format!("{e:#}"))?
            .code;
        let said = std::fs::read_to_string(log).unwrap_or_default();
        check(code == Some(0), format!("{args:?} exited {code:?}: {said}"))
    };
    let log = dir.path().join("count.log");
    run(&["--count-head"], &log)?;
    let stats = std::fs::read_to_string(tree.dir.join("STATS.md")).map_err(|e| format!("{e}"))?;
    check(
        stats.contains(&format!("Counted at the merge of {side} into {main}, over")),
        format!("STATS.md: {stats}"),
    )?;
    let log = dir.path().join("range.log");
    run(&["--range", "main..HEAD"], &log)?;
    let said = std::fs::read_to_string(&log).unwrap_or_default();
    check(
        said.contains(&format!("lines {main}..")) && said.contains("code 1 -> 2 (+1)"),
        format!("the range line: {said}"),
    )
}

/// A job stopped at its limit takes every process it started with it, a process that left its
/// group by `setsid` too: one that would write a file after the stop never writes it.
fn a_stopped_jailed_job_leaves_nothing_of_itself_running() -> Result<(), String> {
    if refused_as_root()? {
        return Ok(());
    }
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).map_err(|e| format!("{e}"))?;
    let (place, tree, jail) = jailed_rig(dir.path())?;
    std::fs::create_dir_all(&tree.dir).map_err(|e| format!("{e}"))?;
    let t = tree.dir.display();
    let body = format!(
        "setsid sh -c 'echo began > {t}/began; sleep 3; echo late > {t}/late' </dev/null >/dev/null 2>&1 &\n\
         echo started; exec sleep 60"
    );
    let log = dir.path().join("stopped.log");
    let job = Job {
        tree: &tree,
        ro: Vec::new(),
        argv: ["sh".into(), "-c".into(), OsString::from(body)].to_vec(),
        env: Vec::new(),
        log: &log,
        limit: Duration::from_secs(1),
    };
    let e = jail
        .run_blocking(&place, &job)
        .err()
        .ok_or("a job that outlived its limit was not stopped")?;
    check(
        format!("{e:#}").contains("was stopped"),
        format!("not a stop: {e:#}"),
    )?;
    std::thread::sleep(Duration::from_secs(5));
    check(
        tree.dir.join("began").exists(),
        "the process that left the group never started",
    )?;
    check(
        !tree.dir.join("late").exists(),
        "a process of the stopped job ran on after the stop",
    )
}

/// The count's runs are bounded by the clock (issue 45): a job that keeps printing is stopped at its
/// limit, where a build's bound on silence would let it run on to 24 times that.
fn a_clocked_job_that_keeps_printing_is_stopped_at_its_limit() -> Result<(), String> {
    if refused_as_root()? {
        return Ok(());
    }
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).map_err(|e| format!("{e}"))?;
    let (place, tree, jail) = jailed_rig(dir.path())?;
    for d in [&tree.dir, &tree.target] {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    let log = dir.path().join("printing.log");
    let job = Job {
        tree: &tree,
        ro: Vec::new(),
        argv: ["sh", "-c", "while :; do echo tick; sleep 0.2; done"]
            .map(OsString::from)
            .to_vec(),
        env: Vec::new(),
        log: &log,
        limit: Duration::from_secs(3),
    };
    let t0 = Instant::now();
    let r = jail.run_clocked(&place, &job);
    let took = t0.elapsed();
    check(
        !matches!(r, Ok(Clocked { code: Some(0), .. })),
        format!("a job stopped at its limit said it passed: {r:?}"),
    )?;
    check(
        took < Duration::from_secs(20),
        format!("a printing job ran {took:?} past a 3 s clock"),
    )?;
    let said = std::fs::read_to_string(&log).unwrap_or_default();
    check(
        said.contains("tick"),
        format!("its output was not kept: {said:?} ({r:?})"),
    )
}

/// The count reads its `--range` line only from output it kept whole (issue 45): through the join's own range run,
/// in L1 on the clock, a counter that prints a good line and blank lines up to the cap gives that line; one that then
/// prints one line more, exiting 0 all the same, gives none, and its log says in this code's words that it was cut.
fn a_range_whose_output_was_cut_gives_no_lines() -> Result<(), String> {
    if refused_as_root()? {
        return Ok(());
    }
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).map_err(|e| format!("{e}"))?;
    let (mut place, tree, jail) = jailed_rig(dir.path())?;
    place.jail = Some(jail);
    for d in [&tree.dir, &tree.target] {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    let good =
        "lines 0123abcd..89efcdab: code 1 -> 2 (+1); test code 0 -> 0 (+0); comments 0 -> 0 (+0)\n";
    let full = format!("{good}{}", "\n".repeat(CLOCKED_OUTPUT - good.len()));
    let frozen = dir.path().join("stats-frozen.py");
    let range = |after: &str| -> Result<Option<String>, String> {
        let printed = format!("{full}{after}");
        let body = format!("import sys\nsys.stdout.write({printed:?})\nsys.exit(0)\n");
        std::fs::write(&frozen, body).map_err(|e| format!("{e}"))?;
        let limit = Duration::from_secs(60);
        Ok(theseus_core::rsi::join::count_range(
            &place,
            &tree,
            &frozen,
            "0123abcd",
            Instant::now() + limit,
            limit,
        ))
    };
    let whole = range("")?;
    check(
        whole.as_deref() == Some("code 1 -> 2 (+1); test code 0 -> 0 (+0); comments 0 -> 0 (+0)"),
        format!("a good line in output kept whole, up to the cap, was not read: {whole:?}"),
    )?;
    let cut = range("stats: one more thing\n")?;
    check(
        cut.is_none(),
        format!("a line from output cut before its end was read: {cut:?}"),
    )?;
    let log = place.gates().join(format!("stats-range-{}.log", tree.slug));
    let said = std::fs::read_to_string(&log).map_err(|e| format!("{}: {e}", log.display()))?;
    check(
        said.ends_with(&format!(
            "[the job's standard output was cut at {CLOCKED_OUTPUT} bytes]\n"
        )),
        format!(
            "the log does not say it was cut: {:?}",
            &said[said.len().saturating_sub(200)..]
        ),
    )
}
