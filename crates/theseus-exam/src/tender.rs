//! The probe through a running index tender (theseus-emc): each item's task
//! asked of `theseus-index serve` over its socket (`index.query`), so the
//! exam can say what vectors and their fusion find, not only BM25. It speaks
//! the tender's protocol (JSON-RPC over NDJSON, through [`Client`]), never
//! its code, so this crate builds no model.
//!
//! - **The store is the exam's own**: `theseus-exam write-store` wrote it,
//!   the tender follows its WAL, and the manifest maps the tender's node ids
//!   back to the exam's keys (`<item>/<session>.<n>`).
//! - **An arm** is a set of sources, each with an optional weight for the
//!   fusion (`bm25,entity,vector:2`). A source without one takes the
//!   tender's default.
//! - **A node's rank** is its place among the distinct nodes the tender
//!   returns, each at its best chunk (recall admits nodes), from 1. A gold
//!   node past the hits is a miss. Nodes the manifest does not key (the
//!   tender also indexes tool calls) still take their places, as they would
//!   in recall.
//! - **The held-out half is asked only when named** ([`Half::Out`], `--half
//!   out`): it judges once, and never tunes (§2.9).

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{bail, ensure, Context, Result};
use serde::Serialize;
use serde_json::{json, Value};

use crate::client::Client;
use crate::fixture::Manifest;
use crate::item::Exam;
use crate::probe::{self, ItemProbe, KS};

/// The sources a tender ranks.
pub const SOURCES: [&str; 3] = ["bm25", "entity", "vector"];

/// The arms when none is named: lexical, fused, and vectors alone.
pub const DEFAULT_ARMS: [&str; 3] = ["bm25,entity", "bm25,entity,vector", "vector"];

/// A gold node's rank when the tender did not return it.
pub const MISSING: usize = usize::MAX;

/// A set of sources, and the weights asked for some of them.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Arm {
    pub name: String,
    pub sources: Vec<String>,
    /// Empty: the tender's default weights.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub weights: BTreeMap<String, f64>,
}

impl Arm {
    /// `bm25,entity,vector:2`: sources, comma-separated, each with an
    /// optional `:weight`.
    pub fn parse(spec: &str) -> Result<Arm> {
        let mut sources: Vec<String> = Vec::new();
        let mut weights = BTreeMap::new();
        let mut names = Vec::new();
        for part in spec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let (source, weight) = match part.split_once(':') {
                Some((s, w)) => (s.trim(), Some(w.trim())),
                None => (part, None),
            };
            ensure!(
                SOURCES.contains(&source),
                "arm {spec:?}: no source {source:?} (bm25, entity, vector)"
            );
            ensure!(
                !sources.iter().any(|s| s == source),
                "arm {spec:?}: {source} twice"
            );
            sources.push(source.to_string());
            match weight {
                Some(w) => {
                    let w: f64 = w
                        .parse()
                        .with_context(|| format!("arm {spec:?}: the weight {w:?}"))?;
                    ensure!(
                        w.is_finite() && w >= 0.0,
                        "arm {spec:?}: a weight is a finite number, 0 or more"
                    );
                    weights.insert(source.to_string(), w);
                    names.push(format!("{source}×{w}"));
                }
                None => names.push(source.to_string()),
            }
        }
        ensure!(!sources.is_empty(), "arm {spec:?}: no source");
        Ok(Arm {
            name: names.join(" + "),
            sources,
            weights,
        })
    }

    fn uses_vectors(&self) -> bool {
        self.sources.iter().any(|s| s == "vector")
    }
}

/// Which items are asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Half {
    /// The held-in items: what tuning may look at.
    In,
    /// The held-out items: the judgment, once.
    Out,
    All,
}

impl Half {
    pub fn parse(s: &str) -> Result<Half> {
        match s {
            "in" | "held-in" => Ok(Half::In),
            "out" | "held-out" => Ok(Half::Out),
            "all" => Ok(Half::All),
            o => bail!("no half {o:?}: in, out, or all"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Half::In => "held-in",
            Half::Out => "held-out",
            Half::All => "both halves",
        }
    }

    pub fn admits(self, held_out: bool) -> bool {
        match self {
            Half::In => !held_out,
            Half::Out => held_out,
            Half::All => true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub socket: PathBuf,
    pub arms: Vec<Arm>,
    /// Hits asked of the tender (at most 100).
    pub k: usize,
    pub half: Half,
    /// How long a query may wait for the model to load.
    pub wait_ms: u64,
    /// How long to wait for the tender to read the whole store, and to embed
    /// it when an arm uses vectors.
    pub settle: Duration,
}

/// The tender's own times for one arm's queries, in ms.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Times {
    pub total_ms: Vec<f64>,
    pub embed_ms: Vec<f64>,
    pub fuse_ms: Vec<f64>,
}

#[derive(Debug, Clone)]
pub struct ArmRun {
    pub arm: Arm,
    pub items: Vec<ItemProbe>,
    pub times: Times,
    /// The weights the tender says it fused with (its answer's `weights`),
    /// when it says.
    pub weights_used: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct TenderRun {
    pub half: Half,
    pub k: usize,
    /// `index.status` once the tender had read and embedded the store.
    pub status: Value,
    pub arms: Vec<ArmRun>,
}

/// The status when the tender has read the whole store (and embedded it, if
/// `vectors`); `None` while it has not.
fn settled(s: &Value, last_position: u64, vectors: bool) -> Option<()> {
    let ready = s["state"].as_str()? == "ready" && s["lag"]["bytes"].as_u64()? == 0;
    let through = s["position"].as_u64()? >= last_position;
    let embedded = !vectors || {
        let v = &s["vectors"];
        v["pending"].as_u64()? == 0 && v["vectors"].as_u64()? == v["chunks"].as_u64()?
    };
    (ready && through && embedded).then_some(())
}

/// Wait for the tender to hold the exam's store; its status then.
fn settle(c: &mut Client, m: &Manifest, plan: &Plan, log: &mut dyn FnMut(&str)) -> Result<Value> {
    let vectors = plan.arms.iter().any(Arm::uses_vectors);
    let deadline = Instant::now() + plan.settle;
    let mut told = Instant::now();
    loop {
        let s = c.call("index.status", json!({}), Duration::from_secs(30))?;
        if settled(&s, m.last_position, vectors).is_some() {
            return Ok(s);
        }
        if vectors && s["mode"].as_str() == Some("bm25_only") {
            bail!(
                "the tender answers bm25_only ({}): an arm with vectors needs its weights",
                s["vectors"]["last_error"].as_str().unwrap_or("no weights")
            );
        }
        if Instant::now() >= deadline {
            bail!(
                "the tender did not settle in {} s: state {}, position {} of {}, {} chunks, {} vectors, {} pending",
                plan.settle.as_secs(),
                s["state"],
                s["position"],
                m.last_position,
                s["vectors"]["chunks"],
                s["vectors"]["vectors"],
                s["vectors"]["pending"]
            );
        }
        if told.elapsed() >= Duration::from_secs(10) {
            log(&format!(
                "waiting for the tender: state {}, position {} of {}, {} of {} chunks embedded",
                s["state"],
                s["position"],
                m.last_position,
                s["vectors"]["vectors"],
                s["vectors"]["chunks"]
            ));
            told = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// Each returned node's rank among the distinct nodes, from 1, at its best
/// chunk.
pub fn node_ranks(hits: &[Value]) -> Result<HashMap<String, usize>> {
    let mut rank = HashMap::new();
    for h in hits {
        let id = h["node_id"].as_str().context("a hit without a node_id")?;
        let next = rank.len() + 1;
        rank.entry(id.to_string()).or_insert(next);
    }
    Ok(rank)
}

/// Ask the tender every item of `plan.half` with gold, under every arm.
pub fn run(exam: &Exam, m: &Manifest, plan: &Plan, log: &mut dyn FnMut(&str)) -> Result<TenderRun> {
    ensure!(
        m.digest == exam.digest,
        "the manifest is of {} ({}), not this exam ({}): write the store again",
        m.exam,
        m.digest,
        exam.digest
    );
    ensure!(!plan.arms.is_empty(), "no arm");
    let mut c = Client::connect(&plan.socket)?;
    let status = settle(&mut c, m, plan, log)?;
    if plan.arms.iter().any(Arm::uses_vectors) {
        c.call("index.warm", json!({}), Duration::from_secs(30))?;
    }
    let key_of: HashMap<&str, &str> = m
        .nodes
        .iter()
        .map(|(k, e)| (e.node_id.as_str(), k.as_str()))
        .collect();
    let items: Vec<_> = exam
        .file
        .items
        .iter()
        .filter(|i| !i.gold.is_empty() && plan.half.admits(i.held_out))
        .collect();
    let mut runs: Vec<ArmRun> = plan
        .arms
        .iter()
        .map(|a| ArmRun {
            arm: a.clone(),
            items: Vec::new(),
            times: Times::default(),
            weights_used: None,
        })
        .collect();
    let timeout = Duration::from_millis(plan.wait_ms) + Duration::from_secs(30);
    // Item by item, each under every arm, so the arms see the same load.
    for item in &items {
        let gold: Vec<&str> = item
            .gold
            .iter()
            .map(|g| {
                m.nodes
                    .get(&Manifest::key(&item.id, g))
                    .map(|e| e.node_id.as_str())
                    .with_context(|| format!("the manifest has no node {}/{g}", item.id))
            })
            .collect::<Result<_>>()?;
        for r in &mut runs {
            let mut p = json!({
                "text": item.task,
                "k": plan.k,
                "sources": r.arm.sources,
                "wait_ms": plan.wait_ms,
            });
            if !r.arm.weights.is_empty() {
                p["weights"] = json!(r.arm.weights);
            }
            let a = c.call("index.query", p, timeout)?;
            if let Some(s) = a
                .get("skipped")
                .and_then(Value::as_object)
                .filter(|s| !s.is_empty())
            {
                bail!(
                    "{} under {}: the tender skipped {}",
                    item.id,
                    r.arm.name,
                    Value::Object(s.clone())
                );
            }
            let hits = a["hits"]
                .as_array()
                .with_context(|| format!("{}: an answer without hits", item.id))?;
            let rank = node_ranks(hits)?;
            let trap = rank
                .iter()
                .filter_map(|(id, &n)| {
                    let (owner, node) = key_of.get(id.as_str())?.split_once('/')?;
                    (owner == item.id && !item.gold.iter().any(|g| g == node))
                        .then(|| (node.to_string(), n))
                })
                .min_by_key(|(_, n)| *n);
            r.items.push(ItemProbe {
                id: item.id.clone(),
                family: item.family,
                held_out: item.held_out,
                gold_ranks: gold
                    .iter()
                    .map(|g| rank.get(*g).copied().unwrap_or(MISSING))
                    .collect(),
                trap,
            });
            let t = &a["timings"];
            r.times.total_ms.push(t["total_ms"].as_f64().unwrap_or(0.0));
            r.times.embed_ms.push(t["embed_ms"].as_f64().unwrap_or(0.0));
            r.times.fuse_ms.push(t["fuse_ms"].as_f64().unwrap_or(0.0));
            if r.weights_used.is_none() {
                r.weights_used = a.get("weights").cloned();
            }
        }
    }
    Ok(TenderRun {
        half: plan.half,
        k: plan.k,
        status,
        arms: runs,
    })
}

/// The p-th percentile of `xs` (nearest rank).
pub fn percentile(xs: &[f64], p: f64) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    let mut v = xs.to_vec();
    v.sort_by(f64::total_cmp);
    let i = ((p / 100.0) * v.len() as f64).ceil() as usize;
    v[i.clamp(1, v.len()) - 1]
}

fn rank_str(r: usize) -> String {
    if r == MISSING {
        "-".into()
    } else {
        r.to_string()
    }
}

/// The run as Markdown: the tender, then items with all their gold in the
/// top k and gold nodes in the top k, each cell every arm's in order; then
/// the tender's times; with `per_item`, each item's gold ranks.
pub fn render(exam: &Exam, run: &TenderRun, per_item: bool) -> String {
    let mut o = String::new();
    let s = &run.status;
    let v = &s["vectors"];
    let _ = writeln!(
        o,
        "- Exam: {} ({}); the {} items with gold, through a tender (k = {}).",
        exam.file.version,
        exam.digest,
        run.half.as_str(),
        run.k
    );
    let _ = writeln!(
        o,
        "- The tender: {} nodes, {} chunks, mode {}, {} vectors (model {}, engine {}).",
        s["nodes"],
        s["documents"],
        s["mode"].as_str().unwrap_or("?"),
        v["vectors"],
        v["stamp"]["model"].as_str().unwrap_or("none"),
        v["stamp"]["engine"].as_str().unwrap_or("none")
    );
    let legend: Vec<String> = run
        .arms
        .iter()
        .map(|a| match &a.weights_used {
            Some(w) => format!("{} (fused with {})", a.arm.name, w),
            None => a.arm.name.clone(),
        })
        .collect();
    let _ = writeln!(o, "- Arms, in each cell's order: {}.", legend.join("; "));
    let _ = writeln!(o);
    let ks: Vec<String> = KS.iter().map(|k| format!("@{k}")).collect();
    let rows: Vec<Vec<(String, probe::Recall)>> =
        run.arms.iter().map(|a| probe::rows(&a.items)).collect();
    for (title, nodes) in [
        ("Items with all their gold in the top k:", false),
        ("Gold nodes in the top k:", true),
    ] {
        let _ = writeln!(o, "{title}");
        let _ = writeln!(o);
        let _ = writeln!(o, "| | items | gold | {} |", ks.join(" | "));
        let _ = writeln!(o, "|---|---|---|{}", "---|".repeat(KS.len()));
        for (i, (name, first)) in rows[0].iter().enumerate() {
            let cells: Vec<String> = (0..KS.len())
                .map(|j| {
                    rows.iter()
                        .map(|r| {
                            let x = &r[i].1;
                            if nodes { x.nodes[j] } else { x.items_all[j] }.to_string()
                        })
                        .collect::<Vec<_>>()
                        .join(" / ")
                })
                .collect();
            let _ = writeln!(
                o,
                "| {name} | {} | {} | {} |",
                first.items,
                first.gold,
                cells.join(" | ")
            );
        }
        let _ = writeln!(o);
    }
    let _ = writeln!(
        o,
        "The tender's own times per query, ms (p50 / p95): total, the query's embedding, the fusion."
    );
    let _ = writeln!(o);
    let _ = writeln!(o, "| arm | queries | total | embed | fuse |");
    let _ = writeln!(o, "|---|---|---|---|---|");
    for a in &run.arms {
        let t = &a.times;
        let pp = |xs: &[f64]| format!("{:.3} / {:.3}", percentile(xs, 50.0), percentile(xs, 95.0));
        let _ = writeln!(
            o,
            "| {} | {} | {} | {} | {} |",
            a.arm.name,
            t.total_ms.len(),
            pp(&t.total_ms),
            pp(&t.embed_ms),
            pp(&t.fuse_ms)
        );
    }
    if per_item {
        let _ = writeln!(o);
        let _ = writeln!(
            o,
            "Each item's gold ranks, then its best own non-gold node and its rank, per arm in order:"
        );
        let _ = writeln!(o);
        let _ = writeln!(
            o,
            "| item | family | {} |",
            run.arms
                .iter()
                .map(|a| a.arm.name.as_str())
                .collect::<Vec<_>>()
                .join(" | ")
        );
        let _ = writeln!(o, "|---|---|{}", "---|".repeat(run.arms.len()));
        for (i, first) in run.arms[0].items.iter().enumerate() {
            let cells: Vec<String> = run
                .arms
                .iter()
                .map(|a| {
                    let p = &a.items[i];
                    let g: Vec<String> = p.gold_ranks.iter().map(|&r| rank_str(r)).collect();
                    let trap = p
                        .trap
                        .as_ref()
                        .map_or("–".to_string(), |(k, r)| format!("{k} @{r}"));
                    format!("{}; {trap}", g.join(", "))
                })
                .collect();
            let _ = writeln!(
                o,
                "| {} | {} | {} |",
                first.id,
                first.family.as_str(),
                cells.join(" | ")
            );
        }
    }
    o
}

/// The run as JSON: every item's gold ranks under every arm (`null` for a
/// miss), for a later analysis.
pub fn to_json(exam: &Exam, m: &Manifest, run: &TenderRun) -> Value {
    let arms: Vec<Value> = run
        .arms
        .iter()
        .map(|a| {
            let items: Vec<Value> = a
                .items
                .iter()
                .map(|p| {
                    json!({
                        "id": p.id,
                        "family": p.family,
                        "hard": p.family.is_hard(),
                        "held_out": p.held_out,
                        "gold_ranks": p.gold_ranks.iter().map(|&r| (r != MISSING).then_some(r)).collect::<Vec<_>>(),
                        "trap": p.trap,
                    })
                })
                .collect();
            json!({
                "arm": a.arm,
                "weights_used": a.weights_used,
                "items": items,
                "times": a.times,
            })
        })
        .collect();
    json!({
        "exam": exam.file.version,
        "digest": exam.digest,
        "manifest_last_position": m.last_position,
        "half": run.half,
        "k": run.k,
        "status": run.status,
        "arms": arms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;
    use std::sync::{Arc, Mutex};

    /// Three items: two held in (one a paraphrase), one held out.
    const EXAM: &str = r#"
version = "t"
utc_offset_min = -420
[[item]]
id = "fact-1"
family = "fact"
task = "which port does the web ui use"
gold = ["a.1"]
check = 'reply has "7433"'
[[item.session]]
key = "a"
place = "cli"
[[item.session.node]]
at = "2026-09-01 10:00"
who = "eddie"
text = "the web ui listens on 7433"
[[item.session.node]]
at = "2026-09-01 10:01"
who = "eddie"
text = "the web ui once listened on 8080"
[[item]]
id = "paraphrase-1"
family = "paraphrase"
task = "where does the browser front end attach"
gold = ["a.1"]
answer = ["9147"]
check = 'reply has "9147"'
[[item.session]]
key = "a"
place = "cli"
[[item.session.node]]
at = "2026-09-02 10:00"
who = "eddie"
text = "the dashboard binds port 9147"
[[item.session.node]]
at = "2026-09-02 10:01"
who = "eddie"
text = "front end attach browser where does"
[[item]]
id = "fact-2"
family = "fact"
held_out = true
task = "the held out question"
gold = ["a.1"]
check = 'reply has "x"'
[[item.session]]
key = "a"
place = "cli"
[[item.session.node]]
at = "2026-09-03 10:00"
who = "eddie"
text = "held out gold"
"#;

    /// A tender that answers from a script: `index.status`, `index.warm`,
    /// and `index.query`, whose hits are the node ids `answer` gives for the
    /// query; every query's params are kept.
    struct Fake {
        socket: PathBuf,
        seen: Arc<Mutex<Vec<Value>>>,
        _dir: tempfile::TempDir,
    }

    fn fake(status: Value, answer: impl Fn(&Value) -> Value + Send + 'static) -> Fake {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("sock");
        let l = UnixListener::bind(&socket).unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let kept = seen.clone();
        std::thread::spawn(move || {
            let (conn, _) = l.accept().unwrap();
            let mut w = conn.try_clone().unwrap();
            for line in BufReader::new(conn).lines() {
                let Ok(line) = line else { return };
                let req: Value = serde_json::from_str(&line).unwrap();
                let result = match req["method"].as_str().unwrap() {
                    "index.status" => status.clone(),
                    "index.warm" => json!({"model": "loaded", "mode": "hybrid"}),
                    "index.query" => {
                        kept.lock().unwrap().push(req["params"].clone());
                        answer(&req["params"])
                    }
                    m => panic!("{m}"),
                };
                let out = json!({"jsonrpc": "2.0", "id": req["id"], "result": result});
                if w.write_all(format!("{out}\n").as_bytes()).is_err() {
                    return;
                }
            }
        });
        Fake {
            socket,
            seen,
            _dir: dir,
        }
    }

    fn ready(last: u64) -> Value {
        json!({"state": "ready", "lag": {"bytes": 0, "ms": 0}, "position": last, "mode": "hybrid",
               "nodes": 5, "documents": 5,
               "vectors": {"pending": 0, "chunks": 5, "vectors": 5,
                           "stamp": {"model": "tiny@1", "engine": "e"}}})
    }

    fn hits(ids: &[&str]) -> Value {
        json!({"hits": ids.iter().map(|i| json!({"node_id": i, "chunk": 0})).collect::<Vec<_>>(),
               "timings": {"total_ms": 2.0, "embed_ms": 1.5, "fuse_ms": 0.01},
               "weights": {"bm25": 1.0, "entity": 1.0, "vector": 1.0}})
    }

    fn plan(socket: &std::path::Path, arms: &[&str], half: Half) -> Plan {
        Plan {
            socket: socket.to_path_buf(),
            arms: arms.iter().map(|a| Arm::parse(a).unwrap()).collect(),
            k: 100,
            half,
            wait_ms: 1000,
            settle: Duration::from_secs(5),
        }
    }

    fn written() -> (Exam, Manifest, tempfile::TempDir) {
        let exam = Exam::parse(EXAM).unwrap();
        let d = tempfile::tempdir().unwrap();
        let m = fixture::write(&exam, d.path()).unwrap();
        (exam, m, d)
    }

    #[test]
    fn an_arm_names_its_sources_and_weights() {
        let a = Arm::parse("bm25, entity,vector:2").unwrap();
        assert_eq!(a.sources, ["bm25", "entity", "vector"]);
        assert_eq!(a.weights, BTreeMap::from([("vector".to_string(), 2.0)]));
        assert_eq!(a.name, "bm25 + entity + vector×2");
        assert!(Arm::parse("vector").unwrap().weights.is_empty());
        for bad in [
            "",
            "bm25,bm25",
            "words",
            "vector:-1",
            "vector:x",
            "vector:inf",
        ] {
            assert!(Arm::parse(bad).is_err(), "{bad}");
        }
        assert_eq!(Half::parse("held-out").unwrap(), Half::Out);
        assert!(Half::parse("most").is_err());
    }

    /// Ranks are among distinct nodes, at each one's best chunk; a node the
    /// manifest does not key still takes its place; a missing gold is a miss;
    /// the trap is the item's own best non-gold node.
    #[test]
    fn the_probe_maps_hits_to_keys_by_node() {
        let (exam, m, _d) = written();
        let id = |k: &str| m.nodes[k].node_id.clone();
        let (gold, trap) = (id("fact-1/a.1"), id("fact-1/a.2"));
        let (pgold, pother) = (id("paraphrase-1/a.1"), id("paraphrase-1/a.2"));
        let f = fake(ready(m.last_position), move |p| {
            let vector = p["sources"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s == "vector");
            match (p["text"].as_str().unwrap(), vector) {
                // The trap twice (two chunks), an unkeyed node, then the gold.
                ("which port does the web ui use", _) => {
                    hits(&[&trap, &trap, "a-tool-call", &gold])
                }
                // BM25 finds only the decoy; vectors find the gold first.
                ("where does the browser front end attach", false) => hits(&[&pother]),
                ("where does the browser front end attach", true) => hits(&[&pgold, &pother]),
                (t, _) => panic!("asked {t:?}"),
            }
        });
        let run = run(
            &exam,
            &m,
            &plan(
                &f.socket,
                &["bm25,entity", "bm25,entity,vector:2"],
                Half::In,
            ),
            &mut |_| {},
        )
        .unwrap();
        let [lexical, fused] = &run.arms[..] else {
            panic!()
        };
        assert_eq!(lexical.items[0].id, "fact-1");
        assert_eq!(lexical.items[0].gold_ranks, [3]);
        assert_eq!(lexical.items[0].trap, Some(("a.2".into(), 1)));
        assert_eq!(lexical.items[1].gold_ranks, [MISSING]);
        assert_eq!(fused.items[1].gold_ranks, [1]);
        assert_eq!(fused.items[1].trap, Some(("a.2".into(), 2)));
        // The weights went only with the arm that named them.
        let seen = f.seen.lock().unwrap();
        assert_eq!(seen.len(), 4);
        assert!(seen[0].get("weights").is_none());
        assert_eq!(seen[1]["weights"], json!({"vector": 2.0}));
        assert_eq!(seen[1]["sources"], json!(["bm25", "entity", "vector"]));
        assert_eq!(seen[1]["k"], 100);
        // The tables: items with all their gold in the top k, both arms.
        let md = render(&exam, &run, true);
        assert!(md.contains("| paraphrase | 1 | 1 | 0 / 1 |"), "{md}");
        assert!(
            md.contains("| fact | 1 | 1 | 0 / 0 | 1 / 1 | 1 / 1 |"),
            "{md}"
        );
        assert!(
            md.contains("| paraphrase-1 | paraphrase | -; a.2 @1 | 1; a.2 @2 |"),
            "{md}"
        );
        let j = to_json(&exam, &m, &run);
        assert_eq!(j["arms"][0]["items"][1]["gold_ranks"], json!([null]));
        assert_eq!(j["arms"][1]["weights_used"]["vector"], 1.0);
    }

    /// Held in is the default, and the held-out half's task is never sent
    /// unless it is named; then only it is.
    #[test]
    fn the_held_out_half_is_asked_only_when_named() {
        let (exam, m, _d) = written();
        for (half, want) in [
            (
                Half::In,
                vec![
                    "which port does the web ui use",
                    "where does the browser front end attach",
                ],
            ),
            (Half::Out, vec!["the held out question"]),
        ] {
            let f = fake(ready(m.last_position), |_| hits(&[]));
            run(&exam, &m, &plan(&f.socket, &["vector"], half), &mut |_| {}).unwrap();
            let asked: Vec<String> = f
                .seen
                .lock()
                .unwrap()
                .iter()
                .map(|p| p["text"].as_str().unwrap().to_string())
                .collect();
            assert_eq!(asked, want, "{half:?}");
        }
    }

    /// A tender that skipped the vector source, or that has not read the
    /// store through its last position, is an error, not a result.
    #[test]
    fn a_skipped_source_or_a_short_store_is_an_error() {
        let (exam, m, _d) = written();
        let f = fake(
            ready(m.last_position),
            |_| json!({"hits": [], "timings": {}, "skipped": {"vector": "the model is loading"}}),
        );
        let e = run(
            &exam,
            &m,
            &plan(&f.socket, &["vector"], Half::In),
            &mut |_| {},
        )
        .unwrap_err()
        .to_string();
        assert!(
            e.contains("fact-1 under vector") && e.contains("loading"),
            "{e}"
        );
        let f = fake(ready(m.last_position - 1), |_| hits(&[]));
        let mut p = plan(&f.socket, &["bm25"], Half::In);
        p.settle = Duration::from_millis(600);
        let e = run(&exam, &m, &p, &mut |_| {}).unwrap_err().to_string();
        assert!(e.contains("did not settle"), "{e}");
    }

    #[test]
    fn percentiles_by_nearest_rank() {
        let xs: Vec<f64> = (1..=20).map(f64::from).collect();
        assert_eq!(percentile(&xs, 50.0), 10.0);
        assert_eq!(percentile(&xs, 95.0), 19.0);
        assert_eq!(percentile(&xs, 100.0), 20.0);
        assert_eq!(percentile(&[], 50.0), 0.0);
    }
}
