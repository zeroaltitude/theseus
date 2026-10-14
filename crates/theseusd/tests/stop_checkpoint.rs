//! A clean stop writes the checkpoint the next start reads (theseus-8ub95):
//! a daemon with the services a real config turns on (the MCP server with its
//! memory tools, the index tender, the ledger chain with its pin) stops, and
//! the next start replays nothing. The stop's checkpoint is made durable by
//! itself, not by redb's close: anything that still holds the store at exit
//! (a thread, a task, a connection) keeps the close from ever happening.

mod common;

use std::time::{Duration, Instant};

use common::Served;
use serde_json::Value;

const KEY: &str = "stop-checkpoint-key-0000";

fn table<'a>(t: &'a mut toml::Table, key: &str) -> &'a mut toml::Table {
    t.entry(key)
        .or_insert_with(|| toml::Value::Table(Default::default()))
        .as_table_mut()
        .unwrap()
}

/// The services the owner's config turns on, but for Discord and voice.
fn services_on(t: &mut toml::Table) {
    table(t, "secrets").insert("mcp_server_key".into(), "env:STOP_CP_KEY".into());
    let m = table(t, "mcp_server");
    m.insert("enabled".into(), true.into());
    m.insert("port".into(), 0.into());
    m.insert("memory".into(), true.into());
    let i = table(t, "index");
    i.insert("enabled".into(), true.into());
    i.insert("weights_dir".into(), "/nonexistent-weights".into());
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

fn wait_exit(s: &mut Served) {
    let t0 = Instant::now();
    while s.daemon.try_wait().is_none() {
        assert!(
            t0.elapsed() < Duration::from_secs(30),
            "no stop:\n{}",
            s.log()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_clean_stop_with_every_service_on_leaves_nothing_to_replay() {
    stop_and_reopen(false);
}

/// A handle on the store that is never dropped keeps redb from closing; the
/// stop's checkpoint is durable by itself, so the next start replays nothing,
/// and the log names the leak.
#[test]
fn a_handle_that_outlives_the_runtime_costs_the_next_start_no_replay() {
    stop_and_reopen(true);
}

fn stop_and_reopen(leak: bool) {
    let hook = move |c: &mut std::process::Command| {
        c.env("STOP_CP_KEY", KEY);
        if leak {
            c.env("THESEUS_TEST_LEAK_STORE", "1");
        }
    };
    let mut s = Served::start_with(|_| {}, services_on, hook);
    // The index tender starts 2 s after serving, and the pin runs after it.
    let t0 = Instant::now();
    loop {
        let h = s.call("health", Value::Null).unwrap();
        if h["index"]["state"] == "ready" {
            break;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(30),
            "{}\n{}",
            h["index"],
            s.log()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    s.call("shutdown", Value::Null).unwrap();
    wait_exit(&mut s);
    // The store as the stop left it, started again.
    let state = s.path("state/store");
    let second = Served::start_with(
        |d| {
            let to = d.join("state/store");
            std::fs::create_dir_all(&to).unwrap();
            let ok = std::process::Command::new("cp")
                .arg("-a")
                .arg(format!("{}/.", state.display()))
                .arg(&to)
                .status()
                .unwrap();
            assert!(ok.success());
        },
        services_on,
        hook,
    );
    if leak {
        let t0 = Instant::now();
        while !s.log().contains("outlived the runtime") {
            assert!(
                t0.elapsed() < Duration::from_secs(5),
                "no leak named:\n{}",
                s.log()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    let p = store_phase(&second);
    // (redb repairs a file it was never closed on, which is its own cost.)
    assert_eq!(
        p["replayed_into_index"],
        0,
        "a clean stop left records to replay: {p}\n{}",
        s.log()
    );
    if !leak {
        assert_eq!(
            p["index_repaired"],
            false,
            "the store did not close: {p}\n{}",
            s.log()
        );
    }
}
