//! The localhost web UI (spec §3.14, first form). Serves the built Vite/React
//! app embedded in the binary and bridges a WebSocket to the protocol: each
//! text frame is one JSON-RPC line, so the browser is just another client
//! with no privileged path into the kernel. Each connection keeps its two
//! addresses, so that a judged act can find the process that holds the
//! client's end (theseus-6qy).
//!
//! Loopback only, and only for its own page (theseus-70f). A browser lets
//! any page it shows open a WebSocket to this port, and a page whose site
//! resolves to 127.0.0.1 can reach every route (DNS rebinding). So every
//! request's `Host` must be the UI's own address (its bind address or
//! `localhost`, at its port), and a WebSocket upgrade's `Origin` must be the
//! UI's own page (`http://` and one of those). A browser sets both, and a
//! page cannot change them. Refusals are counted in health and ledgered
//! (`web.refused`, `Core::web_refused`), never narrated.
//!
//! There is no per-start token. It would have to reach the page from this
//! same server, so it would reach exactly the clients that already pass
//! both checks: the UI's own page, and any local process, which can send
//! whatever `Host` and `Origin` it likes. A boundary against another local
//! user needs something the port does not hand out, such as the connecting
//! socket's owner (theseus-3qf).

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::{
    body::Body,
    extract::{
        connect_info::Connected,
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
        ConnectInfo, Path, Request, State,
    },
    http::{header, HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use rust_embed::Embed;
use serde_json::json;
use theseus_core::approval::{Client, Peer, Surface};
use theseus_core::webui::{clip, Why};
use theseus_core::Core;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Embed)]
#[folder = "web/dist/"]
struct Assets;

pub async fn serve(core: Arc<Core>, bind: &str, port: u16) -> Result<()> {
    // `Config::validate` refuses a bind that is not a loopback address
    // (theseus-2fo); this is the same check, where the socket is made.
    let ip: IpAddr = bind
        .parse()
        .with_context(|| format!("bad web bind {bind:?}"))?;
    let addr = SocketAddr::new(ip, port);
    if !addr.ip().is_loopback() {
        anyhow::bail!("web.bind must be a loopback address (reachability rule); got {bind}");
    }
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("binding web UI on {addr}"))?;
    // The port the socket got: the config's, or the one the kernel picked
    // for port 0.
    let addr = listener.local_addr().unwrap_or(addr);
    let ui = Arc::new(Ui {
        core: core.clone(),
        own: Own {
            ip: addr.ip(),
            port: addr.port(),
        },
    });
    let app = Router::new()
        .route("/", get(index))
        .route("/ws", get(ws_upgrade))
        .route("/{*path}", get(asset))
        .layer(middleware::from_fn_with_state(ui.clone(), own_host))
        .with_state(ui);
    tracing::info!(url = %format!("http://{addr}/"), "web UI listening (loopback only)");
    let shutdown = async move { core.shutdown.notified().await };
    axum::serve(listener, app.into_make_service_with_connect_info::<Ends>())
        .with_graceful_shutdown(shutdown)
        .await?;
    Ok(())
}

/// What every route shares: the core, and the UI's own address.
struct Ui {
    core: Arc<Core>,
    own: Own,
}

/// The UI's own address (theseus-70f): the loopback address it listens on,
/// or `localhost`, at its port.
#[derive(Clone, Copy, Debug)]
struct Own {
    ip: IpAddr,
    port: u16,
}

impl Own {
    /// A `Host` (or an authority) that names the UI.
    fn host(&self, host: &str) -> bool {
        let Some((name, port)) = split_host(host) else {
            return false;
        };
        port == self.port
            && (name.eq_ignore_ascii_case("localhost")
                || name.parse::<IpAddr>().is_ok_and(|ip| ip == self.ip))
    }

    /// An `Origin` that is the UI's own page.
    fn origin(&self, origin: &str) -> bool {
        origin.strip_prefix("http://").is_some_and(|h| self.host(h))
    }
}

/// `name:port`, `[v6]:port`, or a bare name, whose port is http's 80.
fn split_host(host: &str) -> Option<(&str, u16)> {
    if let Some(rest) = host.strip_prefix('[') {
        let (name, after) = rest.split_once(']')?;
        let port = match after {
            "" => 80,
            p => p.strip_prefix(':')?.parse().ok()?,
        };
        return Some((name, port));
    }
    match host.rsplit_once(':') {
        Some((name, port)) => Some((name, port.parse().ok()?)),
        None => Some((host, 80)),
    }
}

/// Every route: a request whose `Host` does not name the UI is refused
/// before it reaches one (DNS rebinding). An absolute-form target's
/// authority must name it too.
async fn own_host(
    State(ui): State<Arc<Ui>>,
    ConnectInfo(ends): ConnectInfo<Ends>,
    req: Request,
    next: Next,
) -> Response {
    let host = req.headers().get(header::HOST);
    let authority = req.uri().authority();
    let named = host.is_some() || authority.is_some();
    let own = host.is_none_or(|h| h.to_str().is_ok_and(|h| ui.own.host(h)))
        && authority.is_none_or(|a| ui.own.host(a.as_str()));
    if named && own {
        return next.run(req).await;
    }
    ui.core.web_refused(
        Why::Host,
        json!({
            "host": clip(host.map(|h| h.as_bytes())),
            "authority": clip(authority.map(|a| a.as_str().as_bytes())),
            "path": clip(Some(req.uri().path().as_bytes())),
            "client": ends.client.to_string(),
        }),
    );
    (
        StatusCode::FORBIDDEN,
        "refused: the request's Host is not the web UI's own address\n",
    )
        .into_response()
}

/// A TCP connection's two ends, as it was accepted.
#[derive(Clone, Copy, Debug)]
struct Ends {
    server: Option<SocketAddr>,
    client: SocketAddr,
}

impl Connected<axum::serve::IncomingStream<'_, tokio::net::TcpListener>> for Ends {
    fn connect_info(s: axum::serve::IncomingStream<'_, tokio::net::TcpListener>) -> Self {
        Self {
            server: s.io().local_addr().ok(),
            client: *s.remote_addr(),
        }
    }
}

impl Ends {
    /// The process on the other end, looked up only when a judged act
    /// arrives (theseus-6qy).
    fn peer(self) -> Peer {
        match self.server {
            Some(server) => Peer::Loopback {
                server,
                client: self.client,
            },
            None => Peer::Unknown(format!(
                "the web UI's connection from {} had no local address",
                self.client
            )),
        }
    }
}

async fn index() -> Response {
    serve_embedded("index.html")
}

async fn asset(Path(path): Path<String>) -> Response {
    match Assets::get(&path) {
        Some(_) => serve_embedded(&path),
        // Client-side routes fall back to the app shell.
        None => serve_embedded("index.html"),
    }
}

fn serve_embedded(path: &str) -> Response {
    match Assets::get(path) {
        Some(file) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            let cache = if path.starts_with("assets/") {
                "public, max-age=31536000, immutable"
            } else {
                "no-cache"
            };
            (
                [
                    (header::CONTENT_TYPE, mime.as_ref().to_string()),
                    (header::CACHE_CONTROL, cache.to_string()),
                ],
                Body::from(file.data.into_owned()),
            )
                .into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            "web UI assets are not built into this binary (run `npm run build` in web/)",
        )
            .into_response(),
    }
}

/// The protocol's WebSocket, for the UI's own page only: a browser sends
/// the page's `Origin` with every upgrade, and any other page's, or none,
/// is refused (theseus-70f).
async fn ws_upgrade(
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    ConnectInfo(ends): ConnectInfo<Ends>,
    State(ui): State<Arc<Ui>>,
) -> Response {
    let origin = headers.get(header::ORIGIN);
    if !origin.is_some_and(|o| o.to_str().is_ok_and(|o| ui.own.origin(o))) {
        ui.core.web_refused(
            Why::Origin,
            json!({
                "origin": clip(origin.map(|o| o.as_bytes())),
                "client": ends.client.to_string(),
            }),
        );
        return (
            StatusCode::FORBIDDEN,
            "refused: a WebSocket to the web UI must come from its own page\n",
        )
            .into_response();
    }
    let core = ui.core.clone();
    ws.on_upgrade(move |socket| bridge(socket, core, ends.peer()))
}

/// WebSocket ⇄ protocol connection. The core serves one end of an in-memory
/// duplex exactly as it would a Unix socket; this task pumps frames.
async fn bridge(socket: WebSocket, core: Arc<Core>, peer: Peer) {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let client = format!("web#{n}");
    tracing::info!(client = %client, "web client connected");

    let (ours, theirs) = tokio::io::duplex(256 * 1024);
    let (core_r, core_w) = tokio::io::split(theirs);
    let server = tokio::spawn(core.serve_connection(
        core_r,
        core_w,
        Client::new(client.clone(), Surface::Web).with_peer(peer),
    ));

    let (from_core, mut to_core) = tokio::io::split(ours);
    let (mut ws_tx, mut ws_rx) = socket.split();

    // core → browser: one line per frame
    let mut lines = BufReader::new(from_core).lines();
    let out = tokio::spawn(async move {
        while let Ok(Some(line)) = lines.next_line().await {
            if ws_tx.send(WsMessage::Text(line.into())).await.is_err() {
                break;
            }
        }
        let _ = ws_tx.close().await;
    });

    // browser → core
    while let Some(Ok(frame)) = ws_rx.next().await {
        match frame {
            WsMessage::Text(t) => {
                let mut line = t.to_string();
                line.push('\n');
                if to_core.write_all(line.as_bytes()).await.is_err() {
                    break;
                }
            }
            WsMessage::Binary(b) => {
                if to_core.write_all(&b).await.is_err() || to_core.write_all(b"\n").await.is_err() {
                    break;
                }
            }
            WsMessage::Close(_) => break,
            _ => {}
        }
    }
    let _ = to_core.shutdown().await;
    drop(to_core);
    let _ = out.await;
    let _ = server.await;
    tracing::info!(client = %client, "web client disconnected");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The UI's own `Host` and `Origin` (theseus-70f): its bind address or
    /// `localhost`, at its port, and nothing else; a v6 bind in brackets.
    #[test]
    fn only_the_uis_own_host_and_origin_pass() {
        let own = Own {
            ip: "127.0.0.1".parse().unwrap(),
            port: 7433,
        };
        for h in ["127.0.0.1:7433", "localhost:7433", "LocalHost:7433"] {
            assert!(own.host(h), "{h}");
        }
        for h in [
            "127.0.0.1",
            "127.0.0.1:7434",
            "127.0.0.2:7433",
            "evil.example:7433",
            "localhost.:7433",
            "127.0.0.1.nip.io:7433",
            "[::1]:7433",
            "0.0.0.0:7433",
            "localhost:7433:1",
            "localhost:",
            "",
        ] {
            assert!(!own.host(h), "{h}");
        }
        assert!(own.origin("http://127.0.0.1:7433"));
        assert!(own.origin("http://localhost:7433"));
        for o in [
            "null",
            "https://127.0.0.1:7433",
            "http://evil.example:7433",
            "http://127.0.0.1:7433/",
            "http://127.0.0.1",
            "file://",
        ] {
            assert!(!own.origin(o), "{o}");
        }
        let v6 = Own {
            ip: "::1".parse().unwrap(),
            port: 80,
        };
        assert!(v6.host("[::1]:80") && v6.host("[::1]") && v6.host("localhost"));
        assert!(!v6.host("::1") && !v6.host("[::1]:8080") && !v6.host("127.0.0.1"));
    }
}
