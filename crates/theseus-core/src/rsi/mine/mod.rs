//! The pattern miner (theseus-pw1q.21.3; RSI loop 1, §2): the nightly read
//! that finds recurring tool-call work in the ledger and the bench and writes
//! tool skeletons with **no model anywhere in it**. Everything below is
//! counting, alignment and arithmetic over what the record already holds.
//!
//! **What it reads** ([`read`]): the transcript's tool calls and results, the
//! assistant nodes' usage and cost, `provider.call`'s timing, the session's
//! directory (the repository is the scope), and the bench's ATIF trajectories
//! ([`bench`]). It reads incrementally: a cursor (META `self.mine.cursor`)
//! and a compact list of each day's runs (META `self.mine.shapes.<date>`,
//! token ids and keyed digests, never a value), so a night costs what the
//! day's calls cost and the 14-day window is the union of the day lists.
//!
//! **What it finds** ([`motif`], [`align`]): each run (a turn's call list) is
//! split into *stretches* at the calls the model authored and at the calls it
//! never mines; every contiguous 3-to-16-gram of a stretch is counted once per
//! run; closed patterns that reach the thresholds are aligned to their medoid
//! (core steps in at least 60% of the occurrences, optional steps in at least
//! 25%), branched on result class, and given the arms and hand-back exits the
//! occurrences show; slots are classified across the occurrences
//! ([`slots`]); cost and saving are attributed to the loops that emitted the
//! calls ([`cost`]).
//!
//! **What it keeps**: a [`Motif`] and its skeleton spec ([`Motif::spec`], §2.5's
//! JSON without a name or a description: those are the mint step's model
//! work). The types here (`Motif`, `Shape`, `SlotKind`, `Occurrence`, the
//! spec) are the contract the mint step reads (R7).
//!
//! **Hygiene.** Raw values are held in memory while a night folds the calls
//! and are never stored. A shape holds a tool, a program and its subcommand
//! words, flag names and typed placeholders. A slot's value is kept as a
//! keyed digest, and as text only when it is a *safe constant*: short, it
//! passes the daemon's scrubber and the shape check (no absolute path outside
//! the repository, no person-like string, no secret shape). Evidence is
//! positions, never values.
//!
//! **Off by default.** `[self.mint] mine = false` runs nothing. The nightly
//! entry also asks [`super::gate`] and reads nothing with the mode off or the
//! switch halted; `theseus self patterns` is the owner's read-only run on
//! demand and writes nothing, not even the cursor.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub mod align;
pub mod bench;
pub mod cost;
pub mod motif;
mod night;
pub mod read;
pub mod shape;
pub mod slots;

pub use cost::{Cost, Saving, Tokens};
pub use night::row_of;
pub use shape::{Shape, SlotShape, SlotType};
pub use slots::{Bound, SlotBinding, SlotKind};

/// The mining window, in days (`[self.mint] window_days`).
pub const WINDOW_DAYS: u32 = 14;
/// A bench motif needs this many trials across this many tasks (§2.3.5).
pub const BENCH_TRIALS: u32 = 4;
pub const BENCH_TASKS: u32 = 3;
/// A value's text is kept as a possible constant only when it recurs this
/// often in the window (a constant needs at least four of five occurrences).
pub const MIN_CONSTANT_USES: u32 = 3;
/// The motifs a night keeps, best first.
pub const MAX_MOTIFS: usize = 50;
/// A seed pattern is this long, in calls (§2.3.2).
pub const MIN_LEN: usize = 3;
pub const MAX_LEN: usize = 16;
/// An aligned step is core at this share of the occurrences, optional at the
/// second (§2.3.3).
pub const CORE_SHARE: f64 = 0.6;
pub const OPTIONAL_SHARE: f64 = 0.25;

/// A token's id: the first eight bytes of the SHA-256 of its text, never 0.
pub fn id_of(text: &str) -> u64 {
    let d = Sha256::digest(text.as_bytes());
    let v = u64::from_be_bytes(d[..8].try_into().unwrap_or_default());
    v.max(1)
}

/// A value's keyed digest: equal values have equal digests under one key,
/// and a digest says nothing of its value without the key. 0 is reserved for
/// a value the miner would not keep.
pub fn digest_of(key: &[u8], value: &str) -> u64 {
    let mut h = Sha256::new();
    h.update(key);
    h.update([0]);
    h.update(value.as_bytes());
    u64::from_be_bytes(h.finalize()[..8].try_into().unwrap_or_default()).max(1)
}

/// A call's flags in a stored run.
pub const AUTHORED: u8 = 1;
pub const NEVER: u8 = 2;

/// One call as the miner stores it: token ids and digests, no values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Call {
    /// The call's shape (`id_of` its text; the text is in the vocabulary).
    #[serde(rename = "s")]
    pub shape: u64,
    /// The result's class (`id_of` its text).
    #[serde(rename = "r")]
    pub res: u64,
    /// The keyed digest of each slot's value, in the shape's slot order.
    #[serde(rename = "v", default, skip_serializing_if = "Vec::is_empty")]
    pub slots: Vec<u64>,
    /// Digests of the references and paths the result named.
    #[serde(rename = "o", default, skip_serializing_if = "Vec::is_empty")]
    pub outs: Vec<u64>,
    /// The call node's position in the WAL: the evidence.
    #[serde(rename = "p")]
    pub pos: u64,
    /// The model loop that emitted it (an index into the run's loops).
    #[serde(rename = "l", default)]
    pub loop_ix: u32,
    /// How long the call ran.
    #[serde(rename = "m", default, skip_serializing_if = "is_zero")]
    pub ms: u32,
    /// `AUTHORED` or `NEVER`.
    #[serde(rename = "f", default, skip_serializing_if = "is_zero_u8")]
    pub flags: u8,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}
fn is_zero_u8(n: &u8) -> bool {
    *n == 0
}

impl Call {
    /// Whether the call cuts a stretch.
    pub fn cuts(&self) -> bool {
        self.flags != 0
    }
}

/// One model loop of a run: what it cost, kept as its parts (§2.4).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Loop {
    /// Uncached input, cache reads, cache writes, output.
    #[serde(rename = "t")]
    pub tokens: [u64; 4],
    #[serde(rename = "u")]
    pub usd_micros: u64,
    /// The model's time for the call (`provider.call`'s `total_ms`).
    #[serde(rename = "m", default)]
    pub model_ms: u64,
    /// The calls it emitted that the run holds.
    #[serde(rename = "n", default)]
    pub calls: u32,
}

/// An execution: one turn's calls, in order, in one scope.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Run {
    /// The session (or, in the bench, the trial).
    pub session: String,
    /// The turn; empty for a bench trial.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub turn: String,
    /// A bench trial's task.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub task: String,
    /// The local day, `2026-10-17`.
    pub day: String,
    /// The repository, `r:5f1c…`, or `bench:<suite>`.
    pub scope: String,
    /// The place class: `cli`, `dm`, `channel`, `task`.
    pub place: String,
    pub calls: Vec<Call>,
    #[serde(default)]
    pub loops: Vec<Loop>,
    /// When its last call ran (unix ms).
    #[serde(default)]
    pub last_ms: u64,
}

impl Run {
    /// Whether this run's identity is `other`'s: a turn that went on across
    /// two nights is one run.
    pub fn same(&self, other: &Run) -> bool {
        (
            &self.session,
            &self.turn,
            &self.task,
            &self.day,
            &self.scope,
        ) == (
            &other.session,
            &other.turn,
            &other.task,
            &other.day,
            &other.scope,
        )
    }
}

/// The names of the tokens: what the ids stand for. Shape texts and result
/// classes are public by construction; a constant's text is here only when it
/// passed the scrubber and the shape check.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Vocab {
    #[serde(default)]
    pub shapes: BTreeMap<String, Shape>,
    #[serde(default)]
    pub results: BTreeMap<String, String>,
    #[serde(default)]
    pub constants: BTreeMap<String, String>,
}

fn hex(id: u64) -> String {
    format!("{id:016x}")
}

impl Vocab {
    pub fn shape(&self, id: u64) -> Option<&Shape> {
        self.shapes.get(&hex(id))
    }
    pub fn result(&self, id: u64) -> &str {
        self.results.get(&hex(id)).map_or("?", String::as_str)
    }
    pub fn constant(&self, id: u64) -> Option<String> {
        self.constants.get(&hex(id)).cloned()
    }
    pub fn add_shape(&mut self, s: &Shape) -> u64 {
        let id = id_of(&s.text);
        self.shapes.entry(hex(id)).or_insert_with(|| s.clone());
        id
    }
    pub fn add_result(&mut self, class: &str) -> u64 {
        let id = id_of(class);
        self.results
            .entry(hex(id))
            .or_insert_with(|| class.to_string());
        id
    }
    pub fn add_constant(&mut self, digest: u64, text: &str) {
        self.constants
            .entry(hex(digest))
            .or_insert_with(|| text.to_string());
    }
    /// Keep only what `runs` use. A constant's text stays only when its value
    /// recurs ([`MIN_CONSTANT_USES`]): a value seen once or twice cannot be a
    /// constant, so its text is not kept.
    pub fn retain(&mut self, runs: &[Run]) {
        let (mut sh, mut re) = (
            std::collections::BTreeSet::new(),
            std::collections::BTreeSet::new(),
        );
        let mut co: BTreeMap<String, u32> = BTreeMap::new();
        for c in runs.iter().flat_map(|r| &r.calls) {
            sh.insert(hex(c.shape));
            re.insert(hex(c.res));
            for d in &c.slots {
                *co.entry(hex(*d)).or_default() += 1;
            }
        }
        self.shapes.retain(|k, _| sh.contains(k));
        self.results.retain(|k, _| re.contains(k));
        self.constants
            .retain(|k, _| co.get(k).is_some_and(|n| *n >= MIN_CONSTANT_USES));
    }
    /// Fold another vocabulary in.
    pub fn merge(&mut self, other: &Vocab) {
        for (k, v) in &other.shapes {
            self.shapes.entry(k.clone()).or_insert_with(|| v.clone());
        }
        for (k, v) in &other.results {
            self.results.entry(k.clone()).or_insert_with(|| v.clone());
        }
        for (k, v) in &other.constants {
            self.constants.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
}

/// The thresholds and the window (`[self.mint]`).
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    pub min_occurrences: u32,
    pub min_sessions: u32,
    pub min_days: u32,
    pub window_days: u32,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            min_occurrences: 5,
            min_sessions: 3,
            min_days: 2,
            window_days: WINDOW_DAYS,
        }
    }
}

/// How many times a motif was seen, and by how many of what.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Support {
    /// Runs (executions) that hold it, each counted once.
    pub occurrences: u32,
    /// Distinct sessions (a bench motif's: trials).
    pub sessions: u32,
    /// Distinct days.
    pub days: u32,
    /// Distinct bench tasks; 0 for the ledger.
    pub tasks: u32,
}

/// One occurrence: where the evidence is, never what it held.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Occurrence {
    pub session: String,
    pub turn: String,
    pub day: String,
    /// The first and last call node's WAL position (0 in the bench).
    pub first: u64,
    pub last: u64,
}

/// One step of a motif: an aligned column.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    /// A short name, unique in the motif (`merge`, `test`, `test2`).
    pub id: String,
    pub shape: Shape,
    /// In at least 60% of the occurrences (else optional, at least 25%).
    pub core: bool,
    /// The share of occurrences that have it.
    pub presence: f64,
    pub slots: Vec<SlotBinding>,
}

/// How an arm ends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "why", rename_all = "snake_case")]
pub enum Exit {
    /// The flow ran to its end.
    Ok,
    /// The arm ends in work the model does: the tool stops and hands back its
    /// state, the class that led there.
    Handback(String),
    /// The arm ends in an error.
    Err(String),
}

impl Exit {
    pub fn word(&self) -> String {
        match self {
            Exit::Ok => "ok".into(),
            Exit::Handback(w) => format!("handback:{w}"),
            Exit::Err(w) => format!("exit:{w}"),
        }
    }
}

/// One way a branch goes on: the result class at the step, the steps that
/// followed, how many occurrences, and how it ends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Arm {
    /// The result class at the branching step, `err:1+conflict+rerere-left`.
    pub on: String,
    /// The ids of the steps that came next.
    pub then: Vec<String>,
    pub count: u32,
    pub exit: Exit,
}

/// A core step whose occurrences split by result class.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Branch {
    /// The step's id.
    pub at: String,
    pub arms: Vec<Arm>,
}

/// An input of the tool: an INPUT slot, named and typed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Input {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: SlotType,
    /// The step and slot that takes it.
    pub step: String,
    pub slot: usize,
}

/// A recurring flow of tool calls, with everything the mint step needs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Motif {
    /// The repository (`r:…`) or `bench:<suite>`.
    pub scope: String,
    /// `sha256:` of the skeleton spec: the motif's identity.
    pub digest: String,
    /// Born in the bench's trials alone: generic, and labelled so.
    pub bench_born: bool,
    /// The place classes it was seen in.
    pub places: Vec<String>,
    /// The seed: the contiguous calls the pattern was found by.
    pub seed: Vec<Shape>,
    pub steps: Vec<Step>,
    pub branches: Vec<Branch>,
    pub inputs: Vec<Input>,
    pub support: Support,
    /// What one occurrence cost, on average.
    pub cost: Cost,
    pub saving: Saving,
    /// When the newest occurrence ran (unix ms).
    pub last_ms: u64,
    pub occurrences: Vec<Occurrence>,
}

impl Motif {
    /// The skeleton spec (§2.5), data only: the steps with their bindings,
    /// the inputs, the branches and exits, the evidence and the cost. It has no
    /// name and no description: the mint step's model writes those. Its
    /// `digest` is the SHA-256 of the rest, in canonical form.
    pub fn spec(&self) -> Value {
        let mut inputs = serde_json::Map::new();
        for i in &self.inputs {
            inputs.insert(i.name.clone(), json!({"type": i.ty.input_name()}));
        }
        let steps: Vec<Value> = self
            .steps
            .iter()
            .enumerate()
            .map(|(ix, s)| self.step_spec(ix, s))
            .collect();
        let mut exits = serde_json::Map::new();
        for b in &self.branches {
            for a in &b.arms {
                if let Exit::Handback(w) = &a.exit {
                    exits.insert(
                        format!("handback:{w}"),
                        json!({"state": ["conflict_files", "resolved_files"], "at": b.at}),
                    );
                }
            }
        }
        exits.insert("ok".into(), json!("done"));
        let mut v = json!({
            "v": 1, "kind": "flow",
            "scope": {"repos": [self.scope], "places": self.places},
            "inputs": inputs, "steps": steps, "exits": exits,
            "evidence": {
                "occurrences": self.support.occurrences, "sessions": self.support.sessions,
                "days": self.support.days,
                "tokens_per_occ": self.cost.tokens.total(),
                "usd_per_occ": round(self.cost.usd, 4),
                "secs_per_occ": round(self.cost.model_secs + self.cost.tool_secs, 1),
                "decisions_per_occ": round(self.cost.decisions, 1),
            },
        });
        // The digest names the structure; the evidence changes every night.
        let mut body = v.clone();
        body.as_object_mut().map(|o| o.remove("evidence"));
        let d = format!("sha256:{}", hex::encode(Sha256::digest(body.to_string())));
        v["digest"] = json!(d);
        v
    }

    /// The backlog cause this motif is filed under: stable while its seed is,
    /// whatever the nights add to its counts.
    pub fn cause_key(&self) -> String {
        let mut h = Sha256::new();
        h.update(self.scope.as_bytes());
        for s in &self.seed {
            h.update([0]);
            h.update(s.text.as_bytes());
        }
        format!("pattern:{}:{}", self.scope, hex::encode(&h.finalize()[..6]))
    }

    fn step_spec(&self, ix: usize, s: &Step) -> Value {
        let argv_of = |s: &Step| -> Vec<Value> {
            let mut out: Vec<Value> = s.shape.head.split(' ').map(|w| json!(w)).collect();
            let mut slot = 0usize;
            // Flags first, a valued one with its slot; then the positionals.
            for f in &s.shape.flags {
                out.push(json!(f.trim_end_matches('=')));
                if f.ends_with('=') {
                    out.push(self.binding(ix, s, slot));
                    slot += 1;
                }
            }
            while slot < s.slots.len() {
                out.push(self.binding(ix, s, slot));
                slot += 1;
            }
            out
        };
        let mut o = serde_json::Map::new();
        o.insert("id".into(), json!(s.id));
        o.insert("tool".into(), json!(s.shape.tool));
        let args = if s.shape.tool == "proc.run" {
            json!({ "argv": argv_of(s) })
        } else {
            let mut m = serde_json::Map::new();
            for k in 0..s.slots.len() {
                m.insert(format!("arg{k}"), self.binding(ix, s, k));
            }
            Value::Object(m)
        };
        o.insert("args".into(), args);
        if !s.core {
            o.insert("optional".into(), json!(true));
        }
        if let Some(b) = self.branches.iter().find(|b| b.at == s.id) {
            let mut on = serde_json::Map::new();
            for a in &b.arms {
                let goto = match (&a.exit, a.then.first()) {
                    (Exit::Ok, Some(n)) => format!("goto:{n}"),
                    (Exit::Ok, None) => "exit:ok".into(),
                    (e, _) => e.word(),
                };
                on.insert(a.on.clone(), json!(goto));
            }
            o.insert("on".into(), Value::Object(on));
        }
        Value::Object(o)
    }

    /// A slot as the spec binds it: a constant's text, `$name` for an input,
    /// `$step.slot` for a slot bound to an earlier one.
    fn binding(&self, ix: usize, s: &Step, k: usize) -> Value {
        let Some(b) = s.slots.get(k) else {
            return Value::Null;
        };
        match (b.kind, &b.constant, &b.bound) {
            (SlotKind::Constant, Some(c), _) => json!(c),
            (SlotKind::Bound, _, Some(Bound::Slot { step, slot })) => {
                let other = self.steps.get(*step).filter(|_| *step < ix);
                match other.and_then(|o| {
                    self.inputs
                        .iter()
                        .find(|i| i.step == o.id && i.slot == *slot)
                }) {
                    Some(i) => json!(format!("${}", i.name)),
                    None => json!(format!("${}.{slot}", other.map_or("?", |o| o.id.as_str()))),
                }
            }
            (SlotKind::Bound, _, Some(Bound::Result { step })) => json!(format!(
                "$result.{}",
                self.steps.get(*step).map_or("?", |o| o.id.as_str())
            )),
            _ => match self.inputs.iter().find(|i| i.step == s.id && i.slot == k) {
                Some(i) => json!(format!("${}", i.name)),
                None => json!("$?"),
            },
        }
    }
}

fn round(x: f64, places: i32) -> f64 {
    let m = 10f64.powi(places);
    (x * m).round() / m
}

/// The result of a mining pass.
#[derive(Debug, Clone, Default)]
pub struct Mined {
    pub motifs: Vec<Motif>,
    /// Runs and calls read from the window.
    pub runs: usize,
    pub calls: usize,
    pub window_days: u32,
}

/// Find the motifs in `runs`: the whole pass, with no model call. `now_ms`
/// dates the recency of a motif's last occurrence.
pub fn mine(runs: &[Run], vocab: &Vocab, p: &Params, now_ms: u64) -> Mined {
    let mut by_scope: BTreeMap<&str, Vec<&Run>> = BTreeMap::new();
    for r in runs {
        by_scope.entry(r.scope.as_str()).or_default().push(r);
    }
    let mut motifs = Vec::new();
    for (scope, rs) in by_scope {
        motifs.extend(motif::find(scope, &rs, vocab, p));
    }
    motifs.sort_by(|a, b| {
        b.score(now_ms, p.window_days)
            .cmp(&a.score(now_ms, p.window_days))
            .then_with(|| a.digest.cmp(&b.digest))
    });
    motifs.truncate(MAX_MOTIFS);
    Mined {
        motifs,
        runs: runs.len(),
        calls: runs.iter().map(|r| r.calls.len()).sum(),
        window_days: p.window_days,
    }
}

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_mine;
