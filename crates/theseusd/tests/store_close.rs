//! A clean stop closes the store (theseus-7okz9): a daemon with every service
//! on, a turn, a watcher that stays connected, and a batch of reads stops, and
//! no handle on the store outlives the runtime, so the next start finds the
//! index closed and repairs nothing (a repair of a big index took 8 s).

mod common;

use common::table;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use common::model::FakeModel;
use common::Served;
use serde_json::{json, Value};

const KEY: &str = "store-close-key-0000";

/// A free loopback port for the web UI: a fixed one collides with the
/// other tests' daemons and with whatever holds it on the host.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn services_on(base: String, web: u16) -> impl FnOnce(&mut toml::Table) {
    move |t| {
        table(t, "secrets").insert("mcp_server_key".into(), "env:STORE_CLOSE_KEY".into());
        let m = table(t, "mcp_server");
        m.insert("enabled".into(), true.into());
        m.insert("port".into(), 0.into());
        m.insert("memory".into(), true.into());
        let i = table(t, "index");
        i.insert("enabled".into(), true.into());
        i.insert("weights_dir".into(), "/nonexistent-weights".into());
        let j = table(t, "judge");
        j.insert("enabled".into(), true.into());
        j.insert("api_base".into(), base.clone().into());
        j.insert("learning_hour".into(), 0.into());
        table(t, "secrets").insert("jev_api_key".into(), "env:STORE_CLOSE_KEY".into());
        let te = table(t, "telemetry");
        te.insert("otlp_endpoint".into(), base.clone().into());
        te.insert("metrics_interval_secs".into(), 1.into());
        let w = table(t, "web");
        w.insert("enabled".into(), true.into());
        w.insert("port".into(), i64::from(web).into());
        table(t, "model").insert("api_base".into(), base.clone().into());
        for (_, p) in table(t, "providers").iter_mut() {
            p.as_table_mut()
                .unwrap()
                .insert("api_base".into(), base.clone().into());
        }
    }
}

/// The stop systemd sends (SIGTERM), or the `shutdown` method.
fn stop(s: &Served, sigterm: bool) {
    if sigterm {
        // SAFETY: a signal to the daemon this test started.
        unsafe { libc::kill(s.daemon.id() as i32, libc::SIGTERM) };
    } else {
        s.call("shutdown", Value::Null).unwrap();
    }
}

fn store_phase(s: &Served) -> Value {
    let h = s.call("health", Value::Null).unwrap();
    h["startup"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "store")
        .map(|p| p["detail"].clone())
        .unwrap()
}

/// A connection that stays open through the stop, watching the work board.
fn watcher(s: &Served) -> UnixStream {
    let c = UnixStream::connect(s.path("sock")).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let req = json!({"jsonrpc": "2.0", "id": 1, "method": "work.watch", "params": {}});
    (&c).write_all(format!("{req}\n").as_bytes()).unwrap();
    let mut line = String::new();
    BufReader::new(&c).read_line(&mut line).unwrap();
    assert!(line.contains("\"id\":1"), "{line}");
    c
}

#[test]
fn a_shutdown_with_work_on_every_surface_closes_the_store() {
    every_surface(false);
}

/// A signal's stop reaches the MCP server by no wake (theseus-7okz9): the core
/// kept a closure that held the server, which held the core, and the store
/// never closed (install #29: 9 handles, a repair of 584 MB at the next start).
#[test]
fn a_sigterm_with_work_on_every_surface_closes_the_store() {
    every_surface(true);
}

/// A turn, then the reads and writes of every surface the daemon has.
fn use_everything(s: &Served) {
    let ses = s.call("session.open", json!({"label": "close"})).unwrap();
    let _ = s.call(
        "turn.submit",
        json!({"session_id": ses["session_id"], "input": "hello there",
               "author": "test", "attachments": []}),
    );
    let file = s.path("watched.txt");
    std::fs::write(&file, "one").unwrap();
    let reads: [(&str, Value); 22] = [
        ("session.list", Value::Null),
        ("session.history", json!({"session_id": ses["session_id"]})),
        ("execution.list", Value::Null),
        ("ledger.tail", json!({"limit": 5})),
        ("ledger.verify", json!({})),
        ("ontology.list", Value::Null),
        (
            "context.census",
            json!({"session_id": ses["session_id"], "exact": true}),
        ),
        ("routine.list", Value::Null),
        (
            "routine.create",
            json!({"name": "r", "brief": "do it", "schedule": "every 1d"}),
        ),
        ("watch.list", Value::Null),
        (
            "watch.create",
            json!({"kind": "file", "subject": {"path": file}, "note": "w"}),
        ),
        ("wake.create", json!({"when": "1d", "note": "later"})),
        ("wake.list", Value::Null),
        ("thread.list", Value::Null),
        ("thread.new", json!({"title": "t", "slug": "t"})),
        ("memory.search", json!({"query": "hello"})),
        ("memory.recalls", json!({})),
        ("judge.list", json!({})),
        ("import.sessions", json!({})),
        ("books.list", Value::Null),
        ("task.list", Value::Null),
        ("work.watch", json!({})),
    ];
    for (m, p) in reads {
        let r = s.call(m, p);
        eprintln!("{m}: {}", r.map(|v| v.to_string().len()).unwrap_or(0));
    }
}

fn every_surface(sigterm: bool) {
    let model = FakeModel::start(|_| vec![]);
    let hook = |c: &mut std::process::Command| {
        c.env("STORE_CLOSE_KEY", KEY);
    };
    let web = free_port();
    let mut s = Served::start_with(|_| {}, services_on(model.base.clone(), web), hook);
    let t0 = Instant::now();
    while s.call("health", Value::Null).unwrap()["index"]["state"] != "ready" {
        assert!(t0.elapsed() < Duration::from_secs(30), "{}", s.log());
        std::thread::sleep(Duration::from_millis(50));
    }
    let _watching = watcher(&s);
    {
        // A connect to a loopback port nothing listens on can hang (WSL's
        // mirrored networking drops it rather than refusing): each try is
        // bounded, as web_ui.rs's.
        let ui = std::net::SocketAddr::from(([127, 0, 0, 1], web));
        let t0 = Instant::now();
        let mut c = loop {
            match std::net::TcpStream::connect_timeout(&ui, Duration::from_millis(250)) {
                Ok(c) => break c,
                Err(e) => assert!(
                    t0.elapsed() < Duration::from_secs(15),
                    "no web UI: {e}\n{}",
                    s.log()
                ),
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        c.write_all(
            format!("GET / HTTP/1.1\r\nHost: 127.0.0.1:{web}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .unwrap();
        let mut body = String::new();
        let _ = std::io::Read::read_to_string(&mut c, &mut body);
        eprintln!("web: {}", body.lines().next().unwrap_or(""));
    }
    use_everything(&s);
    stop(&s, sigterm);
    let t0 = Instant::now();
    while s.daemon.try_wait().is_none() {
        assert!(
            t0.elapsed() < Duration::from_secs(30),
            "no stop:\n{}",
            s.log()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let log = s.log();
    assert!(
        !log.contains("outlived the runtime"),
        "a handle outlived the runtime:\n{log}"
    );
    let state = s.path("state/store");
    let second = Served::start_with(
        |d| {
            let to = d.join("state/store");
            std::fs::create_dir_all(&to).unwrap();
            assert!(std::process::Command::new("cp")
                .arg("-a")
                .arg(format!("{}/.", state.display()))
                .arg(&to)
                .status()
                .unwrap()
                .success());
        },
        services_on(model.base, web),
        hook,
    );
    let p = store_phase(&second);
    assert_eq!(
        p["index_repaired"], false,
        "the store did not close: {p}\n{log}"
    );
    assert_eq!(p["replayed_into_index"], 0, "{p}");
}

/// A stop soon after a start, at several delays, with every service on: the
/// startup work (the index, the history check, the pin, the warm-ups) is in
/// flight when the stop comes.
#[test]
fn a_shutdown_soon_after_a_start_closes_the_store_too() {
    stop_soon(false);
}

#[test]
fn a_sigterm_soon_after_a_start_closes_the_store_too() {
    stop_soon(true);
}

fn stop_soon(sigterm: bool) {
    let model = FakeModel::start(|_| vec![]);
    let hook = |c: &mut std::process::Command| {
        c.env("STORE_CLOSE_KEY", KEY);
    };
    // A first run lays down a store with a session in it.
    let first = Served::start_with(|_| {}, services_on(model.base.clone(), free_port()), hook);
    let ses = first
        .call("session.open", json!({"label": "close"}))
        .unwrap();
    let _ = first.call(
        "turn.submit",
        json!({"session_id": ses["session_id"], "input": "hello there",
               "author": "test", "attachments": []}),
    );
    first.call("shutdown", Value::Null).unwrap();
    let mut first = first;
    while first.daemon.try_wait().is_none() {
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut state = first.path("state/store");
    let mut keep = vec![first];
    for ms in [0u64, 50, 300, 1000, 2500, 4000] {
        let from = state.clone();
        let mut s = Served::start_with(
            |d| {
                let to = d.join("state/store");
                std::fs::create_dir_all(&to).unwrap();
                assert!(std::process::Command::new("cp")
                    .arg("-a")
                    .arg(format!("{}/.", from.display()))
                    .arg(&to)
                    .status()
                    .unwrap()
                    .success());
            },
            services_on(model.base.clone(), free_port()),
            hook,
        );
        std::thread::sleep(Duration::from_millis(ms));
        stop(&s, sigterm);
        let t0 = Instant::now();
        while s.daemon.try_wait().is_none() {
            assert!(
                t0.elapsed() < Duration::from_secs(30),
                "no stop:\n{}",
                s.log()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let log = s.log();
        assert!(
            !log.contains("outlived the runtime"),
            "after {ms} ms, a handle outlived the runtime:\n{log}"
        );
        state = s.path("state/store");
        keep.push(s);
    }
}
