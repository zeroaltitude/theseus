//! `context.explain` (theseus-7n3e): what a turn sees, part by part, with
//! token counts, for the cockpit's context explorer.
//!
//! - **The parts are built, never compiled** (`context_parts`): the header's,
//!   each context file's and each category's guidance, as the session's next
//!   turn would build them with the memberships the asked turn's compilation
//!   recorded; then the tools, the recall the turn put in front of the model,
//!   and the conversation (the rest of the turn's estimate). Their digest
//!   against the compilation's says whether the turn saw these bytes.
//! - **Read only**: no frame, no row, no model call, no warning a turn would
//!   have given (the files are peeked). On the blocking pool: it reads the
//!   session's compiles and recall nodes through the store's index, a page
//!   each.
//! - **Text to a private place only**, as `import.sessions` gives it: any
//!   other surface reads the sizes, the digests and the labels.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, bail, Result};
use serde_json::Value;
use theseus_protocol::context::{
    ContextExplainParams, ContextExplainResult, ContextPart, ContextSource, ContextTurn,
};
use theseus_protocol::memory::RecallManifest;
use theseus_protocol::{error_code, CompilationInfo, ContextCompiled, LedgerKind, PlaceClass};
use theseus_store::pages::ledger_kind_session;
use theseus_store::{kinds, Page, Store as _};

use super::import::WITHHELD;
use super::server::{parse, Conn, RpcFailure};
use super::Core;
use crate::catalog::TokenRates;
use crate::compiler::{manifest_for, Compilation, Manifest};
use crate::context_parts::{json_tokens, text_tokens, Built};
use crate::ledger::LedgerRow;
use crate::node::Body;
use crate::session::SessionRecord;

/// The session's turns listed, newest first.
const TURNS: usize = 50;
/// The compiles read back, newest first: enough for the turns listed.
const COMPILES: usize = 600;
/// The recall nodes read back, newest first.
const RECALL_NODES: usize = 200;

/// Whether `rpc_prefixed` routes `name`: `context.*`.
pub(super) fn prefixed(name: &str) -> bool {
    name.starts_with("context.")
}

impl Core {
    /// `context.explain`, off the serving workers.
    pub(super) async fn rpc_context(
        self: Arc<Self>,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        let p: ContextExplainParams = parse(params)?;
        let private = conn.surface.reads_private();
        let core = self.clone();
        let r = tokio::task::spawn_blocking(move || core.context_explain(&p, private))
            .await
            .map_err(|e| RpcFailure::new(error_code::INTERNAL, e.to_string()))?
            .map_err(RpcFailure::invalid)?;
        serde_json::to_value(r).map_err(|e| RpcFailure::invalid(e.into()))
    }

    /// What `p`'s turn saw (its session's latest, by default), part by part.
    pub(crate) fn context_explain(
        &self,
        p: &ContextExplainParams,
        private: bool,
    ) -> Result<ContextExplainResult> {
        let t0 = Instant::now();
        let sid = p.session_id.as_str();
        let rec: SessionRecord = self
            .store
            .get_session(sid)?
            .ok_or_else(|| anyhow!("no session is named {sid}"))?;
        let mut out = blank(sid, &rec, private);
        // An imported session takes no turn: its episode, and no parts.
        if let Some(imp) = rec.imported.as_deref() {
            out.class = PlaceClass::Private.as_str().to_string();
            out.place = crate::import::place_name(&self.store, sid);
            out.imported = Some(episode_shown(sid, rec.title.as_deref(), imp, private));
            out.ms = t0.elapsed().as_secs_f64() * 1e3;
            return Ok(out);
        }
        out.class = self.runner.view_of(sid).class.as_str().to_string();
        out.place = self.runner.place_name(sid);
        let compiles = self.compiles_of(sid)?;
        out.turns = turns_of(&compiles);
        let turn = p
            .turn_id
            .clone()
            .or_else(|| out.turns.first().map(|t| t.turn_id.clone()));
        let compiled = turn
            .as_deref()
            .and_then(|t| compiles.iter().rev().find(|(_, c)| c.turn_id == t))
            .map(|(_, c)| c.clone());
        if let (Some(t), None) = (&p.turn_id, &compiled) {
            bail!("{sid} has no compile of turn {t} among its newest {COMPILES}");
        }
        let compilation = match compiled
            .as_ref()
            .map(|c| c.compilation_id.clone())
            .or_else(|| rec.compilation_id.clone())
        {
            Some(id) => self.store.get_compilation(&id)?,
            None => None,
        };
        out.turn_id = compiled.as_ref().map(|c| c.turn_id.clone());
        out.compilation = compilation
            .as_ref()
            .map(|c| info_of(c, rec.compilation_id.as_deref()));
        self.system_of(&mut out, &rec, compilation.as_ref(), private);
        let recall =
            self.recall_parts(sid, out.turn_id.as_deref(), compilation.as_ref(), private)?;
        let before: u64 = out.parts.iter().map(|p| p.tokens).sum::<u64>()
            + recall.iter().map(|p| p.tokens).sum::<u64>();
        out.parts.extend(recall);
        if let Some(c) = &compiled {
            out.parts.push(ContextPart {
                block: "conversation".into(),
                name: format!(
                    "{} nodes in the prefix, {} in the tail",
                    c.prefix_nodes, c.tail_nodes
                ),
                tokens: conversation_tokens(c, &out.parts, TokenRates::of(&out.model), before),
                note: Some(
                    "its messages, their tool calls and results, and their framing: the turn's request \
                     less the parts above, at the model's figures"
                        .into(),
                ),
                ..ContextPart::default()
            });
        }
        out.compiled = compiled;
        if let Some(t) = out.turn_id.clone() {
            out.recalls = self.turn_recalls(sid, &t, private)?;
        }
        out.sources = self.sources_of(&out, private);
        out.ms = t0.elapsed().as_secs_f64() * 1e3;
        Ok(out)
    }

    /// The turn's target, window and system parts into `out`, said against
    /// `compilation`'s manifest; or why they could not be built.
    fn system_of(
        &self,
        out: &mut ContextExplainResult,
        rec: &SessionRecord,
        compilation: Option<&Compilation>,
        private: bool,
    ) {
        let sid = rec.session_id.as_str();
        let (live, _) = self.live_profile();
        match self.runner.target_for_session(rec, &live) {
            Err(e) => out.unbuilt = Some(format!("{e:#}")),
            Ok(target) => {
                out.profile = target.profile.clone();
                out.model = target.model.clone();
                out.window = compilation
                    .and_then(|c| c.manifest.context_window)
                    .or_else(|| self.catalog.get(&target.model).map(|e| e.context_window));
                let built = self.runner.built_parts(
                    sid,
                    &target,
                    rec.kind,
                    compilation.map(|c| c.manifest.memberships.as_slice()),
                );
                let now = manifest_for(&built.spec, &self.catalog, None, false);
                let then = compilation.map(|c| &c.manifest);
                out.digest_now = now.system_digest.clone();
                out.digest_then = then.map(|m| m.system_digest.clone());
                out.unchanged = then.map(|m| m.system_digest == now.system_digest);
                out.parts =
                    system_parts(&built, &now, then, TokenRates::of(&target.model), private);
            }
        }
    }

    /// The session's newest `context.compiled` rows, oldest first, through
    /// the store's index (none while its shape is built).
    fn compiles_of(&self, sid: &str) -> Result<Vec<(u64, ContextCompiled)>> {
        let page = Page {
            kind: kinds::LEDGER,
            tags: vec![ledger_kind_session(
                LedgerKind::ContextCompiled.as_str(),
                sid,
            )],
            limit: COMPILES,
            ..Page::default()
        };
        let Some(got) = self.store.inner().page(&page)? else {
            return Ok(Vec::new());
        };
        let mut out = Vec::with_capacity(got.records.len());
        for r in &got.records {
            let row: LedgerRow = r.decode()?;
            if let Ok(c) = serde_json::from_value::<ContextCompiled>(row.data) {
                out.push((row.at_unix_ms, c));
            }
        }
        Ok(out)
    }

    /// The recall in front of the model for the turn: the Recall nodes it
    /// wrote, and an assembled compilation's recall section, an item each.
    fn recall_parts(
        &self,
        sid: &str,
        turn: Option<&str>,
        compilation: Option<&Compilation>,
        private: bool,
    ) -> Result<Vec<ContextPart>> {
        let mut nodes = Vec::new();
        if let Some(rid) = compilation.and_then(|c| c.recall_id.as_deref()) {
            if let Some((_, n)) = self.store.get_node(rid)? {
                nodes.push(("assembled", n));
            }
        }
        if let Some(t) = turn {
            let listed = self
                .nodes_paged(Some("recall"), Some(sid), RECALL_NODES)?
                .unwrap_or_default();
            for (_, n) in listed {
                if n.turn_id.as_deref() == Some(t) && !nodes.iter().any(|(_, m)| m.id == n.id) {
                    nodes.push(("this turn's", n));
                }
            }
        }
        let mut out = Vec::new();
        for (which, n) in nodes {
            let Body::Recall {
                recall_id, items, ..
            } = &n.body
            else {
                continue;
            };
            for item in items {
                let shown = match (private, self.store.get_node(&item.node_id)?) {
                    (true, Some((_, src))) => {
                        let text = crate::recall::text_of(&src);
                        let (a, b) = (item.chunk.0 as usize, item.chunk.1 as usize);
                        text.get(a.min(text.len())..b.min(text.len()))
                            .map(|t| format!("{}\n{t}", item.header))
                    }
                    _ => None,
                };
                out.push(ContextPart {
                    block: "recall".into(),
                    name: if private {
                        item.header.clone()
                    } else {
                        item.node_id.clone()
                    },
                    bytes: u64::from(item.chunk.1.saturating_sub(item.chunk.0))
                        + item.header.len() as u64,
                    tokens: item.tokens,
                    digest: None,
                    then: None,
                    version: None,
                    text: shown,
                    note: Some(format!(
                        "{which} recall {recall_id}: node {} of session {}",
                        item.node_id, item.session_id
                    )),
                });
            }
        }
        Ok(out)
    }

    /// The turn's recall manifests, each admitted node's excerpt filled to a
    /// private place, as `memory.recalls` fills them.
    fn turn_recalls(&self, sid: &str, turn: &str, private: bool) -> Result<Vec<RecallManifest>> {
        let item_tokens = self.runner.memory.cfg().params().item_tokens;
        let mut out = Vec::new();
        for r in self
            .store
            .scope_after(&crate::fact::recall::scope(sid), 0)?
        {
            let row: LedgerRow = r.decode()?;
            if row.turn_id.as_deref() != Some(turn) || row.kind == LedgerKind::MemoryArm.as_str() {
                continue;
            }
            let Ok(mut m) = serde_json::from_value::<RecallManifest>(row.data) else {
                continue;
            };
            for item in &mut m.admitted {
                item.text = None;
                if private {
                    if let Some((_, n)) = self.store.get_node(&item.node_id)? {
                        item.text = Some(theseus_memory::recall::excerpt(
                            &crate::recall::text_of(&n),
                            item_tokens,
                        ));
                    }
                }
            }
            out.push(m);
        }
        Ok(out)
    }

    /// Each session a recall drew on: its place by name, and an imported
    /// one's episode.
    fn sources_of(
        &self,
        out: &ContextExplainResult,
        private: bool,
    ) -> BTreeMap<String, ContextSource> {
        let mut ids: Vec<&str> = out
            .recalls
            .iter()
            .flat_map(|m| m.admitted.iter().map(|i| i.session_id.as_str()))
            .collect();
        ids.sort_unstable();
        ids.dedup();
        let mut map = BTreeMap::new();
        for s in ids {
            let rec = self.store.get_session::<SessionRecord>(s).ok().flatten();
            let imported = rec.as_ref().and_then(|r| {
                r.imported
                    .as_deref()
                    .map(|i| episode_shown(s, r.title.as_deref(), i, private))
            });
            map.insert(
                s.to_string(),
                ContextSource {
                    place: if private {
                        self.runner.place_name(s)
                    } else {
                        String::new()
                    },
                    title: rec.and_then(|r| r.title).filter(|_| private),
                    imported,
                },
            );
        }
        map
    }
}

/// The conversation's tokens: the turn's request by its bytes (`context.compiled`'s census) less the system's parts,
/// the tools and the recall (which renders in a message), at the model's figures, so it holds whatever the provider
/// counted (a stand-in's count is not one). A row from before the census: the rest of its estimate.
fn conversation_tokens(
    c: &ContextCompiled,
    parts: &[ContextPart],
    rates: TokenRates,
    before: u64,
) -> u64 {
    let Some(e) = &c.estimate else {
        return c.est_tokens.saturating_sub(before);
    };
    let bytes = |block: &str| -> u64 {
        parts
            .iter()
            .filter(|p| p.block == block)
            .map(|p| p.bytes)
            .sum()
    };
    let system = bytes("header") + bytes("context") + bytes("guidance") + bytes("recall");
    crate::provider::Census {
        json: e.census.json.saturating_sub(bytes("tools")),
        text: e.census.text.saturating_sub(system),
        opaque: e.census.opaque,
        messages: e.census.messages,
        blocks: e.census.blocks,
        ids: e.census.ids,
    }
    .tokens(rates)
}

/// An answer with nothing found yet: the session's own fields.
fn blank(sid: &str, rec: &SessionRecord, private: bool) -> ContextExplainResult {
    ContextExplainResult {
        session_id: sid.to_string(),
        title: rec.title.clone().filter(|_| private),
        kind: rec.kind.as_str().to_string(),
        class: String::new(),
        place: String::new(),
        imported: None,
        turns: Vec::new(),
        turn_id: None,
        compiled: None,
        compilation: None,
        profile: String::new(),
        model: String::new(),
        window: None,
        parts: Vec::new(),
        digest_now: String::new(),
        digest_then: None,
        unchanged: None,
        unbuilt: None,
        recalls: Vec::new(),
        sources: BTreeMap::new(),
        withheld: (!private).then(|| WITHHELD.to_string()),
        ms: 0.0,
    }
}

/// An imported session's episode, its text kept from a place that is not
/// private.
fn episode_shown(
    sid: &str,
    title: Option<&str>,
    imp: &crate::import::ImportedFrom,
    private: bool,
) -> theseus_protocol::import::ImportedEpisode {
    let mut ep = crate::import::catalog::episode_of(sid, title, imp);
    if !private {
        ep.title = None;
        ep.place_name = None;
    }
    ep
}

/// The system's parts and the tools, each against the turn's manifest.
fn system_parts(
    built: &Built,
    now: &Manifest,
    then: Option<&Manifest>,
    rates: TokenRates,
    private: bool,
) -> Vec<ContextPart> {
    let text = |t: &str| private.then(|| t.to_string());
    let word = |same: bool| if same { "same" } else { "changed" };
    let mut out = Vec::new();
    for (name, t) in &built.header {
        out.push(ContextPart {
            block: "header".into(),
            name: (*name).to_string(),
            bytes: t.len() as u64,
            tokens: text_tokens(t.len(), rates),
            text: text(t),
            ..ContextPart::default()
        });
    }
    for f in &built.files {
        let was = then.and_then(|m| {
            m.context_files
                .iter()
                .find(|x| x.path == f.file.path && x.persona == f.file.persona)
        });
        let note = f
            .file
            .missing
            .as_ref()
            .map(|m| format!("missing: {m}"))
            .or_else(|| f.file.withheld.clone())
            .or_else(|| f.file.cut.then(|| "cut at the file limit".to_string()));
        out.push(ContextPart {
            block: "context".into(),
            name: f.file.path.clone(),
            bytes: f.section.len() as u64,
            tokens: text_tokens(f.section.len(), rates),
            digest: f.file.digest.clone(),
            then: then.map(|_| match was {
                Some(w) => word(w.digest == f.file.digest).to_string(),
                None => "new".to_string(),
            }),
            text: text(&f.section),
            note,
            ..ContextPart::default()
        });
    }
    if let Some(m) = then {
        for x in &m.context_files {
            if !built
                .files
                .iter()
                .any(|f| f.file.path == x.path && f.file.persona == x.persona)
            {
                out.push(ContextPart {
                    block: "context".into(),
                    name: x.path.clone(),
                    bytes: x.bytes,
                    tokens: text_tokens(x.bytes as usize, rates),
                    digest: x.digest.clone(),
                    then: Some("gone".into()),
                    note: Some("the turn carried it; the next turn does not".into()),
                    ..ContextPart::default()
                });
            }
        }
    }
    for (name, used, t) in &built.guidance {
        let was = used
            .as_ref()
            .and_then(|g| then.and_then(|m| m.guidance.iter().find(|x| x.category == g.category)));
        out.push(ContextPart {
            block: "guidance".into(),
            name: name.clone(),
            bytes: t.len() as u64,
            tokens: text_tokens(t.len(), rates),
            digest: used.as_ref().map(|g| g.digest.clone()),
            version: used.as_ref().map(|g| g.version),
            then: match (used, then) {
                (Some(g), Some(_)) => Some(match was {
                    Some(w) => word(w.digest == g.digest).to_string(),
                    None => "new".to_string(),
                }),
                _ => None,
            },
            text: text(t),
            ..ContextPart::default()
        });
    }
    let tools = serde_json::to_string(&built.spec.tools).unwrap_or_default();
    out.push(ContextPart {
        block: "tools".into(),
        name: format!("{} tools", built.spec.tools.len()),
        bytes: tools.len() as u64,
        tokens: json_tokens(tools.len(), rates),
        digest: Some(now.tools_digest.clone()),
        then: then.map(|m| word(m.tools_digest == now.tools_digest).to_string()),
        note: Some(now.tools.join(", ")),
        ..ContextPart::default()
    });
    out
}

/// The turns of `compiles` (oldest first), newest first: each turn's first
/// compile's time, its loops, and its last loop's estimate.
pub(crate) fn turns_of(compiles: &[(u64, ContextCompiled)]) -> Vec<ContextTurn> {
    let mut order: Vec<String> = Vec::new();
    let mut by: BTreeMap<String, ContextTurn> = BTreeMap::new();
    for (at, c) in compiles {
        if c.turn_id.is_empty() {
            continue;
        }
        let t = by.entry(c.turn_id.clone()).or_insert_with(|| {
            order.push(c.turn_id.clone());
            ContextTurn {
                turn_id: c.turn_id.clone(),
                at_ms: *at,
                ..ContextTurn::default()
            }
        });
        t.loops += 1;
        t.est_tokens = c.est_tokens;
        t.compilation_id = c.compilation_id.clone();
    }
    order
        .iter()
        .rev()
        .take(TURNS)
        .filter_map(|t| by.remove(t))
        .collect()
}

/// A compilation as `compilation.list` shows it.
fn info_of(c: &Compilation, current: Option<&str>) -> CompilationInfo {
    CompilationInfo {
        compilation_id: c.id.clone(),
        session_id: c.session_id.clone(),
        created_at_ms: c.created_at_ms,
        trigger: c.trigger.clone(),
        strategy: c.strategy.clone(),
        as_of: c.as_of,
        includes: c.includes.len() as u32,
        derived_from: c.derived_from.clone(),
        manifest: serde_json::to_value(&c.manifest).unwrap_or(Value::Null),
        current: current == Some(c.id.as_str()),
    }
}
