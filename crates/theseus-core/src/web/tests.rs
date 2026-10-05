//! The web tools against a test's own HTTP server on 127.0.0.1 (DD5).
//!
//! The private-address rule is what a test must get past to reach that
//! server, and it does so only through the resolver's two test tables
//! (`Dns::hosts` and `Dns::public`): names such as `site.test` resolve to the
//! server, and 127.0.0.1 is taken as public for them. No config key or tool
//! input reaches either table. A URL that names 127.0.0.1 itself is still
//! private, at the gate and on every hop.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_tools::{AsyncResult, External, Tool, ToolCtx, ToolOutput};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::fetch::Fetch;
use super::net::Dns;
use super::search::Search;
use super::Web;
use crate::broker::{Bound, Broker};
use crate::config::{BrokerConfig, WebToolsConfig};
use crate::cpu::CpuPool;
use crate::policy::{Posture, ToolPolicy};
use crate::secrets::{Secret, SecretBoard};

/// One request the server saw.
#[derive(Debug, Clone)]
pub(crate) struct Hit {
    /// Its path, with its query.
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub came: Instant,
    pub answered: Option<Instant>,
}

/// A test's HTTP/1.1 server on 127.0.0.1, answering `route`.
pub(crate) struct Server {
    pub port: u16,
    pub hits: Arc<Mutex<Vec<Hit>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Server {
    /// The paths it was asked for, in order.
    pub fn paths(&self) -> Vec<String> {
        self.hits
            .lock()
            .unwrap()
            .iter()
            .map(|h| h.path.clone())
            .collect()
    }
}

struct Reply {
    status: u16,
    reason: &'static str,
    headers: Vec<(&'static str, String)>,
    body: Vec<u8>,
    delay_ms: u64,
}

fn ok(ctype: &str, body: &[u8]) -> Reply {
    Reply {
        status: 200,
        reason: "OK",
        headers: vec![("Content-Type", ctype.into())],
        body: body.to_vec(),
        delay_ms: 0,
    }
}

fn redirect(to: &str) -> Reply {
    Reply {
        status: 302,
        reason: "Found",
        headers: vec![("Location", to.into())],
        body: vec![],
        delay_ms: 0,
    }
}

pub(crate) const PAGE: &str = r##"<!DOCTYPE html><html><head><title>Fixture &amp; page</title>
<script>document.write("<p>INJECTED</p>"); var x = "</div>";</script>
<style>p { color: red }</style></head>
<body><nav><a href="#main">Skip to main</a></nav>
<h1>The fixture</h1>
<p>A <a href="/other.html">relative link</a>, &lt;tags&gt; as text, and &quot;quotes&quot; &mdash; decoded.</p>
<ul><li>first</li><li>second</li></ul>
<pre>let x = a &lt; b;
    indented();</pre>
<noscript>Turn on JavaScript</noscript><svg viewBox="0 0 1 1"><text>DRAWN</text></svg>
<h2>Answer</h2><p>The answer is 42.</p>
</body></html>"##;

/// A search answer in the Brave Search API's shape, made by hand.
pub(crate) const BRAVE: &str = r#"{"type":"search",
 "query":{"original":"rust ignore WalkParallel","more_results_available":true},
 "mixed":{"type":"mixed","main":[{"type":"web","index":0,"all":false}]},
 "web":{"type":"search","family_friendly":true,"results":[
  {"title":"WalkParallel in ignore - Rust","url":"https://docs.rs/ignore/latest/ignore/struct.WalkParallel.html",
   "is_source_local":false,"description":"<strong>WalkParallel</strong> is a parallel recursive directory iterator over files paths in one or more directories.",
   "page_age":"2025-08-01T00:00:00","profile":{"name":"Docs.rs","url":"https://docs.rs"},"language":"en",
   "family_friendly":true,"type":"search_result","subtype":"generic",
   "meta_url":{"scheme":"https","netloc":"docs.rs","hostname":"docs.rs","path":"› ignore › latest"}},
  {"title":"ignore - crates.io: Rust Package Registry","url":"https://crates.io/crates/ignore",
   "description":"A fast library for efficiently matching ignore files such as <strong>.gitignore</strong> against file paths.","type":"search_result"},
  {"title":"ripgrep&#x27;s <b>ignore</b> crate","url":"https://github.com/BurntSushi/ripgrep/tree/master/crates/ignore",
   "description":"","type":"search_result"}
 ]}}"#;

/// Brave's answer to a key: the fixture for `tv-good`, and its refusals.
fn brave(headers: &BTreeMap<String, String>) -> Reply {
    let refusal = |status, reason, code: &str| {
        Reply {
        status,
        reason,
        headers: vec![("Content-Type", "application/json".into())],
        body: format!(
            r#"{{"type":"ErrorResponse","error":{{"id":"e1","status":{status},"code":"{code}","detail":"The request was not processed (detail-7c1e).","meta":{{}}}},"time":1}}"#
        )
        .into_bytes(),
        delay_ms: 0,
    }
    };
    match headers.get("x-subscription-token").map(String::as_str) {
        Some("tv-good") => ok("application/json", BRAVE.as_bytes()),
        Some("tv-none") => ok(
            "application/json",
            br#"{"type":"search","web":{"results":[]}}"#,
        ),
        Some("tv-bad") => refusal(401, "Unauthorized", "SUBSCRIPTION_TOKEN_INVALID"),
        Some("tv-busy") => refusal(429, "Too Many Requests", "RATE_LIMITED"),
        _ => refusal(422, "Unprocessable Entity", "VALIDATION"),
    }
}

fn route(path: &str, headers: &BTreeMap<String, String>, port: u16) -> Reply {
    let bare = path.split('?').next().unwrap_or(path);
    match bare {
        "/page.html" => ok("text/html; charset=utf-8", PAGE.as_bytes()),
        "/plain.txt" => ok("text/plain", b"line one\n  line <two> &amp; three\n"),
        "/data.json" => ok("application/json", br#"{"a": [1, 2], "b": "<c>"}"#),
        "/doc.pdf" => ok(
            "application/pdf",
            &theseus_files::pdf::sample(&["Tide table for March", "High water 06:12"]),
        ),
        "/odd.pdf" => ok("application/pdf", &[b'%'; 12_400]),
        "/big.txt" => ok("text/plain", &[b'x'; 100_000]),
        "/slow" => Reply {
            delay_ms: 2_000,
            ..ok("text/plain", b"late")
        },
        // A page that never comes in a test's time (a cancel's abort, 18a).
        "/hang" => Reply {
            delay_ms: 600_000,
            ..ok("text/plain", b"never")
        },
        "/wait" => Reply {
            delay_ms: 400,
            ..ok("text/plain", b"waited")
        },
        "/to-private" => redirect("http://10.0.0.1/secret"),
        "/to-metadata" => redirect("http://169.254.169.254/latest/meta-data/"),
        "/to-loopback" => redirect(&format!("http://127.0.0.1:{port}/plain.txt")),
        "/to-nowhere" => redirect("http://said so.test/words-7c1e"),
        "/search" => brave(headers),
        p if p.starts_with("/r/") => match p[3..].parse::<u32>() {
            Ok(0) => ok("text/plain", b"arrived"),
            Ok(n) => redirect(&format!("/r/{}", n - 1)),
            Err(_) => redirect("/r/0"),
        },
        _ => Reply {
            status: 404,
            reason: "Not Found",
            ..ok("text/plain", b"no such page")
        },
    }
}

pub(crate) async fn serve() -> Server {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let hits = Arc::new(Mutex::new(Vec::new()));
    let seen = hits.clone();
    let task = tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            let hits = seen.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    match sock.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    }
                }
                let head = String::from_utf8_lossy(&buf).into_owned();
                let mut lines = head.split("\r\n");
                let path = lines
                    .next()
                    .and_then(|l| l.split(' ').nth(1))
                    .unwrap_or("/")
                    .to_string();
                let headers: BTreeMap<String, String> = lines
                    .filter_map(|l| l.split_once(':'))
                    .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
                    .collect();
                let i = {
                    let mut v = hits.lock().unwrap();
                    v.push(Hit {
                        path: path.clone(),
                        headers: headers.clone(),
                        came: Instant::now(),
                        answered: None,
                    });
                    v.len() - 1
                };
                let r = route(&path, &headers, port);
                if r.delay_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(r.delay_ms)).await;
                }
                let mut out = format!(
                    "HTTP/1.1 {} {}\r\nConnection: close\r\nContent-Length: {}\r\n",
                    r.status,
                    r.reason,
                    r.body.len()
                );
                for (k, v) in &r.headers {
                    out.push_str(&format!("{k}: {v}\r\n"));
                }
                out.push_str("\r\n");
                let _ = sock.write_all(out.as_bytes()).await;
                let _ = sock.write_all(&r.body).await;
                let _ = sock.shutdown().await;
                hits.lock().unwrap()[i].answered = Some(Instant::now());
            });
        }
    });
    Server { port, hits, task }
}

/// The web tools over the server: `site.test`, `other.test`, `search.test`,
/// and `rebind.test` resolve to it, and with `public` its address is taken
/// as public.
pub(crate) fn web(port: u16, cfg: WebToolsConfig, public: bool) -> Arc<Web> {
    let lo: IpAddr = "127.0.0.1".parse().unwrap();
    let mut dns = Dns::checked();
    for n in ["site.test", "other.test", "search.test", "rebind.test"] {
        dns.hosts.insert(n.into(), vec![lo]);
    }
    if public {
        dns.public = vec![lo];
    }
    Web::with_dns(
        &cfg,
        30_000,
        CpuPool::new(2),
        dns,
        &format!("http://search.test:{port}/search"),
        Default::default(),
    )
}

fn ctx(approved: bool) -> ToolCtx {
    ToolCtx {
        approved,
        ..ToolCtx::for_tests(&std::env::temp_dir())
    }
}

async fn fetch(w: &Arc<Web>, input: Value, approved: bool) -> AsyncResult {
    let tool = Fetch(w.clone());
    let c = ctx(approved);
    tool.plan(&input, &c).expect("the input plans");
    tool.run_async(&input, &c).await
}

async fn fetch_media(
    w: &Arc<Web>,
    input: Value,
) -> (ToolOutput, Option<External>, Option<theseus_tools::Media>) {
    let tool = Fetch(w.clone());
    let c = ctx(false);
    tool.plan(&input, &c).expect("the input plans");
    tool.run_async_with_media(&input, &c)
        .await
        .unwrap_or_else(|f| panic!("{input}: {}", f.message))
}

async fn fetched(w: &Arc<Web>, url: &str) -> (String, Value, String) {
    let (out, ext) = fetch(w, json!({ "url": url }), false)
        .await
        .unwrap_or_else(|f| panic!("{url}: {}", f.message));
    (out.text, out.meta, ext.expect("marked external").url)
}

async fn failure(w: &Arc<Web>, input: Value, approved: bool) -> String {
    match fetch(w, input, approved).await {
        Ok((o, _)) => panic!("fetched: {}", o.text),
        Err(f) => f.message,
    }
}

#[tokio::test]
async fn a_page_becomes_text_and_is_marked_external() {
    let s = serve().await;
    let w = web(s.port, WebToolsConfig::default(), true);
    let url = format!("http://site.test:{}/page.html", s.port);
    let (text, meta, external) = fetched(&w, &url).await;
    let other = format!("http://site.test:{}/other.html", s.port);
    assert_eq!(
        text,
        format!(
            "GET {url} → 200 OK, text/html, {}.\nTitle: Fixture & page\n\n\
             Skip to main\n\n\
             # The fixture\n\n\
             A relative link ({other}), <tags> as text, and \"quotes\" — decoded.\n\n\
             - first\n- second\n\n\
             let x = a < b;\n    indented();\n\n\
             ## Answer\n\n\
             The answer is 42.\n",
            crate::narrative::bytes(PAGE.len() as u64)
        )
    );
    for gone in ["INJECTED", "color: red", "JavaScript", "DRAWN"] {
        assert!(!text.contains(gone), "{gone}");
    }
    assert_eq!(external, url);
    assert_eq!(
        (
            meta["status"].clone(),
            meta["read"].clone(),
            meta["bytes"].clone()
        ),
        (json!(200), json!("html"), json!(PAGE.len()))
    );
}

#[tokio::test]
async fn text_json_and_a_pdf_come_back_and_a_pdf_that_is_not_one_says_so() {
    let s = serve().await;
    let w = web(s.port, WebToolsConfig::default(), true);
    let at = |p: &str| format!("http://site.test:{}{p}", s.port);
    let (text, ..) = fetched(&w, &at("/plain.txt")).await;
    assert_eq!(
        text,
        format!(
            "GET {} → 200 OK, text/plain, 34 bytes.\n\nline one\n  line <two> &amp; three\n",
            at("/plain.txt")
        )
    );
    let (text, ..) = fetched(&w, &at("/data.json")).await;
    assert!(
        text.ends_with("\n\n{\"a\": [1, 2], \"b\": \"<c>\"}"),
        "{text}"
    );
    // A PDF comes back as its pages, as fs.read returns them (theseus-c9l6),
    // with the file for the model; one that is not a PDF says why.
    let (out, ext, media) = fetch_media(&w, json!({"url": at("/doc.pdf"), "pages": "2"})).await;
    assert!(
        out.text
            .ends_with("It is a PDF of 2 pages, 746 bytes: page 2 shown."),
        "{}",
        out.text
    );
    assert_eq!(out.meta["read"], "pdf");
    assert_eq!(ext.expect("marked external").url, at("/doc.pdf"));
    let Some(theseus_tools::Media::Pdf(p)) = media else {
        panic!("the PDF's pages")
    };
    assert_eq!(
        (p.name.as_str(), p.read.texts.clone()),
        ("doc.pdf, page 2 of 2", vec!["High water 06:12".to_string()])
    );
    let (text, meta, _) = fetched(&w, &at("/odd.pdf")).await;
    assert!(
        text.ends_with(
            "It was not read as a PDF (12400 bytes): it is not a PDF (no %PDF- header)."
        ),
        "{text}"
    );
    assert_eq!(meta["read"], "no");
    // A page that is not there is still a page: its status says so.
    let (text, meta, _) = fetched(&w, &at("/missing")).await;
    assert!(text.starts_with(&format!(
        "GET {} → 404 Not Found, text/plain",
        at("/missing")
    )));
    assert_eq!(meta["status"], json!(404));
}

#[tokio::test]
async fn five_redirects_are_followed_and_a_sixth_fails() {
    let s = serve().await;
    let w = web(s.port, WebToolsConfig::default(), true);
    let at = |p: &str| format!("http://site.test:{}{p}", s.port);
    let (text, meta, external) = fetched(&w, &at("/r/5")).await;
    assert!(
        text.starts_with(&format!(
            "GET {} (after 5 redirects from {}) → 200 OK",
            at("/r/0"),
            at("/r/5")
        )),
        "{text}"
    );
    assert_eq!(
        (meta["redirects"].clone(), external),
        (json!(5), at("/r/0"))
    );
    let why = failure(&w, json!({"url": at("/r/6")}), false).await;
    assert_eq!(
        why,
        format!(
            "Not fetched: {} redirected 5 times, the most a fetch follows, and {} redirects \
             again, to {}.",
            at("/r/6"),
            at("/r/1"),
            at("/r/0")
        )
    );
    // A Location that is not a URL fails the call, and its text is not echoed.
    let why = failure(&w, json!({"url": at("/to-nowhere")}), false).await;
    assert_eq!(
        why,
        format!(
            "Not fetched: {} redirects to a Location that is not a URL (invalid \
             international domain name).",
            at("/to-nowhere")
        )
    );
}

#[tokio::test]
async fn a_redirect_to_a_private_address_is_refused_even_when_approved() {
    let s = serve().await;
    let w = web(s.port, WebToolsConfig::default(), true);
    let p = s.port;
    for (from, to, why) in [
        (
            "/to-private",
            "http://10.0.0.1/secret".to_string(),
            "10.0.0.1 is a private address",
        ),
        (
            "/to-metadata",
            "http://169.254.169.254/latest/meta-data/".to_string(),
            "169.254.169.254 is a link-local address",
        ),
        (
            "/to-loopback",
            format!("http://127.0.0.1:{p}/plain.txt"),
            "127.0.0.1 is a loopback address",
        ),
    ] {
        let first = format!("http://site.test:{p}{from}");
        assert_eq!(
            failure(&w, json!({ "url": first }), false).await,
            format!(
                "Not fetched: {first} redirects to {to}, and {why}. A private address waits for \
                 the operator's approval: to ask, fetch {to} itself."
            )
        );
    }
    // An approval covers the private origin it names, and no other.
    let approved = format!("http://127.0.0.1:{p}/r/2");
    let (out, _) = fetch(&w, json!({ "url": approved }), true).await.unwrap();
    assert!(out.text.contains("after 2 redirects"), "{}", out.text);
    let url = format!("http://127.0.0.1:{p}/to-private");
    let why = failure(&w, json!({ "url": url }), true).await;
    assert!(why.contains("and 10.0.0.1 is a private address"), "{why}");
    assert!(!s.paths().iter().any(|h| h.contains("secret")));
}

#[tokio::test]
async fn a_private_address_is_reached_only_by_an_approved_call() {
    let s = serve().await;
    let w = web(s.port, WebToolsConfig::default(), true);
    let url = format!("http://127.0.0.1:{}/plain.txt", s.port);
    // Below the gate too: a call that was not approved does not connect.
    assert_eq!(
        failure(&w, json!({ "url": url }), false).await,
        format!(
            "Not fetched: 127.0.0.1 is a loopback address. A private address waits for the \
             operator's approval: to ask, fetch {url} itself."
        )
    );
    assert!(s.paths().is_empty());
    let (out, _) = fetch(&w, json!({ "url": url }), true).await.unwrap();
    assert!(out.text.ends_with("line one\n  line <two> &amp; three\n"));
    assert_eq!(s.paths(), vec!["/plain.txt"]);
}

/// `[policy] private_addresses = "open"` (theseus-7gir.20; the bench
/// profile's): a call no one approved reaches a private address, follows a
/// redirect to one, and connects to a name whose answer is one.
#[tokio::test]
async fn an_open_policy_reaches_private_addresses_unapproved() {
    let s = serve().await;
    let p = s.port;
    let mut dns = Dns::checked();
    for n in ["site.test", "rebind.test"] {
        dns.hosts
            .insert(n.into(), vec!["127.0.0.1".parse().unwrap()]);
    }
    let w = Web::with_dns(
        &WebToolsConfig::default(),
        30_000,
        CpuPool::new(2),
        dns,
        &format!("http://search.test:{p}/search"),
        super::net::PrivateAddresses::Open,
    );
    let (out, _) = fetch(
        &w,
        json!({ "url": format!("http://127.0.0.1:{p}/plain.txt") }),
        false,
    )
    .await
    .unwrap();
    assert!(out.text.ends_with("line one\n  line <two> &amp; three\n"));
    let to_loopback = format!("http://site.test:{p}/to-loopback");
    let (out, _) = fetch(&w, json!({ "url": to_loopback }), false)
        .await
        .unwrap();
    assert!(out.text.ends_with("line one\n  line <two> &amp; three\n"));
    let rebind = format!("http://rebind.test:{p}/plain.txt");
    fetch(&w, json!({ "url": rebind }), false).await.unwrap();
    assert_eq!(
        s.paths(),
        ["/plain.txt", "/to-loopback", "/plain.txt", "/plain.txt"]
    );
}

#[tokio::test]
async fn a_name_that_resolves_to_a_loopback_address_is_refused_at_connect() {
    let s = serve().await;
    // `rebind.test` is a public name to the gate; its answer is 127.0.0.1.
    let w = web(s.port, WebToolsConfig::default(), false);
    let url = format!("http://rebind.test:{}/page.html", s.port);
    assert_eq!(
        failure(&w, json!({ "url": url }), false).await,
        format!(
            "Not fetched: rebind.test resolves to 127.0.0.1, a loopback address, so Theseus did \
             not connect: a name that resolves to a private address is refused at connect. A \
             private address waits for the operator's approval: to ask, fetch \
             http://127.0.0.1:{}/page.html, which names it.",
            s.port
        )
    );
    assert!(s.paths().is_empty(), "it connected: {:?}", s.paths());
}

#[tokio::test]
async fn the_byte_cap_truncates_and_the_timeout_fails_and_each_says_so() {
    let s = serve().await;
    let at = |p: &str| format!("http://site.test:{}{p}", s.port);
    let w = web(s.port, WebToolsConfig::default(), true);
    let (out, _) = fetch(&w, json!({"url": at("/big.txt"), "max_bytes": 1000}), false)
        .await
        .unwrap();
    assert!(
        out.text.starts_with(&format!(
            "GET {} → 200 OK, text/plain, the first 1,000 bytes of 97.7 KB ([tools.web] max_bytes, \
             or the call's max_bytes).\n\n",
            at("/big.txt")
        )),
        "{}",
        &out.text[..200]
    );
    assert_eq!(
        (out.meta["truncated"].clone(), out.meta["bytes"].clone()),
        (json!(true), json!(1000))
    );
    // The config's cap is the most a call gets.
    let cap = WebToolsConfig {
        max_bytes: 500,
        ..Default::default()
    };
    let small = web(s.port, cap, true);
    let (out, _) = fetch(
        &small,
        json!({"url": at("/big.txt"), "max_bytes": 5000}),
        false,
    )
    .await
    .unwrap();
    assert_eq!(out.meta["bytes"], json!(500));
    // What the model sees is cut to the result's size, its head kept.
    let (out, _) = fetch(&w, json!({"url": at("/big.txt")}), false)
        .await
        .unwrap();
    assert!(out.text.chars().count() <= 30_000);
    assert!(out
        .text
        .ends_with("more characters of this page's text are not shown]"));
    let brief = WebToolsConfig {
        timeout_secs: 1,
        ..Default::default()
    };
    let quick = web(s.port, brief, true);
    let t0 = Instant::now();
    assert_eq!(
        failure(&quick, json!({"url": at("/slow")}), false).await,
        format!(
            "Not fetched: {} took longer than 1 s ([tools.web] timeout_secs), so the fetch \
             stopped.",
            at("/slow")
        )
    );
    assert!(t0.elapsed() < Duration::from_millis(1_900));
}

/// A policy whose postures are all open, with the floor's programs: what
/// the gate's own steps ask, alone.
fn open_policy() -> ToolPolicy {
    ToolPolicy {
        roots: vec![],
        approve_paths: vec![],
        allow_argv: vec![],
        approve_argv: vec![],
        enforcement: Posture::Open,
        tools: BTreeMap::new(),
        mcp: BTreeMap::new(),
        aws: BTreeMap::new(),
        confirmer: "operator".into(),
        floor_paths: vec![],
        floor_argv: crate::policy::floor_argv(),
        private_addresses: Default::default(),
    }
}

#[test]
fn each_private_url_waits_for_approval_at_the_gate_and_a_public_one_does_not() {
    let w = Web::new(
        &WebToolsConfig::default(),
        30_000,
        CpuPool::new(1),
        Default::default(),
    );
    let policy = open_policy();
    let c = ctx(false);
    let (fetch, search) = (Fetch(w.clone()), Search(w));
    // Planned and decided only: nothing connects.
    for (url, why) in [
        ("http://127.0.0.1:7433/", "127.0.0.1 is a loopback address"),
        ("http://localhost/", "localhost is this machine"),
        ("http://[::1]/", "[::1] is a loopback address"),
        ("http://10.0.0.1/", "10.0.0.1 is a private address"),
        (
            "http://169.254.169.254/",
            "169.254.169.254 is a link-local address",
        ),
    ] {
        let plan = fetch.plan(&json!({ "url": url }), &c).unwrap();
        let d = policy.decide(&fetch, &plan);
        assert_eq!(d.posture, Posture::Approve, "{url}");
        assert_eq!(
            d.reason,
            format!(
                "fetch {url}: http.fetch — approve ({why}, and a private address waits for \
                 approval)"
            )
        );
    }
    for url in ["https://doc.rust-lang.org/std/", "http://rebind.test/"] {
        let plan = fetch.plan(&json!({ "url": url }), &c).unwrap();
        assert_eq!(policy.decide(&fetch, &plan).posture, Posture::Open, "{url}");
    }
    let plan = search.plan(&json!({"query": "rust"}), &c).unwrap();
    assert_eq!(
        plan.url.as_deref(),
        Some("https://api.search.brave.com/res/v1/web/search?q=rust&count=5")
    );
    assert_eq!(policy.decide(&search, &plan).posture, Posture::Open);
    for bad in [
        json!({"url": "ftp://example.com/"}),
        json!({"url": "not a url"}),
        json!({"url": "https://example.com/", "max_bytes": 0}),
        json!({"url": "https://example.com/", "method": "POST"}),
    ] {
        assert!(fetch.plan(&bad, &c).is_err(), "{bad}");
    }
    for bad in [json!({"query": " "}), json!({"query": "x", "count": 21})] {
        assert!(search.plan(&bad, &c).is_err(), "{bad}");
    }
}

/// theseus-94a6: in a shared place, the card of a fetch that waits on a
/// private address also says that the page would join a conversation others
/// can read; in a private place the same fetch asks without the clause; and a
/// public URL asks in neither, its reason unchanged. Planned and decided as
/// the gate does (the policy, then `places::private_fetch`): nothing connects.
#[test]
fn a_private_address_in_a_shared_place_says_so_on_its_card() {
    use crate::places::{private_fetch, PlaceClass};
    let policy = open_policy();
    let fetch = Fetch(Web::new(
        &WebToolsConfig::default(),
        30_000,
        CpuPool::new(1),
        Default::default(),
    ));
    let c = ctx(false);
    let decided = |class, url: &str| {
        let plan = fetch.plan(&json!({ "url": url }), &c).unwrap();
        private_fetch(class, &plan, policy.decide(&fetch, &plan))
    };
    for (url, why) in [
        (
            "http://127.0.0.1:7455/notes",
            "127.0.0.1 is a loopback address",
        ),
        ("http://10.0.0.1/", "10.0.0.1 is a private address"),
    ] {
        let asks = format!(
            "fetch {url}: http.fetch — approve ({why}, and a private address waits for approval"
        );
        let shared = decided(PlaceClass::Shared, url);
        assert_eq!(shared.posture, Posture::Approve, "{url}");
        assert_eq!(
            shared.reason,
            format!(
                "{asks}; this is a shared place, so the page joins a conversation others can read)"
            )
        );
        let private = decided(PlaceClass::Private, url);
        assert_eq!(private.posture, Posture::Approve, "{url}");
        assert_eq!(private.reason, format!("{asks})"));
    }
    let public = "https://doc.rust-lang.org/std/";
    let alone = policy.decide(&fetch, &fetch.plan(&json!({ "url": public }), &c).unwrap());
    let shared = decided(PlaceClass::Shared, public);
    assert_eq!(shared.posture, Posture::Open);
    assert_eq!(
        shared.reason, alone.reason,
        "a public URL's reason is unchanged"
    );
}

/// A board holding `brave_api_key`'s value.
fn board(value: &str) -> Arc<SecretBoard> {
    let b = SecretBoard::new(["brave_api_key".to_string()], Instant::now());
    b.publish(
        [("brave_api_key".to_string(), Ok(Secret::new(value.into())))].into(),
        "test",
    );
    b
}

/// A call's context with the broker `cfg` over `board`, which granted
/// web.search `brave_api_key`, bound to a call at `ran_at`.
fn keyed(cfg: &BrokerConfig, board: Arc<SecretBoard>, ran_at: Posture) -> (Arc<Broker>, ToolCtx) {
    let broker = Arc::new(Broker::new(cfg, board, None));
    broker.grant_tool("web.search", "brave_api_key");
    let bound = Bound {
        broker: broker.clone(),
        tool: "web.search".into(),
        ran_at,
        handed: Mutex::default(),
    };
    let c = ToolCtx {
        secrets: Some(Arc::new(bound)),
        ..ctx(false)
    };
    (broker, c)
}

fn key(value: &str) -> (Arc<Broker>, ToolCtx) {
    keyed(&BrokerConfig::default(), board(value), Posture::Notify)
}

async fn search(w: &Arc<Web>, c: &ToolCtx, input: Value) -> AsyncResult {
    let tool = Search(w.clone());
    tool.plan(&input, c).expect("the input plans");
    tool.run_async(&input, c).await
}

#[tokio::test]
async fn a_search_reads_braves_results_as_a_ranked_list() {
    let s = serve().await;
    let w = web(s.port, WebToolsConfig::default(), true);
    let (broker, c) = key("tv-good");
    let input = json!({"query": "rust ignore WalkParallel", "count": 3});
    let (out, ext) = search(&w, &c, input).await.unwrap();
    assert_eq!(
        out.text,
        "Brave Search: 3 results for \"rust ignore WalkParallel\"\n\n\
         1. WalkParallel in ignore - Rust\n   \
         https://docs.rs/ignore/latest/ignore/struct.WalkParallel.html\n   \
         WalkParallel is a parallel recursive directory iterator over files paths in one or \
         more directories.\n\n\
         2. ignore - crates.io: Rust Package Registry\n   \
         https://crates.io/crates/ignore\n   \
         A fast library for efficiently matching ignore files such as .gitignore against file \
         paths.\n\n\
         3. ripgrep's ignore crate\n   \
         https://github.com/BurntSushi/ripgrep/tree/master/crates/ignore\n"
    );
    let request = format!(
        "http://search.test:{}/search?q=rust+ignore+WalkParallel&count=3",
        s.port
    );
    assert_eq!(ext.unwrap().url, request);
    assert_eq!(out.meta["results"], json!(3));
    // The key goes in its header, and nowhere else.
    let hit = s.hits.lock().unwrap()[0].clone();
    assert_eq!(hit.path, "/search?q=rust+ignore+WalkParallel&count=3");
    assert_eq!(hit.headers["x-subscription-token"], "tv-good");
    assert_eq!(hit.headers["accept"], "application/json");
    assert!(!out.text.contains("tv-good"));
    // Health counts the grant's use.
    let grant = broker
        .status()
        .into_iter()
        .find(|g| g.to == "web.search")
        .unwrap();
    assert_eq!((grant.secret.as_str(), grant.uses), ("brave_api_key", 1));
    let (_, c) = key("tv-none");
    let (out, _) = search(&w, &c, json!({"query": "nothing"})).await.unwrap();
    assert_eq!(out.text, "Brave Search found no results for \"nothing\".");
}

#[tokio::test]
async fn no_key_a_refused_key_and_a_rate_limit_are_results_the_model_reads() {
    let s = serve().await;
    let w = web(s.port, WebToolsConfig::default(), true);
    let q = json!({"query": "rust"});
    let msg = |r: AsyncResult| match r {
        Ok((o, _)) => panic!("searched: {}", o.text),
        Err(f) => f.message,
    };
    let (_, c) = keyed(
        &BrokerConfig::default(),
        SecretBoard::empty(),
        Posture::Notify,
    );
    assert_eq!(
        msg(search(&w, &c, q.clone()).await),
        "web.search has no key: brave_api_key is not a configured secret. Its key is the Brave \
         Search API key, the [secrets] entry brave_api_key; the operator adds it there."
    );
    // A key held stricter than the call ran at is not handed to it.
    let strict: BrokerConfig =
        toml::from_str("[secrets.brave_api_key]\nposture = \"approve\"\n").unwrap();
    let (_, c) = keyed(&strict, board("tv-good"), Posture::Notify);
    let why = msg(search(&w, &c, q.clone()).await);
    assert!(
        why.starts_with(
            "web.search has no key: brave_api_key's posture is approve, and this call ran at \
             notify."
        ),
        "{why}"
    );
    let (_, c) = key("tv-bad");
    assert_eq!(
        msg(search(&w, &c, q.clone()).await),
        "The Brave Search API refused the key: 401 Unauthorized (SUBSCRIPTION_TOKEN_INVALID). \
         The operator checks the [secrets] entry brave_api_key."
    );
    let (_, c) = key("tv-busy");
    let busy = msg(search(&w, &c, q.clone()).await);
    assert_eq!(
        busy,
        "The Brave Search API is limiting this key's requests: 429 Too Many Requests \
         (RATE_LIMITED). Search again later."
    );
    // Only Brave's code comes through, never the answer's free text.
    assert!(!busy.contains("detail-7c1e"));
    // Only the two searches that were handed a key reached the API.
    assert_eq!(s.paths().len(), 2);
}
