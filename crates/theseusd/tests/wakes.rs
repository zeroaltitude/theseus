//! Wakes with the real `theseusd` (DD8, theseus-cff): its Discord binding
//! talks to a stand-in for Discord's REST API (`theseus_sim::fake_discord`),
//! its gateway to a port nothing listens on, and its model to a stand-in for
//! the Messages API. Nothing reaches Discord.
//! - a wake set two seconds ahead runs its turn about then, and the turn's
//!   reply posts once, through the outbox, under the wake's line; a cancel
//!   clears another, and names the surface that cancelled it;
//! - a `kill -9` before the due time, and the daemon down across it: the
//!   wake runs after the restart, marked late, once;
//! - a repeating wake at the floor (`[kernel] min_repeat_minutes = 1`) runs
//!   twice on the daemon's clock, a minute apart, each turn's reply under its
//!   occurrence's line, with its rows (37a).

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::model::FakeModel;
use common::Daemon;
use serde_json::{json, Value};
use theseus_sim::fake_discord::{FakeDiscord, Msg};

const USER: u64 = 271_828_182_845_904_523;
/// The fake's DM channel with `USER`.
const DM: u64 = USER + 1;

/// A fake `op`: every reference's value is `tv-<its item>`.
const FAKE_OP: &str = "#!/bin/sh\n\
    case \"$1\" in\n\
    \x20 inject) sed -E 's#\\{\\{ op://Test/([^/]+)/[^}]* \\}\\}#tv-\\1#g' ;;\n\
    \x20 read) for a; do ref=\"$a\"; done; printf 'tv-%s' \"$(echo \"$ref\" | cut -d/ -f4)\" ;;\n\
    \x20 *) exit 1 ;;\n\
    esac\n";

struct Rig {
    dir: tempfile::TempDir,
    fake: Arc<FakeDiscord>,
    _model: FakeModel,
}

impl Rig {
    /// The stand-in model sets a wake two seconds ahead for `Wake me soon`,
    /// an hour ahead for `Wake me later`, and one every minute from two
    /// seconds ahead for `Wake me every minute`; any other call gets text.
    fn new() -> Self {
        let model = FakeModel::start(|prompt| {
            if prompt.contains("Wake me soon") {
                vec![("wake_at", json!({"after": "2s", "note": "check the build"}))]
            } else if prompt.contains("Wake me later") {
                vec![("wake_at", json!({"after": "1h", "note": "much later"}))]
            } else if prompt.contains("Wake me every minute") {
                vec![(
                    "wake_at",
                    json!({"after": "2s", "every": "1m", "note": "write me a haiku"}),
                )]
            } else {
                vec![]
            }
        });
        let fake = FakeDiscord::start();
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
        table(&mut t, "policy").insert("enforcement".into(), "notify".into());
        // The floor a live check uses (37a): a series every minute.
        table(&mut t, "kernel").insert("min_repeat_minutes".into(), 1.into());
        let discord = table(&mut t, "discord");
        discord.insert("enabled".into(), true.into());
        discord.insert("rest_proxy".into(), fake.addr.clone().into());
        discord.insert("gateway_proxy".into(), "ws://127.0.0.1:9".into());
        std::fs::write(path("config.toml"), toml::to_string(&t).unwrap()).unwrap();
        std::fs::write(
            path("state/bindings.toml"),
            format!(
                "guild_id = \"314159265358979323\"\n[[dm]]\nuser = \"{USER}\"\nname = \"zeroaltitude\"\n"
            ),
        )
        .unwrap();
        Self {
            dir,
            fake,
            _model: model,
        }
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

    fn wait<T>(&self, what: &str, f: impl FnMut() -> Option<T>) -> T {
        self.wait_for(what, 40, f)
    }

    fn wait_for<T>(&self, what: &str, secs: u64, mut f: impl FnMut() -> Option<T>) -> T {
        let t0 = Instant::now();
        loop {
            if let Some(v) = f() {
                return v;
            }
            assert!(
                t0.elapsed() < Duration::from_secs(secs),
                "no {what} in {secs} s; the daemon's log ends:\n{}",
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

    /// The DM place's session, once the binding has bound it.
    fn session(&self) -> String {
        self.wait("the DM place bound", || {
            let h = self.call("health", Value::Null).ok()?;
            h["bindings"][0]["places"][0]["session_id"]
                .as_str()
                .map(str::to_string)
        })
    }

    fn ask(&self, sid: &str, prompt: &str) -> Value {
        self.call(
            "turn.submit",
            json!({"session_id": sid, "input": prompt, "author": "test", "attachments": []}),
        )
        .unwrap()
    }

    fn wakes(&self) -> Vec<Value> {
        self.call("wake.list", json!({})).unwrap()["wakes"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    /// The DM's messages that a wake's turn posted, as the fake keeps them.
    fn woken(&self) -> Vec<Msg> {
        self.fake
            .messages(DM)
            .into_iter()
            .filter(|m| m.content.starts_with("-# ⏰ wake ("))
            .collect()
    }

    fn rows(&self, kind: &str) -> Vec<Value> {
        self.call("ledger.tail", json!({"n": 200, "kind": kind}))
            .unwrap()["rows"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }
}

fn tail(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// A wake two seconds ahead runs its turn about then, not before, and the
/// turn's reply reaches the place once, through the outbox, under the wake's
/// line. A cancel of a second clears it, and its row names the CLI.
#[test]
fn a_wakes_turn_posts_through_the_outbox_once_and_a_cancel_clears_another() {
    let r = Rig::new();
    let _daemon = r.spawn();
    let sid = r.session();
    let t0 = Instant::now();
    let first = r.ask(&sid, "Wake me soon");
    assert_eq!(first["output"], "Done.", "{first}");
    let w = r.wait("the wake listed", || r.wakes().into_iter().next());
    assert_eq!(w["note"], "check the build");
    assert_eq!(w["session_id"], sid.as_str());
    assert_eq!(w["target"], format!("discord:dm:{USER}"));
    let health = r.call("health", Value::Null).unwrap();
    assert_eq!(
        health["wakes"][0]["wake_id"], w["wake_id"],
        "health lists it"
    );
    let got = r.wait("the wake's reply at the fake", || {
        let got = r.woken();
        (!got.is_empty()).then_some(got)
    });
    assert!(
        t0.elapsed() >= Duration::from_millis(1_900),
        "not before its time"
    );
    assert!(
        got[0]
            .content
            .contains(": check the build\nNothing to do.\n-# "),
        "{}",
        got[0].content
    );
    std::thread::sleep(Duration::from_millis(700));
    assert_eq!(r.woken().len(), 1, "once: {:?}", r.woken());
    assert!(r.wakes().is_empty());
    let fired = r.rows("wake.fired");
    assert_eq!(fired.len(), 1, "{fired:?}");
    assert_eq!(fired[0]["data"]["while_down"], false);

    r.ask(&sid, "Wake me later");
    let later = r.wait("the second wake listed", || r.wakes().into_iter().next());
    let res = r
        .call("wake.cancel", json!({"wake": later["short"]}))
        .unwrap();
    assert_eq!(res["wake"]["note"], "much later");
    assert!(r.wakes().is_empty());
    let cancelled = r.rows("wake.cancelled");
    assert_eq!(cancelled[0]["data"]["by"], "the CLI", "{cancelled:?}");
    let again = r
        .call("wake.cancel", json!({"wake": later["short"]}))
        .unwrap_err();
    assert_eq!(again["code"], -32002, "{again}");
}

/// A `kill -9` before the due time, and the daemon down across it: the
/// restarted daemon runs the wake, marked late with the reason, and its
/// reply posts once.
#[test]
fn a_wake_due_while_the_daemon_is_down_runs_after_the_restart_marked_late_once() {
    let r = Rig::new();
    let daemon = r.spawn();
    let sid = r.session();
    r.ask(&sid, "Wake me soon");
    assert_eq!(r.wakes().len(), 1);
    // SIGKILL within the two seconds, and down for nine: the wake runs over
    // 5 s late, where its line says so.
    drop(daemon);
    std::thread::sleep(Duration::from_secs(9));
    assert!(r.woken().is_empty(), "nothing ran while it was down");
    let _daemon = r.spawn();
    let got = r.wait("the late wake's reply at the fake", || {
        let got = r.woken();
        (!got.is_empty()).then_some(got)
    });
    assert!(
        got[0]
            .content
            .contains(" late: the daemon was not running then): check the build\n"),
        "{}",
        got[0].content
    );
    std::thread::sleep(Duration::from_millis(700));
    assert_eq!(r.woken().len(), 1, "once: {:?}", r.woken());
    let fired = r.rows("wake.fired");
    assert_eq!(fired.len(), 1, "{fired:?}");
    assert_eq!(fired[0]["data"]["while_down"], true);
    assert!(fired[0]["data"]["late_ms"].as_u64().unwrap() >= 5_000);
    assert_eq!(sid, r.session(), "the same place and session");
}

/// A repeating wake at the floor, every minute from two seconds ahead (37a):
/// its first occurrence runs about then, and its second a minute after, on
/// the daemon's own clock. Each turn's reply posts once, under its
/// occurrence's line; each `wake.fired` row says which occurrence ran, that
/// none was passed over, and when the next is due; and `wake.list` shows the
/// series, with its third occurrence next. A cancel ends it.
#[test]
fn a_repeating_wake_at_the_floor_runs_twice_a_minute_apart() {
    let r = Rig::new();
    let _daemon = r.spawn();
    let sid = r.session();
    let t0 = Instant::now();
    let set = r.ask(&sid, "Wake me every minute");
    assert_eq!(set["output"], "Done.", "{set}");
    let w = r.wait("the series listed", || r.wakes().into_iter().next());
    assert_eq!(
        (w["every"].as_str(), w["occurrence"].as_u64()),
        (Some("1m"), Some(1))
    );
    let first_due = w["due_at_ms"].as_u64().unwrap();
    let first = r.wait("the first occurrence's reply", || {
        let got = r.woken();
        (!got.is_empty()).then_some(got)
    });
    assert!(
        t0.elapsed() >= Duration::from_millis(1_900),
        "not before its time"
    );
    assert!(
        first[0]
            .content
            .starts_with("-# ⏰ wake (every 1m, #1): write me a haiku\n"),
        "{}",
        first[0].content
    );
    let w = r.wait("the series put back", || {
        r.wakes()
            .into_iter()
            .next()
            .filter(|w| w["occurrence"] == 2)
    });
    assert_eq!(w["due_at_ms"].as_u64(), Some(first_due + 60_000));
    assert!(w["next"].as_str().is_some_and(|n| n.contains(':')), "{w}");
    // The second, a minute after the first: the floor is a minute.
    let both = r.wait_for("the second occurrence's reply", 75, || {
        let got = r.woken();
        (got.len() >= 2).then_some(got)
    });
    assert!(
        t0.elapsed() >= Duration::from_millis(61_900),
        "a minute apart"
    );
    assert!(
        both[1]
            .content
            .starts_with("-# ⏰ wake (every 1m, #2): write me a haiku\n"),
        "{}",
        both[1].content
    );
    let fired = r.wait("both rows", || {
        let f = r.rows("wake.fired");
        (f.len() >= 2).then_some(f)
    });
    let mut occ: Vec<(u64, u64, u64)> = fired
        .iter()
        .map(|f| {
            (
                f["data"]["occurrence"].as_u64().unwrap(),
                f["data"]["missed"].as_u64().unwrap(),
                f["data"]["next_due_at_ms"].as_u64().unwrap(),
            )
        })
        .collect();
    occ.sort();
    assert_eq!(
        occ,
        vec![(1, 0, first_due + 60_000), (2, 0, first_due + 120_000)]
    );
    assert!(fired.iter().all(|f| f["data"]["every"] == "1m"));
    let w = r.wakes();
    assert_eq!(w.len(), 1, "one wake: the series");
    assert_eq!(w[0]["occurrence"], 3);
    std::thread::sleep(Duration::from_millis(700));
    assert_eq!(r.woken().len(), 2, "each once: {:?}", r.woken());
    r.call("wake.cancel", json!({"wake": w[0]["short"]}))
        .unwrap();
    assert!(r.wakes().is_empty(), "a cancel ends the series");
}
