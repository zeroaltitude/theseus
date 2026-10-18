//! The writer's brief and the split behind it (§2.5, "Tests come from the
//! recorded instances"). The occurrences are split by a keyed hash, as
//! self-exams splits its items (`exams::half`), into a **build half** the
//! writer sees and a **check half** it never sees. Code keeps it so: the
//! brief is built by a function that is given the build half and nothing
//! else, and a test asserts on its bytes.
//!
//! The split is by rank, not by coin: the occurrences are ordered by their
//! keyed hash and every other one is the check half, starting with the first,
//! so the halves are as even as they can be (the check half takes the odd
//! one: the dry replay wants at least three instances), and the same key
//! always splits the same occurrences the same way. The key is the META
//! record `self.mint.key`, made on first use; nothing the writer can read
//! holds it.
//!
//! The check half lives at `<state>/self/mint/<name>/check/`, open data for
//! the validator, never in a brief.

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::json;
use sha2::{Digest, Sha256};

use super::kind::Kind;
use crate::rsi::exams::SplitKey;
use crate::rsi::mine::Occurrence;

/// The two halves of a motif's occurrences.
#[derive(Debug)]
pub struct Halves<'a> {
    pub build: Vec<&'a Occurrence>,
    pub check: Vec<&'a Occurrence>,
}

/// An occurrence's id: its session, its turn (a bench trial has none) and the
/// WAL positions it spans. Evidence, never a value.
pub fn occurrence_id(o: &Occurrence) -> String {
    if o.turn.is_empty() {
        format!("{}@{}-{}", o.session, o.first, o.last)
    } else {
        format!("{}/{}@{}-{}", o.session, o.turn, o.first, o.last)
    }
}

fn rank_of(key: &SplitKey, motif: &str, o: &Occurrence) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(key.0);
    h.update(b"mint-split\0");
    h.update(motif.as_bytes());
    h.update([0]);
    h.update(occurrence_id(o).as_bytes());
    h.finalize().into()
}

/// Split `occurrences` of the motif `digest` under `key`.
pub fn split<'a>(key: &SplitKey, digest: &str, occurrences: &'a [Occurrence]) -> Halves<'a> {
    let mut ranked: Vec<(&'a Occurrence, [u8; 32])> = occurrences
        .iter()
        .map(|o| (o, rank_of(key, digest, o)))
        .collect();
    ranked.sort_by_key(|r| r.1);
    let mut h = Halves {
        build: Vec::new(),
        check: Vec::new(),
    };
    for (i, (o, _)) in ranked.into_iter().enumerate() {
        if i % 2 == 0 {
            h.check.push(o);
        } else {
            h.build.push(o);
        }
    }
    h
}

/// The brief for the `"self"` session, from the skeleton's JSON (the flow
/// spec for a flow, `Motif::spec` for another kind) and the build half
/// **only**: the session names the tool, writes its description as a
/// contract, settles ambiguous slot types and, for MCP or Rust, writes the
/// code. The skeleton holds counts and costs, never an id; the ids are the
/// build half's.
pub fn brief(skeleton: &serde_json::Value, kind: Kind, build: &[&Occurrence]) -> String {
    let skeleton = serde_json::to_string_pretty(skeleton).unwrap_or_default();
    let ids: Vec<String> = build
        .iter()
        .map(|o| format!("- {}", occurrence_id(o)))
        .collect();
    let work = match kind {
        Kind::Flow => {
            "Write the flow's name and its description. The steps, their bindings, the \
             inputs, the branches and the exits are mined and stay as they are. End your \
             reply with one line, MINT followed by a JSON object with a \"name\" and a \
             \"description\", and, only for an input whose type (ref, path, num, str, argv) \
             you find ambiguous, \"types\": {\"<input>\": \"<type>\"}. Nothing after it."
        }
        Kind::Mcp => {
            "Write an MCP extension that does this work in one call: it runs in L1, reads \
             only, has no network and no secret. Name it, describe it, give it declared \
             tests (up to 20), and propose it with extend.propose."
        }
        Kind::Rust => {
            "Write a Rust built-in that does this work. Name it, describe it, and propose \
             the change with self.propose_change; it lands through the nightly build."
        }
    };
    format!(
        "Mint a tool from a recurring sequence of tool calls (kind: {kind}).\n\n\
         {work}\n\n\
         The name is lower case letters, digits and single underscores, starting with a \
         letter. The description states the contract: what the tool takes, what it does, \
         and what it returns or hands back. It never says when to prefer it or why it is \
         better: a model is never told to use a tool.\n\n\
         The skeleton the miner wrote (evidence is counts and costs):\n{skeleton}\n\n\
         The recorded instances you may look at, by id ({n}):\n{ids}\n\n\
         Other recorded instances are held back to check the tool; you do not see them.\n",
        kind = kind.name(),
        n = ids.len(),
        ids = ids.join("\n"),
    )
}

/// Where a motif's check half lives: `<mint root>/<name>/check/`, the mint
/// root being `<state>/self/mint` (`flow::Board::root`).
pub fn check_dir(root: &Path, name: &str) -> PathBuf {
    root.join(name).join("check")
}

/// Write the check half as open data: the occurrences' ids and positions.
pub fn write_check(root: &Path, name: &str, check: &[&Occurrence]) -> Result<PathBuf> {
    let dir = check_dir(root, name);
    std::fs::create_dir_all(&dir)?;
    let rows: Vec<_> = check
        .iter()
        .map(|o| {
            json!({"id": occurrence_id(o), "session": o.session, "turn": o.turn,
                   "day": o.day, "first": o.first, "last": o.last})
        })
        .collect();
    let path = dir.join("occurrences.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&rows)?)?;
    Ok(path)
}

/// Read the check half back.
pub fn read_check(root: &Path, name: &str) -> Result<Vec<Occurrence>> {
    let bytes = std::fs::read(check_dir(root, name).join("occurrences.json"))?;
    let rows: Vec<serde_json::Value> = serde_json::from_slice(&bytes)?;
    Ok(rows
        .iter()
        .map(|r| Occurrence {
            session: r["session"].as_str().unwrap_or_default().into(),
            turn: r["turn"].as_str().unwrap_or_default().into(),
            day: r["day"].as_str().unwrap_or_default().into(),
            first: r["first"].as_u64().unwrap_or(0),
            last: r["last"].as_u64().unwrap_or(0),
        })
        .collect())
}
