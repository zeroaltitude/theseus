//! A synthetic store of the owner's shape, for the memory spike
//! (theseus-xxac7): parked sessions of invented text, about 1.6 chunks a
//! node (the index's status counts them), so `--nodes 229000` gives about 360,000 chunks. Every text is made of
//! words from a seeded invented vocabulary (a Zipf-like draw, so BM25 sees
//! common and rare terms), carries its node's number, and names `example.test`
//! only; nothing here is real data. The same seed writes the same store.
//!
//! A spike's tool: drop this module, its `SynthMemory` subcommand in
//! `main.rs`, and `bench/memory/` to remove the spike.

use std::path::Path;
use std::time::Instant;
use theseus_protocol::LedgerKind;

use anyhow::Result;
use serde_json::json;
use theseus_core::ledger::LedgerRow;
use theseus_core::node::{Body, Node};
use theseus_core::session::SessionRecord;
use theseus_core::store::Store;
use theseus_kernel::{new_id, Authority, Budget, ExecState, Execution, SessionKind, Wake, SCHEMA};
use theseus_store::{kinds, NewRecord};

/// Nodes in one session.
const PER_SESSION: u64 = 24;
/// Sessions in one frame.
const PER_FRAME: u64 = 100;
const VOCAB: usize = 30_000;

/// What was written.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Made {
    pub sessions: u64,
    pub nodes: u64,
    pub records: u64,
    pub wal_bytes: u64,
    pub ms: f64,
}

/// SplitMix64.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// Invented words: syllables strung together, the same list for a seed.
fn vocabulary(rng: &mut Rng) -> Vec<String> {
    const SYL: [&str; 24] = [
        "ba", "ko", "ri", "tu", "me", "sa", "lo", "ne", "pi", "da", "vo", "qui", "zan", "fel",
        "dor", "mir", "tek", "bra", "sol", "gu", "han", "yo", "wex", "cri",
    ];
    let mut out = Vec::with_capacity(VOCAB);
    for i in 0..VOCAB {
        let n = 2 + (rng.below(3) as usize);
        let mut w = String::new();
        for _ in 0..n {
            w.push_str(SYL[rng.below(SYL.len() as u64) as usize]);
        }
        // The index keeps every word distinct.
        w.push_str(&format!("{i:x}"));
        out.push(w);
    }
    out
}

/// A Zipf-like pick: the square of a uniform draw favours the low indexes.
fn pick(rng: &mut Rng, n: usize) -> usize {
    let u = rng.below(1 << 24) as f64 / (1u64 << 24) as f64;
    ((u * u * u) * n as f64) as usize % n
}

/// A text of `words` words about node `k`.
fn text(rng: &mut Rng, vocab: &[String], k: u64, words: usize) -> String {
    let mut s = format!("Note {k} from https://notes.example.test/n/{k}:");
    for i in 0..words {
        s.push(if i % 14 == 13 { '.' } else { ' ' });
        s.push(' ');
        s.push_str(&vocab[pick(rng, vocab.len())]);
    }
    s
}

/// The words of node `k`'s text: 70 % short (one chunk), 20 % of two or three
/// chunks, 10 % of three to five, for about 1.6 chunks a node.
fn length(rng: &mut Rng) -> usize {
    match rng.below(10) {
        0..=6 => 30 + rng.below(90) as usize,
        7 | 8 => 600 + rng.below(500) as usize,
        _ => 1_200 + rng.below(1_300) as usize,
    }
}

fn session(rng: &mut Rng, vocab: &[String], first: u64, nodes: u64) -> Result<Vec<NewRecord>> {
    let mut rec = SessionRecord::new(
        SessionKind::Conversation,
        Some(format!("synthetic {first}")),
    );
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
        wakes: vec![],
        wake_parent: false,
        report_wakes: vec![],
        stopped: None,
        turns: nodes / 2,
        interrupted: 0,
        resume_pending: false,
        cancel: None,
        ended_reason: None,
        dismissed: None,
        retry_of: None,
        created_at_ms: now,
        updated_at_ms: now,
    };
    rec.execution_id = Some(exec.id.clone());
    rec.turns = nodes / 2;
    rec.title = Some(format!("synthetic session {first}"));
    let ledger = |kind: LedgerKind, turn: Option<&str>, data: serde_json::Value| {
        NewRecord::json(
            kinds::LEDGER,
            None,
            &LedgerRow::new(kind, Some(&sid), turn, data),
        )
        .map(|r| r.scoped(&sid))
    };
    let mut out = vec![
        NewRecord::json(kinds::EXECUTION, Some(&exec.id), &exec)?.scoped(&sid),
        ledger(
            LedgerKind::ExecutionOpened,
            None,
            json!({"execution_id": exec.id, "kind": "conversation", "limit_usd": 100.0}),
        )?,
        NewRecord::json(kinds::SESSION, Some(&sid), &rec)?,
        ledger(
            LedgerKind::SessionOpened,
            None,
            json!({"execution_id": exec.id}),
        )?,
    ];
    let mut turn_id = new_id("trn");
    for j in 0..nodes {
        let k = first + j;
        let words = length(rng);
        let body = text(rng, vocab, k, words);
        if j % 2 == 0 {
            turn_id = new_id("trn");
            let n = Node::user(&sid, Some(&turn_id), theseus_core::turn::OPERATOR, &body);
            out.push(n.record()?);
        } else {
            let reply: Body = serde_json::from_value(json!({
                "kind": "assistant_message",
                "blocks": [{"type": "text", "text": body}],
                "model": crate::fake_model::MODEL,
                "provider": "anthropic",
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 100, "output_tokens": 100},
                "cost_usd": 0.0,
            }))?;
            out.push(Node::assistant(&sid, &turn_id, 0, reply).record()?);
        }
    }
    Ok(out)
}

/// Write about `nodes` nodes into the store at `dir`, then checkpoint.
pub fn generate(dir: &Path, nodes: u64, seed: u64) -> Result<Made> {
    let t0 = Instant::now();
    let store = Store::open(dir)?;
    let mut rng = Rng(seed);
    let vocab = vocabulary(&mut rng);
    let sessions = nodes.div_ceil(PER_SESSION);
    let (mut records, mut next) = (0u64, 0u64);
    let mut s = 0;
    while s < sessions {
        let n = PER_FRAME.min(sessions - s);
        let mut frame = Vec::new();
        for _ in 0..n {
            let take = PER_SESSION.min(nodes - next);
            frame.extend(session(&mut rng, &vocab, next, take)?);
            next += take;
        }
        records += frame.len() as u64;
        store.append(&frame)?;
        s += n;
    }
    store.checkpoint()?;
    Ok(Made {
        sessions,
        nodes,
        records,
        wal_bytes: store.stats()?.wal_bytes,
        ms: t0.elapsed().as_secs_f64() * 1000.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_writes_the_same_texts() {
        let (mut a, mut b) = (Rng(3), Rng(3));
        let (va, vb) = (vocabulary(&mut a), vocabulary(&mut b));
        assert_eq!(va, vb);
        assert_eq!(text(&mut a, &va, 1, 50), text(&mut b, &vb, 1, 50));
    }

    #[test]
    fn a_small_store_writes_and_opens() {
        let d = tempfile::tempdir().unwrap();
        let m = generate(d.path(), 100, 1).unwrap();
        assert_eq!(m.nodes, 100);
        let store = Store::open(d.path()).unwrap();
        let sessions: Vec<SessionRecord> = store.list_sessions().unwrap();
        assert_eq!(sessions.len() as u64, m.sessions);
    }
}
