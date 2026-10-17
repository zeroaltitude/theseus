//! Threads on the work board against a real daemon (theseus-5i8i): two threads,
//! each with a task that runs and a question that waits, as `work.watch`
//! stamps and groups them, and as the real `theseus tasks` draws them: a group
//! each with its question and its task under it, `--flat` as before, a paused
//! thread out of the needs-you count, and a surface that reads no private text
//! told of none.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};

use common::model::FakeModel;
use common::Daemon;
use serde_json::{json, Value};

/// A fake `op`: every reference's value is `tv-<its item>`.
const FAKE_OP: &str = "#!/bin/sh\n\
    case \"$1\" in\n\
    \x20 inject) sed -E 's#\\{\\{ op://Test/([^/]+)/[^}]* \\}\\}#tv-\\1#g' ;;\n\
    \x20 read) for a; do ref=\"$a\"; done; printf 'tv-%s' \"$(echo \"$ref\" | cut -d/ -f4)\" ;;\n\
    \x20 *) exit 1 ;;\n\
    esac\n";

/// What the stand-in model calls for a prompt that contains a phrase.
fn script(prompt: &str) -> Vec<(&'static str, Value)> {
    if prompt.contains("Run the slow job.") {
        return vec![("proc_run", json!({"argv": ["sleep", "30"]}))];
    }
    if prompt.starts_with("[Task ") && prompt.contains("Count to three.") {
        return vec![];
    }
    let calls: [(&str, Vec<(&'static str, Value)>); 5] = [
        (
            "hold a long job",
            vec![("proc_run", json!({"argv": ["sleep", "30"]}))],
        ),
        (
            "write the note",
            vec![("fs_write", json!({"path": "note.txt", "content": "kept\n"}))],
        ),
        (
            "start a small task",
            vec![(
                "task_create",
                json!({"brief": "Count to three.", "arrangement":
                       {"pieces": [{"quote": "Please start a small task.", "role": "objective"}]}}),
            )],
        ),
        (
            "set a timer",
            vec![("wake_at", json!({"after": "1s", "note": "tick-tock"}))],
        ),
        (
            "start a slow task",
            vec![(
                "task_create",
                json!({"brief": "Run the slow job.", "arrangement":
                       {"pieces": [{"quote": "Please start a slow task.", "role": "objective"}]}}),
            )],
        ),
    ];
    calls
        .into_iter()
        .find(|(p, _)| prompt.contains(p))
        .map(|(_, c)| c)
        .unwrap_or_default()
}

struct Rig {
    dir: tempfile::TempDir,
    _model: FakeModel,
}

impl Rig {
    fn new() -> Self {
        let model = FakeModel::start(script);
        let dir = tempfile::tempdir().unwrap();
        let path = |p: &str| dir.path().join(p);
        let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
        for d in ["bin", "projects", "state"] {
            std::fs::create_dir_all(path(d)).unwrap();
        }
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(path("bin/op"), FAKE_OP).unwrap();
        std::fs::set_permissions(path("bin/op"), std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut t: toml::Table = common::safe_note(&theseusd, &path("projects"), 100.0)
            .parse()
            .unwrap();
        fn table<'a>(t: &'a mut toml::Table, key: &str) -> &'a mut toml::Table {
            t.entry(key)
                .or_insert_with(|| toml::Value::Table(Default::default()))
                .as_table_mut()
                .unwrap()
        }
        table(&mut t, "model").insert("api_base".into(), model.base.clone().into());
        for (_, p) in table(&mut t, "providers").iter_mut() {
            p.as_table_mut()
                .unwrap()
                .insert("api_base".into(), model.base.clone().into());
        }
        table(&mut t, "policy").insert("enforcement".into(), "open".into());
        table(&mut t, "policy")
            .entry("tools")
            .or_insert_with(|| toml::Value::Table(Default::default()))
            .as_table_mut()
            .unwrap()
            .insert("fs.write".into(), "approve".into());
        table(&mut t, "tools").insert("proc_sync_secs".into(), 1.into());
        std::fs::write(path("config.toml"), toml::to_string(&t).unwrap()).unwrap();
        Self { dir, _model: model }
    }

    fn path(&self, p: &str) -> PathBuf {
        self.dir.path().join(p)
    }

    fn spawn(&self) -> Daemon {
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path("theseusd.log"))
            .unwrap();
        let d = Daemon::spawn(
            std::process::Command::new(env!("CARGO_BIN_EXE_theseusd"))
                .arg("--config")
                .arg(self.path("config.toml"))
                .arg("--state-dir")
                .arg(self.path("state"))
                .arg("--socket")
                .arg(self.path("sock"))
                .env(
                    "PATH",
                    format!(
                        "{}:{}",
                        self.path("bin").display(),
                        std::env::var("PATH").unwrap_or_default()
                    ),
                )
                .env("OP_SERVICE_ACCOUNT_TOKEN", "test-not-a-token")
                .env_remove("THESEUS_OP_TOKEN_FILE")
                .env_remove("THESEUS_CONFIG")
                .env_remove("THESEUS_STATE_DIR")
                .env_remove("THESEUS_SOCKET")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(log),
        );
        self.wait("the socket", || self.call("health", Value::Null).ok());
        d
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.path("theseusd.log")).unwrap_or_default()
    }

    fn wait<T>(&self, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
        let t0 = Instant::now();
        loop {
            if let Some(v) = f() {
                return v;
            }
            assert!(
                t0.elapsed() < Duration::from_secs(40),
                "no {what} in 40 s; the daemon's log ends:\n{}",
                tail(&self.log(), 30)
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, Value> {
        let s = UnixStream::connect(self.path("sock")).map_err(|e| json!(e.to_string()))?;
        s.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
        let req = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
        (&s).write_all(format!("{req}\n").as_bytes())
            .map_err(|e| json!(e.to_string()))?;
        for line in BufReader::new(&s).lines() {
            let v: Value = serde_json::from_str(&line.map_err(|e| json!(e.to_string()))?)
                .map_err(|e| json!(e.to_string()))?;
            if v["id"] == 1 {
                return match v.get("error") {
                    Some(e) if !e.is_null() => Err(e.clone()),
                    _ => Ok(v["result"].clone()),
                };
            }
        }
        Err(json!("the connection closed"))
    }

    /// The real `theseus` binary beside the daemon's, on this daemon.
    fn cli(&self, args: &[&str]) -> Option<std::process::Output> {
        let bin = PathBuf::from(env!("CARGO_BIN_EXE_theseusd")).with_file_name("theseus");
        bin.exists().then(|| {
            std::process::Command::new(bin)
                .arg("--socket")
                .arg(self.path("sock"))
                .args(args)
                .env_remove("THESEUS_SESSION")
                // A wide screen: `tasks` gives the activity column up first when the width is short.
                .env("COLUMNS", "200")
                .output()
                .expect("running theseus")
        })
    }
}

fn tail(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

fn thread(r: &Rig, slug: &str, title: &str) {
    r.call(
        "thread.new",
        json!({"slug": slug, "title": title, "author": "test"}),
    )
    .unwrap();
}

fn ask_in(r: &Rig, slug: &str, prompt: &str) -> Value {
    r.call(
        "turn.submit",
        json!({"thread": slug, "input": prompt, "author": "test", "attachments": []}),
    )
    .unwrap()
}

fn work_text(r: &Rig, args: &[&str]) -> String {
    let mut a = vec!["tasks"];
    a.extend_from_slice(args);
    let out = r.cli(&a).expect("theseus beside theseusd");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Two threads each with a running task and a waiting question.
#[test]
fn the_board_groups_two_threads_and_the_cli_draws_them() {
    let r = Rig::new();
    let _d = r.spawn();
    thread(&r, "tides", "Tides survey");
    thread(&r, "moons", "Moon charts");
    let mut sessions = Vec::new();
    for slug in ["tides", "moons"] {
        let first = ask_in(&r, slug, "Please start a slow task.");
        sessions.push(first["session_id"].as_str().unwrap().to_string());
        let asked = ask_in(&r, slug, "Now write the note.");
        assert!(asked["awaiting_confirm"].is_string(), "{asked}");
    }
    let snap = r.wait("both threads, their tasks and their questions", || {
        let s = r
            .call("work.watch", json!({"limit": 0, "interactive": false}))
            .ok()?;
        let views = s["views"].as_array()?;
        let of = |slug: &str, kind: &str| {
            views
                .iter()
                .filter(|v| v["thread"] == slug && v["kind"] == kind)
                .count()
        };
        (s["threads"].as_array()?.len() == 2
            && ["tides", "moons"].iter().all(|t| {
                of(t, "conversation") == 1 && of(t, "task") >= 1 && of(t, "question") == 1
            }))
        .then_some(s)
    });
    // Every view of a thread's tree carries it; nothing else does.
    for v in snap["views"].as_array().unwrap() {
        let root_session = snap["views"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == v["root"])
            .and_then(|c| c["session_id"].as_str());
        let want = match root_session {
            Some(s) if s == sessions[0] => json!("tides"),
            Some(s) if s == sessions[1] => json!("moons"),
            _ => Value::Null,
        };
        assert_eq!(v.get("thread").cloned().unwrap_or(Value::Null), want, "{v}");
    }
    let groups = snap["threads"].as_array().unwrap();
    let tides = groups.iter().find(|g| g["slug"] == "tides").unwrap();
    assert_eq!(tides["title"], "Tides survey");
    assert!(
        tides["today_usd"].is_number(),
        "the stats are read: {tides}"
    );

    // The CLI: a group each, its question and its task under it; two need you.
    let text = work_text(&r, &[]);
    eprintln!("theseus tasks:\n{text}");
    assert!(text.starts_with("Threads: 2 live, 2 need you"), "{text}");
    for title in ["Tides survey", "Moon charts"] {
        assert!(text.contains(title), "{text}");
    }
    assert_eq!(text.matches("confirm fs.write").count(), 2, "{text}");
    let flat = work_text(&r, &["--flat"]);
    assert!(
        !flat.contains("Threads:") && !flat.contains("Tides survey"),
        "{flat}"
    );

    // A paused thread shows paused and leaves the count.
    r.call("thread.pause", json!({"slug": "moons", "author": "test"}))
        .unwrap();
    r.wait("moons paused on the board", || {
        let s = r.call("work.watch", json!({"limit": 0})).ok()?;
        s["threads"]
            .as_array()?
            .iter()
            .any(|g| g["slug"] == "moons" && g["state"] == "paused")
            .then_some(())
    });
    let text = work_text(&r, &[]);
    assert!(text.starts_with("Threads: 2 live, 1 need you"), "{text}");
    let moons = text.lines().find(|l| l.contains("Moon charts")).unwrap();
    assert!(
        moons.contains("paused") && moons.contains("1 need you"),
        "{moons}"
    );
}
