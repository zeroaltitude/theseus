//! The keep-warm read through the real `theseusd` and a stand-in Messages API (theseus-ezeg). A profile that
//! inherits `[cache]`'s 1-hour ttl, a short `[cache] keep_warm_every_minutes`, and a session's turn: then reads at the
//! interval, each a request of one output token whose messages begin as the next turn's do, each with a
//! `keep_warm` row, and health says the profiles' TTLs and the reads. The stand-in reports no cache reads, so what
//! the provider would bill is the maintainer's live check (CLOUD_REPORT.md); here the daemon's own side is proved.

mod common;

use common::{table, Fixture, FAKE_OP_FLAT};

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use common::model::FakeModel;
use common::Daemon;
use serde_json::{json, Value};

struct Rig {
    dir: tempfile::TempDir,
    _daemon: Daemon,
    model: FakeModel,
}

impl Fixture for Rig {
    fn dir(&self) -> &std::path::Path {
        self.dir.path()
    }
}

impl Rig {
    fn start() -> Self {
        let model = FakeModel::start(|_| vec![]);
        let dir = tempfile::tempdir().unwrap();
        let path = |p: &str| dir.path().join(p);
        let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
        for d in ["bin", "projects"] {
            std::fs::create_dir_all(path(d)).unwrap();
        }
        let op = path("bin/op");
        std::fs::write(&op, FAKE_OP_FLAT).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&op, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut t: toml::Table = common::safe_note(&theseusd, &path("projects"), 100.0)
            .parse()
            .unwrap();
        let model_t = table(&mut t, "model");
        model_t.insert("api_base".into(), model.base.clone().into());
        for (_, p) in table(&mut t, "providers").iter_mut() {
            p.as_table_mut()
                .unwrap()
                .insert("api_base".into(), model.base.clone().into());
        }
        let cache = table(&mut t, "cache");
        cache.insert("ttl".into(), "1h".into());
        cache.insert("keep_warm_every_minutes".into(), 0.03.into());
        cache.insert("keep_warm_min_tokens".into(), 1.into());
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
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(log),
        );
        let r = Self {
            dir,
            _daemon: daemon,
            model,
        };
        r.wait("the secrets", || {
            r.call("health", Value::Null)
                .ok()
                .filter(|h| h["secrets"]["state"] == "ready")
        });
        r
    }

    fn wait<T>(&self, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
        let t0 = Instant::now();
        loop {
            if let Some(v) = f() {
                return v;
            }
            assert!(
                t0.elapsed() < Duration::from_secs(40),
                "no {what} in 40 s:\n{}",
                std::fs::read_to_string(self.dir.path().join("theseusd.log")).unwrap_or_default()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn submit(&self, session: &Value, input: &str) {
        self.call(
            "turn.submit",
            json!({"session_id": session, "input": input, "author": "test", "attachments": []}),
        )
        .unwrap();
    }

    fn rows(&self, kind: &str) -> Vec<Value> {
        let t = self.call("ledger.tail", json!({"n": 2000})).unwrap();
        t["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["kind"] == kind)
            .cloned()
            .collect()
    }
}

#[test]
fn a_conversation_is_read_at_the_interval_and_its_next_turn_follows_the_read() {
    let r = Rig::start();
    let s = r.call("session.open", json!({"label": "kw"})).unwrap();
    let sid = s["session_id"].clone();
    r.submit(&sid, "Say one word about the sea.");

    let rows = r.wait("two keep_warm rows", || {
        Some(r.rows("keep_warm")).filter(|v| v.len() >= 2)
    });
    assert_eq!(rows[0]["session_id"], sid, "the row is the session's");
    assert!(rows[0]["data"]["prefix_tokens"].as_u64().unwrap() > 0);

    // The read is one output token, and ends on the harness's line.
    let reads: Vec<Value> = r
        .model
        .requests()
        .into_iter()
        .filter(|q| q["max_tokens"] == 1)
        .collect();
    assert!(reads.len() >= 2, "{} reads reached the model", reads.len());
    let read = &reads[0];
    let last = read["messages"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["role"], "user");
    assert!(last.to_string().contains("cache keep-warm"), "{last}");

    // The next turn's request begins with the read's bytes, up to the line.
    r.submit(&sid, "And one about the sky.");
    let next = r
        .model
        .requests()
        .into_iter()
        .rev()
        .find(|q| q["max_tokens"] != 1 && q.to_string().contains("about the sky"))
        .expect("the next turn's request");
    let n = read["messages"].as_array().unwrap().len() - 1;
    assert_eq!(read["system"], next["system"]);
    assert_eq!(read["tools"], next["tools"]);
    assert_eq!(
        read["messages"].as_array().unwrap()[..n],
        next["messages"].as_array().unwrap()[..n]
    );

    // Health: each profile's TTL (inherited from [cache]) and the reads.
    let h = r.call("health", Value::Null).unwrap();
    let c = &h["cache"];
    assert!(c["reads_today"].as_u64().unwrap() >= 2, "{c}");
    let profiles = c["profiles"].as_array().unwrap();
    assert!(!profiles.is_empty());
    for p in profiles {
        assert_eq!(p["ttl"], "1h", "{c}");
    }
    let of = |name: &str| profiles.iter().find(|p| p["profile"] == name).unwrap();
    assert_eq!(of("fable")["inherited"], true, "{c}");
    assert_eq!(of("opus")["inherited"], true, "{c}");
}
