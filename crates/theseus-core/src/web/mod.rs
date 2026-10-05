//! The web tools (DD5, spec §3.24): `http.fetch` and `web.search`.
//!
//! They wait on the network, so they are async tools on the workspace's
//! reqwest (`Backend::Async`): a call is a future on the daemon's runtime and
//! holds no core while it waits. Only turning a big page into text takes a
//! core, from F3's pool (`fetch::INLINE_HTML_BYTES` says where the line is).
//! Both are class `Read`, so the fetches and searches of one response run
//! together (F3's barrier rule).
//!
//! What they read is external text (§5.2): each result node says so, with the
//! URL it came from.

use std::sync::{Arc, OnceLock};

use theseus_tools::{Tool, ToolFailure};

use crate::config::WebToolsConfig;
use crate::cpu::CpuPool;

pub mod fetch;
pub mod html;
pub mod net;
pub mod search;

/// The web tools' names, for the config's check of `[policy.tools]`.
pub const NAMES: [&str; 2] = ["http.fetch", "web.search"];

/// Where `web.search` asks.
pub const BRAVE_ENDPOINT: &str = "https://api.search.brave.com/res/v1/web/search";

/// What the web tools share: their limits, their clients, and the pool that
/// turns a big page into text.
pub struct Web {
    pub cfg: WebToolsConfig,
    /// Characters of a result the model sees (`[tools] result_max_chars`):
    /// a page's text is cut to fit, its head kept.
    pub text_max: usize,
    cpu: Arc<CpuPool>,
    dns: net::Dns,
    /// Built at the first call, so none is on the start path.
    public: OnceLock<Result<reqwest::Client, String>>,
    approved: OnceLock<Result<reqwest::Client, String>>,
    /// Where `web.search` asks: Brave's API, or a test's server.
    pub search_endpoint: String,
    /// `[policy] private_addresses`: open, a fetch reaches a private address
    /// as any other (theseus-7gir.20).
    pub private: net::PrivateAddresses,
}

impl Web {
    pub fn new(
        cfg: &WebToolsConfig,
        text_max: usize,
        cpu: Arc<CpuPool>,
        private: net::PrivateAddresses,
    ) -> Arc<Self> {
        let dns = net::Dns::checked();
        Self::with_dns(cfg, text_max, cpu, dns, BRAVE_ENDPOINT, private)
    }

    /// The web tools over `dns`, asking `search_endpoint` (tests: names that
    /// resolve to a test's server, and that server).
    pub fn with_dns(
        cfg: &WebToolsConfig,
        text_max: usize,
        cpu: Arc<CpuPool>,
        dns: net::Dns,
        search_endpoint: &str,
        private: net::PrivateAddresses,
    ) -> Arc<Self> {
        Arc::new(Self {
            cfg: cfg.clone(),
            text_max,
            cpu,
            dns,
            public: OnceLock::new(),
            approved: OnceLock::new(),
            search_endpoint: search_endpoint.into(),
            private,
        })
    }

    /// `http.fetch` and `web.search`.
    pub fn tools(self: &Arc<Self>) -> Vec<Arc<dyn Tool>> {
        vec![
            Arc::new(fetch::Fetch(self.clone())),
            Arc::new(search::Search(self.clone())),
        ]
    }

    /// The client a request goes out on. Every name it resolves is checked
    /// (`Dns`), except on the one that reaches the private host the operator
    /// approved.
    fn client(&self, approved: bool) -> Result<&reqwest::Client, ToolFailure> {
        let (cell, check) = if approved {
            (&self.approved, false)
        } else {
            (&self.public, true)
        };
        let dns = net::Dns {
            check,
            ..self.dns.clone()
        };
        cell.get_or_init(|| build(dns))
            .as_ref()
            .map_err(|e| ToolFailure::new(e.clone()))
    }
}

fn build(dns: net::Dns) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        // A fetch judges each hop before it follows it, so the client
        // follows none itself.
        .redirect(reqwest::redirect::Policy::none())
        // A proxy would resolve names itself, past the resolver's check.
        .no_proxy()
        .dns_resolver(Arc::new(dns))
        .user_agent(concat!(
            "Theseus/",
            env!("CARGO_PKG_VERSION"),
            " (+https://github.com/zeroaltitude/theseus)"
        ))
        .build()
        .map_err(|e| format!("the web client did not start: {e}"))
}

/// A response's body, up to `cap` bytes, and whether there was more.
async fn body(resp: &mut reqwest::Response, cap: usize) -> Result<(Vec<u8>, bool), reqwest::Error> {
    let mut out = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        let room = cap - out.len();
        if chunk.len() > room {
            out.extend_from_slice(&chunk[..room]);
            return Ok((out, true));
        }
        out.extend_from_slice(&chunk);
    }
    Ok((out, false))
}

/// Why a request failed, in one line: each distinct cause, outermost first.
fn causes(e: &(dyn std::error::Error + 'static)) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut at = Some(e);
    while let Some(e) = at {
        let s = e.to_string();
        if !parts.iter().any(|p| p.contains(&s)) {
            parts.push(s);
        }
        at = e.source();
    }
    // reqwest's own words name the URL, which the result already does.
    if parts.len() > 1 && parts[0].starts_with("error sending request") {
        parts.remove(0);
    }
    let line = parts.join(": ");
    match line.char_indices().nth(300) {
        Some((i, _)) => format!("{}…", &line[..i]),
        None => line,
    }
}

/// `200 OK`, `429 Too Many Requests`.
fn status_line(s: reqwest::StatusCode) -> String {
    match s.canonical_reason() {
        Some(r) => format!("{} {r}", s.as_u16()),
        None => s.as_u16().to_string(),
    }
}

#[cfg(test)]
pub(crate) mod tests;
