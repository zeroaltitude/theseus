//! `tool.defer` and health's line (theseus-7m7w.2): what each kind of place
//! loads, why, and what its use and misses are. A read of the board's counts
//! and the registry; it writes nothing and takes no session's lock.

use std::collections::{BTreeMap, BTreeSet};

use theseus_protocol::defer::{DeferFamily, DeferHealth, DeferListResult, DeferPlace};

use super::family::{self, PlaceKind};
use super::select::DAY_MS;
use super::Mechanism;

/// What the provider's tool search adds to a request that defers tools.
pub const SEARCH_COST: u32 = (super::SEARCH_TOKENS + super::DEFERRED_TOKENS) as u32;

impl crate::rpc::Core {
    /// How the live profile's model reaches a hidden tool.
    fn defer_mechanism(&self) -> Mechanism {
        let (live, _) = self.live_profile();
        let model = self.cfg.all_profiles().get(&live).map(|p| p.model.as_str());
        match model.and_then(|m| self.runner.catalog.get(m)) {
            Some(e) if e.tool_search => Mechanism::Search,
            _ => Mechanism::Find,
        }
    }

    /// The tools of the catalog by family: wire names, the built-ins and the
    /// MCP servers' (without `tools_find`, which joins a request on its own).
    fn tools_by_family(&self) -> BTreeMap<String, Vec<String>> {
        let tools = &self.runner.tools;
        let mut wires: Vec<String> = tools
            .registry
            .all()
            .filter(|t| super::listed(t.name()))
            .map(|t| t.wire_name())
            .collect();
        wires.extend(tools.mcp.all().iter().map(|t| t.wire.clone()));
        let mut by: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for w in wires {
            by.entry(family::of_wire(&w)).or_default().push(w);
        }
        for v in by.values_mut() {
            v.sort();
        }
        by
    }

    /// The tokens of each family's tool definitions, at the live model's
    /// rate for tool JSON, and of `tools_find`'s.
    fn tokens_by_family(&self) -> (BTreeMap<String, u32>, u32) {
        let (live, _) = self.live_profile();
        let model = self
            .cfg
            .all_profiles()
            .get(&live)
            .map(|p| p.model.clone())
            .unwrap_or_default();
        let rate = self
            .runner
            .catalog
            .get(&model)
            .map_or(crate::catalog::TokenRates::of(&model), |e| {
                e.bytes_per_token
            })
            .json;
        let tokens = |v: &serde_json::Value| {
            (serde_json::to_string(v).map_or(0, |s| s.len()) as f64 / rate).ceil() as u32
        };
        let mut by: BTreeMap<String, u32> = BTreeMap::new();
        let defs = self
            .runner
            .tools
            .definitions_for(crate::places::PlaceClass::Private.into());
        for d in &defs {
            if let Some(n) = d.get("name").and_then(serde_json::Value::as_str) {
                *by.entry(family::of_wire(n)).or_insert(0) += tokens(d);
            }
        }
        (by, tokens(&super::find_definition()))
    }

    /// `tool.defer`.
    pub fn tool_defer(&self) -> DeferListResult {
        let cfg = &self.cfg.tools.defer;
        let enabled = cfg.enabled;
        let board = &self.runner.tools.defer;
        let now = self.kernel.now_ms();
        let by = self.tools_by_family();
        let known: BTreeSet<String> = by.keys().cloned().collect();
        let (cost, find_cost) = self.tokens_by_family();
        let mechanism = self.defer_mechanism();
        let (live, _) = self.live_profile();
        let since = now.saturating_sub(u64::from(cfg.window_days) * DAY_MS);
        let places = PlaceKind::ALL
            .iter()
            .map(|&kind| {
                let stats = board.stats(kind);
                let sel = self
                    .runner
                    .fresh_selection(kind, &live, &known)
                    .unwrap_or_else(|| board.select("", kind, cfg, now, &known, &BTreeSet::new()));
                let used = stats.sum_since(since, |d| &d.used);
                let searched = stats.sum_since(since, |d| &d.searched);
                let missed = stats.sum_since(since, |d| &d.missed);
                let unused = stats.sum_since(since, |d| &d.unused);
                let families: Vec<DeferFamily> = by
                    .iter()
                    .map(|(f, tools)| {
                        let why = sel.families.get(f);
                        DeferFamily {
                            family: f.clone(),
                            summary: family::summary(f).unwrap_or_default(),
                            tools: tools.clone(),
                            tokens: cost.get(f).copied().unwrap_or(0),
                            loaded: why.is_none_or(|w| w.loads()),
                            why: why.map(|w| w.words()).unwrap_or_default(),
                            used_turns: used.get(f).copied().unwrap_or(0),
                            searched: searched.get(f).copied().unwrap_or(0),
                            missed: missed.get(f).copied().unwrap_or(0),
                            unused_turns: unused.get(f).copied().unwrap_or(0),
                            last_used_ms: stats.last_used.get(f).copied(),
                        }
                    })
                    .collect();
                let all: u32 = families.iter().map(|f| f.tokens).sum();
                let held = families.iter().any(|f| !f.loaded);
                let sent: u32 = families
                    .iter()
                    .filter(|f| f.loaded)
                    .map(|f| f.tokens)
                    .sum::<u32>()
                    + match (held, mechanism) {
                        (false, _) => 0,
                        (true, Mechanism::Search) => SEARCH_COST,
                        (true, Mechanism::Find) => find_cost,
                    };
                DeferPlace {
                    place: kind.as_str().into(),
                    turns: stats.turns_since(since),
                    tokens_all: all,
                    tokens_sent: if enabled { sent } else { all },
                    families,
                }
            })
            .collect();
        DeferListResult {
            enabled: cfg.enabled,
            mechanism: self.defer_mechanism().as_str().into(),
            window_days: cfg.window_days,
            hide_after_days: cfg.hide_after_days,
            load_share: cfg.load_share,
            warmed: board.warmed(),
            places,
        }
    }

    /// Health's line.
    pub fn defer_health(&self) -> DeferHealth {
        let cfg = &self.cfg.tools.defer;
        let board = &self.runner.tools.defer;
        let since = self
            .kernel
            .now_ms()
            .saturating_sub(u64::from(cfg.window_days) * DAY_MS);
        let (mut turns, mut searched, mut missed) = (0, 0, 0);
        let mut by_family: BTreeMap<String, u32> = BTreeMap::new();
        for (_, st) in board.all_stats() {
            turns += st.turns_since(since);
            searched += st.sum_since(since, |d| &d.searched).values().sum::<u32>();
            for (f, n) in st.sum_since(since, |d| &d.missed) {
                missed += n;
                *by_family.entry(f).or_insert(0) += n;
            }
        }
        DeferHealth {
            enabled: cfg.enabled,
            mechanism: self.defer_mechanism().as_str().into(),
            warmed: board.warmed(),
            turns,
            searched,
            missed,
            top_miss: by_family
                .into_iter()
                .max_by_key(|(_, n)| *n)
                .map(|(f, _)| f),
        }
    }
}
