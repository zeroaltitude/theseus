//! What a loaded extension was used for (theseus-pw1q.13): its calls, its
//! failures and its call latency over a window, counted from the ledger's
//! tool-call rows. Nothing counts on the call path and no row is written per
//! call: the read is by kind through the index's pages (`action.succeeded`,
//! `action.failed`, `action.outcome_unknown`), at most [`MAX_ROWS`] a read.
//!
//! A call's row names its tool as the producer, `async:mcp:ext-<name>/<tool>`
//! (an MCP tool is an async tool). A call **fails** when it ended in an error
//! result, a timeout or a crash of the server (`action.failed`: a connection
//! that ended is `outcome_unknown` in the result's meta and still settles
//! failed), or when a restart found it dispatched and could not say how it
//! ended (`action.outcome_unknown`, whose producer is the reconciler's, so the
//! action names the tool). An outcome that may have run is never a success.

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::{bail, Result};
use theseus_protocol::extend::ExtendUsage;
use theseus_store::pages::ledger_kind;
use theseus_store::{kinds, Page};

use super::SERVER_PREFIX;
use crate::ledger::LedgerRow;
use crate::rpc::Core;

/// Rows decoded in one read, at most; past it the counts are a floor.
pub const MAX_ROWS: usize = 300_000;
/// A read for `extend.list` is reused for this long (ms).
pub const CACHE_MS: u64 = 60_000;
const DAY_MS: u64 = 86_400_000;

/// The rows a read follows.
const KINDS: &[&str] = &[
    "action.succeeded",
    "action.failed",
    "action.outcome_unknown",
];

/// One call that ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Call {
    pub at_ms: u64,
    pub ms: u64,
    pub failed: bool,
}

/// Every call of every extension that ended in a read's window, by name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Calls {
    pub by_name: BTreeMap<String, Vec<Call>>,
    /// The read stopped at [`MAX_ROWS`].
    pub truncated: bool,
}

impl Calls {
    /// `name`'s use over the calls that ended at or after `since_ms`, for a
    /// window of `window_days` days.
    pub fn usage(&self, name: &str, since_ms: u64, window_days: u32) -> ExtendUsage {
        let mut ms: Vec<u64> = Vec::new();
        let mut failures = 0u64;
        for c in self
            .by_name
            .get(name)
            .into_iter()
            .flatten()
            .filter(|c| c.at_ms >= since_ms)
        {
            ms.push(c.ms);
            failures += u64::from(c.failed);
        }
        ms.sort_unstable();
        ExtendUsage {
            window_days,
            calls: ms.len() as u64,
            failures,
            p50_ms: percentile(&ms, 50),
            p95_ms: percentile(&ms, 95),
            truncated: self.truncated,
        }
    }
}

/// The nearest-rank percentile of sorted `ms`; 0 when there are none.
fn percentile(ms: &[u64], p: usize) -> u64 {
    if ms.is_empty() {
        return 0;
    }
    let rank = (p * ms.len()).div_ceil(100).max(1);
    ms[rank.min(ms.len()) - 1]
}

/// The extension a tool name belongs to: `mcp:ext-<name>/<tool>` or a
/// producer's `async:` form of it.
pub fn name_of(tool: &str) -> Option<&str> {
    let t = tool.strip_prefix("async:").unwrap_or(tool);
    let rest = t
        .strip_prefix(crate::policy::MCP_PREFIX)?
        .strip_prefix(SERVER_PREFIX)?;
    let (name, tool) = rest.split_once('/')?;
    (!name.is_empty() && !tool.is_empty()).then_some(name)
}

impl Core {
    /// Every extension call that ended in `(now - window, now]`, by
    /// extension name, from the ledger's rows by kind.
    pub fn extension_calls(&self, now_ms: u64, window_days: u32) -> Result<Calls> {
        self.tool_calls(now_ms, window_days, |t| name_of(t).map(str::to_string))
    }

    /// Every call that ended in `(now - window, now]` and that `name_of` names
    /// (by whatever key it returns), counted as an extension's are (theseus-pw1q.21.8:
    /// the retire tender counts a flow's the same way).
    pub fn tool_calls(
        &self,
        now_ms: u64,
        window_days: u32,
        name_of: fn(&str) -> Option<String>,
    ) -> Result<Calls> {
        let since = now_ms.saturating_sub(u64::from(window_days) * DAY_MS);
        let tags: Vec<String> = KINDS.iter().map(|k| ledger_kind(k)).collect();
        let mut out = Calls::default();
        let (mut before, mut seen) = (None, 0usize);
        loop {
            let page = Page {
                kind: kinds::LEDGER,
                tags: tags.clone(),
                after: None,
                before,
                since_ms: Some(since),
                until_ms: Some(now_ms),
                limit: 512,
            };
            let Some(got) = self.store.ledger_page(&page)? else {
                bail!("the ledger's index is still being built");
            };
            for r in got.records.iter().rev() {
                seen += 1;
                let Ok(row) = r.decode::<LedgerRow>() else {
                    continue;
                };
                if row.at_unix_ms < since || row.at_unix_ms > now_ms {
                    continue;
                }
                if let Some((name, call)) = self.counted_call(&row, name_of) {
                    out.by_name.entry(name).or_default().push(call);
                }
            }
            match (got.more, got.first) {
                (true, Some(first)) if seen < MAX_ROWS => before = Some(first),
                (true, _) => {
                    out.truncated = true;
                    break;
                }
                _ => break,
            }
        }
        Ok(out)
    }

    /// The call one row ends, if `name_of` names its tool.
    fn counted_call(
        &self,
        row: &LedgerRow,
        name_of: fn(&str) -> Option<String>,
    ) -> Option<(String, Call)> {
        let d = &row.data;
        let tool = match row.kind.as_str() {
            "action.outcome_unknown" => {
                let a = self.kernel.action(d["correlation_id"].as_str()?).ok()??;
                a.tool
            }
            _ => d["producer"].as_str()?.to_string(),
        };
        let name = name_of(&tool)?;
        let failed = row.kind != "action.succeeded";
        Some((
            name,
            Call {
                at_ms: row.at_unix_ms,
                ms: d["duration_ms"].as_u64().unwrap_or(0),
                failed,
            },
        ))
    }

    /// `name`'s use over the last `window_days` days (theseus-pw1q.13).
    pub fn extension_usage(
        &self,
        name: &str,
        now_ms: u64,
        window_days: u32,
    ) -> Result<ExtendUsage> {
        let calls = self.extension_calls(now_ms, window_days)?;
        Ok(calls.usage(
            name,
            now_ms.saturating_sub(u64::from(window_days) * DAY_MS),
            window_days,
        ))
    }

    /// The read `extend.list` shows, reused for [`CACHE_MS`].
    pub(crate) fn extension_calls_cached(
        &self,
        now_ms: u64,
        window_days: u32,
    ) -> Result<Arc<Calls>> {
        let mut cache = self
            .tools
            .extend
            .usage_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((at, w, calls)) = cache.as_ref() {
            if *w == window_days && now_ms.saturating_sub(*at) < CACHE_MS {
                return Ok(calls.clone());
            }
        }
        let calls = Arc::new(self.extension_calls(now_ms, window_days)?);
        *cache = Some((now_ms, window_days, calls.clone()));
        Ok(calls)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tool_name_names_its_extension() {
        assert_eq!(name_of("async:mcp:ext-wordcount/count"), Some("wordcount"));
        assert_eq!(name_of("mcp:ext-word_count/count"), Some("word_count"));
        assert_eq!(name_of("mcp:fake/echo"), None, "a configured server");
        assert_eq!(name_of("async:http.fetch"), None);
        assert_eq!(name_of("mcp:ext-/count"), None);
    }

    #[test]
    fn percentiles_are_the_nearest_rank() {
        assert_eq!(percentile(&[], 50), 0);
        assert_eq!(percentile(&[7], 95), 7);
        let ms: Vec<u64> = (1..=20).collect();
        assert_eq!(percentile(&ms, 50), 10);
        assert_eq!(percentile(&ms, 95), 19);
    }
}
