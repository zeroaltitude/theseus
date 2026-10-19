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

use serde_json::Value;

use crate::compiler::{render_request, Compilation, RequestSpec};
use crate::config::Effort;
use crate::node::{Body, Node};
use crate::provider::ProviderRequest;
use crate::recall::render::Sources;
use crate::rpc::Core;
use crate::session::SessionRecord;
use crate::store::Transcript;

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
            (&compilation, &nodes, &session, &sources),
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
        (compilation, nodes, session, sources): (
            &Compilation,
            &Transcript,
            &SessionRecord,
            &Sources,
        ),
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
                let rendered = render_request(
                    spec,
                    &self.catalog,
                    compilation,
                    nodes,
                    (Some(self.store.blobs()), &session.not_shown, None, sources),
                );
                let digest = rendered.request.digest();
                if digest == want {
                    return Some((rendered.request, digest));
                }
                tried.push(digest);
                if turn_effort.is_none() {
                    break;
                }
            }
        }
        None
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
