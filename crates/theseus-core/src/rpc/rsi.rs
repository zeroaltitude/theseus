//! Self-improvement's methods (theseus-pw1q.2, theseus-pw1q.4): `self.halt`
//! (anyone's), `self.resume` (the owner's alone, from any place; the CLI
//! sends a job's `THESEUS_SESSION` so the core refuses and ledgers it),
//! `self.log` and `self.digest` (reads). The work is `crate::rsi`'s.

use serde_json::Value;
use theseus_protocol::rsi::{SelfHaltParams, SelfResumeParams};
use theseus_protocol::{error_code, method};

use super::server::{or_empty, route, Conn, RpcFailure};
use super::Core;
use crate::approval::Refusal;

/// The methods routed here.
pub(super) const OWN: [&str; 4] = [
    method::SELF_HALT,
    method::SELF_RESUME,
    method::SELF_LOG,
    method::SELF_DIGEST,
];

impl Core {
    pub(super) fn rpc_self(
        &self,
        m: &str,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        let params = or_empty(params);
        match m {
            method::SELF_HALT => route(params, |p: SelfHaltParams| {
                let who = conn.answerer(p.author.clone(), p.discord.clone());
                Ok(self.self_halt(&p, &who)?)
            }),
            method::SELF_RESUME => route(params, |p: SelfResumeParams| {
                let who = conn.answerer(p.author.clone(), p.discord.clone());
                self.self_resume(&p, &who).map_err(refused)
            }),
            method::SELF_LOG => route(params, |p| Ok(self.self_log(&p)?)),
            _ => route(params, |p| Ok(self.self_digest(&p)?)),
        }
    }
}

/// A refused resume, as the owner's acts answer one; any other error is the
/// daemon's.
fn refused(e: anyhow::Error) -> RpcFailure {
    match e.downcast::<Refusal>() {
        Ok(r) => RpcFailure {
            code: error_code::REFUSED,
            message: format!(
                "a resume of self-improvement from {} does not count: {}. It stays halted.",
                r.who, r.why
            ),
            data: serde_json::json!({"who": r.who, "via": r.via, "why": r.why}),
        },
        Err(e) => e.into(),
    }
}
