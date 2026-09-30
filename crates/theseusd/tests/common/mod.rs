//! What the real-binary tests share: a spawned daemon that is always killed
//! and reaped, and a config that is safe to serve in a test.

#![allow(dead_code)]

use std::path::Path;
use std::process::{Child, Command, ExitStatus};

/// A spawned `theseusd`, killed and reaped when dropped, so a test that
/// fails before its explicit stop never leaves a daemon running (theseus-hee:
/// one did, for 45 minutes, on 2026-09-29). A daemon that restarted itself
/// by `exec` keeps its pid, so this kills that one too.
pub struct Daemon(Child);

impl Daemon {
    pub fn spawn(cmd: &mut Command) -> Self {
        Self(cmd.spawn().expect("spawning theseusd"))
    }

    pub fn id(&self) -> u32 {
        self.0.id()
    }

    /// Its protocol pipes, for a `--stdio` daemon spawned with both piped.
    pub fn stdio(&mut self) -> (std::process::ChildStdin, std::process::ChildStdout) {
        (
            self.0.stdin.take().expect("a piped stdin"),
            self.0.stdout.take().expect("a piped stdout"),
        )
    }

    /// The exit status, once it has exited (and is reaped).
    pub fn try_wait(&mut self) -> Option<ExitStatus> {
        self.0.try_wait().expect("waiting on theseusd")
    }
}

/// The Theseus job this test process runs under, if any: a job wrapper
/// among its ancestors (the gate run from a task, DD7). An approval from such
/// a process is refused (theseus-6qy), so a test's operator's part skips.
pub fn job_above_this_test() -> Option<String> {
    let mut p = std::process::id();
    while p > 1 {
        if let Some(job) = theseus_kernel::job::wrapper_job(p) {
            return Some(job);
        }
        let s = std::fs::read_to_string(format!("/proc/{p}/stat")).ok()?;
        p = s[s.rfind(')')? + 2..].split(' ').nth(1)?.parse().ok()?;
    }
    None
}

impl Drop for Daemon {
    fn drop(&mut self) {
        // A daemon already reaped is not signalled again: std keeps its status.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The template, made safe to serve in a test: the web UI and Discord off, no
/// GitHub token, every secret a reference to the vault `Test`, the model
/// endpoints on a port nothing answers, and `projects` as the projects dir.
pub fn safe_note(theseusd: &Path, projects: &Path, spend_limit_usd: f64) -> String {
    let out = Command::new(theseusd)
        .arg("example-config")
        .output()
        .unwrap();
    let mut t: toml::Table = String::from_utf8(out.stdout).unwrap().parse().unwrap();
    fn table<'a>(t: &'a mut toml::Table, key: &str) -> &'a mut toml::Table {
        t.entry(key)
            .or_insert_with(|| toml::Value::Table(Default::default()))
            .as_table_mut()
            .unwrap()
    }
    table(&mut t, "model").insert("api_base".into(), "http://127.0.0.1:9".into());
    for (_, p) in table(&mut t, "providers").iter_mut() {
        p.as_table_mut()
            .unwrap()
            .insert("api_base".into(), "http://127.0.0.1:9".into());
    }
    let secrets = table(&mut t, "secrets");
    let names: Vec<String> = secrets
        .keys()
        .filter(|k| k.as_str() != "github_token")
        .cloned()
        .collect();
    secrets.clear();
    for n in names {
        secrets.insert(n.clone(), format!("op://Test/{n}/credential").into());
    }
    table(&mut t, "discord").insert("enabled".into(), false.into());
    table(&mut t, "web").insert("enabled".into(), false.into());
    table(&mut t, "tools").insert("projects_dir".into(), projects.display().to_string().into());
    table(&mut t, "kernel").insert("spend_limit_usd".into(), spend_limit_usd.into());
    toml::to_string(&t).unwrap()
}

pub mod model;
