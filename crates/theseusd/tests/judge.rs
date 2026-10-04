//! The judge in a real daemon (M5 23a): `loop.v1` in shadow, with the real
//! `theseusd`, a stand-in for the Messages API, and the fake Jev.
//! - a start with the judge enabled and Jev out of reach answers health at
//!   once, and builds nothing of the judge before a judgment;
//! - a turn is judged once, after it, and recorded with its cost, which
//!   health's judge block counts; the model's request is the same bytes with
//!   the judge on and off;
//! - a `kill -9` while a judgment is out leaves today's block reserved past
//!   what was settled: the next start books that rest at its first
//!   judgment, never before.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use common::model::FakeModel;
use common::Daemon;
use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, FakeMode};

/// A fake `op`: every reference's value is `tv-<its item>`.
const FAKE_OP: &str = "#!/bin/sh\n\
    case \"$1\" in\n\
    \x20 inject) sed -E 's#\\{\\{ op://Test/([^/]+)/[^}]* \\}\\}#tv-\\1#g' ;;\n\
    \x20 read) for a; do ref=\"$a\"; done; printf 'tv-%s' \"$(echo \"$ref\" | cut -d/ -f4)\" ;;\n\
    \x20 *) exit 1 ;;\n\
    esac\n";

struct Rig {
    dir: tempfile::TempDir,
    model: FakeModel,
}

impl Rig {
    /// The template on the stand-in model, the judge on (at `jev`, or off).
    fn new(jev: Option<&str>) -> Self {
        let model = FakeModel::start(|_| vec![]);
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
        if let Some(base) = jev {
            let j = table(&mut t, "judge");
            j.insert("enabled".into(), true.into());
            j.insert("api_base".into(), base.into());
            j.insert("connect_secs".into(), 1.into());
            j.insert("total_secs".into(), 5.into());
        }
        std::fs::write(path("config.toml"), toml::to_string(&t).unwrap()).unwrap();
        Self { dir, model }
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
                self.log()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, Value> {
        call(&self.path("sock"), method, params)
    }

    fn rows(&self, kind: &str) -> Vec<Value> {
        self.call("ledger.tail", json!({"n": 500, "kind": kind}))
            .unwrap()["rows"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    fn judge(&self) -> Value {
        self.call("health", Value::Null).unwrap()["judge"].clone()
    }

    fn ask(&self, prompt: &str) -> Value {
        self.call(
            "turn.submit",
            json!({"input": prompt, "author": "test", "attachments": []}),
        )
        .unwrap()
    }

    fn stop(&self, mut d: Daemon) {
        let _ = self.call("shutdown", Value::Null);
        self.wait("the daemon's exit", || d.try_wait());
    }
}

fn call(sock: &Path, method: &str, params: Value) -> Result<Value, Value> {
    let s = UnixStream::connect(sock).map_err(|e| json!(e.to_string()))?;
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

/// Enabled, with Jev out of reach: the daemon answers health with the judge
/// idle, since nothing of it is built before a judgment.
#[test]
fn a_start_with_the_judge_on_builds_nothing_of_it() {
    let rig = Rig::new(Some("http://127.0.0.1:9"));
    let d = rig.spawn();
    let j = rig.judge();
    assert_eq!(j["enabled"], true, "{j}");
    assert_eq!(j["breaker"], "idle", "{j}");
    assert_eq!(
        j["packs"],
        json!([
            "loop.v1: shadow",
            "security.v1: shadow",
            "security.v3: shadow"
        ])
    );
    assert!(rig.rows("judge.call").is_empty());
    rig.stop(d);
}

/// One turn is judged once, after it, with its cost in health's block; the
/// model was asked the same bytes as with the judge off.
#[test]
fn a_turn_is_judged_after_it_and_asks_the_model_the_same_bytes() {
    let jev = FakeJev::start().unwrap();
    let on = Rig::new(Some(&jev.base()));
    let d = on.spawn();
    let res = on.ask("Say done.");
    assert_eq!(res["stop_reason"], "no_tool_calls", "{res}");
    let rows = on.wait("the judgment", || {
        let r = on.rows("judge.call");
        (!r.is_empty()).then_some(r)
    });
    assert_eq!(rows.len(), 1);
    let d0 = &rows[0]["data"];
    assert_eq!(d0["pack"], "loop.v1");
    assert_eq!(d0["outcome"]["outcome"], "answered", "{d0}");
    assert_eq!(rows[0]["session_id"], res["session_id"]);
    let cost = d0["cost_micros"].as_u64().unwrap();
    assert!(cost > 0);
    let j = on.wait("health's count", || {
        let j = on.judge();
        (j["calls_today"] == 1).then_some(j)
    });
    assert_eq!(
        j["spend_today_usd"].as_f64(),
        Some(cost as f64 / 1e6),
        "{j}"
    );
    assert_eq!(jev.seen().len(), 1);
    on.stop(d);

    let off = Rig::new(None);
    let d = off.spawn();
    off.ask("Say done.");
    assert_eq!(off.judge()["enabled"], false);
    off.stop(d);
    // Each rig's own temp dir is the one byte that differs, and is named.
    let bytes = |r: &Rig| {
        let raw = r.model.raw_requests();
        assert_eq!(raw.len(), 1);
        String::from_utf8_lossy(&raw[0]).replace(&r.dir.path().display().to_string(), "<dir>")
    };
    assert_eq!(
        bytes(&on),
        bytes(&off),
        "the model's request, with the judge on and off"
    );
}

/// A `kill -9` while a judgment is out: the next start reads today's block
/// (health shows what was settled) and books its rest at its first
/// judgment, with one `judge.block_booked` row, never before.
#[test]
fn a_kill_mid_block_books_the_rest_at_the_next_starts_first_judgment() {
    let jev = FakeJev::start().unwrap();
    jev.set_mode(FakeMode::Slow(Duration::from_secs(4)));
    let rig = Rig::new(Some(&jev.base()));
    let d = rig.spawn();
    rig.ask("Say done.");
    rig.wait("the call to Jev", || (jev.connections() == 1).then_some(()));
    drop(d); // SIGKILL, then reaped
    jev.set_mode(FakeMode::Up);
    let d = rig.spawn();
    let j = rig.judge();
    assert_eq!(
        j["spend_today_usd"], 0.0,
        "nothing settled before the kill: {j}"
    );
    assert!(
        rig.rows("judge.block_booked").is_empty(),
        "nothing booked before a judgment"
    );
    rig.ask("Say done again.");
    let booked = rig.wait("the booked rest", || {
        let r = rig.rows("judge.block_booked");
        (!r.is_empty()).then_some(r)
    });
    assert_eq!(booked.len(), 1);
    assert_eq!(booked[0]["data"]["booked_micros"], 10_000, "{booked:?}");
    rig.wait("the second judgment", || {
        (rig.rows("judge.call").len() == 1).then_some(())
    });
    let spent = rig.judge()["spend_today_usd"].as_f64().unwrap();
    assert!(spent > 0.01, "the block's rest, then the call: {spent}");
    rig.stop(d);
}
