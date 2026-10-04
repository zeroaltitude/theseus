//! `McpTool` (M7 §2.1, step 36b): one tool an MCP server lists, behind the
//! one `Tool` contract, as an async call (it waits on another process, as
//! `http.fetch` waits on the network).
//!
//! - Its canonical name is `mcp:<server>/<tool>`, which the gate resolves
//!   (`[policy.mcp]`); the model calls it by its wire name,
//!   `mcp__<server>__<tool>` (`theseus_mcp::names`).
//! - It is `Run` and `NonRepeatable` unless the operator lists it in the
//!   server's `read`, which makes it `Read` and `SafeToRepeat`. The server's
//!   own hints are shown on every surface and loosen nothing: the server is
//!   untrusted, and `Read` would let a call past T1's hold.
//! - Its result is outside text unless the server is `external = false`:
//!   a session that reads one holds (T1). So is an error result, which is
//!   the server's text too.
//! - A call to a server that is not up waits for that server alone, then
//!   fails `mcp_unavailable`; a call whose connection ended or timed out is
//!   `outcome_unknown`, and nothing runs it again.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use theseus_mcp::types::Tool as Listed;
use theseus_tools::{
    AsyncRun, Backend, External, Plan, Retry, Tool, ToolClass, ToolCtx, ToolFailure, ToolOutput,
};

use super::Server;

/// The most of a tool's description the model reads: the server's text is
/// in every turn's definitions.
pub const DESCRIPTION_MAX: usize = 2_000;

/// A description cut to [`DESCRIPTION_MAX`] characters, saying so.
pub fn cap_description(d: &mut String) {
    if let Some((i, _)) = d.char_indices().nth(DESCRIPTION_MAX) {
        d.truncate(i);
        d.push_str(" …[cut at 2,000 characters]");
    }
}

pub struct McpTool {
    pub(super) server: Arc<Server>,
    pub listed: Listed,
    /// `mcp:<server>/<tool>`.
    pub canonical: String,
    /// `mcp__<server>__<tool>`, unique on the board.
    pub wire: String,
    pub description: String,
    read: bool,
}

impl McpTool {
    pub(super) fn new(server: Arc<Server>, listed: Listed, wire: String) -> Self {
        let canonical = theseus_mcp::names::canonical(&server.name, &listed.name);
        let mut description = listed
            .description
            .clone()
            .or_else(|| listed.title.clone())
            .unwrap_or_else(|| format!("{} (no description given)", listed.name));
        cap_description(&mut description);
        let read = server.cfg.read.contains(&listed.name);
        Self {
            server,
            listed,
            canonical,
            wire,
            description,
            read,
        }
    }

    pub fn server(&self) -> &str {
        &self.server.name
    }

    /// The server's own hints, shown and never acted on.
    pub fn hints(&self) -> Vec<String> {
        let Some(a) = &self.listed.annotations else {
            return Vec::new();
        };
        [
            (a.read_only_hint, "read-only"),
            (a.destructive_hint, "destructive"),
            (a.idempotent_hint, "idempotent"),
            (a.open_world_hint, "open-world"),
        ]
        .iter()
        .filter_map(|(v, name)| {
            v.map(|b| match b {
                true => name.to_string(),
                false => format!("not {name}"),
            })
        })
        .collect()
    }

    fn start_wait(&self) -> Duration {
        Duration::from_secs(self.server.cfg.start_timeout_secs)
    }

    fn call_timeout(&self) -> Duration {
        Duration::from_secs(self.server.cfg.call_timeout_secs)
    }
}

impl Tool for McpTool {
    fn name(&self) -> &str {
        &self.canonical
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        // A provider wants an object's schema; a server that gave none, or
        // one without its type, gets `object`.
        let mut s = self.listed.input_schema.clone();
        match s.as_object_mut() {
            Some(o) => {
                o.entry("type").or_insert_with(|| json!("object"));
                s
            }
            None => json!({"type": "object"}),
        }
    }

    fn class(&self) -> ToolClass {
        match self.read {
            true => ToolClass::Read,
            false => ToolClass::Run,
        }
    }

    fn backend(&self) -> Backend {
        Backend::Async
    }

    fn retry(&self) -> Retry {
        match self.read {
            true => Retry::SafeToRepeat,
            false => Retry::NonRepeatable,
        }
    }

    fn family(&self) -> &str {
        "mcp"
    }

    fn wire_name(&self) -> String {
        self.wire.clone()
    }

    /// The wait for its server, then the call, within the in-process
    /// deadline's reach.
    fn deadline(&self) -> Option<Duration> {
        Some(self.start_wait() + self.call_timeout() + Duration::from_secs(5))
    }

    /// The gate decides by name, as it always does: the plan names the
    /// server and the tool, and reads no meaning into the arguments.
    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        if !(input.is_object() || input.is_null()) {
            return Err(format!(
                "{}'s input must be an object, as its schema says",
                self.wire
            ));
        }
        Ok(Plan {
            summary: format!("{} {}", self.canonical, super::short(input, 120)),
            ..Plan::default()
        })
    }

    fn run_async(&self, input: &Value, _ctx: &ToolCtx) -> AsyncRun {
        let server = self.server.clone();
        let name = self.listed.name.clone();
        let canonical = self.canonical.clone();
        let args = input.clone();
        let (wait, timeout) = (self.start_wait(), self.call_timeout());
        Box::pin(async move {
            let external = server.cfg.external.then(|| External {
                url: canonical.clone(),
            });
            let client = server.client(wait).await.map_err(|why| ToolFailure {
                message: format!(
                    "mcp_unavailable: {why}. Nothing was sent; it may be called again."
                ),
                meta: json!({"mcp_unavailable": true, "server": server.name}),
            })?;
            server
                .calls
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let opts = theseus_mcp::client::CallOptions {
                timeout: Some(timeout),
                ..Default::default()
            };
            let started = std::time::Instant::now();
            let r = client.call_tool_with(&name, args, &opts).await;
            let ms = started.elapsed().as_millis() as u64;
            match r {
                Ok(r) => {
                    let text = r.text_for_model();
                    let meta = json!({"server": server.name, "tool": name, "mcp_ms": ms,
                        "is_error": r.is_error});
                    if r.is_error {
                        server
                            .errors
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        // The server's own words, still outside text.
                        let mut meta = meta;
                        if let Some(e) = &external {
                            meta["external"] = json!(e.url);
                        }
                        return Err(ToolFailure {
                            message: format!("{canonical} answered an error: {text}"),
                            meta,
                        });
                    }
                    Ok((ToolOutput { text, meta }, external))
                }
                Err(e) => {
                    server
                        .errors
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let unknown = e.outcome_unknown();
                    let then = match (unknown, e) {
                        (true, e) => format!(
                            "{e}. Whether it ran at the server is unknown: check before calling it again."
                        ),
                        (false, e @ theseus_mcp::Error::Unreachable(_)) => {
                            format!("{e}. Nothing was sent.")
                        }
                        (false, e) => e.to_string(),
                    };
                    Err(ToolFailure {
                        message: format!("{canonical}: {then}"),
                        meta: json!({"server": server.name, "tool": name, "mcp_ms": ms,
                            "outcome_unknown": unknown}),
                    })
                }
            }
        })
    }
}
