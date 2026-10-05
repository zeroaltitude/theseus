//! OpenTelemetry as a projection of the record (spec §3.20).
//!
//! The turn trace is already a span tree with absolute timestamps. When a
//! turn ends, its metrics are recorded in the process and its trace is handed
//! to one sender task, which walks it into OTel spans with those exact start
//! and end times and posts them as OTLP/HTTP JSON (`export`), so the exported
//! picture is the ledger's picture and the hot path pays nothing but the
//! hand-over. The metrics are aggregated here with cumulative temporality and
//! exported every `metrics_interval_secs`. Nothing leaves the process until an
//! OTLP endpoint is configured. The exporter is always compiled in, and brings
//! no crate the daemon did not already have (theseus-hee); until then it was
//! the `otel` cargo feature, over the OpenTelemetry SDK and 19 crates. The
//! trace, the ledger, and `turn.trace` are the record either way.

mod export;
mod metrics;
mod otlp;
mod spans;
#[cfg(test)]
pub(crate) mod tests;
#[cfg(test)]
mod tests_aws;
#[cfg(test)]
mod tests_calls;
#[cfg(test)]
mod tests_cancel;
#[cfg(test)]
mod tests_files;
#[cfg(test)]
mod tests_index;
#[cfg(test)]
mod tests_judge;
#[cfg(test)]
mod tests_recall;
#[cfg(test)]
pub(crate) mod tests_resumed;

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use theseus_protocol::{Span, TelemetryStatus, TurnSubmitResult, Usage};

use crate::secrets::Secret;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TelemetryConfig {
    /// OTLP/HTTP base URL of a collector, e.g. `http://127.0.0.1:4318`.
    /// `/v1/traces` and `/v1/metrics` are appended. Unset: nothing is sent.
    #[serde(default)]
    pub otlp_endpoint: Option<String>,
    /// Name of a `[secrets]` entry whose value is `Header: value` lines (or a
    /// single `key=value,key2=value2` string) to send with every export, e.g.
    /// a Honeycomb `x-honeycomb-team` key.
    #[serde(default)]
    pub headers_secret: Option<String>,
    #[serde(default = "default_service_name")]
    pub service_name: String,
    /// Metrics export interval.
    #[serde(default = "default_metrics_interval")]
    pub metrics_interval_secs: u64,
    #[serde(default = "default_export_timeout")]
    pub export_timeout_secs: u64,
}

fn default_service_name() -> String {
    "theseus".into()
}
fn default_metrics_interval() -> u64 {
    15
}
fn default_export_timeout() -> u64 {
    10
}

impl TelemetryConfig {
    /// The endpoint, if one is set and not blank.
    pub fn endpoint(&self) -> Option<&str> {
        self.otlp_endpoint
            .as_deref()
            .filter(|s| !s.trim().is_empty())
    }
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            otlp_endpoint: None,
            headers_secret: None,
            service_name: default_service_name(),
            metrics_interval_secs: default_metrics_interval(),
            export_timeout_secs: default_export_timeout(),
        }
    }
}

/// What a failed turn reports to telemetry.
pub struct FailedTurn<'a> {
    pub profile: &'a str,
    pub provider: &'a str,
    pub model: &'a str,
    pub class: &'a str,
    pub transient: bool,
    pub elapsed_ms: u64,
    pub trace: Option<&'a Span>,
    /// What the turn's finished loops spent before it failed.
    pub usage: &'a Usage,
    pub cost_usd: Option<f64>,
}

/// The telemetry pipeline. Cheap to hold, and to clone (a clone shares the
/// pipeline); off unless an endpoint is set.
#[derive(Clone)]
pub struct Telemetry {
    state: State,
}

#[derive(Clone)]
enum State {
    Off,
    /// An endpoint is set, and its exporter could not be built.
    Failed {
        endpoint: String,
        why: String,
    },
    On(Arc<export::Shared>),
}

impl Telemetry {
    pub fn disabled() -> Self {
        Self { state: State::Off }
    }

    /// The pipeline that could not be built, so health can say why.
    pub fn failed(endpoint: &str, why: String) -> Self {
        Self {
            state: State::Failed {
                endpoint: endpoint.to_string(),
                why,
            },
        }
    }

    pub fn enabled(&self) -> bool {
        matches!(self.state, State::On(_))
    }

    /// Build from config, and start the sender (inside a tokio runtime).
    /// With no endpoint this is `disabled()`.
    pub fn from_config(cfg: &TelemetryConfig, headers: Option<&Secret>) -> anyhow::Result<Self> {
        Self::with_tuning(cfg, headers, export::Tuning::from_config(cfg))
    }

    fn with_tuning(
        cfg: &TelemetryConfig,
        headers: Option<&Secret>,
        tuning: export::Tuning,
    ) -> anyhow::Result<Self> {
        let Some(endpoint) = cfg.endpoint() else {
            tracing::info!("telemetry: no otlp_endpoint configured; nothing is exported");
            return Ok(Self::disabled());
        };
        let endpoint = endpoint.trim().trim_end_matches('/');
        let shared = export::start(cfg, endpoint, headers, tuning)?;
        tracing::info!(endpoint = %endpoint, "telemetry: OTLP/HTTP export on (JSON)");
        Ok(Self {
            state: State::On(shared),
        })
    }

    fn shared(&self) -> Option<&export::Shared> {
        match &self.state {
            State::On(s) => Some(s),
            _ => None,
        }
    }

    /// A finished turn: its metrics now, and its trace to the sender.
    pub fn record_turn(&self, result: &TurnSubmitResult) {
        let Some(s) = self.shared() else { return };
        s.metrics
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .turn(result);
        if let Some(t) = &result.trace {
            s.push(t.clone());
        }
    }

    /// The heat cache of decoded nodes (step 33), read as a turn ends.
    pub fn record_node_cache(&self, h: &theseus_protocol::NodeCacheHealth) {
        let Some(s) = self.shared() else { return };
        s.metrics
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .node_cache(h);
    }

    /// A failed turn: its metrics now, and its partial trace, with error
    /// status, to the sender.
    pub fn record_failure(&self, f: &FailedTurn<'_>) {
        let Some(s) = self.shared() else { return };
        s.metrics
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .failure(f);
        if let Some(t) = f.trace {
            s.push(t.clone());
        }
    }

    /// The push (theseus-in3): `n` `execution.changed` notifications made
    /// from one frame, `delay` after its commit.
    pub fn record_push(&self, n: u64, delay: Duration) {
        let Some(s) = self.shared() else { return };
        s.metrics.lock().unwrap_or_else(|e| e.into_inner()).push(
            theseus_protocol::notify::EXECUTION_CHANGED,
            n,
            delay.as_secs_f64() * 1000.0,
        );
    }

    /// A judgment the judge's sink wrote (M5 23b): its counts, its times,
    /// its error, whether it disagrees with the baseline, and its cost, as
    /// the judge's spend.
    pub fn record_judgment(&self, j: &theseus_judge::Judgment, disagrees: bool) {
        let Some(s) = self.shared() else { return };
        s.metrics
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .judgment(j, disagrees);
    }

    /// The push (theseus-in3): `n` notifications a connection's backlog cap
    /// dropped, counted once the connection hears it.
    pub fn record_push_lost(&self, n: u64) {
        let Some(s) = self.shared() else { return };
        s.metrics
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_lost(n);
    }

    /// A call a cancel or a stop ended (theseus-qdk5), counted where health
    /// counts it (`cancel::Stops`), by its backend and how it ended.
    pub fn record_cancel(&self, backend: &str, state: &str) {
        let Some(s) = self.shared() else { return };
        s.metrics
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .cancel(backend, state);
    }

    /// A sample of the index tender (theseus-gfi4): health's `index` block,
    /// its supervisor's restarts and the tender's last answer.
    pub fn record_index(&self, h: &theseus_protocol::index::IndexHealth) {
        let Some(s) = self.shared() else { return };
        let restarts = h.tender.as_ref().map_or(0, |t| t.restarts);
        s.metrics
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .index(restarts, h.status.as_ref());
    }

    /// The durability tender (AWS step 15): bytes shipped, or the lag a
    /// catch-up closed.
    pub fn record_durability(&self, m: crate::aws::durable::Measure) {
        let Some(s) = self.shared() else { return };
        s.metrics
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .durability(m);
    }

    /// Memory's retention projection (M6 32a): the nodes it holds.
    pub fn record_retention(&self, nodes: u64) {
        let Some(s) = self.shared() else { return };
        s.metrics
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retention(nodes);
    }

    /// Send what waits, and the metrics, now: true once done (or nothing to
    /// do), false when `within` passed first. The daemon's clean shutdown
    /// waits here, bounded.
    pub async fn flush(&self, within: Duration) -> bool {
        match self.shared() {
            Some(s) => s.flush(within).await,
            None => true,
        }
    }

    /// For health.
    pub fn status(&self) -> TelemetryStatus {
        match &self.state {
            State::Off => TelemetryStatus::off(),
            State::Failed { endpoint, why } => TelemetryStatus {
                otlp_endpoint: Some(endpoint.clone()),
                state: "failed".into(),
                detail: Some(why.clone()),
                ..Default::default()
            },
            State::On(s) => s.status(),
        }
    }
}
