//! What the real-binary tests share: a spawned daemon that is always killed
//! and reaped, and a config that is safe to serve in a test.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};

/// A binary of another package, beside the daemon's.
///
/// `cargo test -p theseusd` builds the daemon and its tests and not another package's program, so a
/// target directory serves whatever copy it already holds (an older build, or the image's), and a test
/// that spawns it fails in ways that say nothing about the program (the journey's timeouts of
/// 2026-11-19; `unrecognized subcommand 'owner'` from an older CLI). `stamp_of` is what that copy says it
/// was built from; this says at once, in one line, where there is none or it is another commit's than the
/// daemon's. `what` names the program as the line says it (`the helper`, `the CLI`).
pub fn beside_daemon(
    what: &str,
    package: &str,
    bin: &str,
    stamp_of: impl Fn(&Path) -> Option<String>,
) -> PathBuf {
    let path = Path::new(env!("CARGO_BIN_EXE_theseusd")).with_file_name(bin);
    assert!(
        path.is_file(),
        "{what} is not built: {bin} is missing at {}: build -p {package}",
        path.display()
    );
    let commit = env!("THESEUS_COMMIT");
    if !commit.is_empty() {
        let built = stamp_of(&path);
        assert!(
            built.as_deref() == Some(commit),
            "{what} is stale: {bin} at {} was built from {} and this tree is {commit}: build -p {package}",
            path.display(),
            built.as_deref().unwrap_or("a build that predates its stamp")
        );
    }
    path
}

/// The simulator's program (`theseus-sim`: the fake model, the fake MCP server), current.
pub fn sim_bin() -> PathBuf {
    beside_daemon("the helper", "theseus-sim", "theseus-sim", |p| {
        let out = Command::new(p).arg("stamp").output().ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    })
}

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

    /// Its stderr, for a daemon spawned with it piped.
    pub fn stderr(&mut self) -> std::process::ChildStderr {
        self.0.stderr.take().expect("a piped stderr")
    }

    /// The exit status, once it has exited (and is reaped).
    pub fn try_wait(&mut self) -> Option<ExitStatus> {
        self.0.try_wait().expect("waiting on theseusd")
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        // A daemon already reaped is not signalled again: std keeps its status.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The table at `key`, made if it is not there.
/// The model a test's GLM profiles run where GLM is only a second
/// provider: no request reaches a model of restricted origin without a pass
/// for its data (theseus-a1wvq.3).
pub const UNRESTRICTED_SECOND: &str = "claude-haiku-5-5";

/// The note's `glm` and `glm53` profiles on [`UNRESTRICTED_SECOND`], on their
/// own provider still.
pub fn unrestricted_glm(t: &mut toml::Table) {
    let profiles = table(t, "profiles");
    for name in ["glm", "glm53"] {
        table(profiles, name).insert("model".into(), UNRESTRICTED_SECOND.into());
    }
}

pub fn table<'a>(t: &'a mut toml::Table, key: &str) -> &'a mut toml::Table {
    t.entry(key)
        .or_insert_with(|| toml::Value::Table(Default::default()))
        .as_table_mut()
        .unwrap()
}

/// The last `n` lines of `s`.
pub fn tail(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// A fake `op`: every reference's value is `tv-<its item>`.
pub const FAKE_OP: &str = "#!/bin/sh\n\
    case \"$1\" in\n\
    \x20 inject) sed -E 's#\\{\\{ op://Test/([^/]+)/[^}]* \\}\\}#tv-\\1#g' ;;\n\
    \x20 read) for a; do ref=\"$a\"; done; printf 'tv-%s' \"$(echo \"$ref\" | cut -d/ -f4)\" ;;\n\
    \x20 *) exit 1 ;;\n\
    esac\n";

/// A fake `op`: every reference reads as the same test value.
pub const FAKE_OP_FLAT: &str = "#!/bin/sh\n\
    case \"$1\" in\n\
    \x20 inject) sed -e 's/{{ [^}]* }}/test-secret-value-0000/g' ;;\n\
    \x20 read) printf '%s' test-secret-value-0000 ;;\n\
    \x20 *) exit 1 ;;\n\
    esac\n";

/// The template, made safe to serve in a test: the web UI, Discord, and the
/// index tender off, no GitHub token, every secret a reference to the vault
/// `Test`, the model endpoints on a port nothing answers, and `projects` as
/// the projects dir. A tender would read the operator's model files; the
/// tender's own tests turn it on, with a stand-in.
pub fn safe_note(theseusd: &Path, projects: &Path, spend_limit_usd: f64) -> String {
    let out = Command::new(theseusd)
        .args(["example-config"])
        .output()
        .unwrap();
    let mut t: toml::Table = String::from_utf8(out.stdout).unwrap().parse().unwrap();
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
    // The agent hosts a CLI answer is read against (theseus-5rvss): the test's
    // own, so where the tests run (under an agent's shell, say) decides nothing.
    table(&mut t, "policy").insert(
        "agent_processes".into(),
        toml::Value::Array(vec!["example-agent".into()]),
    );
    table(&mut t, "discord").insert("enabled".into(), false.into());
    table(&mut t, "web").insert("enabled".into(), false.into());
    table(&mut t, "index").insert("enabled".into(), false.into());
    // The judge follows its key (theseus-is8t), and every stand-in secret
    // here resolves as far as the config can tell: off, so no test daemon
    // is ever on its way to Jev's real endpoint. A test that judges turns
    // on its own.
    table(&mut t, "judge").insert("enabled".into(), false.into());
    table(&mut t, "tools").insert("projects_dir".into(), projects.display().to_string().into());
    // Tools are not deferred in a test daemon unless its test turns them on
    // (`tests/defer.rs`): a stand-in model that calls a tool of a family the
    // place does not load would be answered with the search's words
    // (theseus-7m7w.2), and most tests are not about that.
    table(table(&mut t, "tools"), "defer").insert("enabled".into(), false.into());
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
        Self::start_with(prepare, tweak, |_| {})
    }

    /// `start`'s daemon, spawned and not waited for: `hook` on its command before it spawns (an environment
    /// variable, or a limit set in a `pre_exec`).
    pub fn spawn_with(
        prepare: impl FnOnce(&Path),
        tweak: impl FnOnce(&mut toml::Table),
        hook: impl FnOnce(&mut Command),
    ) -> Self {
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
        let mut cmd = Command::new(&theseusd);
        cmd.arg("--config")
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
            .stderr(log);
        hook(&mut cmd);
        let daemon = Daemon::spawn(&mut cmd);
        Self { dir, daemon }
    }

    /// `spawn_with`, to its first `health` answer.
    pub fn start_with(
        prepare: impl FnOnce(&Path),
        tweak: impl FnOnce(&mut toml::Table),
        hook: impl FnOnce(&mut Command),
    ) -> Self {
        let mut s = Self::spawn_with(prepare, tweak, hook);
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

    /// The port the web UI bound, for a config with `[web] port = 0`: the daemon asks the kernel for a free
    /// one and says which in its log ("web UI listening ... url=http://127.0.0.1:<port>/"). A test never picks
    /// a port itself: one picked by binding and dropping a listener is free for whoever else asks in the
    /// meantime, another test's daemon among them, and the test then talks to that one (theseus-20).
    pub fn web_port(&self) -> u16 {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            let log = self.log();
            let mut plain = String::new();
            let mut chars = log.chars();
            while let Some(c) = chars.next() {
                match c {
                    '\u{1b}' => {
                        chars.by_ref().find(|c| *c == 'm');
                    }
                    c => plain.push(c),
                }
            }
            let port = plain
                .lines()
                .find(|l| l.contains("web UI listening"))
                .and_then(|l| l.split("url=http://").nth(1))
                .and_then(|u| u.split_once('/'))
                .and_then(|(addr, _)| addr.rsplit_once(':'))
                .and_then(|(_, port)| port.parse().ok());
            if let Some(port) = port {
                return port;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "no web UI listening in 15 s:\n{log}"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
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

/// Every live descendant of `pid`, as `/proc` says: each one's pid and its `comm`. A zombie is not one.
pub fn descendants(pid: u32) -> Vec<(u32, String)> {
    let mut all = Vec::new();
    for e in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
        let Some(p) = e.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else {
            continue;
        };
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{p}/stat")) else {
            continue;
        };
        // `pid (comm) state ppid ...`: the comm can hold spaces and parentheses.
        let (Some(open), Some(close)) = (stat.find('('), stat.rfind(')')) else {
            continue;
        };
        let comm = stat[open + 1..close].to_string();
        let mut rest = stat[close + 1..].split_whitespace();
        let (state, ppid) = (rest.next(), rest.next().and_then(|v| v.parse::<u32>().ok()));
        if let (Some(state), Some(ppid)) = (state, ppid) {
            if state != "Z" && state != "X" {
                all.push((p, ppid, comm));
            }
        }
    }
    let mut out: Vec<(u32, String)> = Vec::new();
    let mut parents = vec![pid];
    while let Some(parent) = parents.pop() {
        for (p, ppid, comm) in &all {
            if *ppid == parent && !out.iter().any(|(q, _)| q == p) {
                out.push((*p, comm.clone()));
                parents.push(*p);
            }
        }
    }
    out
}

/// Whether `pid` is alive and not a zombie.
pub fn alive(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| {
        s.rfind(')')
            .and_then(|i| s[i + 1..].split_whitespace().next())
            .is_some_and(|st| st != "Z" && st != "X")
    })
}

/// Whether process `pid` holds a descriptor open on `path`.
pub fn holds_open(pid: u32, path: &Path) -> bool {
    std::fs::read_dir(format!("/proc/{pid}/fd"))
        .into_iter()
        .flatten()
        .flatten()
        .any(|e| std::fs::read_link(e.path()).is_ok_and(|l| l == path))
}

pub mod fixture;
pub use fixture::Fixture;
pub mod model;
pub mod rpc;
pub mod stdio;
pub mod terminal;
pub mod vault;

/// The CLI's program (`theseus`), this tree's: it prints its commit when `THESEUS_CLI_STAMP` is set.
pub fn cli_bin() -> PathBuf {
    beside_daemon("the CLI", "theseus", "theseus", |p| {
        let out = Command::new(p)
            .env("THESEUS_CLI_STAMP", "1")
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    })
}
