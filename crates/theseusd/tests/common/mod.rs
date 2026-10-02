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

/// The template, made safe to serve in a test: the web UI, Discord, and the
/// index tender off, no GitHub token, every secret a reference to the vault
/// `Test`, the model endpoints on a port nothing answers, and `projects` as
/// the projects dir. A tender would read the operator's model files; the
/// tender's own tests turn it on, with a stand-in.
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
    table(&mut t, "index").insert("enabled".into(), false.into());
    table(&mut t, "tools").insert("projects_dir".into(), projects.display().to_string().into());
    table(&mut t, "kernel").insert("spend_limit_usd".into(), spend_limit_usd.into());
    toml::to_string(&t).unwrap()
}

/// A socket daemon on `safe_note` in a temp dir, with a stand-in `op` that
/// answers nothing, so health answers and nothing reaches a model, Discord,
/// or the vault. Its state dir is `state`, its socket `sock`, its log
/// `theseusd.log`. Killed and reaped when dropped, as `Daemon` is.
pub struct Served {
    pub dir: tempfile::TempDir,
    pub daemon: Daemon,
}

impl Served {
    /// `prepare` runs on the temp dir first (to lay out an old state dir),
    /// and `tweak` edits the config.
    pub fn start(prepare: impl FnOnce(&Path), tweak: impl FnOnce(&mut toml::Table)) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let theseusd = std::path::PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
        let dir = tempfile::tempdir().unwrap();
        let path = |p: &str| dir.path().join(p);
        prepare(dir.path());
        std::fs::create_dir_all(path("bin")).unwrap();
        std::fs::create_dir_all(path("projects")).unwrap();
        std::fs::write(path("bin/op"), "#!/bin/sh\nexit 1\n").unwrap();
        std::fs::set_permissions(path("bin/op"), std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut t: toml::Table = safe_note(&theseusd, &path("projects"), 100.0)
            .parse()
            .unwrap();
        tweak(&mut t);
        std::fs::write(path("config.toml"), toml::to_string(&t).unwrap()).unwrap();
        let log = std::fs::File::create(path("theseusd.log")).unwrap();
        let daemon = Daemon::spawn(
            Command::new(&theseusd)
                .arg("--config")
                .arg(path("config.toml"))
                .arg("--state-dir")
                .arg(path("state"))
                .arg("--socket")
                .arg(path("sock"))
                .env(
                    "PATH",
                    format!(
                        "{}:{}",
                        path("bin").display(),
                        std::env::var("PATH").unwrap_or_default()
                    ),
                )
                .env("OP_SERVICE_ACCOUNT_TOKEN", "test-not-a-token")
                .env_remove("THESEUS_OP_TOKEN_FILE")
                .env_remove("THESEUS_CONFIG")
                .env_remove("THESEUS_STATE_DIR")
                .env_remove("THESEUS_SOCKET")
                .env_remove("THESEUS_OPERATOR_UMASK")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(log),
        );
        let mut s = Self { dir, daemon };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while s.call("health", serde_json::Value::Null).is_err() {
            if let Some(status) = s.daemon.try_wait() {
                panic!("theseusd exited ({status}):\n{}", s.log());
            }
            assert!(
                std::time::Instant::now() < deadline,
                "no health in 15 s:\n{}",
                s.log()
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        s
    }

    pub fn path(&self, p: &str) -> std::path::PathBuf {
        self.dir.path().join(p)
    }

    pub fn log(&self) -> String {
        std::fs::read_to_string(self.path("theseusd.log")).unwrap_or_default()
    }

    /// One JSON-RPC call on the socket: its result, or its error as text.
    pub fn call(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        use std::io::{BufRead, Write};
        let s = std::os::unix::net::UnixStream::connect(self.path("sock"))
            .map_err(|e| e.to_string())?;
        s.set_read_timeout(Some(std::time::Duration::from_secs(30)))
            .unwrap();
        let req =
            serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
        (&s).write_all(format!("{req}\n").as_bytes())
            .map_err(|e| e.to_string())?;
        for line in std::io::BufReader::new(&s).lines() {
            let v: serde_json::Value = serde_json::from_str(&line.map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            if v["id"] == 1 {
                return match v.get("error") {
                    Some(e) if !e.is_null() => Err(e.to_string()),
                    _ => Ok(v["result"].clone()),
                };
            }
        }
        Err("the connection closed".into())
    }
}

pub mod model;
