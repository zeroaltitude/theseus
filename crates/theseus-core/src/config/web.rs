//! `[tools.web]`: the limits of `http.fetch` and `web.search`, and the
//! secret that holds the search key (DD5). Moved from `config.rs`
//! (theseus-v73m), which re-exports it.

use serde::{Deserialize, Serialize};

/// `[tools.web]`: the limits of `http.fetch` and `web.search`, and the secret
/// that holds the search key (DD5).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebToolsConfig {
    /// A fetch's or a search's whole time, redirects included.
    #[serde(default = "default_web_timeout_secs")]
    pub timeout_secs: u64,
    /// The most bytes of a body a fetch reads; a call may ask for fewer.
    #[serde(default = "default_web_max_bytes")]
    pub max_bytes: usize,
    /// The `[secrets]` entry that holds the Brave Search API key.
    #[serde(default = "default_search_key_secret")]
    pub search_key_secret: String,
}

fn default_web_timeout_secs() -> u64 {
    30
}
fn default_web_max_bytes() -> usize {
    2 * 1024 * 1024
}
fn default_search_key_secret() -> String {
    "brave_api_key".into()
}

impl Default for WebToolsConfig {
    fn default() -> Self {
        Self {
            timeout_secs: default_web_timeout_secs(),
            max_bytes: default_web_max_bytes(),
            search_key_secret: default_search_key_secret(),
        }
    }
}

/// The longest a web call may take: every in-process call ends by the
/// runtime's deadline (120 s), so a web call's own timeout must come first.
pub const WEB_TIMEOUT_MAX_SECS: u64 = 110;
