//! `http.request` (issue 65): one request means one, and an earlier answer is excused only for
//! the very same selectors. A peer that speaks HTTP/2 and refuses each stream sees the key once;
//! a title that is another item's id, a vault's name that is another vault's id, and another port
//! each make the next call ask, with nothing read.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::tests_request::{call, last, rig_req, rig_tool, serve, Req};
use crate::config::WebToolsConfig;
use crate::content::OutsideMode;
use crate::node::ResultStatus;
use crate::secrets::look::tests::logged;

/// A peer that speaks HTTP/2 from the first byte and refuses every stream it is sent
/// (`RST_STREAM`, `REFUSED_STREAM`), the case an HTTP client may send again. It counts the
/// requests (`HEADERS` frames) it was sent: each carried the key.
struct Refuser {
    port: u16,
    heads: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Refuser {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// One frame: its length, type, flags and stream, and its payload.
fn frame(kind: u8, flags: u8, stream: u32, payload: &[u8]) -> Vec<u8> {
    let n = payload.len() as u32;
    let mut out = vec![(n >> 16) as u8, (n >> 8) as u8, n as u8, kind, flags];
    out.extend_from_slice(&stream.to_be_bytes());
    out.extend_from_slice(payload);
    out
}

async fn refuser() -> Refuser {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let heads = Arc::new(AtomicUsize::new(0));
    let seen = heads.clone();
    let task = tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            let seen = seen.clone();
            tokio::spawn(async move {
                let mut preface = [0u8; 24];
                if sock.read_exact(&mut preface).await.is_err() {
                    return;
                }
                // SETTINGS, none set.
                let _ = sock.write_all(&frame(4, 0, 0, &[])).await;
                loop {
                    let mut h = [0u8; 9];
                    if sock.read_exact(&mut h).await.is_err() {
                        return;
                    }
                    let n =
                        (usize::from(h[0]) << 16) | (usize::from(h[1]) << 8) | usize::from(h[2]);
                    let stream = u32::from_be_bytes([h[5] & 0x7f, h[6], h[7], h[8]]);
                    let mut payload = vec![0u8; n];
                    if sock.read_exact(&mut payload).await.is_err() {
                        return;
                    }
                    let reply = match (h[3], h[4] & 1) {
                        // The client's SETTINGS: acknowledged.
                        (4, 0) => frame(4, 1, 0, &[]),
                        // A request: refused (error code 7, REFUSED_STREAM).
                        (1, _) => {
                            seen.fetch_add(1, Ordering::SeqCst);
                            frame(3, 0, stream, &7u32.to_be_bytes())
                        }
                        _ => continue,
                    };
                    if sock.write_all(&reply).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    Refuser { port, heads, task }
}

/// A peer that refuses the stream that carried the key is not sent it again: one request, and the
/// answer says whether it acted is not known.
#[tokio::test]
async fn a_refused_stream_is_not_sent_again() {
    let peer = refuser().await;
    let web = crate::web::tests::web(peer.port, WebToolsConfig::default(), true);
    let request = web.request_h2_plaintext();
    let o = rig_tool(
        serve().await,
        web,
        request,
        OutsideMode::default(),
        true,
        |_| {},
    );
    let sid = o.r.session();
    let post = call(peer.port, "POST", "/v1/things", json!({"json": {"n": 1}}));
    o.r.steps(&sid, vec![("http_request", post)]).await;
    let (status, text) = last(&o.r, &sid, "p1");
    assert_eq!(
        peer.heads.load(Ordering::SeqCst),
        1,
        "the key went out once: {text}"
    );
    assert_eq!(status, ResultStatus::Unknown, "{text}");
    assert!(text.contains("is not known"), "{text}");
}

/// The vault program in `dir` (the rig's stub, whose path the daemon's reader holds) replaced by
/// one whose names collide with ids: in `Harbor`, two items are titled with the third's id; the
/// vault `Dock`'s id is another vault's name. Each read value is the same made-up one.
fn colliding(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let item = |id: &str, title: &str| {
        format!(
            r#"{{\"id\":\"{id}\",\"title\":\"{title}\",\"vault\":{{\"id\":\"$6\",\"name\":\"x\"}},\"category\":\"API_CREDENTIAL\"}}"#
        )
    };
    let items = [
        item("itemid00000000000000000002", "itemid00000000000000000009"),
        item("itemid00000000000000000003", "itemid00000000000000000009"),
        item("itemid00000000000000000009", "Beacon"),
        item("itemid00000000000000000001", "Lantern key"),
    ]
    .join(",");
    let script = format!(
        r#"#!/bin/sh
printf '%s\n' "$*" >> '{d}/op.argv'
case "$1 $2" in
  "whoami --format") echo '{{"url":"https://harbor.example.test/path","account_uuid":"ACCT-UUID-1","user_uuid":"USER-UUID-1","user_type":"SERVICE_ACCOUNT"}}' ;;
  "vault list") echo '[{{"id":"vaultid00000000000000000001","name":"vaultid00000000000000000009"}},{{"id":"vaultid00000000000000000009","name":"Dock"}},{{"id":"vaultid00000000000000000003","name":"Harbor"}}]' ;;
  "item list") echo "[{items}]" ;;
  "item get") echo '{{"id":"itemid00000000000000000001","fields":[{{"id":"credential","type":"CONCEALED","label":"credential","value":"INVENTED-FIELD-VALUE-9"}}]}}' ;;
  "read --no-newline") printf 'INVENTED-FIELD-VALUE-9' ;;
  *) echo "[ERROR] unexpected: $*" >&2; exit 1 ;;
esac
"#,
        d = dir.display()
    );
    let bin = dir.join("op");
    std::fs::write(&bin, script).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// How many values the vault was asked for.
fn reads(dir: &Path) -> usize {
    logged(dir).iter().filter(|l| l.starts_with("read")).count()
}

/// A call to `site.test:port` with the item `item` of the vault `vault`.
fn keyed(port: u16, vault: &str, item: &str) -> serde_json::Value {
    let mut c = call(port, "GET", "/v1/things", json!({}));
    c["key"]["vault"] = json!(vault);
    c["key"]["item"] = json!(item);
    c
}

/// Two calls in one turn, the first's answer being outside text; how many asks and reads followed.
async fn two(first: serde_json::Value, second: serde_json::Value) -> (usize, usize, Req) {
    let o = rig_req().await;
    colliding(&o.vault);
    let sid = o.r.session();
    o.r.steps(
        &sid,
        vec![("http_request", first), ("http_request", second)],
    )
    .await;
    let (status, text) = last(&o.r, &sid, "p1");
    assert_eq!(status, ResultStatus::Ok, "the first call ran: {text}");
    let (asks, read) = (o.r.asked().len(), reads(&o.vault));
    (asks, read, o)
}

/// The earlier answer read the item titled with Y's id (by X's own id); the next call names Y by
/// its id: another item, so it asks, with nothing read for it.
#[tokio::test]
async fn a_title_that_is_another_items_id_asks() {
    let s = serve().await;
    let p = s.port;
    let (asks, read, o) = two(
        keyed(p, "Harbor", "itemid00000000000000000002"),
        keyed(p, "Harbor", "itemid00000000000000000009"),
    )
    .await;
    assert_eq!(asks, 1, "{:?}", o.r.asked());
    assert_eq!(read, 1, "only the first call read the vault");
    // The very same selectors, at the same host: no card.
    let (asks, read, o) = two(
        keyed(p, "Harbor", "itemid00000000000000000002"),
        keyed(p, "Harbor", "itemid00000000000000000002"),
    )
    .await;
    assert_eq!(asks, 0, "{:?}", o.r.asked());
    assert_eq!(read, 2);
}

/// The earlier answer read a vault whose name is another vault's id; the next call names that
/// other vault by its id: it asks, with nothing read for it.
#[tokio::test]
async fn a_vaults_name_that_is_another_vaults_id_asks() {
    let s = serve().await;
    let p = s.port;
    let (asks, read, o) = two(
        keyed(p, "vaultid00000000000000000001", "Lantern key"),
        keyed(p, "vaultid00000000000000000009", "Lantern key"),
    )
    .await;
    assert_eq!(asks, 1, "{:?}", o.r.asked());
    assert_eq!(read, 1, "only the first call read the vault");
}

/// The same selectors at the same host but another port: another service, so it asks.
#[tokio::test]
async fn another_port_asks() {
    let (one, other) = (serve().await, serve().await);
    let (asks, read, o) = two(
        keyed(one.port, "Harbor", "Lantern key"),
        keyed(other.port, "Harbor", "Lantern key"),
    )
    .await;
    assert_eq!(asks, 1, "{:?}", o.r.asked());
    assert_eq!(read, 1);
}
