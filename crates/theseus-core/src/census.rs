//! `context.census` (theseus-7m7w.1): the next request of a session, or a fresh session's first request
//! for a kind of place, compiled once and counted part by part down to each tool's schema. Nothing is sent
//! and nothing is written; `exact` asks the provider's token counter (not a model) when a key resolves.
//!
//! It compiles as a turn does (`TurnRunner::census_request`: the spec a turn builds, its standing block,
//! `compiler::compile` over the session's transcript), so it follows whatever the compile changes, then
//! apportions the compile's own estimate to the parts (`Ledger`): a part's tokens are the difference of the
//! running rounded total before and after it, so the parts add up to the total exactly, however many there
//! are. A tool's use is a read of the ledger's `action.planned` rows (`tool_use`): an operator's read, never
//! a turn's path.

use std::collections::BTreeMap;

use anyhow::{anyhow, Result};
use serde_json::Value;
use theseus_protocol::census::{
    CensusBlock, CensusExact, CensusPart, CensusTool, ContextCensusParams, ContextCensusResult,
    PLACES,
};
use theseus_protocol::{LedgerKind, SessionKind};
use theseus_store::pages::ledger_kind;
use theseus_store::{kinds, Page, Store as _};

use crate::catalog::TokenRates;
use crate::ceiling::PlaceView;
use crate::compiler::{Compiled, RequestSpec};
use crate::ledger::LedgerRow;
use crate::places::PlaceClass;
use crate::provider::{Census, ProviderRequest, BLOCK_TOKENS, ID_TOKENS, MESSAGE_TOKENS};
use crate::session::SessionRecord;
use crate::tools_defer::PlaceKind;
use crate::turn::{Target, TurnRunner};

/// The days of the ledger a tool's use is counted over.
pub const USE_DAYS: u32 = 30;
/// A day, in ms.
const DAY_MS: u64 = 86_400_000;
/// The rows read for the use a page at a time.
const USE_PAGE: usize = 5_000;
/// The bytes of a request's smallest form, a user message of one word.
const SMALLEST: &str = "x";

/// Tokens by running total: a part's tokens are the rounded-up total through it less the rounded-up total
/// before it, so any run of parts adds up to the whole's, which `Census::tokens` rounds once.
pub struct Ledger {
    per: f64,
    bytes: u64,
    tokens: u64,
}

impl Ledger {
    pub fn new(per: f64) -> Self {
        Self {
            per: per.max(0.5),
            bytes: 0,
            tokens: 0,
        }
    }

    /// The tokens `bytes` more add.
    pub fn take(&mut self, bytes: u64) -> u64 {
        self.bytes += bytes;
        let through = (self.bytes as f64 / self.per).ceil() as u64;
        let added = through - self.tokens;
        self.tokens = through;
        added
    }

    /// The tokens each of `sizes` adds, in order, as `take` would.
    pub fn cut(&mut self, sizes: &[u64]) -> Vec<u64> {
        sizes.iter().map(|b| self.take(*b)).collect()
    }
}

/// A census, the request it counted, and its model's provider (for `exact`).
pub type Counted = (
    ContextCensusResult,
    ProviderRequest,
    Option<std::sync::Arc<dyn crate::provider::Provider>>,
);

/// What the compile of the next request holds, for the parts' names.
pub struct NextRequest {
    pub target: Target,
    pub kind: SessionKind,
    pub class: PlaceClass,
    pub compiled: Compiled,
    pub spec: RequestSpec,
    /// `fresh`, `append` or `recompile`.
    pub how: &'static str,
    /// The header's named sections, the context files' and the guidance's, in order.
    pub header: Vec<(&'static str, String)>,
    pub files: Vec<(String, String)>,
    /// The persona in play's files (or every persona's, where a message can engage one), each by its
    /// file's own name: the persona layer's parts.
    pub persona_files: Vec<(String, String)>,
    pub guidance: Vec<(String, String)>,
    pub standing: String,
    /// The tokens of the recalled notes the history holds (a part of its messages).
    pub recall_tokens: u64,
    pub notes: Vec<String>,
}

impl TurnRunner {
    /// The context files `target` carries, read without a warning.
    fn peek_files(&self, target: &Target) -> Vec<crate::context_files::ContextFile> {
        let paths =
            self.cfg
                .context_paths(&self.personas(), target.persona.as_deref(), target.engages);
        self.context_files.peek(&paths)
    }

    /// The target a session's next request, or a fresh session's first on `place_kind`, is built for.
    fn census_target(
        &self,
        rec: Option<&SessionRecord>,
        place_kind: &str,
        live: &str,
    ) -> Result<Target> {
        let Some(r) = rec else {
            let mut t = self.resolve_target(live, None, None, None)?;
            // A fresh session's place is the kind asked for.
            t.engages = self.personas().engaging()
                && matches!(
                    PlaceKind::parse(place_kind),
                    Some(PlaceKind::Dm | PlaceKind::Channel)
                );
            return Ok(t);
        };
        self.target_for_session(r, live)
    }

    /// The next request of `rec`'s session, or a fresh session's first on `place`, compiled and not sent.
    pub fn census_request(
        &self,
        rec: Option<&SessionRecord>,
        place_kind: &str,
        live: &str,
    ) -> Result<NextRequest> {
        self.census_request_with(rec, place_kind, live, None)
    }

    /// `census_request` over the session's transcript with one more node written at its end, that
    /// nothing wrote: a split's dry run reads the next request as a `Moved` node would leave it
    /// (theseus-q9b85).
    pub fn census_request_with(
        &self,
        rec: Option<&SessionRecord>,
        place_kind: &str,
        live: &str,
        extra: Option<&crate::node::Node>,
    ) -> Result<NextRequest> {
        let (kind, view, sid, mut notes) = self.census_place(rec, place_kind);
        let target = self.census_target(rec, place_kind, live)?;
        let mut spec = self.spec_of(&target, kind, view, self.peek_files(&target));
        spec.session_text = self.session_block(rec.and_then(|r| r.dir.as_deref()), view);
        // The thread's card and tools, as a turn takes them (theseus-h4a7).
        self.thread_spec(
            &mut spec,
            &sid,
            rec.and_then(|r| r.compilation_id.as_deref()),
        );
        // The families it loads and defers, as a turn chooses them (theseus-7m7w.2).
        let fresh = rec.is_none().then_some(place_kind);
        spec.defer = self.census_deferral(&sid, kind == SessionKind::Task, fresh, &spec);
        let built = self.built_parts(
            &sid,
            &target,
            kind,
            None::<&[theseus_ontology::MembershipUsed]>,
        );
        if rec.is_some() {
            spec.walk = self.walk(&sid, view.content_class());
        } else {
            notes.push("a fresh session has no category guidance yet".to_string());
        }
        let standing = self.census_standing(
            &sid,
            target.persona.as_deref(),
            view.content_class(),
            &mut spec,
        );
        let mut nodes = match rec {
            Some(r) => self.store.transcript(&r.session_id)?,
            None => Vec::new(),
        };
        if let Some(n) = extra {
            let at = self.store.last_position();
            nodes.push((
                at,
                crate::stub::Stub::hydrated(at, std::sync::Arc::new(n.clone())),
            ));
        }
        let hidden = rec.map(|r| r.not_shown.as_slice()).unwrap_or_default();
        let (nodes, _) = self.stubbed_view(&sid, &spec.model, hidden, nodes, None);
        let current = match rec.and_then(|r| r.compilation_id.as_deref()) {
            Some(id) => self.store.get_compilation(id)?,
            None => None,
        };
        let (recall_tokens, sources) = self.recall_part(&nodes);
        let situation = crate::compiler::situation::given(false, false, false);
        let compiled = crate::compiler::compile(crate::compiler::CompileInput {
            session_id: &sid,
            current: current.as_ref(),
            nodes: &nodes,
            last_position: self.store.last_position(),
            spec: &spec,
            catalog: &self.catalog,
            force: None,
            window_override: self.working_window(&spec),
            blobs: Some(self.store.blobs()),
            hidden,
            strip: None,
            overflowed: None,
            sources: &sources,
            signals: None,
            assembled: None,
            situation: &situation,
            kept: None,
        });
        let how = match (rec.is_some(), compiled.new_compilation) {
            (false, _) => "fresh",
            (true, true) => "recompile",
            (true, false) => "append",
        };
        Ok(NextRequest {
            target,
            kind,
            class: view.class,
            how,
            header: built.header,
            files: built
                .files
                .iter()
                .filter(|f| f.file.persona.is_none())
                .map(|f| (f.file.path.clone(), f.section.clone()))
                .collect(),
            persona_files: built
                .files
                .iter()
                .filter(|f| f.file.persona.is_some())
                .map(|f| (file_name(&f.file.path), f.section.clone()))
                .collect(),
            guidance: built
                .guidance
                .iter()
                .map(|(n, _, t)| (n.clone(), t.clone()))
                .collect(),
            standing,
            recall_tokens,
            compiled,
            spec,
            notes,
        })
    }

    /// The tokens of the recall notes a transcript holds, and the sources they render from.
    fn recall_part(
        &self,
        nodes: &crate::store::Transcript,
    ) -> (u64, crate::recall::render::Sources) {
        let recalls = nodes
            .iter()
            .filter(|(_, n)| n.kind == crate::stub::Kind::Recall);
        let tokens = recalls
            .clone()
            .map(|(_, n)| match &n.body {
                crate::node::Body::Recall { items, .. } => items.iter().map(|i| i.tokens).sum(),
                _ => 0,
            })
            .sum();
        (
            tokens,
            self.memory
                .read_sources(&self.store, recalls.map(|(_, n)| &**n)),
        )
    }

    /// The kind of session, the place it speaks in and its id for a census: the session's own, or a fresh
    /// one's on a kind of place (`dm` is the first DM the binding binds, else the CLI's, said in a note).
    fn census_place(
        &self,
        rec: Option<&SessionRecord>,
        place_kind: &str,
    ) -> (SessionKind, PlaceView, String, Vec<String>) {
        let mut notes = Vec::new();
        match rec {
            // A draft's next request is the draft view's (theseus-a1wvq.13).
            Some(r) => (
                r.kind,
                PlaceView {
                    draft: self.reach_of(r),
                    ..self.view_of(&r.session_id)
                },
                r.session_id.clone(),
                notes,
            ),
            None => {
                let kind = match place_kind {
                    "task" => SessionKind::Task,
                    _ => SessionKind::Conversation,
                };
                let view = match place_kind {
                    "dm" => match self.place_rule.owner_dm_places().first() {
                        Some(p) => self
                            .place_rule
                            .place(&self.cfg, Some(&format!("discord:{p}"))),
                        None => {
                            notes.push("no DM is bound here: counted as the CLI is".to_string());
                            PlaceClass::Private.into()
                        }
                    },
                    // A channel of a group place: the public tools alone.
                    "channel" => PlaceView::from(PlaceClass::Shared),
                    _ => PlaceView::from(PlaceClass::Private),
                };
                (kind, view, "ses_census".to_string(), notes)
            }
        }
    }

    /// The standing block a turn of `sid` would carry, applied to `spec`; its text, empty when none.
    fn census_standing(
        &self,
        sid: &str,
        persona: Option<&str>,
        class: PlaceClass,
        spec: &mut RequestSpec,
    ) -> String {
        let Ok(all) = self.standing.all(&self.store) else {
            return String::new();
        };
        let place = self.target_of(sid);
        let scope = crate::standing::Scope {
            session: sid,
            place: place.as_deref(),
            surface: None,
            persona,
        };
        match crate::standing::Block::of(&all, scope, class, theseus_protocol::now_unix_ms()) {
            Some(b) => {
                b.apply(spec);
                b.text
            }
            None => String::new(),
        }
    }
}

/// The system blocks the request carries, in order, each as (its name, its layer): the persona layer and the
/// project layer are both the context files, so both print as `context`; the session's is `session`.
fn block_names(spec: &RequestSpec) -> Vec<(&'static str, &'static str)> {
    use crate::compiler::layers::{HEADER, PERSONA, PROJECT};
    [
        ("header", HEADER, &spec.system_text),
        ("context", PERSONA, &spec.persona_text),
        ("context", PROJECT, &spec.context_text),
        (
            crate::compiler::session_block::NAME,
            crate::compiler::session_block::NAME,
            &spec.session_text,
        ),
    ]
    .into_iter()
    .filter(|(_, _, text)| !text.is_empty())
    .map(|(name, layer, _)| (name, layer))
    .collect()
}

/// A path's last component.
fn file_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

/// The sections of the block `name` of layer `layer`, as their texts lie in it: bytes that no section holds are
/// `joins`.
fn sections_of(next: &NextRequest, name: &str, layer: &str, text: &str) -> Vec<(String, u64)> {
    use crate::compiler::layers::{HEADER, PERSONA, PROJECT};
    let sizes = |v: &[(String, String)]| -> Vec<(String, u64)> {
        v.iter().map(|(p, t)| (p.clone(), t.len() as u64)).collect()
    };
    let sections: Vec<(String, u64)> = match layer {
        // The standing orders for everywhere end the header layer (`standing::Block::apply`).
        HEADER => next
            .header
            .iter()
            .map(|(n, t)| ((*n).to_string(), t.len() as u64))
            .chain(
                (!next.standing.is_empty())
                    .then(|| ("standing orders".to_string(), next.standing.len() as u64)),
            )
            .collect(),
        PERSONA => sizes(&next.persona_files),
        PROJECT => sizes(&next.files)
            .into_iter()
            .chain(sizes(&next.guidance))
            .collect(),
        _ => vec![("directory".to_string(), text.len() as u64)],
    };
    let held: u64 = sections.iter().map(|(_, b)| b).sum();
    let whole = text.len() as u64;
    if held > whole {
        // The compile changed the block: it is one part.
        return vec![(name.to_string(), whole)];
    }
    let mut out = sections;
    if whole > held {
        out.push(("joins".to_string(), whole - held));
    }
    out
}

/// A tool definition's parts in bytes: the rest (the name, the keys, the quotes), the description's and the
/// schema's, in the JSON a request carries.
fn tool_bytes(t: &Value) -> (u64, u64, u64) {
    let len = |v: &Value| serde_json::to_string(v).map_or(0, |s| s.len() as u64);
    let (d, s) = (len(&t["description"]), len(&t["input_schema"]));
    (len(t).saturating_sub(d + s), d, s)
}

/// Calls planned in the last `USE_DAYS` days by tool (canonical name), across the store: the count and
/// the last one's time. `None` while the index's shape is being built.
pub fn tool_use(
    store: &crate::store::Store,
    since_ms: u64,
) -> Result<Option<BTreeMap<String, (u64, u64)>>> {
    let mut out: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    let mut after = None;
    loop {
        let page = Page {
            kind: kinds::LEDGER,
            tags: vec![ledger_kind(LedgerKind::ActionPlanned.as_str())],
            after,
            since_ms: Some(since_ms),
            limit: USE_PAGE,
            ..Page::default()
        };
        let Some(got) = store.inner().page(&page)? else {
            return Ok(None);
        };
        for r in &got.records {
            let row: LedgerRow = r.decode()?;
            let Some(tool) = row.data.get("tool").and_then(Value::as_str) else {
                continue;
            };
            let e = out.entry(tool.to_string()).or_default();
            e.0 += 1;
            e.1 = e.1.max(row.at_unix_ms);
        }
        match (got.more, got.last) {
            (true, Some(l)) => after = Some(l),
            _ => return Ok(Some(out)),
        }
    }
}

impl crate::rpc::Core {
    /// `context.census`: the census, the request it counted, and its model's provider.
    pub(crate) fn context_census(&self, p: &ContextCensusParams) -> Result<Counted> {
        let t0 = std::time::Instant::now();
        let (live, _) = self.live_profile();
        let place = p.place.as_deref().unwrap_or("cli");
        let rec: Option<SessionRecord> = match p.session_id.as_deref() {
            Some(sid) => {
                let r: SessionRecord = self
                    .store
                    .get_session(sid)?
                    .ok_or_else(|| anyhow!("no session is named {sid}"))?;
                if r.imported.is_some() {
                    anyhow::bail!(
                        "{sid} is an imported session: it takes no turn, so it has no request"
                    );
                }
                Some(r)
            }
            None if PLACES.contains(&place) => None,
            None => anyhow::bail!("no place kind {place:?}: one of {}", PLACES.join(", ")),
        };
        let next = self.runner.census_request(rec.as_ref(), place, &live)?;
        let mut out = self.census_of(&next, rec.as_ref().map(|r| r.session_id.as_str()), place);
        let since = theseus_protocol::now_unix_ms().saturating_sub(u64::from(USE_DAYS) * DAY_MS);
        out.use_since_ms = since;
        out.use_days = USE_DAYS;
        match tool_use(&self.store, since)? {
            Some(use_) => self.fill_use(&mut out, &use_),
            None => out.use_unread = Some("the index's shape is being built".to_string()),
        }
        out.ms = t0.elapsed().as_secs_f64() * 1e3;
        let provider = self.runner.providers.get(&next.target.provider).cloned();
        Ok((out, next.compiled.request, provider))
    }

    /// The tools' use by the name the model sees, from the ledger's by the canonical one.
    fn fill_use(&self, out: &mut ContextCensusResult, use_: &BTreeMap<String, (u64, u64)>) {
        let mut wire: BTreeMap<String, String> = self
            .tools
            .registry
            .all()
            .map(|t| (t.wire_name(), t.name().to_string()))
            .collect();
        for t in self.tools.mcp.all().iter() {
            wire.insert(t.wire.clone(), t.canonical.clone());
        }
        for t in &mut out.tools {
            let canonical = wire.get(&t.name).unwrap_or(&t.name);
            if let Some((n, last)) = use_.get(canonical) {
                t.calls = *n;
                t.last_call_ms = Some(*last);
            }
        }
    }

    /// The census of a compiled request, the use left to the caller.
    fn census_of(
        &self,
        next: &NextRequest,
        session_id: Option<&str>,
        place: &str,
    ) -> ContextCensusResult {
        let request = &next.compiled.request;
        let rates = TokenRates::of(&next.target.model);
        let mut res = census_parts(request, next, rates);
        self.flag_over_cap(&mut res, next);
        res.session_id = session_id.map(str::to_string);
        res.place = if session_id.is_some() {
            "session"
        } else {
            place
        }
        .to_string();
        res.class = next.class.as_str().to_string();
        res.profile = next.target.profile.clone();
        res.model = next.target.model.clone();
        res.window = self
            .catalog
            .get(&next.target.model)
            .map(|e| e.context_window);
        res.compile = next.how.to_string();
        res.compile_tokens = next.compiled.estimate.tokens;
        res.recall_tokens = next.recall_tokens;
        res.notes = next.notes.clone();
        res
    }

    /// `context_census` with the provider's own counts, when `p.exact` and a key resolves.
    pub(crate) async fn context_census_exact(
        self: &std::sync::Arc<Self>,
        p: ContextCensusParams,
    ) -> Result<ContextCensusResult> {
        let core = self.clone();
        let exact = p.exact;
        let session = p.session_id.clone();
        let (mut out, request, provider) =
            tokio::task::spawn_blocking(move || core.context_census(&p)).await??;
        if !exact {
            return Ok(out);
        }
        let Some(provider) = provider else {
            out.exact_error = Some("the model's provider is not configured".to_string());
            return Ok(out);
        };
        // The counter is sent the session's whole request: its labels hold it.
        let provider = match session.as_deref() {
            Some(s) => crate::data_gate::bound(provider, s),
            None => provider,
        };
        match exact_counts(provider.as_ref(), &request).await {
            Ok(e) => out.exact = Some(e),
            Err(e) => out.exact_error = Some(format!("{e:#}")),
        }
        Ok(out)
    }
}

/// The census of `request`: system, tools, messages and images, apportioned from its own estimate.
pub fn census_parts(
    request: &ProviderRequest,
    next: &NextRequest,
    rates: TokenRates,
) -> ContextCensusResult {
    let mut text = Ledger::new(rates.text);
    let mut json = Ledger::new(rates.json);
    let names = block_names(&next.spec);
    let mut system = Vec::new();
    for (i, b) in request.system.iter().enumerate() {
        let body = b["text"].as_str().unwrap_or_default();
        let (name, layer) = names.get(i).copied().unwrap_or(("system", "system"));
        let sections = sections_of(next, name, layer, body);
        let sizes: Vec<u64> = sections.iter().map(|(_, b)| *b).collect();
        let tokens = text.cut(&sizes);
        let parts: Vec<CensusPart> = sections
            .into_iter()
            .zip(tokens.iter())
            .map(|((name, bytes), tokens)| CensusPart {
                over_cap: false,
                name,
                bytes,
                tokens: *tokens,
            })
            .collect();
        system.push(CensusBlock {
            block: name.to_string(),
            bytes: body.len() as u64,
            tokens: tokens.iter().sum::<u64>() + MESSAGE_TOKENS,
            framing: MESSAGE_TOKENS,
            parts,
        });
    }
    let system_tokens = system.iter().map(|b| b.tokens).sum();
    let mut tools = Vec::new();
    // A deferred tool's definition is sent, and the estimate counts one
    // constant for all of them and the search tool its own
    // (`tools_defer::tools_bytes`): the first deferred line carries it.
    let mut constant = Some(crate::tools_defer::DEFERRED_BYTES);
    for t in &request.tools {
        let deferred = t.get("defer_loading") == Some(&Value::Bool(true));
        let (rest, desc, schema) = if deferred {
            (constant.take().unwrap_or(0), 0, 0)
        } else if t["name"] == crate::tools_defer::SEARCH_TOOL {
            (crate::tools_defer::SEARCH_BYTES, 0, 0)
        } else {
            tool_bytes(t)
        };
        let cut = json.cut(&[rest, desc, schema]);
        tools.push(CensusTool {
            name: t["name"].as_str().unwrap_or_default().to_string(),
            bytes: rest + desc + schema,
            description_bytes: desc,
            schema_bytes: schema,
            description_tokens: cut[1],
            schema_tokens: cut[2],
            tokens: cut.iter().sum(),
            deferred,
            ..CensusTool::default()
        });
    }
    let tools_tokens = tools.iter().map(|t| t.tokens).sum();
    let m = Census::of_messages(&request.messages);
    let messages_tokens = json.take(m.json)
        + text.take(m.text)
        + (m.opaque as f64 / crate::provider::OPAQUE_BYTES_PER_TOKEN).ceil() as u64
        + m.messages * MESSAGE_TOKENS
        + m.blocks * BLOCK_TOKENS
        + m.ids * ID_TOKENS;
    ContextCensusResult {
        system,
        system_tokens,
        tools,
        tools_tokens,
        messages_tokens,
        images_tokens: request.image_tokens,
        total_tokens: system_tokens + tools_tokens + messages_tokens + request.image_tokens,
        ..ContextCensusResult::default()
    }
}

/// The provider's counts of the request, its system, its tools and its messages: the request, then the
/// smallest form (one word) with each added alone, so each part is what it adds to the smallest.
async fn exact_counts(
    provider: &dyn crate::provider::Provider,
    request: &ProviderRequest,
) -> Result<CensusExact> {
    // The counter refuses a server tool (the tool search's, theseus-7m7w.2):
    // the request is counted without it, so its own tokens are not in these.
    let mut request = request.clone();
    request
        .tools
        .retain(|t| t.get("type").is_none_or(|ty| ty == "custom"));
    let request = &request;
    let one = |system: bool, tools: bool| {
        let mut r = request.clone();
        r.messages = vec![serde_json::json!({
            "role": "user",
            "content": [{"type": "text", "text": SMALLEST}],
        })];
        if !system {
            r.system.clear();
        }
        if !tools {
            r.tools.clear();
        }
        r
    };
    let (small, with_s, with_t) = (one(false, false), one(true, false), one(false, true));
    // The counter refuses a request with no message (a 400: a fresh session's
    // first request before its first message, theseus-7okz9). That one is not
    // counted: its messages are none, so its total is its system and its tools
    // over the smallest form, which the three counts above already give.
    let empty = request.messages.is_empty();
    let full = async {
        if empty {
            Ok(None)
        } else {
            provider.count_tokens(request).await.map(Some)
        }
    };
    let (full, base, with_system, with_tools) = tokio::try_join!(
        full,
        provider.count_tokens(&small),
        provider.count_tokens(&with_s),
        provider.count_tokens(&with_t),
    )?;
    let system = with_system.saturating_sub(base);
    let tools = with_tools.saturating_sub(base);
    let full = full.unwrap_or(system + tools);
    Ok(CensusExact {
        total: full,
        system,
        tools,
        messages: full.saturating_sub(system + tools),
    })
}

/// A surface that reads no private text sees the sizes, not the owner's file names.
pub fn withhold_names(r: &mut ContextCensusResult) {
    for b in r.system.iter_mut().filter(|b| b.block == "context") {
        for p in &mut b.parts {
            if p.name != "standing orders" && p.name != "joins" {
                p.name = "context".to_string();
            }
        }
    }
}

impl crate::rpc::Core {
    /// Flag the persona parts whose soul is over `[context] persona_max_tokens`
    /// (theseus-utfnl): the persona layer's parts are named by their files.
    fn flag_over_cap(&self, res: &mut ContextCensusResult, next: &NextRequest) {
        let cap = self.cfg.context.persona_cap();
        let souls: Vec<&String> = next.persona_files.iter().map(|(n, _)| n).collect();
        for p in res.system.iter_mut().flat_map(|b| b.parts.iter_mut()) {
            let tokens = p.bytes.div_ceil(crate::task_graph::view::BYTES_PER_TOKEN);
            p.over_cap = souls.contains(&&p.name) && tokens > cap;
        }
    }
}
