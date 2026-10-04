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
//! The app is the cockpit (theseus-45n5), served at `/`. It took the place of the first app, the Observatory, on
//! 2026-10-03 (theseus-vm3n.6), and its old address still works: `/cockpit/…` redirects to the same route at `/…`.
//!
//! There is no per-start token. It would have to reach the page from this
//! same server, so it would reach exactly the clients that already pass
//! both checks: the UI's own page, and any local process, which can send
//! whatever `Host` and `Origin` it likes. The boundary against another local
//! user is the connecting socket's owner (theseus-3qf): a connection whose
//! client socket another uid owns is refused as it is accepted, before any
//! of its request is read (`OwnUser`).
//!
//! For UI development, `[web] dev_origin` names the Vite dev page
//! (theseus-zab). Off by default; while set, `/ws` serves that one origin
//! too, counted and ledgered (`web.dev_origin`), whether the page opens it
//! straight, as the cockpit's does, or through a dev server's proxy, which
//! passes the page's `Host` and `Origin`. A proxy rewrites neither header,
//! so another page's upgrade through it still carries its own `Origin`.

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
    http::{header, HeaderMap, StatusCode, Uri},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use rust_embed::Embed;
use serde_json::json;
use theseus_core::approval::{Client, Surface};
use theseus_core::webui::{clip, Why};
use theseus_core::Core;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

/// The app, the cockpit (theseus-45n5), served at `/`. It is built by `npm run build` in `cockpit/`, and the
/// build is not committed (it is several MB and changes with every edit): a binary built without it answers each
/// page with a 404 that says how to build it. `/ws` is the same either way.
#[derive(Embed)]
#[folder = "cockpit/dist/"]
#[allow_missing = true]
struct Cockpit;

const COCKPIT_MISSING: &str =
    "the cockpit is not built into this binary: run `npm ci && npm run build` in cockpit/, then rebuild theseusd";

#[expect(clippy::cognitive_complexity, reason = "shape budget: split it")]
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
    let dev = core.cfg.web.dev_origin.as_deref().map(Dev::new);
    if let Some(d) = &dev {
        tracing::warn!(
            origin = %d.origin,
            "web UI: /ws also serves the dev page ([web] dev_origin); unset it when you are done"
        );
    }
    let ui = Arc::new(Ui {
        core: core.clone(),
        own: Own {
            ip: addr.ip(),
            port: addr.port(),
        },
        dev,
    });
    let app = Router::new()
        .route("/", get(index))
        .route("/ws", get(ws_upgrade))
        .route("/cockpit", get(moved))
        .route("/cockpit/", get(moved))
        .route("/cockpit/{*path}", get(moved))
        .route("/{*path}", get(asset))
        .layer(middleware::from_fn_with_state(ui.clone(), own_host))
        .with_state(ui);
    tracing::info!(url = %format!("http://{addr}/"), "web UI listening (loopback only)");
    if !cfg!(target_os = "linux") {
        tracing::warn!("web UI: {}", theseus_core::webui::PEER_UNCHECKED);
    }
    let refusals = core.clone();
    let listener = OwnUser {
        inner: listener,
        uid: theseus_core::peer::own_uid(),
        refused: Arc::new(move |detail| refusals.web_refused(Why::Peer, detail)),
    };
    let shutdown = async move { core.shutdown.notified().await };
    axum::serve(listener, app.into_make_service_with_connect_info::<Ends>())
        .with_graceful_shutdown(shutdown)
        .await?;
    Ok(())
}

/// The UI's listener (theseus-3qf): a connection is served only when its
/// client socket is owned by the daemon's own uid, read as it is accepted,
/// before any of its request is read. `Host` and `Origin` keep out other
/// pages in the operator's browser, but a local process can send any it
/// likes, and a TCP port, unlike the Unix socket (0600), is open to every
/// user on the machine. A client that closed before the accept (a port
/// probe) is dropped, and not counted: no one is there to refuse.
struct OwnUser {
    inner: tokio::net::TcpListener,
    /// The daemon's uid.
    uid: u32,
    /// Where a refusal is counted and ledgered: `web.refused`, kind `peer`.
    refused: Arc<dyn Fn(serde_json::Value) + Send + Sync>,
}

impl axum::serve::Listener for OwnUser {
    type Io = tokio::net::TcpStream;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        use theseus_core::peer::{admit, client_uid, Admit};
        loop {
            let (io, client) = axum::serve::Listener::accept(&mut self.inner).await;
            // The tables take a millisecond or two to read (the kernel walks
            // every hash bucket), so off the runtime's workers.
            let owner = match io.local_addr() {
                Ok(server) => tokio::task::spawn_blocking(move || client_uid(server, client))
                    .await
                    .map_err(|e| format!("its owner's lookup failed: {e}")),
                Err(e) => Err(format!("its server end had no address: {e}")),
            };
            let verdict = match owner {
                Ok(owner) => admit(&owner, self.uid),
                Err(why) => Admit::Refuse { why, uid: None },
            };
            match verdict {
                Admit::Serve => return (io, client),
                Admit::Gone => {
                    tracing::debug!(%client, "web UI: the client closed before its connection was accepted");
                }
                Admit::Refuse { why, uid } => {
                    (self.refused)(json!({"client": client.to_string(), "uid": uid, "why": why}));
                    turn_away(io);
                }
            }
        }
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        self.inner.local_addr()
    }
}

/// A refused connection's answer: a 403, then a close. A task writes it, so
/// the accept loop never waits on a client, and what the client sent is read
/// and dropped (for at most 2 s) so that the close is a FIN, not a reset that
/// could take the answer with it.
fn turn_away(mut io: tokio::net::TcpStream) {
    const BODY: &str = "refused: the web UI serves only the user that runs the daemon\n";
    tokio::spawn(async move {
        let answer = format!(
            "HTTP/1.1 403 Forbidden\r\ncontent-type: text/plain; charset=utf-8\r\n\
             content-length: {}\r\nconnection: close\r\n\r\n{BODY}",
            BODY.len()
        );
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), async move {
            let _ = io.write_all(answer.as_bytes()).await;
            let _ = io.shutdown().await;
            let mut sink = [0u8; 1024];
            while matches!(io.read(&mut sink).await, Ok(n) if n > 0) {}
        })
        .await;
    });
}

/// What every route shares: the core, the UI's own address, and the dev
/// page's origin when the config names one.
struct Ui {
    core: Arc<Core>,
    own: Own,
    dev: Option<Dev>,
}

/// The Vite dev page (`[web] dev_origin`, theseus-zab), served on `/ws`
/// beside the UI's own page. The dev server's proxy passes the page's own
/// `Host` (the dev server's address) and `Origin`, and a browser sets both,
/// so another page's upgrade through the proxy still carries its own
/// `Origin`, and is refused.
#[derive(Clone, Debug)]
struct Dev {
    /// `http://localhost:5173`, as the config gives it (validated at load).
    origin: String,
    /// `localhost:5173`: the `Host` its proxy passes.
    authority: String,
}

impl Dev {
    fn new(origin: &str) -> Self {
        Self {
            origin: origin.to_string(),
            authority: origin.trim_start_matches("http://").to_string(),
        }
    }
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
    // The dev page's proxy (theseus-zab): its own `Host`, on `/ws` only,
    // where the upgrade's `Origin` is checked next.
    let dev = ui.dev.as_ref().is_some_and(|d| {
        req.uri().path() == "/ws"
            && host.is_some_and(|h| {
                h.to_str()
                    .is_ok_and(|h| h.eq_ignore_ascii_case(&d.authority))
            })
            && authority.is_none_or(|a| a.as_str().eq_ignore_ascii_case(&d.authority))
    });
    if named && (own || dev) {
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

/// A TCP connection's client end, as it was accepted.
#[derive(Clone, Copy, Debug)]
struct Ends {
    client: SocketAddr,
}

impl Connected<axum::serve::IncomingStream<'_, OwnUser>> for Ends {
    fn connect_info(s: axum::serve::IncomingStream<'_, OwnUser>) -> Self {
        Self {
            client: *s.remote_addr(),
        }
    }
}

async fn index() -> Response {
    serve_embedded::<Cockpit>("index.html", COCKPIT_MISSING)
}

async fn asset(Path(path): Path<String>) -> Response {
    match Cockpit::get(&path) {
        Some(_) => serve_embedded::<Cockpit>(&path, COCKPIT_MISSING),
        // The cockpit's own client-side routes (`/session/…`) fall back to its app shell.
        None => serve_embedded::<Cockpit>("index.html", COCKPIT_MISSING),
    }
}

/// The cockpit's old address (theseus-vm3n.6): `/cockpit/<route>?<query>` moves for good to `/<route>?<query>`,
/// so a bookmark or a link from before still lands.
async fn moved(uri: Uri) -> Redirect {
    Redirect::permanent(&moved_to(
        uri.path_and_query().map_or("/cockpit", |p| p.as_str()),
    ))
}

/// Where an old cockpit address goes: always a path on this same address. Its leading slashes and backslashes
/// fold into one, since a browser reads `//host` (or `/\host`) as another site's address.
fn moved_to(old: &str) -> String {
    let rest = old.strip_prefix("/cockpit").unwrap_or(old);
    format!("/{}", rest.trim_start_matches(['/', '\\']))
}

fn serve_embedded<E: Embed>(path: &str, missing: &'static str) -> Response {
    match E::get(path) {
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
        None => (StatusCode::NOT_FOUND, missing).into_response(),
    }
}

/// The protocol's WebSocket, for the UI's own page only: a browser sends
/// the page's `Origin` with every upgrade, and any other page's, or none,
/// is refused (theseus-70f). With `[web] dev_origin` set, the dev page is
/// served too, and each use is counted and ledgered (theseus-zab).
async fn ws_upgrade(
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    ConnectInfo(ends): ConnectInfo<Ends>,
    State(ui): State<Arc<Ui>>,
) -> Response {
    let origin = headers.get(header::ORIGIN);
    let own = origin.is_some_and(|o| o.to_str().is_ok_and(|o| ui.own.origin(o)));
    let dev = !own
        && ui.dev.as_ref().is_some_and(|d| {
            origin.is_some_and(|o| o.to_str().is_ok_and(|o| o.eq_ignore_ascii_case(&d.origin)))
        });
    if dev {
        ui.core.web_dev_origin(json!({
            "origin": clip(origin.map(|o| o.as_bytes())),
            "host": clip(headers.get(header::HOST).map(|h| h.as_bytes())),
            "client": ends.client.to_string(),
        }));
    }
    if !own && !dev {
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
    ws.on_upgrade(move |socket| bridge(socket, core))
}

/// WebSocket ⇄ protocol connection. The core serves one end of an in-memory
/// duplex exactly as it would a Unix socket; this task pumps frames.
#[expect(clippy::cognitive_complexity, reason = "shape budget: split it")]
async fn bridge(socket: WebSocket, core: Arc<Core>) {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let client = format!("web#{n}");
    tracing::info!(client = %client, "web client connected");

    let (ours, theirs) = tokio::io::duplex(256 * 1024);
    let (core_r, core_w) = tokio::io::split(theirs);
    let server = tokio::spawn(core.serve_connection(
        core_r,
        core_w,
        Client::new(client.clone(), Surface::Web),
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

    /// theseus-3qf, at accept: a connection from this user is served while
    /// the daemon is this user, and refused while the daemon is any other,
    /// with a 403 and one refusal naming the client and its uid. Real sockets
    /// and the real tables; changing the daemon's uid stands in for another
    /// user's client, which a test cannot make without privilege.
    #[tokio::test]
    async fn only_the_daemons_own_users_connections_are_accepted() {
        use axum::serve::Listener;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let me = theseus_core::peer::own_uid();
        let seen = Arc::new(std::sync::Mutex::new(Vec::<serde_json::Value>::new()));
        let sink = seen.clone();
        let inner = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = inner.local_addr().unwrap();
        let mut own = OwnUser {
            inner,
            uid: me,
            refused: Arc::new(move |v| sink.lock().unwrap().push(v)),
        };
        let wait = std::time::Duration::from_secs(5);
        // A port probe, closed before the accept, is passed over uncounted;
        // the next connection is served.
        drop(tokio::net::TcpStream::connect(addr).await.unwrap());
        let c = tokio::net::TcpStream::connect(addr).await.unwrap();
        let (io, client) = tokio::time::timeout(wait, own.accept()).await.unwrap();
        assert_eq!(client, c.local_addr().unwrap());
        drop((io, c));
        assert!(seen.lock().unwrap().is_empty());

        own.uid = me + 1;
        let accepting = tokio::spawn(async move {
            let _ = own.accept().await;
        });
        let mut c = tokio::net::TcpStream::connect(addr).await.unwrap();
        c.write_all(b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
            .await
            .unwrap();
        let mut got = String::new();
        tokio::time::timeout(wait, c.read_to_string(&mut got))
            .await
            .unwrap()
            .unwrap();
        assert!(
            got.starts_with("HTTP/1.1 403 Forbidden\r\n")
                && got.ends_with(
                    "\r\n\r\nrefused: the web UI serves only the user that runs the daemon\n"
                ),
            "{got}"
        );
        let seen = seen.lock().unwrap().clone();
        assert_eq!(seen.len(), 1, "{seen:?}");
        assert_eq!(seen[0]["uid"], me);
        assert_eq!(seen[0]["client"], c.local_addr().unwrap().to_string());
        assert_eq!(
            seen[0]["why"],
            format!(
                "its socket belongs to uid {me}, not the daemon's ({})",
                me + 1
            )
        );
        accepting.abort();
    }

    /// The cockpit (theseus-45n5) is the app at `/` (theseus-vm3n.6): its app shell when it is built into the
    /// binary, as the gate builds it before the suite, and otherwise a 404 that says how to build it. Its
    /// client-side routes fall back to the shell.
    #[tokio::test]
    async fn the_cockpit_serves_its_shell_at_the_root_or_says_how_to_build_it() {
        let built = Cockpit::get("index.html").is_some();
        for (r, what) in [
            (index().await, "/"),
            (
                asset(Path("session/ses_x".to_string())).await,
                "/session/ses_x",
            ),
        ] {
            let status = r.status();
            let body = axum::body::to_bytes(r.into_body(), usize::MAX)
                .await
                .unwrap();
            let body = String::from_utf8_lossy(&body);
            if built {
                assert_eq!(status, StatusCode::OK, "{what}");
                assert!(
                    body.contains("<title>Theseus · Cockpit</title>")
                        && body.contains(r#"<div id="root">"#),
                    "{what}: {body}"
                );
            } else {
                assert_eq!(status, StatusCode::NOT_FOUND, "{what}");
                assert_eq!(body, COCKPIT_MISSING, "{what}");
            }
        }
    }

    /// The cockpit's old address moves for good to the same route at the root, its query kept, and never to
    /// another site's address.
    #[tokio::test]
    async fn the_cockpits_old_address_moves_to_the_root() {
        for (old, new) in [
            ("/cockpit", "/"),
            ("/cockpit/", "/"),
            ("/cockpit?calm=1", "/?calm=1"),
            ("/cockpit/ship", "/ship"),
            (
                "/cockpit/session/ses_x?call=act_1",
                "/session/ses_x?call=act_1",
            ),
            ("/cockpit//elsewhere.example/x", "/elsewhere.example/x"),
            ("/cockpit/\\elsewhere.example", "/elsewhere.example"),
        ] {
            assert_eq!(moved_to(old), new, "{old}");
        }
        let r = moved(Uri::from_static("/cockpit/ledger?family=tool"))
            .await
            .into_response();
        assert_eq!(r.status(), StatusCode::PERMANENT_REDIRECT);
        assert_eq!(r.headers()[header::LOCATION], "/ledger?family=tool");
    }
}
