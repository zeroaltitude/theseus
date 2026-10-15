//! The first request's budget in tokens, as sent (theseus-to0xw).
//!
//! A fresh session's first request is counted for a kind of place the way the
//! provider bills it: the deferred tools at their constant and the search tool
//! at its size (`ProviderRequest::census`), at the model's figures. It is the
//! number `theseus census --place <kind>` prints as the request's total, and
//! it is deterministic: nothing is sent and no network is read. It runs in the
//! bench and in `census`, never on a turn's path.

use anyhow::Result;
use serde_json::Value;

use crate::catalog::TokenRates;
use crate::tools_defer::{FIND, SEARCH_TOOL};

/// What a fresh session's first request is on a kind of place.
#[derive(Debug, Clone, PartialEq)]
pub struct FirstRequest {
    /// The whole request, in tokens as sent.
    pub tokens: u64,
    /// The tools it loads: not deferred, and neither the search tool nor `tools_find`.
    pub loaded: Vec<String>,
}

impl crate::rpc::Core {
    /// The first request of a fresh session on `place`, counted as the census counts it.
    pub fn first_request(&self, place: &str) -> Result<FirstRequest> {
        let (live, _) = self.live_profile();
        let next = self.runner.census_request(None, place, &live)?;
        let rates = TokenRates::of(&next.target.model);
        let tokens = crate::census::census_parts(&next.compiled.request, &next, rates).total_tokens;
        let loaded = next
            .compiled
            .request
            .tools
            .iter()
            .filter(|t| t.get("defer_loading") != Some(&Value::Bool(true)))
            .filter_map(|t| t["name"].as_str())
            .filter(|n| *n != SEARCH_TOOL && *n != FIND)
            .map(str::to_string)
            .collect();
        Ok(FirstRequest { tokens, loaded })
    }
}
