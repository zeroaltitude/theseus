//! A small harness for the sandbox's test binaries (`harness = false`),
//! which must also be the job's init and the probe inside it. It speaks
//! nextest's protocol (`--list --format terse`, then `--exact <name>`), and
//! runs every case in order under a plain `cargo test`.

#![allow(dead_code)]

use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::time::Instant;

use theseus_sandbox::{Exit, Init, SandboxChild, Spec, Stdio, INIT_ROLE};

pub struct Case {
    pub name: &'static str,
    pub run: fn() -> Result<(), String>,
}

/// The binary's entry: its init role, then `roles` (a probe), then the
/// cases. `ignored` cases run only when asked (`--ignored`, or
/// `--include-ignored`), as libtest's `#[ignore]` do.
pub fn main(cases: &[Case], ignored_cases: &[Case], roles: impl FnOnce(&[String])) -> ! {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some(INIT_ROLE) {
        theseus_sandbox::init_main();
    }
    roles(&args);
    let has = |flag: &str| args.iter().any(|a| a == flag);
    let (list, exact) = (has("--list"), has("--exact"));
    let pool: Vec<&Case> = if has("--ignored") {
        ignored_cases.iter().collect()
    } else if has("--include-ignored") {
        cases.iter().chain(ignored_cases).collect()
    } else {
        cases.iter().collect()
    };
    let mut filters: Vec<&str> = Vec::new();
    let mut value_next = false;
    for a in &args {
        if value_next {
            value_next = false;
        } else if matches!(
            a.as_str(),
            "--format" | "--test-threads" | "--color" | "--skip" | "-Z"
        ) {
            value_next = true;
        } else if !a.starts_with('-') {
            filters.push(a);
        }
    }
    let chosen: Vec<&Case> = pool
        .into_iter()
        .filter(|c| {
            filters.is_empty()
                || filters.iter().any(|f| {
                    if exact {
                        c.name == *f
                    } else {
                        c.name.contains(f)
                    }
                })
        })
        .collect();
    if list {
        for c in &chosen {
            println!("{}: test", c.name);
        }
        std::process::exit(0);
    }
    let mut failed = 0;
    for c in &chosen {
        let t = Instant::now();
        match (c.run)() {
            Ok(()) => println!("test {} ... ok ({} ms)", c.name, t.elapsed().as_millis()),
            Err(e) => {
                failed += 1;
                println!("test {} ... FAILED\n{e}", c.name);
            }
        }
    }
    println!(
        "test result: {}. {} passed; {failed} failed",
        if failed == 0 { "ok" } else { "FAILED" },
        chosen.len() - failed
    );
    std::process::exit(if failed == 0 { 0 } else { 101 })
}

/// This test binary, as the job's init.
pub fn init() -> Init {
    Init {
        exe: std::env::current_exe().expect("the test binary's path"),
        args: vec![INIT_ROLE.into()],
    }
}

/// A job's output file on the host, outside the view, as the wrapper's
/// spool file is.
pub struct Output {
    pub dir: tempfile::TempDir,
    pub path: PathBuf,
}

impl Output {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("a temp dir");
        let path = dir.path().join("out");
        Self { dir, path }
    }

    pub fn stdio(&self) -> Stdio {
        let f = std::fs::File::create(&self.path).expect("the output file");
        let err: OwnedFd = f.try_clone().expect("a second handle").into();
        Stdio {
            stdin: None,
            stdout: f.into(),
            stderr: err,
        }
    }

    pub fn read(&self) -> String {
        std::fs::read_to_string(&self.path).unwrap_or_default()
    }
}

/// Runs `spec` to its end: (how it ended, its output).
pub fn run(spec: &Spec) -> Result<(Exit, String), String> {
    let out = Output::new();
    let mut child = spawn(spec, &out)?;
    let exit = child.wait().map_err(|e| format!("wait: {e}"))?;
    Ok((exit, out.read()))
}

/// A failed spawn's message carries the job's output, where an init that
/// died before it could report wrote why.
pub fn spawn(spec: &Spec, out: &Output) -> Result<SandboxChild, String> {
    theseus_sandbox::spawn(spec, &init(), out.stdio())
        .map_err(|e| format!("spawn: {e}\nthe job's output: {}", out.read()))
}

/// The operator's HOME on the host.
pub fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/root".into()))
}

/// A job as 17b will start one: a workspace root, HOME, and the job's
/// environment of `proc_env` names and one granted name, nothing else.
pub fn job(argv: Vec<String>, workspace: &Path) -> Spec {
    let mut spec = Spec::new(argv, workspace);
    spec.workspace = vec![workspace.to_path_buf()];
    spec.home = Some(home());
    spec.env = vec![
        ("PATH".into(), "/usr/local/bin:/usr/bin:/bin".into()),
        ("HOME".into(), home().display().to_string()),
        ("LANG".into(), "C.UTF-8".into()),
        ("THESEUS_GRANTED".into(), "granted-value".into()),
    ];
    spec
}

pub fn check(ok: bool, what: impl std::fmt::Display) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(what.to_string())
    }
}
