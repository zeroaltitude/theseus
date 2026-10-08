//! The metrics, aggregated in the process with cumulative temporality: the
//! instruments, units, and attributes the OpenTelemetry SDK exported before
//! theseus-hee, with theseus-yf1's corrections: duration bounds up to 10
//! minutes, the provider call's provider and model, each tool call's name,
//! family, backend, outcome, and time, the tool calls of a failed turn, and a
//! finished turn's requested model. A turn records into them when it ends; the
//! sender exports every point each `metrics_interval_secs`, from the
//! pipeline's start.

use std::collections::BTreeMap;

use theseus_protocol::{Span, TurnSubmitResult, Usage};

use super::otlp::{self, AnyValue, KeyValue};
use super::spans::{self, semconv};
use super::FailedTurn;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    IntSum,
    DoubleSum,
    Histogram,
    /// The last value set, exported as a non-monotonic sum: a gauge.
    IntLast,
}

struct Instrument {
    name: &'static str,
    description: &'static str,
    unit: &'static str,
    kind: Kind,
}

const TURNS: Instrument = Instrument {
    name: "theseus.turns",
    description: "Turns completed, by outcome",
    unit: "",
    kind: Kind::IntSum,
};
const TOKENS: Instrument = Instrument {
    name: "theseus.tokens",
    description: "Tokens by direction (input, output, cache_read, cache_write)",
    unit: "",
    kind: Kind::IntSum,
};
const PROVIDER_ERRORS: Instrument = Instrument {
    name: "theseus.provider.errors",
    description: "Classified provider failures",
    unit: "",
    kind: Kind::IntSum,
};
const TURN_DURATION: Instrument = Instrument {
    name: "theseus.turn.duration_ms",
    description: "",
    unit: "ms",
    kind: Kind::Histogram,
};
const PROVIDER_CALL: Instrument = Instrument {
    name: "theseus.provider.call.duration_ms",
    description: "",
    unit: "ms",
    kind: Kind::Histogram,
};
const FIRST_TOKEN: Instrument = Instrument {
    name: "theseus.provider.first_token_ms",
    description: "",
    unit: "ms",
    kind: Kind::Histogram,
};
const COST: Instrument = Instrument {
    name: "theseus.cost.usd",
    description: "Dollars spent on provider calls, priced by the model catalog",
    unit: "USD",
    kind: Kind::DoubleSum,
};
const TOOL_CALLS: Instrument = Instrument {
    name: "theseus.tool.calls",
    description: "Tool calls proposed by the model, by tool, family, backend, and outcome",
    unit: "",
    kind: Kind::IntSum,
};
const TOOL_DURATION: Instrument = Instrument {
    name: "theseus.tool.duration_ms",
    description: "Each tool call's time in its turn, by tool, family, backend, and outcome",
    unit: "ms",
    kind: Kind::Histogram,
};
const WAKES_FIRED: Instrument = Instrument {
    name: "theseus.wakes.fired",
    description: "Wakes a turn took (DD8), by whether they repeat (37a)",
    unit: "",
    kind: Kind::IntSum,
};
const WAKE_LATE: Instrument = Instrument {
    name: "theseus.wakes.late_ms",
    description: "How long after its due time a turn took each wake",
    unit: "ms",
    kind: Kind::Histogram,
};
const COMPACTIONS: Instrument = Instrument {
    name: "theseus.compactions",
    description:
        "Where the ring would cut (M6 30c): a summary written (compaction), or the ring, by outcome",
    unit: "",
    kind: Kind::IntSum,
};
const COMPACTION_TOKENS: Instrument = Instrument {
    name: "theseus.compaction.tokens",
    description: "A compaction's summary, in output tokens",
    unit: "",
    kind: Kind::Histogram,
};

const PUSH_EVENTS: Instrument = Instrument {
    name: "theseus.push.events",
    description: "Notifications the push made (theseus-in3), by method",
    unit: "",
    kind: Kind::IntSum,
};
const PUSH_LOST: Instrument = Instrument {
    name: "theseus.push.lost",
    description: "Notifications a connection's backlog cap dropped",
    unit: "",
    kind: Kind::IntSum,
};
const PUSH_DELAY: Instrument = Instrument {
    name: "theseus.push.delay_ms",
    description: "From a frame's commit to its execution.changed being queued",
    unit: "ms",
    kind: Kind::Histogram,
};

const DURABILITY_SHIPPED: Instrument = Instrument {
    name: "theseus.durability.shipped",
    description: "What the durability tender shipped (AWS step 15): bytes of WAL segments, tails, and blobs, and index rows, by object",
    unit: "",
    kind: Kind::IntSum,
};
const DURABILITY_LAG: Instrument = Instrument {
    name: "theseus.durability.lag_ms",
    description: "The oldest unshipped record's age when the durability tender caught up: the recovery point's exposure",
    unit: "ms",
    kind: Kind::Histogram,
};
const LSP_REQUEST: Instrument = Instrument {
    name: "theseus.lsp.request.duration",
    description: "Each language-server request a tool made (L2), by server, method, and outcome",
    unit: "ms",
    kind: Kind::Histogram,
};
const FILE_READ: Instrument = Instrument {
    name: "theseus.file.read.duration",
    description:
        "Each file read for a model (theseus-c9l6), by how it came, its type, and its outcome",
    unit: "ms",
    kind: Kind::Histogram,
};
const JUDGE_CALLS: Instrument = Instrument {
    name: "theseus.judge.calls",
    description:
        "Jev's judgments (M5), by pack, mode, band (or skipped, failed), and workload class",
    unit: "",
    kind: Kind::IntSum,
};
const JUDGE_DURATION: Instrument = Instrument {
    name: "theseus.judge.duration_ms",
    description: "Each judgment's call to Jev, end to end, by pack and workload class",
    unit: "ms",
    kind: Kind::Histogram,
};
const JUDGE_ON_PATH: Instrument = Instrument {
    name: "theseus.judge.on_path_ms",
    description:
        "What each judgment kept its turn waiting (0 in shadow), by pack and workload class",
    unit: "ms",
    kind: Kind::Histogram,
};
const JUDGE_ERRORS: Instrument = Instrument {
    name: "theseus.judge.errors",
    description: "Judgments whose call to Jev failed, by error class",
    unit: "",
    kind: Kind::IntSum,
};
const JUDGE_DISAGREEMENTS: Instrument = Instrument {
    name: "theseus.judge.disagreements",
    description: "Answered judgments whose pack, in its act band, would have done otherwise than the baseline",
    unit: "",
    kind: Kind::IntSum,
};

const TASK_CHANGES: Instrument = Instrument {
    name: "theseus.tasks.changes",
    description: "Task graph edits that applied (39a), by verb (task.create, task.update, task.split, task.close)",
    unit: "",
    kind: Kind::IntSum,
};
const TASKS_OPEN: Instrument = Instrument {
    name: "theseus.tasks.open",
    description: "Open tasks in the scope of the last turn that saw the task graph (39a)",
    unit: "",
    kind: Kind::IntLast,
};

const RETENTION_NODES: Instrument = Instrument {
    name: "theseus.memory.retention.nodes",
    description: "Nodes with an FSRS-6 retention in memory's projection (M6 32a)",
    unit: "",
    kind: Kind::IntLast,
};
const ACTIVATE: Instrument = Instrument {
    name: "theseus.recall.activate_ms",
    description: "Spreading activation's refresh and spread in a recall (M6 32b), by outcome",
    unit: "ms",
    kind: Kind::Histogram,
};
const ACTIVATED: Instrument = Instrument {
    name: "theseus.recall.activated",
    description:
        "Nodes spreading activation added to a recall's candidates, and those admitted (M6 32b)",
    unit: "",
    kind: Kind::IntSum,
};
const RECALL_INDEX: Instrument = Instrument {
    name: "theseus.recall.index_ms",
    description: "A turn's recall's wait for the index, by outcome: ran (the whole answer), \
                  words_only (the vector search late), deadline, unavailable (theseus-w9qv)",
    unit: "ms",
    kind: Kind::Histogram,
};
const NODE_CACHE_BYTES: Instrument = Instrument {
    name: "theseus.node_cache.bytes",
    description: "The heat cache of decoded nodes (step 33): the record bytes it holds",
    unit: "By",
    kind: Kind::IntLast,
};
const NODE_CACHE_READS: Instrument = Instrument {
    name: "theseus.node_cache.reads",
    description: "The heat cache's reads, by outcome (hit, miss), its decodes, evictions, and failed rehydrations",
    unit: "",
    kind: Kind::IntSum,
};

const AWS_CALLS: Instrument = Instrument {
    name: "theseus.aws.calls",
    description: "AWS requests, by service, operation and outcome (C1; theseus-ku5f)",
    unit: "",
    kind: Kind::IntSum,
};
const AWS_DURATION: Instrument = Instrument {
    name: "theseus.aws.duration_ms",
    description: "AWS request time, by service, operation and outcome (C1; theseus-ku5f)",
    unit: "ms",
    kind: Kind::Histogram,
};

const ROUTE_WAIT: Instrument = Instrument {
    name: "theseus.route.wait",
    description: "What each turn route.v1 acts on waited for its verdict after the first compile (M5 25e), by whether the verdict missed the bound",
    unit: "ms",
    kind: Kind::Histogram,
};

const CANCELS: Instrument = Instrument {
    name: "theseus.cancel",
    description: "Calls a cancel or a stop ended (M4 §2.11), by backend and how they ended, as health counts them",
    unit: "",
    kind: Kind::IntSum,
};

const INDEX_LAG_BYTES: Instrument = Instrument {
    name: "theseus.index.lag_bytes",
    description:
        "The index tender's lag behind the WAL (M6 §2.13), in bytes, as its last answer said",
    unit: "By",
    kind: Kind::IntLast,
};
const INDEX_LAG_MS: Instrument = Instrument {
    name: "theseus.index.lag_ms",
    description: "The index tender's lag behind the WAL (M6 §2.13), in ms, as its last answer said",
    unit: "ms",
    kind: Kind::IntLast,
};
const INDEX_DOCUMENTS: Instrument = Instrument {
    name: "theseus.index.documents",
    description: "The documents the index tender holds (M6 §2.13), as its last answer said",
    unit: "",
    kind: Kind::IntLast,
};
const INDEX_RSS: Instrument = Instrument {
    name: "theseus.index.rss_bytes",
    description: "The index tender's resident memory (M6 §2.13), as its last answer said",
    unit: "By",
    kind: Kind::IntLast,
};
const INDEX_RESTARTS: Instrument = Instrument {
    name: "theseus.index.restarts",
    description: "The index tender's restarts by its supervisor since the daemon started",
    unit: "",
    kind: Kind::IntSum,
};

const VOICE_CUTS: Instrument = Instrument {
    name: "theseus.voice.cuts",
    description:
        "Replies and reports cut short in a voice call (`voice.cut`, theseus-qb8o), by why",
    unit: "",
    kind: Kind::IntSum,
};
const VOICE_DEAF: Instrument = Instrument {
    name: "theseus.voice.deaf",
    description: "Voice calls found deaf: joined, hearing nothing a listed speaker sent (`voice.deaf`, theseus-d93y), by why",
    unit: "",
    kind: Kind::IntSum,
};
const VOICE_RESUMED: Instrument = Instrument {
    name: "theseus.voice.resumed",
    description:
        "Stops in a voice call that resumed (`voice.resumed`, theseus-qb8o), by what was heard over them",
    unit: "",
    kind: Kind::IntSum,
};

/// Every instrument, in the order a request lists them.
const INSTRUMENTS: [&Instrument; 45] = [
    &TURNS,
    &TOKENS,
    &PROVIDER_ERRORS,
    &TURN_DURATION,
    &PROVIDER_CALL,
    &FIRST_TOKEN,
    &COST,
    &TOOL_CALLS,
    &TOOL_DURATION,
    &WAKES_FIRED,
    &WAKE_LATE,
    &COMPACTIONS,
    &COMPACTION_TOKENS,
    &PUSH_EVENTS,
    &PUSH_LOST,
    &PUSH_DELAY,
    &DURABILITY_SHIPPED,
    &DURABILITY_LAG,
    &LSP_REQUEST,
    &FILE_READ,
    &JUDGE_CALLS,
    &JUDGE_DURATION,
    &JUDGE_ON_PATH,
    &JUDGE_ERRORS,
    &JUDGE_DISAGREEMENTS,
    &ROUTE_WAIT,
    &TASK_CHANGES,
    &TASKS_OPEN,
    &RETENTION_NODES,
    &ACTIVATE,
    &ACTIVATED,
    &RECALL_INDEX,
    &NODE_CACHE_BYTES,
    &NODE_CACHE_READS,
    &AWS_CALLS,
    &AWS_DURATION,
    &CANCELS,
    &INDEX_LAG_BYTES,
    &INDEX_LAG_MS,
    &INDEX_DOCUMENTS,
    &INDEX_RSS,
    &INDEX_RESTARTS,
    &VOICE_CUTS,
    &VOICE_RESUMED,
    &VOICE_DEAF,
];

/// A judgment's attributes (M5 23b).
const JUDGE_PACK: &str = "theseus.judge.pack";
const JUDGE_MODE: &str = "theseus.judge.mode";
const JUDGE_BAND: &str = "theseus.judge.band";
const JUDGE_CLASS: &str = "theseus.judge.class";

/// A tool call's attributes (§3.23). `theseus.tool.name` was `theseus.tool`
/// until theseus-yf1: OTel's naming rules keep a name from being both an
/// attribute and the namespace of others. The shell-fallback ratio is the
/// calls whose name is `proc.run` over all of them.
const TOOL_NAME: &str = "theseus.tool.name";
const TOOL_FAMILY: &str = "theseus.tool.family";
const TOOL_BACKEND: &str = "theseus.tool.backend";
const TOOL_OUTCOME: &str = "theseus.tool.outcome";

/// Every duration histogram's bounds, in ms: bucket i counts the values in
/// (bounds[i-1], bounds[i]], and the last one those above 10 minutes.
/// - Up to 10 s, the SDK's defaults, kept: first tokens, tool calls, and short
///   provider calls land there.
/// - Then 20 s, 30 s, 1 min, 2 min, 5 min, and 10 min, where turns and long
///   calls land (theseus-yf1; with the SDK's alone, every turn over 10 s fell
///   in the last bucket). No step is more than 2.5×, as in the SDK's own.
/// - 10 min is a provider call's default timeout (`[model.timeouts]
///   total_secs`) and `proc.run`'s (`proc_timeout_secs`), so the last bucket
///   holds turns longer than any one default call may be.
pub(super) const BOUNDS: [f64; 21] = [
    0.0, 5.0, 10.0, 25.0, 50.0, 75.0, 100.0, 250.0, 500.0, 750.0, 1000.0, 2500.0, 5000.0, 7500.0,
    10000.0, 20000.0, 30000.0, 60000.0, 120000.0, 300000.0, 600000.0,
];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Attr {
    S(String),
    B(bool),
}

/// An attribute set, sorted by key: one series per instrument and set.
type Attrs = Vec<(&'static str, Attr)>;

#[derive(Debug, Clone, Default)]
struct Point {
    int: u64,
    double: f64,
    count: u64,
    sum: f64,
    min: f64,
    max: f64,
    buckets: [u64; BOUNDS.len() + 1],
}

pub(super) struct Metrics {
    /// Every cumulative point's start: the pipeline's.
    start_ns: u64,
    points: BTreeMap<(&'static str, Attrs), Point>,
    /// The heat cache's totals as last recorded, so each record adds what
    /// changed since.
    node_cache: theseus_protocol::NodeCacheHealth,
    /// The index tender's restarts as last sampled, so each sample adds
    /// their rise.
    index_restarts: u64,
}

/// The attributes every turn's points carry.
fn turn_attrs(profile: &str, provider: &str, model: &str, outcome: &str) -> Attrs {
    sorted(vec![
        ("theseus.profile", Attr::S(profile.to_string())),
        (semconv::GEN_AI_PROVIDER_NAME, Attr::S(provider.to_string())),
        (semconv::GEN_AI_REQUEST_MODEL, Attr::S(model.to_string())),
        ("theseus.outcome", Attr::S(outcome.to_string())),
    ])
}

fn sorted(mut a: Attrs) -> Attrs {
    a.sort_by_key(|(k, _)| *k);
    a
}

fn with(base: &Attrs, key: &'static str, value: &str) -> Attrs {
    let mut a = base.clone();
    a.push((key, Attr::S(value.to_string())));
    sorted(a)
}

impl Metrics {
    pub(super) fn new(start_ns: u64) -> Self {
        Self {
            start_ns,
            points: BTreeMap::new(),
            node_cache: Default::default(),
            index_restarts: 0,
        }
    }

    /// Nothing recorded yet: nothing to export.
    pub(super) fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    fn point(&mut self, i: &Instrument, attrs: Attrs) -> &mut Point {
        self.points.entry((i.name, attrs)).or_default()
    }

    fn add(&mut self, i: &Instrument, attrs: Attrs, n: u64) {
        self.point(i, attrs).int += n;
    }

    fn add_f64(&mut self, i: &Instrument, attrs: Attrs, x: f64) {
        if x.is_finite() {
            self.point(i, attrs).double += x;
        }
    }

    fn record(&mut self, i: &Instrument, attrs: Attrs, x: f64) {
        if !x.is_finite() {
            return;
        }
        let p = self.point(i, attrs);
        if p.count == 0 || x < p.min {
            p.min = x;
        }
        if p.count == 0 || x > p.max {
            p.max = x;
        }
        p.count += 1;
        p.sum += x;
        p.buckets[BOUNDS.partition_point(|b| *b < x)] += 1;
    }

    /// A finished turn: its outcome and time, its tokens and dollars, its
    /// tool calls, and each provider call's time.
    pub(super) fn turn(&mut self, r: &TurnSubmitResult) {
        // `gen_ai.request.model` is the model asked for, as the trace's root
        // recorded it, as a failed turn's is: the result's `model` is the one
        // that answered last, which an alias or a fallback makes differ
        // (theseus-yf1).
        let model = r
            .trace
            .as_ref()
            .and_then(|t| t.attrs.get("model"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or(&r.model);
        let attrs = turn_attrs(&r.profile, &r.provider, model, "complete");
        self.add(&TURNS, attrs.clone(), 1);
        self.record(&TURN_DURATION, attrs.clone(), r.elapsed_ms as f64);
        self.tokens(&r.usage, &attrs);
        if let Some(c) = r.cost_usd.filter(|c| *c > 0.0) {
            self.add_f64(&COST, attrs.clone(), c);
        }
        if let Some(t) = &r.trace {
            self.tool_calls(t, &attrs);
            self.provider_calls(t);
            self.wakes(t);
            self.lsp_requests(t);
            self.files_read(t);
            self.aws_requests(t);
            self.tasks(t);
            self.compactions(t);
            self.activations(t);
        }
    }

    /// The turn's recalls: each one's wait for the index by its outcome
    /// (theseus-w9qv); and spreading activation's (32b): each spread's time
    /// by outcome, and the nodes it added and those admitted.
    fn activations(&mut self, trace: &Span) {
        fn walk<'a>(s: &'a Span, name: &str, out: &mut Vec<&'a Span>) {
            if s.name == name {
                out.push(s);
            }
            for c in &s.children {
                walk(c, name, out);
            }
        }
        let mut recalls = Vec::new();
        walk(trace, "recall", &mut recalls);
        for s in recalls {
            let outcome = s.attrs.get("outcome").and_then(serde_json::Value::as_str);
            let index_ms = s.attrs.get("index_ms").and_then(serde_json::Value::as_f64);
            if let (Some(outcome), Some(ms)) = (outcome, index_ms) {
                let attrs = vec![("theseus.outcome", Attr::S(outcome.to_string()))];
                self.record(&RECALL_INDEX, attrs, ms);
            }
        }
        let mut out = Vec::new();
        walk(trace, "recall.activate", &mut out);
        for s in out {
            let n = |k: &str| {
                s.attrs
                    .get(k)
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0)
            };
            let outcome = s
                .attrs
                .get("outcome")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .to_string();
            self.record(
                &ACTIVATE,
                vec![("theseus.outcome", Attr::S(outcome))],
                s.duration_us() as f64 / 1000.0,
            );
            for (stage, k) in [("added", "added"), ("admitted", "admitted_added")] {
                self.add(
                    &ACTIVATED,
                    vec![("theseus.recall.stage", Attr::S(stage.into()))],
                    n(k),
                );
            }
        }
    }

    /// A failed turn: its outcome and time, the failure by class, what its
    /// finished loops spent, and the tool calls and provider calls its trace
    /// holds.
    pub(super) fn failure(&mut self, f: &FailedTurn<'_>) {
        let attrs = turn_attrs(f.profile, f.provider, f.model, "failed");
        self.add(&TURNS, attrs.clone(), 1);
        self.record(&TURN_DURATION, attrs.clone(), f.elapsed_ms as f64);
        // What its finished loops spent before it failed.
        self.tokens(f.usage, &attrs);
        if let Some(c) = f.cost_usd.filter(|c| *c > 0.0) {
            self.add_f64(&COST, attrs.clone(), c);
        }
        if let Some(t) = f.trace {
            self.tool_calls(t, &attrs);
            self.lsp_requests(t);
            self.files_read(t);
            self.aws_requests(t);
        }
        // A call the daemon's stop kept from being sent is no provider's
        // error (theseus-36re).
        if f.class != crate::provider::STOPPING_CLASS {
            self.add(
                &PROVIDER_ERRORS,
                sorted(vec![
                    (
                        semconv::GEN_AI_PROVIDER_NAME,
                        Attr::S(f.provider.to_string()),
                    ),
                    (semconv::GEN_AI_REQUEST_MODEL, Attr::S(f.model.to_string())),
                    ("theseus.error.class", Attr::S(f.class.to_string())),
                    ("theseus.error.transient", Attr::B(f.transient)),
                ]),
                1,
            );
        }
        if let Some(t) = f.trace {
            self.provider_calls(t);
            self.wakes(t);
            self.tasks(t);
            self.compactions(t);
        }
    }

    /// The task graph (39a): each task tool call that applied, by verb, and
    /// the open tasks the turn's last view showed.
    fn tasks(&mut self, trace: &Span) {
        let mut calls = Vec::new();
        spans::tool_calls(trace, &mut calls);
        for c in calls
            .iter()
            .filter(|c| c.name.starts_with("task.") && c.outcome == "ok")
        {
            self.add(
                &TASK_CHANGES,
                vec![("theseus.task.verb", Attr::S(c.name.clone()))],
                1,
            );
        }
        if let Some(open) = spans::tasks_open(trace) {
            self.point(&TASKS_OPEN, Vec::new()).int = open;
        }
    }

    /// The heat cache's state (step 33): its bytes now, and what it did
    /// since the last record.
    pub(super) fn node_cache(&mut self, h: &theseus_protocol::NodeCacheHealth) {
        self.point(&NODE_CACHE_BYTES, Vec::new()).int = h.bytes;
        let was = std::mem::replace(&mut self.node_cache, h.clone());
        for (what, now, before) in [
            ("hit", h.hits, was.hits),
            ("miss", h.misses, was.misses),
            ("decode", h.decodes, was.decodes),
            ("eviction", h.evictions, was.evictions),
            ("failed", h.failed, was.failed),
        ] {
            let n = now.saturating_sub(before);
            if n > 0 {
                let attrs = vec![("theseus.node_cache.outcome", Attr::S(what.into()))];
                self.add(&NODE_CACHE_READS, attrs, n);
            }
        }
    }

    /// A sample of the index tender (theseus-gfi4): its restarts' rise since
    /// the last sample, and, when it answered (or answered last), its lag,
    /// documents and RSS.
    pub(super) fn index(
        &mut self,
        restarts: u64,
        s: Option<&theseus_protocol::index::IndexStatus>,
    ) {
        let was = std::mem::replace(&mut self.index_restarts, restarts);
        self.add(&INDEX_RESTARTS, Vec::new(), restarts.saturating_sub(was));
        let Some(s) = s else { return };
        for (i, v) in [
            (&INDEX_LAG_BYTES, s.lag.bytes),
            (&INDEX_LAG_MS, s.lag.ms),
            (&INDEX_DOCUMENTS, s.documents),
            (&INDEX_RSS, s.rss_bytes),
        ] {
            self.point(i, Vec::new()).int = v;
        }
    }

    /// The push (theseus-in3): `n` notifications of `method`, from a frame
    /// committed `delay_ms` before they were queued.
    pub(super) fn push(&mut self, method: &str, n: u64, delay_ms: f64) {
        self.add(
            &PUSH_EVENTS,
            vec![("theseus.push.method", Attr::S(method.to_string()))],
            n,
        );
        self.record(&PUSH_DELAY, Vec::new(), delay_ms);
    }

    /// A judgment (M5 23b): counted by pack, mode, band, and workload
    /// class (`band` is its headline answer's, or `skipped` or `failed` when
    /// there is none); a call that reached Jev timed; what it kept the turn
    /// waiting; a failure by its class; a disagreement with the baseline; and
    /// its cost, as `theseus.spend = judge`.
    pub(super) fn judgment(&mut self, j: &theseus_judge::Judgment, disagrees: bool) {
        use theseus_judge::Outcome;
        let ctx = |k: &str| j.context.get(k);
        let class = ctx("class")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        let s = |v: serde_json::Value| v.as_str().unwrap_or_default().to_string();
        let mode = s(serde_json::to_value(j.mode).unwrap_or_default());
        let band = match &j.outcome {
            Outcome::Answered => crate::fact::judge::headline(j).map_or_else(
                || "none".to_string(),
                |a| s(serde_json::to_value(a.band.band).unwrap_or_default()),
            ),
            Outcome::Skipped { .. } => "skipped".into(),
            Outcome::Failed { .. } => "failed".into(),
        };
        let pack = vec![(JUDGE_PACK, Attr::S(j.pack.clone()))];
        let by_class = with(&pack, JUDGE_CLASS, class);
        let calls = with(&with(&by_class, JUDGE_MODE, &mode), JUDGE_BAND, &band);
        self.add(&JUDGE_CALLS, calls, 1);
        if !matches!(j.outcome, Outcome::Skipped { .. }) {
            self.record(&JUDGE_DURATION, by_class.clone(), j.timing.total_ms as f64);
        }
        let on_path = ctx("on_path_ms").and_then(serde_json::Value::as_f64);
        self.record(&JUDGE_ON_PATH, by_class, on_path.unwrap_or(0.0));
        if let Outcome::Failed { class, .. } = &j.outcome {
            self.add(
                &JUDGE_ERRORS,
                vec![("theseus.error.class", Attr::S(class.clone()))],
                1,
            );
        }
        if disagrees {
            self.add(&JUDGE_DISAGREEMENTS, pack.clone(), 1);
        }
        if let Some(m) = j.cost_micros.filter(|m| *m > 0) {
            let spend = with(&pack, "theseus.spend", "judge");
            self.add_f64(&COST, spend, m as f64 / 1_000_000.0);
        }
    }

    /// A call a cancel ended (theseus-qdk5): its backend and its state, as
    /// health's `cancels` names them.
    pub(super) fn cancel(&mut self, backend: &str, state: &str) {
        let attrs = vec![
            ("theseus.cancel.backend", Attr::S(backend.to_string())),
            ("theseus.cancel.state", Attr::S(state.to_string())),
        ];
        self.add(&CANCELS, attrs, 1);
    }

    /// A voice call's cut (`cut`), resumed stop (`resumed`, theseus-qb8o)
    /// or deafness (`deaf`, theseus-d93y), by why, counted where its ledger
    /// row is written.
    pub(super) fn voice(&mut self, event: &str, why: &str) {
        let i = match event {
            "cut" => &VOICE_CUTS,
            "deaf" => &VOICE_DEAF,
            _ => &VOICE_RESUMED,
        };
        self.add(i, vec![("theseus.voice.why", Attr::S(why.to_string()))], 1);
    }

    /// A routed turn's wait (theseus-ddbi), with `theseus.route.late`.
    pub(super) fn route_wait(&mut self, wait: std::time::Duration, late: bool) {
        let attrs = vec![("theseus.route.late", Attr::B(late))];
        self.record(&ROUTE_WAIT, attrs, wait.as_secs_f64() * 1000.0);
    }

    /// The push (theseus-in3): `n` notifications dropped at a backlog cap.
    pub(super) fn push_lost(&mut self, n: u64) {
        self.add(&PUSH_LOST, Vec::new(), n);
    }

    /// The durability tender (AWS step 15): what it shipped, or the lag it
    /// closed as it caught up.
    /// The retention projection's size (M6 32a).
    pub(super) fn retention(&mut self, nodes: u64) {
        self.point(&RETENTION_NODES, Vec::new()).int = nodes;
    }

    pub(super) fn durability(&mut self, m: crate::aws::durable::Measure) {
        use crate::aws::durable::Measure;
        match m {
            Measure::Shipped { object, bytes } => self.add(
                &DURABILITY_SHIPPED,
                vec![("theseus.durability.object", Attr::S(object.to_string()))],
                bytes,
            ),
            Measure::CaughtUp { lag_ms } => {
                self.record(&DURABILITY_LAG, Vec::new(), lag_ms as f64);
            }
        }
    }

    fn tokens(&mut self, u: &Usage, base: &Attrs) {
        for (dir, n) in [
            ("input", u.input_tokens),
            ("output", u.output_tokens),
            ("cache_read", u.cache_read_input_tokens),
            ("cache_write", u.cache_creation_input_tokens),
        ] {
            if n > 0 {
                self.add(&TOKENS, with(base, "theseus.token.direction", dir), n);
            }
        }
    }

    /// Each tool call, counted once at its answer and timed by its run, with
    /// the turn's attributes and the tool's name, family, backend, and
    /// outcome (§3.23).
    fn tool_calls(&mut self, trace: &Span, base: &Attrs) {
        let mut calls = Vec::new();
        spans::tool_calls(trace, &mut calls);
        // Each call counts once, at its answer (theseus-8pei): one that
        // waits for the operator in the continuation that answers it, a
        // background job at its late result. Their proposing turn's span
        // keeps what happened then.
        let answered = calls
            .into_iter()
            .filter(|c| !matches!(c.outcome.as_str(), "awaiting_confirm" | "background"));
        for c in answered {
            let mut attrs = base.clone();
            attrs.extend([
                (TOOL_NAME, Attr::S(c.name)),
                (TOOL_FAMILY, Attr::S(c.family)),
                (TOOL_BACKEND, Attr::S(c.backend)),
                (TOOL_OUTCOME, Attr::S(c.outcome)),
            ]);
            let attrs = sorted(attrs);
            self.add(&TOOL_CALLS, attrs.clone(), 1);
            self.record(&TOOL_DURATION, attrs, c.ms);
        }
    }

    /// Each language-server request a tool call made (L2): its
    /// `lsp.request` span's time, by its server, method, and outcome.
    fn lsp_requests(&mut self, trace: &Span) {
        fn walk(s: &Span, out: &mut Vec<(Attrs, f64)>) {
            if s.name == "lsp.request" {
                let a = |k: &str| {
                    Attr::S(
                        s.attrs
                            .get(k)
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                    )
                };
                out.push((
                    sorted(vec![
                        ("lsp.server", a("lsp.server")),
                        ("lsp.method", a("lsp.method")),
                        ("theseus.outcome", a("outcome")),
                    ]),
                    s.duration_us() as f64 / 1000.0,
                ));
            }
            for c in &s.children {
                walk(c, out);
            }
        }
        let mut out = Vec::new();
        walk(trace, &mut out);
        for (attrs, ms) in out {
            self.record(&LSP_REQUEST, attrs, ms);
        }
    }

    /// Each file the turn read for a model (theseus-c9l6): its `file.read`
    /// span's time, by how it came (an attachment, `fs.read`, `http.fetch`),
    /// its type, and its outcome.
    fn files_read(&mut self, trace: &Span) {
        fn walk(s: &Span, out: &mut Vec<(Attrs, f64)>) {
            if s.name == "file.read" {
                let a = |k: &str| {
                    Attr::S(
                        s.attrs
                            .get(k)
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                    )
                };
                out.push((
                    sorted(vec![
                        ("theseus.file.via", a("via")),
                        ("theseus.file.type", a("media_type")),
                        ("theseus.outcome", a("outcome")),
                    ]),
                    s.duration_us() as f64 / 1000.0,
                ));
            }
            for c in &s.children {
                walk(c, out);
            }
        }
        let mut out = Vec::new();
        walk(trace, &mut out);
        for (attrs, ms) in out {
            self.record(&FILE_READ, attrs, ms);
        }
    }

    /// Each AWS request a tool call made (C1): its `aws` span, counted and
    /// timed by its service, operation, and outcome (`ok`, `unbound`, or
    /// `error`), wherever it sits in the trace. The AWS error's code stays on
    /// the span and the row: it is the service's own text, so it makes no
    /// attribute here, and the series stay as many as the catalog's
    /// operations (theseus-ku5f).
    fn aws_requests(&mut self, trace: &Span) {
        fn walk(s: &Span, out: &mut Vec<(Attrs, f64)>) {
            if s.kind == "aws" {
                let a = |k: &str, or: &str| {
                    Attr::S(
                        s.attrs
                            .get(k)
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or(or)
                            .to_string(),
                    )
                };
                out.push((
                    sorted(vec![
                        ("rpc.service", a("rpc.service", "")),
                        ("rpc.method", a("rpc.method", "")),
                        ("theseus.outcome", a("status", "unknown")),
                    ]),
                    s.duration_us() as f64 / 1000.0,
                ));
            }
            for c in &s.children {
                walk(c, out);
            }
        }
        let mut out = Vec::new();
        walk(trace, &mut out);
        for (attrs, ms) in out {
            self.add(&AWS_CALLS, attrs.clone(), 1);
            self.record(&AWS_DURATION, attrs, ms);
        }
    }

    /// Each wake the turn took (37a), counted by whether it repeats, and
    /// how late it was taken.
    fn wakes(&mut self, trace: &Span) {
        let mut out = Vec::new();
        spans::wakes(trace, &mut out);
        for (repeat, late_ms) in out {
            self.add(
                &WAKES_FIRED,
                vec![("theseus.wake.repeat", Attr::B(repeat))],
                1,
            );
            self.record(
                &WAKE_LATE,
                vec![("theseus.wake.repeat", Attr::B(repeat))],
                late_ms,
            );
        }
    }

    /// Each compaction the turn ran or fell back from (30c), by outcome, and
    /// each summary's tokens.
    fn compactions(&mut self, trace: &Span) {
        let mut out = Vec::new();
        spans::compactions(trace, &mut out);
        for (outcome, tokens) in out {
            let attrs = vec![("theseus.compaction.outcome", Attr::S(outcome.clone()))];
            self.add(&COMPACTIONS, attrs.clone(), 1);
            if outcome == "compaction" {
                self.record(&COMPACTION_TOKENS, attrs, tokens as f64);
            }
        }
    }

    /// Each provider call's time, and its first token's when it had one, by
    /// the provider and model its span recorded (theseus-yf1: the SDK
    /// exporter's had no attributes; theseus-8u02: the first token was the
    /// result's, the last call's alone, with the turn's attributes). A failed
    /// call's time also carries its `error.type` (theseus-lmhp).
    fn provider_calls(&mut self, trace: &Span) {
        let mut calls = Vec::new();
        spans::provider_calls(trace, &mut calls);
        for c in calls {
            let mut attrs = Vec::new();
            if let Some(p) = c.provider {
                attrs.push((semconv::GEN_AI_PROVIDER_NAME, Attr::S(p)));
            }
            if let Some(m) = c.model {
                attrs.push((semconv::GEN_AI_REQUEST_MODEL, Attr::S(m)));
            }
            // A first token is the call's own, whatever came after it, so
            // it stays in the model's one series; the call's time splits by
            // `error.type`, so a 404 in 150 ms never lands among the answers.
            let mut attrs = sorted(attrs);
            if let Some(ft) = c.first_token_ms {
                self.record(&FIRST_TOKEN, attrs.clone(), ft);
            }
            if let Some(e) = c.error_type {
                attrs.push((semconv::ERROR_TYPE, Attr::S(e)));
                attrs = sorted(attrs);
            }
            self.record(&PROVIDER_CALL, attrs, c.ms);
        }
    }

    /// Every point, cumulative from the start, as of `now_ns`.
    pub(super) fn request(
        &self,
        resource: otlp::Resource,
        now_ns: u64,
    ) -> otlp::ExportMetricsServiceRequest {
        let metrics = INSTRUMENTS
            .iter()
            .filter_map(|i| self.metric(i, now_ns))
            .collect();
        otlp::ExportMetricsServiceRequest {
            resource_metrics: vec![otlp::ResourceMetrics {
                resource,
                scope_metrics: vec![otlp::ScopeMetrics {
                    scope: otlp::Scope {
                        name: crate::NAME.into(),
                    },
                    metrics,
                }],
            }],
        }
    }

    fn metric(&self, i: &Instrument, now_ns: u64) -> Option<otlp::Metric> {
        let points: Vec<(&Attrs, &Point)> = self
            .points
            .iter()
            .filter(|((name, _), _)| *name == i.name)
            .map(|((_, a), p)| (a, p))
            .collect();
        if points.is_empty() {
            return None;
        }
        let (start, time) = (self.start_ns, now_ns);
        let data = match i.kind {
            Kind::IntSum | Kind::DoubleSum => otlp::Data::Sum(otlp::Sum {
                data_points: points
                    .into_iter()
                    .map(|(a, p)| otlp::NumberDataPoint {
                        attributes: key_values(a),
                        start_time_unix_nano: start,
                        time_unix_nano: time,
                        value: match i.kind {
                            Kind::DoubleSum => otlp::Number::AsDouble(p.double),
                            _ => otlp::Number::AsInt(p.int.to_string()),
                        },
                    })
                    .collect(),
                aggregation_temporality: otlp::CUMULATIVE,
                is_monotonic: true,
            }),
            Kind::IntLast => otlp::Data::Sum(otlp::Sum {
                data_points: points
                    .into_iter()
                    .map(|(a, p)| otlp::NumberDataPoint {
                        attributes: key_values(a),
                        start_time_unix_nano: start,
                        time_unix_nano: time,
                        value: otlp::Number::AsInt(p.int.to_string()),
                    })
                    .collect(),
                aggregation_temporality: otlp::CUMULATIVE,
                is_monotonic: false,
            }),
            Kind::Histogram => otlp::Data::Histogram(otlp::Histogram {
                data_points: points
                    .into_iter()
                    .map(|(a, p)| otlp::HistogramDataPoint {
                        attributes: key_values(a),
                        start_time_unix_nano: start,
                        time_unix_nano: time,
                        count: p.count,
                        sum: p.sum,
                        bucket_counts: p.buckets.iter().map(u64::to_string).collect(),
                        explicit_bounds: BOUNDS.to_vec(),
                        min: p.min,
                        max: p.max,
                    })
                    .collect(),
                aggregation_temporality: otlp::CUMULATIVE,
            }),
        };
        Some(otlp::Metric {
            name: i.name.to_string(),
            description: i.description,
            unit: i.unit,
            data,
        })
    }
}

fn key_values(a: &Attrs) -> Vec<KeyValue> {
    a.iter()
        .map(|(k, v)| {
            KeyValue::new(
                *k,
                match v {
                    Attr::S(s) => AnyValue::String(s.clone()),
                    Attr::B(b) => AnyValue::Bool(*b),
                },
            )
        })
        .collect()
}
