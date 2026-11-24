//! A turn's first request, rebuilt byte for byte (own-benchmark).
//!
//! What is kept: the answer node names its compilation and the digest of the
//! request it was made from (`compiled.digest`, taken before the recall,
//! standing, task and wake views are appended to the last block, which are
//! volatile); the compilation's manifest names the digest of each system
//! block (`CacheLayout`, the text in the blobs), the digest of the tools, and
//! the model; the nodes before the answer are the conversation. The request
//! is rendered again from those, and it is the original only if its digest is
//! the recorded one. When a part is missing or the bytes differ, the refusal
//! names the part.
//!
//! A turn the view built laid its front, window and beside block from outside
//! the session: they are read back from the blobs its view record names
//! (`compiler::view::kept`); a source erased since refuses the turn before
//! anything is sent. A turn whose compilation is not a view's (the old
//! transcript builder's) is refused in words (`LEGACY_REFUSED`): no saved
//! request of a builder that no longer exists is rebuilt or sent again (issue
//! 44, 2026-11-22). Its records still decode and list.

use serde_json::Value;

use crate::compiler::view::Frozen;
use crate::compiler::{Compilation, RequestSpec};
use crate::config::Effort;
use crate::node::{Body, Node};
use crate::provider::ProviderRequest;
use crate::recall::render::Sources;
use crate::rpc::Core;
use crate::session::SessionRecord;
use crate::store::Transcript;

/// Why a turn whose request the view built is refused when its view record keeps nothing of it.
pub const VIEW_REFUSED: &str = "this turn was built before views kept what a replay needs";
/// Why a turn is refused whose compilation the old transcript builder made: nothing of it is rebuilt or sent.
pub const LEGACY_REFUSED: &str =
    "this turn's request was made by a builder that no longer exists, so it is not replayed";
/// Why a view turn is refused when something it laid was erased since.
pub const VIEW_ERASED: &str =
    "something this turn's view laid has been erased since, so it is not replayed";

/// A reply: what a turn's answer said and asked for (the wire's own type).
pub use theseus_protocol::own_bench::ReplayReply as Said;

/// A first request, rebuilt.
pub struct Original {
    pub session: SessionRecord,
    pub compilation: Compilation,
    /// The spec the original was rendered from: the stored system blocks,
    /// the tools whose digest the manifest names, the effort that matched.
    pub spec: RequestSpec,
    /// The nodes before the answer, as the compile saw them.
    pub nodes: Transcript,
    pub sources: Sources,
    /// The view the request was laid from, read back from its blobs.
    pub view: Frozen,
    /// The request, and the digest that matched.
    pub request: ProviderRequest,
    pub digest: String,
    /// The last user message before the answer.
    pub ask: String,
    pub said: Said,
}

/// Why a first request cannot be rebuilt: the part, and what is wrong with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotRebuildable {
    pub part: &'static str,
    pub why: String,
}

impl std::fmt::Display for NotRebuildable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the turn's first request is not rebuildable ({}): {}",
            self.part, self.why
        )
    }
}

impl std::error::Error for NotRebuildable {}

fn no(part: &'static str, why: impl Into<String>) -> NotRebuildable {
    NotRebuildable {
        part,
        why: why.into(),
    }
}

/// The reply a node holds.
pub fn said_of(n: &Node) -> Option<Said> {
    let Body::AssistantMessage {
        blocks,
        model,
        usage,
        cost_usd,
        ..
    } = &n.body
    else {
        return None;
    };
    Some(Said {
        model: model.clone(),
        text: crate::provider::text_of(blocks),
        tool_calls: blocks
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_use"))
            .filter_map(|b| b.get("name").and_then(Value::as_str).map(str::to_string))
            .collect(),
        input_tokens: usage.input_tokens
            + usage.cache_read_input_tokens
            + usage.cache_creation_input_tokens,
        output_tokens: usage.output_tokens,
        cost_usd: cost_usd.unwrap_or(0.0),
    })
}

/// The efforts to try when the profile's own does not reproduce the bytes:
/// none, then each level. A turn's effort was Jev's or the profile's, and
/// the request does not record which.
fn efforts() -> Vec<Option<Effort>> {
    use Effort::{High, Low, Max, Medium, Xhigh};
    vec![
        None,
        Some(Low),
        Some(Medium),
        Some(High),
        Some(Xhigh),
        Some(Max),
    ]
}

impl Core {
    /// The turn's first request, rebuilt, or the part that cannot be.
    /// Reads only: it takes no lock and writes nothing.
    pub fn rebuild_first(&self, session_id: &str, turn: &str) -> Result<Original, NotRebuildable> {
        let session: SessionRecord = self
            .store
            .get_session(session_id)
            .map_err(|e| no("session", format!("{e:#}")))?
            .ok_or_else(|| no("session", format!("no session is named {session_id}")))?;
        let transcript = self
            .store
            .transcript(session_id)
            .map_err(|e| no("conversation", format!("{e:#}")))?;
        // The first answer of the turn that carries a request digest.
        let at = transcript
            .iter()
            .position(|(_, n)| {
                n.turn_id.as_deref() == Some(turn)
                    && n.kind == crate::stub::Kind::AssistantMessage
                    && n.loop_index.is_none_or(|i| i == 0)
            })
            .ok_or_else(|| {
                no(
                    "answer",
                    format!("{session_id} has no answer of turn {turn}"),
                )
            })?;
        let answer: &Node = &transcript[at].1;
        let Body::AssistantMessage {
            compilation_id: Some(cid),
            request_digest: Some(want),
            effort: placed,
            ..
        } = &answer.body
        else {
            return Err(no(
                "answer",
                "the turn's first answer records no compilation or request digest",
            ));
        };
        let said = said_of(answer).ok_or_else(|| no("answer", "not an answer"))?;
        let compilation = self
            .store
            .get_compilation(cid)
            .map_err(|e| no("compilation", format!("{e:#}")))?
            .ok_or_else(|| {
                no(
                    "compilation",
                    format!("{cid} was never stored (a detour's compile is not)"),
                )
            })?;
        let nodes: Transcript = transcript[..at].to_vec();
        // Only a view turn's request is rebuilt: an old transcript compilation is refused before anything is
        // read for it, and nothing of it is sent (issue 44, 2026-11-22).
        if compilation.strategy != crate::compiler::view::STRATEGY {
            return Err(no("compilation", LEGACY_REFUSED));
        }
        let view = self.kept_view(&compilation, (session_id, turn))?;
        let mut spec = self.kept_spec(&session, &compilation)?;
        let recalls = nodes
            .iter()
            .filter(|(_, n)| n.kind == crate::stub::Kind::Recall);
        let sources = match recalls.clone().next() {
            Some(_) => self
                .runner
                .memory
                .read_sources(&self.store, recalls.map(|(_, n)| &**n)),
            None => Sources::new(),
        };
        let mut tried = Vec::new();
        if let Some((request, digest)) = self.reproduce(
            &mut spec,
            (&compilation, &nodes, &session, &sources, &view),
            (want, *placed),
            &mut tried,
        ) {
            let ask = last_user_text(&nodes);
            return Ok(Original {
                session,
                compilation,
                spec,
                nodes,
                sources,
                view,
                request,
                digest,
                ask,
                said,
            });
        }
        Err(no(
            "messages",
            format!(
                "rendered again from {} nodes, the request digests to {} where the answer \
                 recorded {want}: the conversation as compiled differs (a stubbed attachment, \
                 or a node or setting changed since)",
                nodes.len(),
                tried.first().map_or("nothing", String::as_str)
            ),
        ))
    }

    /// The request rendered with each effort it might have run at, until
    /// its digest is `want`: the request and the digest, or none (`tried`
    /// holds each digest that was not).
    fn reproduce(
        &self,
        spec: &mut RequestSpec,
        laid: Laid<'_>,
        (want, placed): (&str, Option<Effort>),
        tried: &mut Vec<String>,
    ) -> Option<(ProviderRequest, String)> {
        let target_effort = spec.effort;
        let mut candidates = vec![target_effort];
        candidates.extend(efforts().into_iter().filter(|e| *e != target_effort));
        for effort in candidates {
            for turn_effort in [placed, None] {
                spec.effort = effort;
                spec.turn_effort = turn_effort;
                let request = self.rendered(spec, laid);
                let digest = request.digest();
                if digest == want {
                    return Some((request, digest));
                }
                tried.push(digest);
                if turn_effort.is_none() {
                    break;
                }
            }
        }
        None
    }

    /// The request `laid` renders under `spec`, from its frozen view (`compiler::view::rendered_again`).
    pub(super) fn rendered(&self, spec: &RequestSpec, laid: Laid<'_>) -> ProviderRequest {
        let (compilation, nodes, session, sources, view) = laid;
        let media = (
            Some(self.store.blobs()),
            &session.not_shown[..],
            None,
            sources,
        );
        crate::compiler::view::rendered_again(
            (spec, &self.catalog),
            compilation,
            view,
            nodes,
            media,
        )
    }

    /// The view a view turn's request was laid from, read back from the blobs its record names, once none of
    /// what it laid was erased since: a turn from before views kept it is refused in words, as is one a source
    /// of which was erased since.
    fn kept_view(
        &self,
        compilation: &Compilation,
        (session_id, turn): (&str, &str),
    ) -> Result<Frozen, NotRebuildable> {
        let record = crate::view::record::read(&self.store, &compilation.id)
            .map_err(|e| no("view", format!("{e:#}")))?;
        let k = record
            .and_then(|r| r.kept)
            .ok_or_else(|| no("compilation", VIEW_REFUSED))?;
        if erased_since(&self.store, &k.exchanges, &k.recalled) {
            return Err(no("view", VIEW_ERASED));
        }
        let f = crate::compiler::view::kept::frozen(self.store.blobs(), &k, (session_id, turn))
            .map_err(|why| no("view", why))?;
        Ok(f)
    }

    /// The spec the compilation's requests were rendered from: this
    /// daemon's, with the system blocks the blobs hold by digest in place
    /// of the ones built now, and the tools only if they are the manifest's.
    fn kept_spec(
        &self,
        session: &SessionRecord,
        compilation: &Compilation,
    ) -> Result<RequestSpec, NotRebuildable> {
        let m = &compilation.manifest;
        let layout = m
            .cache
            .as_ref()
            .ok_or_else(|| no("system", "the compilation records no block layout"))?;
        let mut texts = std::collections::HashMap::new();
        for b in &layout.blocks {
            let digest = b.digest.as_deref().ok_or_else(|| {
                no(
                    "system",
                    format!(
                        "the {} block was not kept by digest (the compilation predates it)",
                        b.block
                    ),
                )
            })?;
            let bytes = self.store.blobs().read(digest).ok_or_else(|| {
                no(
                    "system",
                    format!(
                        "the {} block's blob {digest} is missing or does not hash to its name",
                        b.block
                    ),
                )
            })?;
            if crate::blobs::digest(&bytes) != digest {
                return Err(no(
                    "system",
                    format!(
                        "the {} block's blob {digest} does not hash to its name",
                        b.block
                    ),
                ));
            }
            let text = String::from_utf8(bytes)
                .map_err(|_| no("system", format!("the {} block is not text", b.block)))?;
            texts.insert(b.block.as_str(), text);
        }
        let (live, _) = self.live_profile();
        let target = self
            .runner
            .resolve_target(&live, Some(&m.profile), Some(&m.provider), Some(&m.model))
            .map_err(|e| no("profile", format!("{e:#}")))?;
        let mut spec = self
            .runner
            .built_parts(
                &session.session_id,
                &target,
                session.kind,
                Some(m.memberships.as_slice()),
            )
            .spec;
        // The layers' names (theseus-h4a7); a compilation from before them
        // names its project block `context`.
        let project = texts
            .remove(crate::compiler::layers::PROJECT)
            .or_else(|| texts.remove("context"));
        for (name, slot) in [
            (crate::compiler::layers::HEADER, &mut spec.system_text),
            (crate::compiler::layers::PERSONA, &mut spec.persona_text),
            (crate::compiler::session_block::NAME, &mut spec.session_text),
        ] {
            *slot = texts.remove(name).unwrap_or_default();
        }
        spec.context_text = project.unwrap_or_default();
        if let Some(extra) = texts.keys().next() {
            return Err(no("system", format!("a block named {extra} has no place")));
        }
        // The tools as its requests sent them: the families the compilation
        // recorded loading (theseus-7m7w.2), whose digest the manifest holds.
        let spec = crate::tools_defer::as_recorded(spec, m);
        let tools = theseus_kernel::digest_json(&Value::Array(spec.tools.to_vec()));
        if tools[..16] != m.tools_digest {
            return Err(no(
                "tools",
                format!(
                    "the tools offered now digest to {}, the compilation's to {}",
                    &tools[..16],
                    m.tools_digest
                ),
            ));
        }
        Ok(spec)
    }
}

/// What a first request is rendered from: its compilation, the nodes before its answer, its session, the
/// recall's sources, and the view it was laid from.
pub(super) type Laid<'a> = (
    &'a Compilation,
    &'a Transcript,
    &'a SessionRecord,
    &'a Sources,
    &'a Frozen,
);

/// The last user message's text among `nodes`.
fn last_user_text(nodes: &Transcript) -> String {
    nodes
        .iter()
        .rev()
        .find_map(|(_, n)| match &n.body {
            Body::UserMessage { text, .. } => Some(text.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

/// Whether something a view laid was erased since: an exchange that no longer reads, a node its recall
/// laid that is gone or erased (`Body::Erased`), or a look back's result or report, in a laid exchange or
/// recalled (a view kept before recall left copies out did recall them), that showed words erased since
/// (`copies::since::shows_erased`). Erased words are never sent again from a replay's copy; what cannot be
/// read is treated as erased.
fn erased_since(store: &crate::store::Store, exchanges: &[String], recalled: &[String]) -> bool {
    let gone =
        (exchanges.iter()).any(|id| !matches!(crate::exchange::exchange(store, id), Ok(Some(_))));
    gone || recalled.iter().any(|id| match store.get_node(id) {
        Ok(Some((_, n))) => {
            matches!(n.body, Body::Erased { .. })
                || crate::copies::since::shows_erased(store, &n).unwrap_or(true)
        }
        Ok(None) | Err(_) => true,
    }) || exchanges.iter().any(|id| showed_erased(store, id))
}

/// Whether a node of the exchange `turn` showed words erased since; one that cannot be read is treated so.
fn showed_erased(store: &crate::store::Store, turn: &str) -> bool {
    let Ok(Some(x)) = crate::exchange::exchange(store, turn) else {
        return true;
    };
    let Ok(nodes) = store.transcript(&x.session_id) else {
        return true;
    };
    (nodes.iter())
        .filter(|(_, n)| n.turn_id.as_deref() == Some(turn))
        .filter(|(_, n)| {
            matches!(
                n.kind,
                crate::stub::Kind::ToolResult | crate::stub::Kind::UserMessage
            )
        })
        .any(|(_, n)| crate::copies::since::shows_erased(store, n).unwrap_or(true))
}
