//! `initiative.rate` and `initiative.week` (theseus-slvl.9): the owner's 👍
//! or 👎 on a message Theseus sent first, and the week's promises and
//! verdicts. The rate is an owner's act (`judge_act`, from a private place;
//! the CLI refuses it inside a job); the week is a read.

use serde_json::Value;
use theseus_protocol::initiative::InitiativeRateParams;
use theseus_protocol::{error_code, method};

use super::server::{parse, reply, Conn, RpcFailure};
use super::Core;
use crate::approval::Refusal;

impl Core {
    pub(super) fn rpc_initiative(
        &self,
        name: &str,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        if name == method::INITIATIVE_WEEK {
            return reply(
                self.initiative_week(&parse(params)?)
                    .map_err(RpcFailure::invalid)?,
            );
        }
        let p: InitiativeRateParams = parse(params)?;
        // Only the binding names a reaction's place.
        let who = conn.answerer(None, p.discord.clone());
        let done = self
            .initiative_rate(&p, who)
            .map_err(|e| match e.downcast::<Refusal>() {
                Ok(r) => RpcFailure {
                    code: error_code::REFUSED,
                    message: format!(
                        "a verdict from {} does not count: {}. Nothing was written.",
                        r.who, r.why
                    ),
                    data: serde_json::json!({"who": r.who, "via": r.via, "why": r.why}),
                },
                Err(e) => RpcFailure::invalid(e),
            })?;
        reply(done)
    }
}
