//! Stand-ins for the two tools whose results come from outside, registered
//! over the built-ins (`Parts::toollets`), so a run needs no network and no
//! sandbox. Each runs in process and returns at once, with an atom:
//! - `http.fetch`: a page, outside text that anyone may read; its result is
//!   marked external, so the core labels it untrusted and public and its
//!   session holds external text (T1);
//! - `proc.run`: what a job returned. With `egress: true` it is what 18c's L1
//!   job that connected out returns: marked external, so the core labels it
//!   untrusted and the owner's (`labels::for_result` by the tool's name), and
//!   the session holds it `via: egress`. Without, the owner's, trusted.
//!
//! What they stand in for is the label at write and the hold; the job's own
//! path (the wrapper, the proxy, a late result) is 17b's and 18c's to prove.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use theseus_tools::{
    AsyncRun, Backend, External, Plan, Retry, Tool, ToolClass, ToolCtx, ToolOutput,
};

use super::atoms::{Origin, R};
use super::world::Shared;

/// Where a stand-in job "connected out" to.
const EGRESS_TO: &str = "api.example.invalid:443";

pub struct Fetch {
    pub shared: Arc<Mutex<Shared>>,
}

impl Tool for Fetch {
    fn name(&self) -> &'static str {
        "http.fetch"
    }
    fn description(&self) -> &'static str {
        "Fetch a page (the disclosure simulator's stand-in): its text, from outside."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {"url": {"type": "string"}}, "required": ["url"]})
    }
    fn class(&self) -> ToolClass {
        ToolClass::Read
    }
    fn backend(&self) -> Backend {
        Backend::Async
    }
    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }
    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let url = input["url"].as_str().ok_or("no url")?;
        Ok(Plan {
            url: Some(url.to_string()),
            summary: format!("fetch {url}"),
            ..Default::default()
        })
    }
    fn run_async(&self, input: &Value, _ctx: &ToolCtx) -> AsyncRun {
        let url = input["url"].as_str().unwrap_or("").to_string();
        let m = mint(&self.shared, R::Public, Origin::Page);
        Box::pin(async move {
            let text = format!("The page at {url} says {m}.");
            Ok((
                ToolOutput {
                    text,
                    meta: json!({}),
                },
                Some(External { url }),
            ))
        })
    }
}

pub struct Run {
    pub shared: Arc<Mutex<Shared>>,
}

impl Tool for Run {
    fn name(&self) -> &'static str {
        "proc.run"
    }
    fn description(&self) -> &'static str {
        "Run a job (the disclosure simulator's stand-in): `egress: true` connects out."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {
            "argv": {"type": "array", "items": {"type": "string"}},
            "egress": {"type": "boolean"}
        }, "required": ["argv"]})
    }
    fn class(&self) -> ToolClass {
        ToolClass::Run
    }
    fn backend(&self) -> Backend {
        Backend::Async
    }
    fn retry(&self) -> Retry {
        Retry::NonRepeatable
    }
    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let argv: Vec<String> = input["argv"]
            .as_array()
            .ok_or("no argv")?
            .iter()
            .filter_map(|a| a.as_str().map(str::to_string))
            .collect();
        Ok(Plan {
            summary: format!("run {}", argv.join(" ")),
            argv: Some(argv),
            ..Default::default()
        })
    }
    fn run_async(&self, input: &Value, _ctx: &ToolCtx) -> AsyncRun {
        let egress = input["egress"].as_bool().unwrap_or(false);
        let origin = if egress { Origin::Egress } else { Origin::Run };
        let m = mint(&self.shared, R::Owner, origin);
        Box::pin(async move {
            let output = ToolOutput {
                text: format!("exit 0\nthe job printed {m}"),
                meta: json!({"exit_code": 0}),
            };
            let external = egress.then(|| External {
                url: EGRESS_TO.into(),
            });
            Ok((output, external))
        })
    }
}

/// A new atom for a result, counted.
fn mint(shared: &Arc<Mutex<Shared>>, readers: R, origin: Origin) -> String {
    let mut s = shared
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if origin == Origin::Egress {
        s.egress += 1;
    }
    s.atoms.mint(readers, origin).1
}
