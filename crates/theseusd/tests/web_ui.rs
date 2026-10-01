//! Review 2's H1 (theseus-70f), against a real daemon: the web UI answers
//! only its own page and address. A request whose `Host` is not the UI's
//! (DNS rebinding) is refused on every route, and a WebSocket upgrade whose
//! `Origin` is not the UI's page (any other page in the operator's browser,
//! or none) is refused. Health counts each refusal; the ledger has one
//! `web.refused` row per kind at once, and the rest wait for that kind's
//! minute.

mod common;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use common::Served;
use serde_json::{json, Value};

/// A free loopback port, for the UI to bind.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// The UI on `port`: the status line it answers `head` with.
fn status(port: u16, head: &str) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(head.as_bytes()).unwrap();
    let mut got = Vec::new();
    let mut buf = [0u8; 1024];
    while !got.windows(4).any(|w| w == b"\r\n\r\n") {
        match s.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => got.extend_from_slice(&buf[..n]),
        }
    }
    String::from_utf8_lossy(&got)
        .lines()
        .next()
        .unwrap_or("")
        .to_string()
}

fn get(path: &str, host: &str) -> String {
    format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n")
}

fn upgrade(host: &str, origin: Option<&str>) -> String {
    let origin = origin.map_or(String::new(), |o| format!("Origin: {o}\r\n"));
    format!(
        "GET /ws HTTP/1.1\r\nHost: {host}\r\n{origin}Upgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n"
    )
}

#[test]
fn the_web_ui_answers_only_its_own_page_and_address() {
    let port = free_port();
    let s = Served::start(
        |_| {},
        |t| {
            let w = t.get_mut("web").unwrap().as_table_mut().unwrap();
            w.insert("enabled".into(), true.into());
            w.insert("bind".into(), "127.0.0.1".into());
            w.insert("port".into(), i64::from(port).into());
        },
    );
    // A connect to a loopback port nothing listens on can hang here (WSL's
    // mirrored networking drops it rather than refusing), so each try is
    // bounded.
    let ui = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while TcpStream::connect_timeout(&ui, Duration::from_millis(250)).is_err() {
        assert!(
            std::time::Instant::now() < deadline,
            "no web UI:\n{}",
            s.log()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let own = format!("127.0.0.1:{port}");
    let local = format!("localhost:{port}");
    let page = format!("http://{own}");
    let ok = |st: &str, what: &str| assert!(st.contains(" 200 "), "{what}: {st}");
    let switched = |st: &str, what: &str| assert!(st.contains(" 101 "), "{what}: {st}");
    let refused = |st: &str, what: &str| assert!(st.contains(" 403 "), "{what}: {st}");

    // The UI's own address and page: served, and the socket opens.
    ok(&status(port, &get("/", &own)), "the app");
    ok(&status(port, &get("/", &local)), "the app at localhost");
    switched(&status(port, &upgrade(&own, Some(&page))), "its own page");
    switched(
        &status(port, &upgrade(&local, Some(&format!("http://{local}")))),
        "its own page at localhost",
    );

    // DNS rebinding: a page's own name for 127.0.0.1, on any route.
    let rebound = format!("rebind.attacker.example:{port}");
    refused(&status(port, &get("/", &rebound)), "a rebound name");
    refused(&status(port, &get("/assets/x.js", &rebound)), "an asset");
    refused(
        &status(port, &upgrade(&rebound, Some(&page))),
        "a rebound upgrade",
    );

    // Another page in the operator's browser, or a client with no page.
    refused(
        &status(port, &upgrade(&own, Some("http://attacker.example"))),
        "another page",
    );
    refused(
        &status(port, &upgrade(&own, Some("null"))),
        "a sandboxed page",
    );
    refused(&status(port, &upgrade(&own, None)), "no Origin");

    let h = s.call("health", Value::Null).unwrap();
    assert_eq!(
        h["web"],
        json!({"refused_host": 3, "refused_origin": 3}),
        "{h:#}"
    );
    let rows = s
        .call("ledger.tail", json!({"n": 50, "kind": "web.refused"}))
        .unwrap();
    let rows = rows["rows"].as_array().unwrap();
    let kinds: Vec<(&str, u64)> = rows
        .iter()
        .map(|r| {
            (
                r["data"]["why"].as_str().unwrap_or(""),
                r["data"]["count"].as_u64().unwrap_or(0),
            )
        })
        .collect();
    // The first of each kind at once; the rest wait for the minute.
    assert_eq!(kinds, vec![("host", 1), ("origin", 1)], "{rows:?}");
    assert_eq!(
        rows[0]["data"]["last"]["host"],
        format!("rebind.attacker.example:{port}")
    );
    assert_eq!(rows[1]["data"]["last"]["origin"], "http://attacker.example");
}
