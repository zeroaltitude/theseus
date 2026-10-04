//! The owner's runs over the learning ledger (M5 25d; design §2.9):
//! `judge.replay` and `judge.audit`. Each is judged as an approval is
//! (`judge_act(Act::JudgeRun)`: the owner, from a private place; the CLI
//! refuses each inside a job), and runs on a thread of its own at low
//! priority (`learning::tender::on_low_thread`). The runs themselves are
//! `learning::replay` and `learning::audit`.

use serde_json::{json, Value};
use theseus_protocol::error_code;

use super::server::{parse, Conn, RpcFailure};
use super::Core;
use crate::approval::Refusal;

/// A run's error on the wire: a refusal is `REFUSED`, with who, through
/// what, and why; anything else the caller's to fix.
pub(super) fn failure(e: anyhow::Error) -> RpcFailure {
    match e.downcast::<Refusal>() {
        Ok(r) => RpcFailure {
            code: error_code::REFUSED,
            message: format!(
                "a run from {} does not count: {}. Nothing was sent or written.",
                r.who, r.why
            ),
            data: json!({"who": r.who, "via": r.via, "why": r.why}),
        },
        Err(e) => RpcFailure::invalid(e),
    }
}

impl Core {
    pub(super) async fn rpc_judge_replay(
        self: &std::sync::Arc<Self>,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        let p = parse(params)?;
        let who = conn.answerer(None, None);
        let r = self.judge_replay(p, who).await.map_err(failure)?;
        Ok(serde_json::to_value(r).unwrap_or(Value::Null))
    }

    pub(super) async fn rpc_judge_audit(
        self: &std::sync::Arc<Self>,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        let p = parse(params)?;
        let who = conn.answerer(None, None);
        let r = self.judge_audit(p, who).await.map_err(failure)?;
        Ok(serde_json::to_value(r).unwrap_or(Value::Null))
    }
}
