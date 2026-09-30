//! Durable delivery with the real `theseusd` (theseus-q4v): its Discord
//! binding talks to a stand-in for Discord's REST API
//! (`theseus_sim::fake_discord`), its gateway to a port nothing listens on,
//! and its model to a stand-in for the Messages API. Nothing reaches Discord,
//! and the bot token is a fake `op`'s `tv-discord_bot_token`.
//! - `kill -9` between the send and the settle: after the restart the post
//!   goes again with the same nonce, and the channel holds one message;
//! - a continuation at startup, with Discord away: the driver runs it at once,
//!   its reply waits in the outbox, and is posted once when Discord is back.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::model::FakeModel;
use common::Daemon;
use serde_json::{json, Value};
use theseus_sim::fake_discord::{FakeDiscord, Mode, Msg};

const USER: u64 = 159_471_966_640_799_744;
/// The fake's DM channel with `USER`.
const DM: u64 = USER + 1;

type Script = Arc<Mutex<Vec<(String, Vec<(&'static str, Value)>)>>>;

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
    script: Script,
    _model: FakeModel,
}

impl Rig {
    fn new() -> Self {
        let script = Script::default();
        let asks = script.clone();
        let model = FakeModel::start(move |prompt| {
            asks.lock()
                .unwrap()
                .iter()
                .find(|(p, _)| prompt.starts_with(p.as_str()))
                .map(|(_, calls)| calls.clone())
                .unwrap_or_default()
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
        table(&mut t, "tools").insert("proc_sync_secs".into(), 1.into());
        let discord = table(&mut t, "discord");
        discord.insert("enabled".into(), true.into());
        discord.insert("rest_proxy".into(), fake.addr.clone().into());
        discord.insert("gateway_proxy".into(), "ws://127.0.0.1:9".into());
        std::fs::write(path("config.toml"), toml::to_string(&t).unwrap()).unwrap();
        std::fs::write(
            path("state/bindings.toml"),
            format!(
                "guild_id = \"712398310421561444\"\n[[dm]]\nuser = \"{USER}\"\nname = \"eddie\"\n"
            ),
        )
        .unwrap();
        Self {
            dir,
            fake,
            script,
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

    /// The DM place's session, once the binding has bound it.
    fn session(&self) -> String {
        self.wait("the DM place bound", || {
            let h = self.call("health", Value::Null).ok()?;
            h["bindings"][0]["places"][0]["session_id"]
                .as_str()
                .map(str::to_string)
        })
    }

    fn outbox(&self) -> Value {
        self.call("health", Value::Null).unwrap()["bindings"][0]["outbox"].clone()
    }

    fn ask(&self, sid: &str, prompt: &str) -> Value {
        self.call(
            "turn.submit",
            json!({"session_id": sid, "input": prompt, "author": "test", "attachments": []}),
        )
        .unwrap()
    }

    fn ledger(&self, kind: &str) -> Vec<Value> {
        let t = self.call("ledger.tail", json!({"n": 3000})).unwrap();
        t["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["kind"] == kind)
            .cloned()
            .collect()
    }

    /// The DM's messages that are not the bind notice.
    fn replies(&self) -> Vec<Msg> {
        self.fake
            .messages(DM)
            .into_iter()
            .filter(|m| !m.content.starts_with("🔗 Theseus is bound here"))
            .collect()
    }
}

fn tail(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// `kill -9` after Discord received the create and before the settle was
/// written: the restarted daemon sends it again with the same nonce, and the
/// fake, enforcing it as Discord does, keeps one message.
#[test]
fn a_kill_between_send_and_settle_leaves_one_message() {
    let r = Rig::new();
    let daemon = r.spawn();
    let sid = r.session();
    r.wait("the bind notice", || {
        (r.fake.messages(DM).len() == 1).then_some(())
    });
    r.fake.set_mode(Mode::HangCreates);
    let res = r.ask(&sid, "say done");
    let turn = res["turn_id"].as_str().unwrap().to_string();
    r.wait("the create at Discord", || {
        r.fake
            .seen()
            .iter()
            .any(|s| s.outcome == "hung")
            .then_some(())
    });
    // SIGKILL: the create landed, and its answer and the settle died here.
    drop(daemon);
    r.fake.set_mode(Mode::Up);
    let _daemon = r.spawn();
    r.wait("the retry settled", || {
        (r.outbox()["pending"] == 0 && r.outbox()["sent"].as_u64() >= Some(2)).then_some(())
    });
    let got = r.replies();
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(
        got[0].content.starts_with("Nothing to do."),
        "{}",
        got[0].content
    );
    let nonce = theseus_discord::nonce(&format!("{turn}:L0:p0"));
    let tries: Vec<String> = r
        .fake
        .seen()
        .into_iter()
        .filter(|s| s.nonce.as_deref() == Some(nonce.as_str()))
        .map(|s| s.outcome)
        .collect();
    assert_eq!(tries, ["hung", "deduped"]);
}

/// The M3 exit test's lost answer (A3, 2026-09-26): a job outlives a
/// `kill -9`, and the continuation that reports it runs at the next start.
/// Now the driver does not wait for the binding, Discord is away when the
/// continuation ends, and its reply is posted once Discord is back.
#[test]
fn a_continuation_at_startup_delivers_its_reply_once_discord_is_back() {
    let r = Rig::new();
    r.script.lock().unwrap().push((
        "build it".into(),
        vec![(
            "proc_run",
            json!({"argv": ["sh", "-c", "sleep 3; echo built"], "timeout_secs": 30}),
        )],
    ));
    let daemon = r.spawn();
    let sid = r.session();
    r.wait("the bind notice", || {
        (r.fake.messages(DM).len() == 1).then_some(())
    });
    let first = r.ask(&sid, "build it");
    assert_eq!(first["output"], "Done.", "{first}");
    // Quiet notices and "should have asked" (2b, DD3) ride on the live tool
    // message: its notified line, and one menu.
    let tools = r.wait("the tool message", || {
        r.fake
            .messages(DM)
            .into_iter()
            .find(|m| m.content.contains("`proc.run` sh -c"))
    });
    assert!(tools.content.contains("🔔 notified"), "{}", tools.content);
    assert_eq!(tools.components, 1, "the \"Should have asked…\" menu");
    drop(daemon);
    r.fake.set_mode(Mode::Down);
    let _daemon = r.spawn();
    // The job finishes, the continuation runs and ends, and its reply waits.
    let cont = r.wait("the continuation's end", || {
        r.ledger("turn.ended")
            .into_iter()
            .find(|t| t["data"]["continuation"] == true)
    });
    assert!(r
        .ledger("driver.started")
        .iter()
        .all(|d| d["data"].get("waited_for_bindings_ms").is_none()));
    let replies_before = r.replies().len();
    r.wait("the reply pending", || {
        (r.outbox()["pending"].as_u64() >= Some(1)).then_some(())
    });
    assert_eq!(
        r.replies().len(),
        replies_before,
        "nothing while Discord is away"
    );
    r.fake.set_mode(Mode::Up);
    r.wait("the reply delivered", || {
        (r.outbox()["pending"] == 0).then_some(())
    });
    let got = r.replies();
    let continued: Vec<&Msg> = got
        .iter()
        .filter(|m| m.content.contains("continued"))
        .collect();
    assert_eq!(continued.len(), 1, "posted once: {got:?}");
    assert_eq!(
        cont["turn_id"],
        r.ledger("turn.ended").last().unwrap()["turn_id"],
        "{got:?}"
    );
    // The first turn's reply, streamed before the kill, has its final form
    // (its footer) whichever side of the kill its post was delivered on.
    let first: Vec<&Msg> = got
        .iter()
        .filter(|m| m.content.starts_with("Done."))
        .collect();
    assert_eq!(first.len(), 1, "{got:?}");
    assert!(first[0].content.contains("\n-# "), "{}", first[0].content);
}

/// The scratch rule, for every test here: the binding's REST goes to the
/// fake, which keeps no header and so never the token.
#[test]
fn the_bindings_rest_goes_to_the_fake_which_never_keeps_the_token() {
    let r = Rig::new();
    let _daemon = r.spawn();
    r.session();
    r.wait("the bind notice", || {
        (r.fake.messages(DM).len() == 1).then_some(())
    });
    let seen = r.fake.seen();
    assert!(seen.iter().any(|s| s.path == "/users/@me"), "{seen:?}");
    let text = serde_json::to_string(&seen).unwrap();
    assert!(!text.contains("tv-discord_bot_token"));
    assert!(!text.to_lowercase().contains("authorization"));
}
