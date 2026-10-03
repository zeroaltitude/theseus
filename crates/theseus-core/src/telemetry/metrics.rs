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

/// Every instrument, in the order a request lists them.
const INSTRUMENTS: [&Instrument; 12] = [
    &TURNS,
    &TOKENS,
    &PROVIDER_ERRORS,
    &TURN_DURATION,
    &PROVIDER_CALL,
    &FIRST_TOKEN,
    &COST,
    &TOOL_CALLS,
    &TOOL_DURATION,
    &PUSH_EVENTS,
    &PUSH_LOST,
    &PUSH_DELAY,
];

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
        }
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
        if let Some(t) = f.trace {
            self.provider_calls(t);
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

    /// The push (theseus-in3): `n` notifications dropped at a backlog cap.
    pub(super) fn push_lost(&mut self, n: u64) {
        self.add(&PUSH_LOST, Vec::new(), n);
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

    /// Each tool call, counted and timed, with the turn's attributes and the
    /// tool's name, family, backend, and outcome (§3.23).
    fn tool_calls(&mut self, trace: &Span, base: &Attrs) {
        let mut calls = Vec::new();
        spans::tool_calls(trace, &mut calls);
        for c in calls {
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

    /// Each provider call's time, and its first token's when it had one, by
    /// the provider and model its span recorded (theseus-yf1: the SDK
    /// exporter's had no attributes; theseus-8u02: the first token was the
    /// result's, the last call's alone, with the turn's attributes).
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
            let attrs = sorted(attrs);
            if let Some(ft) = c.first_token_ms {
                self.record(&FIRST_TOKEN, attrs.clone(), ft);
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
