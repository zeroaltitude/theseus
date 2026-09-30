//! A synthetic store of parked sessions for the lifecycle bench
//! (theseus-qa0). Each session is what one turn of a conversation leaves:
//! its execution waiting for input, its record, the operator's message and
//! the reply, and the ledger rows `session.open` and a turn write. The
//! records are the product's own types, but many sessions share a frame, so
//! ten thousand take one fsync per `PER_FRAME` sessions instead of three each.

use std::path::Path;
use std::time::Instant;

use anyhow::Result;
use serde_json::json;
use theseus_core::ledger::LedgerRow;
use theseus_core::node::{Body, Node};
use theseus_core::session::SessionRecord;
use theseus_core::store::Store;
use theseus_kernel::{new_id, Authority, Budget, ExecState, Execution, SessionKind, Wake, SCHEMA};
use theseus_store::{kinds, NewRecord};

/// Sessions per frame: about 700 KB, one fsync.
pub const PER_FRAME: u64 = 250;

/// What was written, and how long it took.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Generated {
    pub sessions: u64,
    pub records: u64,
    pub frames: u64,
    pub wal_bytes: u64,
    pub ms: f64,
}

/// Write `sessions` parked sessions into the store at `dir`, then
/// checkpoint, so the store opens with no tail to replay.
pub fn generate(dir: &Path, sessions: u64) -> Result<Generated> {
    let t0 = Instant::now();
    let store = Store::open(dir)?;
    let (mut records, mut frames) = (0u64, 0u64);
    let mut i = 0u64;
    while i < sessions {
        let n = PER_FRAME.min(sessions - i);
        let mut frame = Vec::with_capacity((n * 7) as usize);
        for k in i..i + n {
            frame.extend(parked_session(k)?);
        }
        records += frame.len() as u64;
        store.append(&frame)?;
        frames += 1;
        i += n;
    }
    store.checkpoint()?;
    let wal_bytes = store.stats()?.wal_bytes;
    Ok(Generated {
        sessions,
        records,
        frames,
        wal_bytes,
        ms: t0.elapsed().as_secs_f64() * 1000.0,
    })
}

/// One session after one turn, as the product writes it.
fn parked_session(k: u64) -> Result<Vec<NewRecord>> {
    let mut rec = SessionRecord::new(SessionKind::Conversation, Some(format!("bench {k}")));
    let sid = rec.session_id.clone();
    let now = theseus_protocol::now_unix_ms();
    let exec = Execution {
        id: new_id("exe"),
        schema: SCHEMA,
        session_id: sid.clone(),
        kind: SessionKind::Conversation,
        state: ExecState::Waiting,
        authority: Authority {
            principal: theseus_core::turn::OPERATOR.to_string(),
            ..Default::default()
        },
        budget: Budget::new(100_000_000),
        wake: Some(Wake::Input),
        outstanding: vec![],
        queued_results: vec![],
        parent: None,
        reports_to: None,
        reports: vec![],
        turns: 1,
        interrupted: 0,
        resume_pending: false,
        cancel: None,
        ended_reason: None,
        created_at_ms: now,
        updated_at_ms: now,
    };
    let turn_id = new_id("trn");
    let question = format!(
        "Session {k}: what changed in the store layout since yesterday, and does the \
         nightly backup still cover the WAL segments?"
    );
    let user = Node::user(
        &sid,
        Some(&turn_id),
        theseus_core::turn::OPERATOR,
        &question,
    );
    let reply: Body = serde_json::from_value(json!({
        "kind": "assistant_message",
        "blocks": [{"type": "text", "text": format!(
            "Nothing in the layout changed for session {k}: the WAL still rolls at 64 MB, the \
             index is rebuilt from the WAL on open, and the nightly backup copies every closed \
             segment. The open segment is covered by the checkpoint that follows it, so a \
             restore replays at most one segment's tail. Two things to watch: the backup job \
             runs before the checkpoint on Sundays, and a segment that rolled during the copy \
             is picked up the next night, not the same one."
        )}],
        "model": crate::fake_model::MODEL,
        "provider": "anthropic",
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 1800, "output_tokens": 120},
        "cost_usd": 0.0054,
    }))?;
    let answer = Node::assistant(&sid, &turn_id, 0, reply);
    rec.execution_id = Some(exec.id.clone());
    rec.turns = 1;
    rec.last_turn_id = Some(turn_id.clone());
    rec.cost_usd = 0.0054;
    rec.title = Some(question.chars().take(60).collect());
    let ledger = |kind: &str, turn: Option<&str>, data: serde_json::Value| {
        NewRecord::json(
            kinds::LEDGER,
            None,
            &LedgerRow::new(kind, Some(&sid), turn, data),
        )
        .map(|r| r.scoped(&sid))
    };
    Ok(vec![
        NewRecord::json(kinds::EXECUTION, Some(&exec.id), &exec)?.scoped(&sid),
        ledger(
            "execution.opened",
            None,
            json!({"execution_id": exec.id, "kind": "conversation", "limit_usd": 100.0}),
        )?,
        NewRecord::json(kinds::SESSION, Some(&sid), &rec)?,
        ledger("session.opened", None, json!({"execution_id": exec.id}))?,
        user.record()?,
        answer.record()?,
        ledger(
            "turn.ended",
            Some(&turn_id),
            json!({"execution_id": exec.id, "outcome": "waiting", "loops": 1, "cost_usd": 0.0054}),
        )?,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sessions read back as the product reads them: each has its
    /// execution, waiting for input, and its two nodes.
    #[test]
    fn a_synthetic_store_reads_back_as_parked_sessions() {
        let d = tempfile::tempdir().unwrap();
        let g = generate(d.path(), PER_FRAME + 3).unwrap();
        assert_eq!((g.sessions, g.frames), (PER_FRAME + 3, 2));
        assert_eq!(g.records, (PER_FRAME + 3) * 7);
        let store = Store::open(d.path()).unwrap();
        let sessions: Vec<SessionRecord> = store.list_sessions().unwrap();
        assert_eq!(sessions.len() as u64, PER_FRAME + 3);
        let kernel = theseus_kernel::Kernel::new(
            store.shared(),
            std::sync::Arc::new(theseus_kernel::RealClock),
            theseus_kernel::KernelConfig::default(),
        );
        let execs = kernel.open_executions().unwrap();
        assert_eq!(execs.len() as u64, PER_FRAME + 3);
        assert!(execs
            .iter()
            .all(|e| e.state == ExecState::Waiting && e.wake == Some(Wake::Input)));
        let s = &sessions[0];
        assert_eq!(
            store.session_nodes(&s.session_id).unwrap().len(),
            2,
            "the question and the reply"
        );
        assert_eq!(
            s.execution_id.as_deref().map(|e| e.starts_with("exe_")),
            Some(true)
        );
    }
}
