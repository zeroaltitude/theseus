//! The owner's persona writes (theseus-utfnl): `persona.add`, `.soul`, `.set`,
//! `.soul_read`, `.retire`, `.default`. Each is judged as an ontology write is
//! (`Act::Ontology`: the owner, from a private place; refused inside a job by
//! the CLI), before it reads anything; the work is `personas/write.rs`.

use std::sync::Arc;

use serde_json::Value;
use theseus_protocol::method;

use super::confirms::Act;
use super::server::{parse, reply, Conn, RpcFailure};
use super::Core;

pub(super) const OWN: [&str; 6] = [
    method::PERSONA_ADD,
    method::PERSONA_SOUL,
    method::PERSONA_SOUL_READ,
    method::PERSONA_SET,
    method::PERSONA_RETIRE,
    method::PERSONA_DEFAULT,
];

impl Core {
    pub(super) async fn rpc_persona_write(
        self: Arc<Self>,
        name: &'static str,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        let core = Arc::clone(&self);
        let out = match name {
            method::PERSONA_ADD => {
                let p: theseus_protocol::PersonaAddParams = parse(params)?;
                let who = conn.answerer(p.author.clone(), p.discord.clone());
                blocking(move || core.persona_add(&p, &who)).await?
            }
            method::PERSONA_SOUL => {
                let p: theseus_protocol::PersonaSoulParams = parse(params)?;
                let who = conn.answerer(p.author.clone(), p.discord.clone());
                blocking(move || core.persona_soul(&p, &who)).await?
            }
            method::PERSONA_SOUL_READ => {
                let p: theseus_protocol::PersonaSoulReadParams = parse(params)?;
                let who = conn.answerer(p.author.clone(), p.discord.clone());
                return reply(blocking(move || core.persona_soul_read(&p, &who)).await?);
            }
            method::PERSONA_SET => {
                let p: theseus_protocol::PersonaSetParams = parse(params)?;
                let who = conn.answerer(p.author.clone(), p.discord.clone());
                blocking(move || core.persona_set(&p, &who)).await?
            }
            method::PERSONA_RETIRE => {
                let p: theseus_protocol::PersonaKeyParams = parse(params)?;
                let who = conn.answerer(p.author.clone(), p.discord.clone());
                blocking(move || core.persona_retire(&p, &who)).await?
            }
            _ => {
                let p: theseus_protocol::PersonaKeyParams = parse(params)?;
                let who = conn.answerer(p.author.clone(), p.discord.clone());
                blocking(move || core.persona_default(&p, &who)).await?
            }
        };
        reply(out)
    }
}

impl Core {
    /// A persona write is the owner's, from a private place, as an ontology
    /// write is: judged before anything is read. The one gate every entry
    /// point (the methods, a Discord press) passes.
    pub(crate) fn judge_persona(
        &self,
        method: &'static str,
        what: &str,
        who: &crate::approval::Answerer,
    ) -> anyhow::Result<()> {
        self.judge_act(who, Act::Ontology { method, what })
    }
}

/// A refusal is the owner's to read; any other failure is invalid input.
fn failure(e: anyhow::Error) -> RpcFailure {
    match e.downcast::<crate::approval::Refusal>() {
        Ok(r) => RpcFailure {
            code: theseus_protocol::error_code::REFUSED,
            message: format!(
                "a persona write does not count: {}. Nothing was written.",
                r.why
            ),
            data: serde_json::json!({"who": r.who, "via": r.via, "why": r.why}),
        },
        Err(e) => RpcFailure::invalid(e),
    }
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> Result<T, RpcFailure> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| RpcFailure::invalid(anyhow::anyhow!("{e}")))?
        .map_err(failure)
}
