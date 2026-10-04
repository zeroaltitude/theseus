//! Review 2's H1 (theseus-70f), against a real daemon: the web UI answers
//! only its own page and address. A request whose `Host` is not the UI's
//! (DNS rebinding) is refused on every route, and a WebSocket upgrade whose
//! `Origin` is not the UI's page (any other page in the operator's browser,
//! or none) is refused. Health counts each refusal; the ledger has one
//! `web.refused` row per kind at once, and the rest wait for that kind's
//! minute. Every connection here is also checked at accept for its client
//! socket's owner (theseus-3qf), on IPv4 and IPv6, and this user's pass.

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
    status_at(std::net::SocketAddr::from(([127, 0, 0, 1], port)), head)
}

fn status_at(ui: std::net::SocketAddr, head: &str) -> String {
    let mut s = TcpStream::connect_timeout(&ui, Duration::from_secs(5)).unwrap();
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

/// The UI at `ui`: its whole answer to `head`, which closes the connection, as the status line and the body.
fn fetch(ui: std::net::SocketAddr, head: &str) -> (String, String) {
    let mut s = TcpStream::connect_timeout(&ui, Duration::from_secs(5)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(head.as_bytes()).unwrap();
    let mut got = Vec::new();
    let _ = s.read_to_end(&mut got);
    let got = String::from_utf8_lossy(&got).into_owned();
    let (head, body) = got.split_once("\r\n\r\n").unwrap_or((&got, ""));
    (
        head.lines().next().unwrap_or("").to_string(),
        body.to_string(),
    )
}

/// What `/` answers (theseus-vm3n.6): the cockpit's app shell when its build is there, as the gate builds it
/// before the suite (a debug daemon reads `cockpit/dist` as it serves), and otherwise the 404 that says how to
/// build it.
fn assert_cockpit(ui: std::net::SocketAddr, host: &str, what: &str) {
    let (st, body) = fetch(ui, &get("/", host));
    let built = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("cockpit/dist/index.html")
        .exists();
    if built {
        assert!(
            st.contains(" 200 ")
                && body.contains("<title>Theseus · Cockpit</title>")
                && body.contains(r#"<div id="root">"#),
            "{what}: {st}\n{body}"
        );
    } else {
        assert!(
            st.contains(" 404 ") && body.contains("the cockpit is not built"),
            "{what}: {st}\n{body}"
        );
    }
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
    let switched = |st: &str, what: &str| assert!(st.contains(" 101 "), "{what}: {st}");
    let refused = |st: &str, what: &str| assert!(st.contains(" 403 "), "{what}: {st}");

    // The UI's own address and page: the cockpit, and the socket opens.
    assert_cockpit(ui, &own, "the app");
    assert_cockpit(ui, &local, "the app at localhost");
    let (st, _) = fetch(ui, &get("/cockpit/session/ses_x", &own));
    assert!(st.contains(" 308 "), "the cockpit's old address: {st}");
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

    // The Vite dev page with no `[web] dev_origin` (theseus-zab): through
    // its proxy (its own Host), or straight to the UI's address.
    refused(
        &status(
            port,
            &upgrade("localhost:5173", Some("http://localhost:5173")),
        ),
        "the dev page's proxy, not configured",
    );
    refused(
        &status(port, &upgrade(&own, Some("http://localhost:5173"))),
        "the dev page, not configured",
    );

    let h = s.call("health", Value::Null).unwrap();
    // This user's own client passed the socket owner's check every time
    // (theseus-3qf).
    assert_eq!(
        h["web"],
        json!({"refused_host": 4, "refused_origin": 4, "refused_peer": 0}),
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

/// theseus-zab: with `[web] dev_origin` set, the Vite dev page is served on
/// `/ws`, through its proxy (the page's own Host and Origin) or straight to
/// the UI's address. Any other page through the proxy still carries its own
/// Origin and is refused, and so are the dev Host on any other route and
/// another dev port. Each use is counted in health and ledgered as
/// `web.dev_origin`.
#[test]
fn the_dev_page_is_served_on_ws_only_while_dev_origin_is_set() {
    let port = free_port();
    let dev = "http://localhost:5173";
    let s = Served::start(
        |_| {},
        |t| {
            let w = t.get_mut("web").unwrap().as_table_mut().unwrap();
            w.insert("enabled".into(), true.into());
            w.insert("bind".into(), "127.0.0.1".into());
            w.insert("port".into(), i64::from(port).into());
            w.insert("dev_origin".into(), dev.into());
        },
    );
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
    let switched = |st: &str, what: &str| assert!(st.contains(" 101 "), "{what}: {st}");
    let refused = |st: &str, what: &str| assert!(st.contains(" 403 "), "{what}: {st}");

    switched(
        &status(port, &upgrade("localhost:5173", Some(dev))),
        "the dev page through its proxy",
    );
    switched(
        &status(port, &upgrade(&own, Some(dev))),
        "the dev page straight to the UI",
    );
    refused(
        &status(
            port,
            &upgrade("localhost:5173", Some("http://attacker.example")),
        ),
        "another page through the proxy",
    );
    refused(
        &status(port, &upgrade("localhost:5173", None)),
        "no Origin through the proxy",
    );
    refused(
        &status(port, &get("/", "localhost:5173")),
        "the dev Host on another route",
    );
    refused(
        &status(
            port,
            &upgrade("localhost:5174", Some("http://localhost:5174")),
        ),
        "another dev port",
    );

    let h = s.call("health", Value::Null).unwrap();
    assert_eq!(
        h["web"],
        json!({"refused_host": 2, "refused_origin": 2, "refused_peer": 0,
               "dev_origin": dev, "dev_origin_served": 2}),
        "{h:#}"
    );
    let rows = s
        .call("ledger.tail", json!({"n": 50, "kind": "web.dev_origin"}))
        .unwrap();
    let rows = rows["rows"].as_array().unwrap();
    // The first use at once; the second waits for the minute.
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["data"]["count"], 1);
    assert_eq!(rows[0]["data"]["last"]["origin"], dev);
    assert_eq!(rows[0]["data"]["last"]["host"], "localhost:5173");
}

/// theseus-3qf over IPv6: a UI bound to `::1` finds this user's client on
/// its row in `/proc/net/tcp6`, and serves it.
#[test]
fn the_web_ui_on_ipv6_serves_its_own_user() {
    let Ok(probe) = std::net::TcpListener::bind("[::1]:0") else {
        eprintln!("skipped: this machine has no IPv6 loopback");
        return;
    };
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    let s = Served::start(
        |_| {},
        |t| {
            let w = t.get_mut("web").unwrap().as_table_mut().unwrap();
            w.insert("enabled".into(), true.into());
            w.insert("bind".into(), "::1".into());
            w.insert("port".into(), i64::from(port).into());
        },
    );
    let ui = std::net::SocketAddr::from((std::net::Ipv6Addr::LOCALHOST, port));
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while TcpStream::connect_timeout(&ui, Duration::from_millis(250)).is_err() {
        assert!(
            std::time::Instant::now() < deadline,
            "no web UI:\n{}",
            s.log()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let own = format!("[::1]:{port}");
    assert_cockpit(ui, &own, "the app");
    let st = status_at(ui, &upgrade(&own, Some(&format!("http://{own}"))));
    assert!(st.contains(" 101 "), "its own page: {st}");
    let h = s.call("health", Value::Null).unwrap();
    assert_eq!(
        h["web"],
        json!({"refused_host": 0, "refused_origin": 0, "refused_peer": 0}),
        "{h:#}"
    );
}
