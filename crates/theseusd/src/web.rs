//! The localhost web UI (spec §3.14, first form). Serves the built Vite/React
//! app embedded in the binary and bridges a WebSocket to the protocol: each
//! text frame is one JSON-RPC line, so the browser is just another client
//! with no privileged path into the kernel. Loopback only; no auth yet. Each
//! connection keeps its two addresses, so that a judged act can find the
//! process that holds the client's end (theseus-6qy).

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::{
    body::Body,
    extract::{
        connect_info::Connected,
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
        ConnectInfo, Path, State,
    },
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use rust_embed::Embed;
use theseus_core::approval::{Client, Peer, Surface};
use theseus_core::Core;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Embed)]
#[folder = "web/dist/"]
struct Assets;

pub async fn serve(core: Arc<Core>, bind: &str, port: u16) -> Result<()> {
    // `Config::validate` refuses a bind that is not a loopback address
    // (theseus-2fo); this is the same check, where the socket is made.
    let ip: std::net::IpAddr = bind
        .parse()
        .with_context(|| format!("bad web bind {bind:?}"))?;
    let addr = SocketAddr::new(ip, port);
    if !addr.ip().is_loopback() {
        anyhow::bail!("web.bind must be a loopback address (reachability rule); got {bind}");
    }
    let app = Router::new()
        .route("/", get(index))
        .route("/ws", get(ws_upgrade))
        .route("/{*path}", get(asset))
        .with_state(core.clone());
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("binding web UI on {addr}"))?;
    tracing::info!(url = %format!("http://{addr}/"), "web UI listening (loopback only)");
    let shutdown = async move { core.shutdown.notified().await };
    axum::serve(listener, app.into_make_service_with_connect_info::<Ends>())
        .with_graceful_shutdown(shutdown)
        .await?;
    Ok(())
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

async fn ws_upgrade(
    ws: WebSocketUpgrade,
    ConnectInfo(ends): ConnectInfo<Ends>,
    State(core): State<Arc<Core>>,
) -> Response {
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
