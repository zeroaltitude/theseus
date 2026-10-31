//! The tools a place is offered, built once (theseus-nsp9l.2). Every turn made the catalog again, each tool's
//! description and schema as JSON, 100 KB of it, before its request could leave; a place's catalog changes only when
//! the registry, the MCP servers' tools, the loaded flows, the public roots or the wait cap do, so it is kept under
//! those. The MCP list and the flows are shared lists that a change swaps whole, so a kept catalog names the lists it
//! was made from, which keeps them alive, and a swap cannot be mistaken for the old one.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::{json, Value};
use theseus_tools::Tool;

use super::{jobs_tools, ToolRuntime};
use crate::ceiling::PlaceView;
use crate::flow;
use crate::mcp::McpTool;
use crate::places::PlaceClass;
use crate::tool_set::Tools;

/// The catalogs kept: a private place, a shared one, and the few that have a ceiling.
const KEPT: usize = 8;

/// What a place's catalog was made from.
struct Key {
    class: PlaceClass,
    /// A ceiling is leaked once and never freed, so its address names it.
    ceiling: usize,
    mcp: Arc<Vec<Arc<McpTool>>>,
    flows: Vec<Arc<flow::load::Entry>>,
    roots: Vec<PathBuf>,
    wait_cap: u64,
    registered: usize,
}

impl Key {
    fn is(&self, other: &Key) -> bool {
        self.class == other.class
            && self.ceiling == other.ceiling
            && Arc::ptr_eq(&self.mcp, &other.mcp)
            && self.flows.len() == other.flows.len()
            && self
                .flows
                .iter()
                .zip(&other.flows)
                .all(|(a, b)| Arc::ptr_eq(a, b))
            && self.roots == other.roots
            && self.wait_cap == other.wait_cap
            && self.registered == other.registered
    }
}

/// The catalogs a runtime has made, newest last.
#[derive(Default)]
pub struct Catalogs {
    kept: Mutex<Vec<(Key, Tools)>>,
    built: AtomicU64,
}

impl Catalogs {
    /// How many catalogs this runtime has made (a test's count: a plain turn makes none that exists already).
    pub fn built(&self) -> u64 {
        self.built.load(Ordering::Relaxed)
    }
}

impl ToolRuntime {
    /// The tools a place is offered (the place rule, narrowed by its
    /// ceiling): its model never sees one it may not use.
    pub fn definitions_for(&self, place: PlaceView) -> Tools {
        let key = Key {
            class: place.class,
            ceiling: place.ceiling.map_or(0, |c| std::ptr::from_ref(c) as usize),
            mcp: self.mcp.all(),
            flows: self.flows.all(),
            roots: self.public_roots.clone(),
            wait_cap: self.job_wait_max_secs,
            registered: self.registry.len(),
        };
        let mut kept = self
            .catalogs
            .kept
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some((_, t)) = kept.iter().find(|(k, _)| k.is(&key)) {
            return t.clone();
        }
        let tools = Tools::from(self.built_for(place, &key));
        self.catalogs.built.fetch_add(1, Ordering::Relaxed);
        if kept.len() >= KEPT {
            kept.remove(0);
        }
        kept.push((key, tools.clone()));
        tools
    }

    fn built_for(&self, place: PlaceView, key: &Key) -> Vec<Value> {
        let offered = |name: &str| place.offered(name, &self.public_roots);
        let mut defs = self
            .registry
            .definitions_of(true, |n| offered(n) && crate::tools_defer::listed(n));
        jobs_tools::say_wait_cap(&mut defs, self.job_wait_max_secs);
        super::proc_cap::say_proc_cap(
            &mut defs,
            self.ctx.proc_timeout_secs,
            self.ctx.proc_timeout_max_secs,
        );
        // The MCP servers' tools, after the built-ins, by canonical name (the
        // catalog's order): none in a shared place (the place rule), and in a
        // place with a ceiling only its `mcp:<server>` entries' (38a).
        if self.enabled() {
            defs.extend(key.mcp.iter().filter(|t| offered(&t.canonical)).map(|t| {
                json!({
                    "name": t.wire,
                    "description": t.description,
                    "input_schema": Tool::input_schema(t.as_ref()),
                })
            }));
            // The loaded flows' tools, in their own deferred families; the
            // turn leaves out those not in its directory's scope.
            defs.extend(
                key.flows
                    .iter()
                    .filter(|e| offered(e.tool.name()))
                    .map(|e| {
                        json!({
                            "name": e.tool.wire_name(),
                            "description": e.tool.description(),
                            "input_schema": e.tool.input_schema(),
                        })
                    }),
            );
        }
        defs
    }
}
