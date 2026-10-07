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

const USER: u64 = 271_828_182_845_904_523;
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
        Self::with(|_| {})
    }

    /// The rig, with `tweak` run on its config last.
    fn with(tweak: impl FnOnce(&mut toml::Table)) -> Self {
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
        tweak(&mut t);
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
            script,
            _model: model,
        }
    }

    fn path(&self, p: &str) -> PathBuf {
        self.dir.path().join(p)
    }

    fn spawn(&self) -> Daemon {
        self.spawn_bin(std::path::Path::new(env!("CARGO_BIN_EXE_theseusd")))
    }

    fn spawn_bin(&self, theseusd: &std::path::Path) -> Daemon {
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path("theseusd.log"))
            .unwrap();
        let d = Daemon::spawn(
            std::process::Command::new(theseusd)
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

/// The outcomes the fake gave each create of one message, by its nonce.
fn tries(r: &Rig, nonce: &str) -> Vec<String> {
    r.fake
        .seen()
        .into_iter()
        .filter(|s| s.nonce.as_deref() == Some(nonce))
        .map(|s| s.outcome)
        .collect()
}

/// The outbox's settle rows for posts that wrote the message `key`.
fn settles_of(r: &Rig, key: &str) -> Vec<Value> {
    r.ledger("action.succeeded")
        .into_iter()
        .filter(|row| !row["data"]["outbox"].is_null())
        .filter(|row| {
            row["data"]["detail"]["messages"]
                .as_array()
                .is_some_and(|m| m.iter().any(|m| m["key"] == key))
        })
        .collect()
}

/// The reply's footer, which only its post writes: the stream's text never
/// has it.
const FOOTER: &str = "\n-# ";

/// A turn's reply, its post's write held at the fake: returns its message's
/// key and the creates under its nonce so far (the stream's, if it made it).
fn reply_in_flight(r: &Rig) -> (String, Vec<String>) {
    let sid = r.session();
    r.wait("the bind notice", || {
        (r.fake.messages(DM).len() == 1).then_some(())
    });
    r.fake.hold_writes_containing(Some(FOOTER));
    let res = r.ask(&sid, "say done");
    let key = format!("{}:L0:p0", res["turn_id"].as_str().unwrap());
    r.wait("the post's write at Discord", || {
        r.fake
            .seen()
            .iter()
            .any(|s| s.outcome == "held")
            .then_some(())
    });
    let before = tries(r, &theseus_discord::nonce(&key));
    (key, before)
}

/// A clean stop while a turn's reply post is being written (theseus-pfv):
/// the stop waits for the write's answer, the post settles before the
/// process exits, and the next start sends nothing again. Before, the
/// process ended with the write unanswered, and the next start sent the post
/// again. The grace here is long, so that a loaded test machine lands the
/// answer inside it; the default's timing is the report's.
#[test]
fn a_clean_stop_lets_the_reply_in_flight_settle_before_it_exits() {
    let r = Rig::with(|t| {
        t.entry("server")
            .or_insert_with(|| toml::Value::Table(Default::default()))
            .as_table_mut()
            .unwrap()
            .insert("stop_grace_ms".into(), 5000.into());
    });
    let mut daemon = r.spawn();
    let (key, before) = reply_in_flight(&r);
    // The stop, while the post's write waits for its answer.
    let t0 = Instant::now();
    r.call("shutdown", Value::Null).unwrap();
    r.wait("the socket gone", || {
        (!r.path("sock").exists()).then_some(())
    });
    assert!(
        daemon.try_wait().is_none(),
        "the daemon exited with its post in flight"
    );
    r.fake.hold_writes_containing(None);
    let status = r.wait("the stop", || daemon.try_wait());
    assert!(status.success(), "{status}");
    eprintln!(
        "the stop, the answer released once the socket was gone: {} ms",
        t0.elapsed().as_millis()
    );
    let stopped = r.log();
    // The next start: the settle is in the store, so nothing goes again.
    let _daemon = r.spawn();
    r.wait("the start's bind notice", || {
        let o = r.outbox();
        (o["pending"] == 0 && o["sent"].as_u64() >= Some(1)).then_some(())
    });
    assert_eq!(
        tries(&r, &theseus_discord::nonce(&key)),
        before,
        "sent again after the stop"
    );
    assert_eq!(settles_of(&r, &key).len(), 1);
    let got = r.replies();
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(got[0].content.contains(FOOTER), "{}", got[0].content);
    assert!(
        stopped.contains("stopping: the posts in flight settled"),
        "{}",
        tail(&stopped, 20)
    );
}

/// A post whose answer does not come holds a clean stop no longer than its
/// grace, the default (theseus-pfv): the post stays dispatched, as after a
/// crash, and the next start sends it again under the same nonce, so the
/// place still holds one message.
#[test]
fn a_clean_stop_waits_for_a_post_in_flight_no_longer_than_its_grace() {
    let r = Rig::new();
    let mut daemon = r.spawn();
    let (key, before) = reply_in_flight(&r);
    let t0 = Instant::now();
    r.call("shutdown", Value::Null).unwrap();
    let status = r.wait("the stop", || daemon.try_wait());
    let stop_ms = t0.elapsed().as_millis();
    assert!(status.success(), "{status}");
    eprintln!("the stop, the post never answered: {stop_ms} ms (the grace is 50 ms)");
    assert!(stop_ms < 1000, "the stop waited {stop_ms} ms");
    assert!(
        r.log().contains("stay dispatched"),
        "{}",
        tail(&r.log(), 20)
    );
    r.fake.hold_writes_containing(None);
    let _daemon = r.spawn();
    r.wait("the retry settled", || {
        let o = r.outbox();
        (o["pending"] == 0 && o["sent"].as_u64() >= Some(2)).then_some(())
    });
    let mut again = before;
    again.push("deduped".into());
    assert_eq!(tries(&r, &theseus_discord::nonce(&key)), again);
    let got = r.replies();
    assert_eq!(got.len(), 1, "{got:?}");
}

/// The stop's timing with a post in flight (theseus-pfv), for the report,
/// not the gate: `cargo nextest run -p theseusd --test outbox --run-ignored
/// only --no-capture -E 'test(stop_timing)'`, on any build's `theseusd`
/// (`THESEUS_STOP_BIN`, default this one's; `THESEUS_STOP_RUNS`, default 10).
/// From the `shutdown` request to the process's exit: with no post in
/// flight; with the post's answer let go as soon as the request is sent;
/// and with the answer never given during the stop.
#[test]
#[ignore]
fn stop_timing_with_a_post_in_flight() {
    let bin = std::env::var("THESEUS_STOP_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_BIN_EXE_theseusd")));
    let runs: usize = std::env::var("THESEUS_STOP_RUNS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(10);
    // A build before theseus-pfv refuses the new key; this one defaults it.
    let r = Rig::with(|t| {
        t["server"].as_table_mut().unwrap().remove("stop_grace_ms");
    });
    let mut times: Vec<(&str, Vec<f64>)> =
        vec![("idle", vec![]), ("released", vec![]), ("never", vec![])];
    for run in 0..runs * 3 {
        let case = run % 3;
        let mut daemon = r.spawn_bin(&bin);
        r.session();
        // The start's own posts first: the bind notice, and any post the
        // last stop left dispatched.
        r.wait("the outbox idle", || {
            let o = r.outbox();
            (o["pending"] == 0 && o["sent"].as_u64() >= Some(1)).then_some(())
        });
        if case > 0 {
            reply_in_flight_again(&r);
        }
        let s = UnixStream::connect(r.path("sock")).unwrap();
        let req = json!({"jsonrpc": "2.0", "id": 1, "method": "shutdown", "params": null});
        let t0 = Instant::now();
        let sent = theseus_protocol::now_unix_ms();
        (&s).write_all(format!("{req}\n").as_bytes()).unwrap();
        if case == 1 {
            r.fake.hold_writes_containing(None);
        }
        let status = loop {
            if let Some(st) = daemon.try_wait() {
                break st;
            }
            assert!(t0.elapsed() < Duration::from_secs(10), "no exit");
            std::thread::sleep(Duration::from_micros(500));
        };
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        times[case].1.push(ms);
        assert!(status.success(), "{status}");
        r.fake.hold_writes_containing(None);
        if ms > 150.0 {
            eprintln!(
                "  run {run} ({}) took {ms:.1} ms, from unix ms {sent}; its log ends:\n{}",
                times[case].0,
                tail(&r.log(), 12)
            );
        }
    }
    eprintln!("stop timing, {} runs each, {}", runs, bin.display());
    for (case, mut t) in times {
        t.sort_by(f64::total_cmp);
        let at = |p: f64| t[((p * t.len() as f64).ceil() as usize).clamp(1, t.len()) - 1];
        eprintln!(
            "  {case:<9} p50 {:6.1} ms  p95 {:6.1} ms  max {:6.1} ms",
            at(0.5),
            at(0.95),
            t[t.len() - 1]
        );
    }
}

/// `reply_in_flight` on a rig whose bind notice is long since posted.
fn reply_in_flight_again(r: &Rig) {
    let sid = r.session();
    let held_before = r.fake.seen().iter().filter(|s| s.outcome == "held").count();
    r.fake.hold_writes_containing(Some(FOOTER));
    r.ask(&sid, "say done");
    r.wait("the post's write at Discord", || {
        (r.fake.seen().iter().filter(|s| s.outcome == "held").count() > held_before).then_some(())
    });
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
