//! The run: items × arms × runs against a scratch daemon over the exam's
//! store, through its socket, as any protocol client drives it (§2.9's "the
//! present: a task sent through the protocol to a scratch daemon on that
//! store").
//!
//! - **Arms.** `none` sends the task alone: today's compiler, which sees only
//!   the session's own transcript. `oracle` sends the item's gold, rendered as
//!   the recall note, before the task (§3.1's 34a row, until 30b exists).
//! - **A cell** is one item under one arm, once: a fresh session, one turn on
//!   the given profile. A call that waits for approval is declined, as an
//!   operator who wants a text answer would; the continuation runs, and the
//!   cell ends when the session's last turn does. The reply is every
//!   assistant text of the session; the calls are its tool calls.
//! - **Pairing.** Cells are ordered by a seeded shuffle, run by run, with an
//!   item's arms next to each other, so both arms of an item meet the same
//!   provider weather.
//! - **Spend.** Each cell's cost is its session's, as the daemon books it
//!   (every turn, continuations and failures included). A cell starts only
//!   while what is spent, plus a reserve for each cell in flight, is under the
//!   limit; the limit counts the records already in the output file.
//! - **Resume.** Cells with a verdict in the output file are skipped, so a run
//!   that stopped (the cap, an abort) goes on where it left off; an errored
//!   cell runs again.
//! - **Preflight.** The manifest must be this exam's; the profile must be the
//!   daemon's live one (a continuation runs on the live profile,
//!   theseus-kol); and every keyed node the manifest names must be served at
//!   its position with its text.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use theseus_protocol::TurnSubmitResult;

use crate::check::{Answer, Call, LineResult};
use crate::client::Client;
use crate::fixture::Manifest;
use crate::item::{Exam, Item};
use crate::render;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Arm {
    None,
    Oracle,
}

impl Arm {
    pub fn as_str(self) -> &'static str {
        match self {
            Arm::None => "none",
            Arm::Oracle => "oracle",
        }
    }

    pub fn parse(s: &str) -> Result<Arm> {
        match s {
            "none" => Ok(Arm::None),
            "oracle" => Ok(Arm::Oracle),
            o => bail!("unknown arm {o:?}: 34a has none and oracle"),
        }
    }
}

/// One cell's record: a JSON line in the run's output.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Record {
    pub exam: String,
    pub digest: String,
    pub item: String,
    pub family: String,
    pub held_out: bool,
    pub arm: String,
    pub run: u32,
    /// None: no verdict (`error` says why).
    pub pass: Option<bool>,
    #[serde(default)]
    pub lines: Vec<LineResult>,
    #[serde(default)]
    pub error: Option<String>,
    pub cost_usd: f64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_read_tokens: u64,
    pub turns: u32,
    pub loops: u32,
    pub tool_calls: u32,
    /// Calls declined for waiting on approval.
    pub declined: u32,
    pub models: Vec<String>,
    pub latency_ms: u64,
    #[serde(default)]
    pub session_id: Option<String>,
    pub input_chars: usize,
    pub reply: String,
    #[serde(default)]
    pub calls: Vec<Value>,
    pub started_at_ms: u64,
}

pub struct Plan {
    pub socket: PathBuf,
    pub profile: String,
    pub arms: Vec<Arm>,
    pub runs: u32,
    /// Item ids; empty: every item.
    pub items: Vec<String>,
    pub limit_usd: f64,
    pub workers: usize,
    pub seed: u64,
    pub timeout: Duration,
    pub out: PathBuf,
}

/// What a cell in flight may still spend, at most: GLM-5.3 Flash at $0.15
/// and $0.50 per million tokens makes a turn a fraction of a cent, so this
/// is generous.
pub const RESERVE_USD: f64 = 0.05;
/// Approvals declined before a cell gives up.
pub const MAX_DECLINES: u32 = 4;
/// A stored reply's cap, enough to score it again.
pub const REPLY_CHARS: usize = 20_000;

const RPC: Duration = Duration::from_secs(30);

/// SplitMix64: a small seeded generator, so a run's order is reproducible.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Fisher–Yates.
    pub fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = (self.next_u64() % (i as u64 + 1)) as usize;
            v.swap(i, j);
        }
    }
}

/// The cells, in run order: run by run, the items shuffled, each item's arms
/// next to each other in a shuffled order.
pub fn order(items: &[&Item], arms: &[Arm], runs: u32, seed: u64) -> Vec<(u32, String, Arm)> {
    let mut rng = Rng::new(seed);
    let mut out = Vec::new();
    for run in 1..=runs {
        let mut ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
        rng.shuffle(&mut ids);
        for id in ids {
            let mut a = arms.to_vec();
            rng.shuffle(&mut a);
            out.extend(a.into_iter().map(|arm| (run, id.to_string(), arm)));
        }
    }
    out
}

/// The records in an output file (a torn last line, from a kill, is skipped).
pub fn read_records(path: &Path) -> Result<Vec<Record>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let f = std::fs::File::open(path).with_context(|| format!("reading {}", path.display()))?;
    let mut out = Vec::new();
    for line in std::io::BufReader::new(f).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(r) = serde_json::from_str::<Record>(&line) {
            out.push(r);
        }
    }
    Ok(out)
}

/// The note the oracle arm shows for `item`, from the manifest.
pub fn oracle_note(m: &Manifest, item: &Item) -> Result<Option<String>> {
    let gold = m.gold(item)?;
    Ok(render::note(&gold, m.utc_offset_min))
}

/// What a turn sends for `item` under `arm`.
pub fn input_for(m: &Manifest, item: &Item, arm: Arm) -> Result<String> {
    let note = match arm {
        Arm::None => None,
        Arm::Oracle => oracle_note(m, item)?,
    };
    Ok(render::input(note.as_deref(), &item.task))
}

/// The daemon serves every keyed node at the manifest's position with its
/// text: the scratch store is the one the manifest describes.
pub fn verify_store(c: &mut Client, m: &Manifest) -> Result<usize> {
    let mut by_session: BTreeMap<&str, Vec<(&String, &crate::fixture::NodeEntry)>> =
        BTreeMap::new();
    for (k, e) in &m.nodes {
        by_session
            .entry(e.session_id.as_str())
            .or_default()
            .push((k, e));
    }
    let mut checked = 0;
    for (sid, entries) in by_session {
        let h = c.call("session.history", json!({"session_id": sid}), RPC)?;
        let nodes = h["nodes"]
            .as_array()
            .context("session.history has no nodes")?;
        for (k, e) in entries {
            let n = nodes
                .iter()
                .find(|n| n["node_id"] == e.node_id.as_str())
                .with_context(|| format!("the daemon does not serve {k} ({})", e.node_id))?;
            ensure!(
                n["position"].as_u64() == Some(e.position)
                    && n["text"].as_str() == Some(e.text.as_str()),
                "the daemon serves {k} differently: position {} and text {:?}",
                n["position"],
                n["text"]
            );
            checked += 1;
        }
    }
    Ok(checked)
}

/// Before a run: the store is this exam's, the profile is live, the nodes
/// are served as written.
pub fn preflight(plan: &Plan, exam: &Exam, m: &Manifest) -> Result<String> {
    ensure!(
        m.digest == exam.digest,
        "the store was written from {} ({}), not this exam ({}): write it again",
        m.exam,
        m.digest,
        exam.digest
    );
    let mut c = Client::connect(&plan.socket)?;
    let p = c.call("profile.list", Value::Null, RPC)?;
    let live = p["live"].as_str().unwrap_or("");
    ensure!(
        live == plan.profile,
        "the daemon's live profile is {live:?}, not {:?}: a continuation runs on the live profile \
         (theseus-kol), so set it first with `theseus --socket … profile use {}`",
        plan.profile,
        plan.profile
    );
    let model = p["profiles"]
        .as_array()
        .and_then(|ps| ps.iter().find(|x| x["name"] == plan.profile.as_str()))
        .and_then(|x| x["model"].as_str())
        .unwrap_or("?")
        .to_string();
    let n = verify_store(&mut c, m)?;
    Ok(format!(
        "profile {} ({model}) is live; {n} keyed nodes served as written",
        plan.profile
    ))
}

struct Acc {
    turns: u32,
    loops: u32,
    tool_calls: u32,
}

impl Acc {
    fn add(&mut self, r: &TurnSubmitResult) {
        self.turns += 1;
        self.loops += r.loops;
        self.tool_calls += r.tool_calls;
    }
}

fn cancel(c: &mut Client, sid: &str) {
    if let Ok(h) = c.call("session.history", json!({"session_id": sid, "n": 1}), RPC) {
        if let Some(x) = h["session"]["execution_id"].as_str() {
            let _ = c.call("execution.cancel", json!({"execution_id": x}), RPC);
        }
    }
}

/// One cell, start to end. Never fails: a failure is the record's `error`.
pub fn run_cell(plan: &Plan, exam: &Exam, m: &Manifest, item: &Item, arm: Arm, run: u32) -> Record {
    let started_at_ms = theseus_protocol::now_unix_ms();
    let t0 = Instant::now();
    let mut rec = Record {
        exam: exam.file.version.clone(),
        digest: exam.digest.clone(),
        item: item.id.clone(),
        family: item.family.as_str().into(),
        held_out: item.held_out,
        arm: arm.as_str().into(),
        run,
        started_at_ms,
        ..Default::default()
    };
    let mut acc = Acc {
        turns: 0,
        loops: 0,
        tool_calls: 0,
    };
    let mut declined = 0;
    let mut sid: Option<String> = None;
    let outcome = (|| -> Result<Client> {
        let input = input_for(m, item, arm)?;
        rec.input_chars = input.chars().count();
        let mut c = Client::connect(&plan.socket)?;
        let label = format!("exam {} {} r{run}", item.id, arm.as_str());
        let info = c.call(
            "session.open",
            json!({"kind": "conversation", "label": label}),
            RPC,
        )?;
        let s = info["session_id"]
            .as_str()
            .context("session.open gave no id")?
            .to_string();
        sid = Some(s.clone());
        let deadline = t0 + plan.timeout;
        let left = || deadline.saturating_duration_since(Instant::now());
        let r: TurnSubmitResult = serde_json::from_value(c.call(
            "turn.submit",
            json!({"session_id": s, "input": input, "profile": plan.profile}),
            left(),
        )?)?;
        acc.add(&r);
        let mut parked = r.awaiting_confirm;
        while let Some(corr) = parked.take() {
            if declined >= MAX_DECLINES {
                bail!("it asked for approval {} times", declined + 1);
            }
            declined += 1;
            c.call(
                "action.confirm",
                json!({"correlation_id": corr, "approve": false, "watch": true}),
                RPC,
            )?;
            loop {
                let Some((method, p)) = c.notification(deadline)? else {
                    bail!("no end to the turn in {} s", plan.timeout.as_secs());
                };
                if p["session_id"].as_str() != Some(s.as_str()) {
                    continue;
                }
                if method == "turn.ended" {
                    let t: TurnSubmitResult = serde_json::from_value(p)?;
                    acc.add(&t);
                    parked = t.awaiting_confirm;
                    break;
                }
                if method == "turn.failed" {
                    bail!(
                        "the continuation failed: {}",
                        p["error"].as_str().unwrap_or("?")
                    );
                }
            }
        }
        Ok(c)
    })();
    rec.latency_ms = t0.elapsed().as_millis() as u64;
    rec.turns = acc.turns;
    rec.loops = acc.loops;
    rec.tool_calls = acc.tool_calls;
    rec.declined = declined;
    rec.session_id = sid.clone();
    let mut c = match outcome {
        Ok(c) => c,
        Err(e) => {
            rec.error = Some(format!("{e:#}"));
            match Client::connect(&plan.socket) {
                Ok(mut c) => {
                    if let Some(s) = &sid {
                        cancel(&mut c, s);
                    }
                    c
                }
                Err(_) => return rec,
            }
        }
    };
    // What the session holds, as the daemon serves it: the reply, the calls,
    // the cost and tokens of every turn.
    let Some(s) = sid else {
        return rec;
    };
    match c.call("session.history", json!({"session_id": s}), RPC) {
        Ok(h) => {
            let si = &h["session"];
            rec.cost_usd = si["cost_usd"].as_f64().unwrap_or(0.0);
            rec.input_tokens = si["usage"]["input_tokens"].as_u64().unwrap_or(0);
            rec.output_tokens = si["usage"]["output_tokens"].as_u64().unwrap_or(0);
            rec.cache_read_tokens = si["usage"]["cache_read_input_tokens"].as_u64().unwrap_or(0);
            let nodes = h["nodes"].as_array().cloned().unwrap_or_default();
            let mut texts = Vec::new();
            let mut models = BTreeSet::new();
            let mut calls = Vec::new();
            for n in &nodes {
                match n["kind"].as_str() {
                    Some("assistant_message") => {
                        if let Some(t) = n["text"].as_str().filter(|t| !t.trim().is_empty()) {
                            texts.push(t.to_string());
                        }
                        if let Some(mdl) = n["detail"]["model"].as_str() {
                            models.insert(mdl.to_string());
                        }
                    }
                    Some("tool_call") => calls.push(Call {
                        tool: n["detail"]["tool"].as_str().unwrap_or("").into(),
                        input: n["detail"]["input"].clone(),
                    }),
                    _ => {}
                }
            }
            let reply = texts.join("\n\n");
            rec.models = models.into_iter().collect();
            rec.calls = calls
                .iter()
                .map(|c| json!({"tool": c.tool, "input": c.input}))
                .collect();
            if rec.error.is_none() {
                let check = &exam.checks[&item.id];
                let a = Answer {
                    reply: reply.clone(),
                    calls,
                    root: None,
                };
                rec.lines = check.run(&a);
                rec.pass = Some(rec.lines.iter().all(|l| l.pass));
            }
            rec.reply = reply.chars().take(REPLY_CHARS).collect();
        }
        Err(e) => {
            let prior = rec
                .error
                .take()
                .map(|p| format!("{p}; "))
                .unwrap_or_default();
            rec.error = Some(format!("{prior}reading the session: {e:#}"));
            rec.pass = None;
        }
    }
    rec
}

struct Shared {
    queue: VecDeque<(u32, String, Arm)>,
    spent: f64,
    in_flight: usize,
    done: usize,
    stopped_at_cap: bool,
}

/// What a run did.
#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub planned: usize,
    pub skipped: usize,
    pub ran: usize,
    pub errors: usize,
    pub spent_before_usd: f64,
    pub spent_usd: f64,
    pub stopped_at_cap: bool,
}

pub fn run(plan: &Plan, exam: &Exam, m: &Manifest) -> Result<Summary> {
    let items: Vec<&Item> = if plan.items.is_empty() {
        exam.file.items.iter().collect()
    } else {
        plan.items
            .iter()
            .map(|id| exam.item(id).with_context(|| format!("no item {id}")))
            .collect::<Result<_>>()?
    };
    let prior = read_records(&plan.out)?;
    let spent_before: f64 = prior.iter().map(|r| r.cost_usd).sum();
    let done: BTreeSet<(String, String, u32)> = prior
        .iter()
        .filter(|r| r.pass.is_some() && r.digest == exam.digest)
        .map(|r| (r.item.clone(), r.arm.clone(), r.run))
        .collect();
    let all = order(&items, &plan.arms, plan.runs, plan.seed);
    let planned = all.len();
    let queue: VecDeque<_> = all
        .into_iter()
        .filter(|(run, id, arm)| !done.contains(&(id.clone(), arm.as_str().to_string(), *run)))
        .collect();
    let skipped = planned - queue.len();
    let shared = Arc::new(Mutex::new(Shared {
        queue,
        spent: spent_before,
        in_flight: 0,
        done: 0,
        stopped_at_cap: false,
    }));
    let out = Arc::new(Mutex::new(
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&plan.out)
            .with_context(|| format!("opening {}", plan.out.display()))?,
    ));
    let total = planned - skipped;
    let errors = Arc::new(Mutex::new(0usize));
    std::thread::scope(|scope| {
        for _ in 0..plan.workers.max(1) {
            let (shared, out, errors) = (shared.clone(), out.clone(), errors.clone());
            scope.spawn(move || loop {
                let next = {
                    let mut s = shared.lock().expect("lock");
                    if s.spent + (s.in_flight as f64 + 1.0) * RESERVE_USD > plan.limit_usd {
                        if !s.queue.is_empty() {
                            s.stopped_at_cap = true;
                        }
                        None
                    } else {
                        let n = s.queue.pop_front();
                        if n.is_some() {
                            s.in_flight += 1;
                        }
                        n
                    }
                };
                let Some((run, id, arm)) = next else {
                    break;
                };
                let item = exam.item(&id).expect("planned from the exam");
                let r = run_cell(plan, exam, m, item, arm, run);
                let line = serde_json::to_string(&r).expect("a record serializes");
                {
                    let mut f = out.lock().expect("lock");
                    let _ = writeln!(f, "{line}");
                    let _ = f.flush();
                }
                let (k, spent) = {
                    let mut s = shared.lock().expect("lock");
                    s.in_flight -= 1;
                    s.done += 1;
                    s.spent += r.cost_usd;
                    (s.done, s.spent)
                };
                if r.error.is_some() {
                    *errors.lock().expect("lock") += 1;
                }
                let verdict = match (r.pass, &r.error) {
                    (Some(true), _) => "pass".to_string(),
                    (Some(false), _) => "FAIL".to_string(),
                    (None, Some(e)) => format!("ERROR {}", e.chars().take(160).collect::<String>()),
                    (None, None) => "no verdict".to_string(),
                };
                println!(
                    "[{k}/{total}] {id} {} r{run}: {verdict}  ${:.4}  {:.1}s  {} loops{}  (spent ${spent:.4})",
                    arm.as_str(),
                    r.cost_usd,
                    r.latency_ms as f64 / 1000.0,
                    r.loops,
                    if r.declined > 0 { format!(", {} declined", r.declined) } else { String::new() },
                );
            });
        }
    });
    let s = shared.lock().expect("lock");
    let errors = *errors.lock().expect("lock");
    Ok(Summary {
        planned,
        skipped,
        ran: s.done,
        errors,
        spent_before_usd: spent_before,
        spent_usd: s.spent,
        stopped_at_cap: s.stopped_at_cap,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::EXAM_V1;

    #[test]
    fn the_order_pairs_each_items_arms_and_is_reproducible() {
        let exam = Exam::parse(EXAM_V1).unwrap();
        let items: Vec<&Item> = exam.file.items.iter().collect();
        let a = order(&items, &[Arm::None, Arm::Oracle], 3, 7);
        assert_eq!(a.len(), 240);
        assert_eq!(
            a,
            order(&items, &[Arm::None, Arm::Oracle], 3, 7),
            "same seed, same order"
        );
        assert_ne!(
            a,
            order(&items, &[Arm::None, Arm::Oracle], 3, 8),
            "another seed, another order"
        );
        for pair in a.chunks(2) {
            assert_eq!(
                (pair[0].0, &pair[0].1),
                (pair[1].0, &pair[1].1),
                "an item's arms are adjacent"
            );
            assert_ne!(pair[0].2, pair[1].2);
        }
        // Every cell once; runs in order.
        let set: BTreeSet<_> = a.iter().cloned().collect();
        assert_eq!(set.len(), 240);
        assert!(a.windows(2).all(|w| w[0].0 <= w[1].0));
        // Both arms lead about half the time (a fair coin per item).
        let oracle_first = a.chunks(2).filter(|p| p[0].2 == Arm::Oracle).count();
        assert!((40..=80).contains(&oracle_first), "{oracle_first} of 120");
    }

    #[test]
    fn records_read_back_and_a_torn_last_line_is_skipped() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("runs.jsonl");
        let r = Record {
            item: "fact-1".into(),
            arm: "oracle".into(),
            run: 1,
            pass: Some(true),
            cost_usd: 0.001,
            ..Default::default()
        };
        let line = serde_json::to_string(&r).unwrap();
        std::fs::write(&p, format!("{line}\n{line}\n{}", &line[..20])).unwrap();
        let back = read_records(&p).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(
            (back[0].item.as_str(), back[0].pass),
            ("fact-1", Some(true))
        );
        assert!(read_records(&d.path().join("none.jsonl"))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn arms_parse_by_name() {
        assert_eq!(Arm::parse("none").unwrap(), Arm::None);
        assert_eq!(Arm::parse("oracle").unwrap(), Arm::Oracle);
        assert!(Arm::parse("bm25")
            .unwrap_err()
            .to_string()
            .contains("none and oracle"));
    }
}
