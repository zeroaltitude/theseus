//! OpenTelemetry as a projection of the record (spec §3.20).
//!
//! The turn trace is already a span tree with absolute timestamps. When a
//! turn ends, a build with the `otel` cargo feature walks it and emits OTel
//! spans with those exact start and end times, so the exported picture is the
//! ledger's picture and the hot path pays nothing extra; metrics are recorded
//! at the same moment, and nothing leaves the process until an OTLP endpoint
//! is configured. The feature is off by default (theseus-0g4): such a build
//! parses `[telemetry]` the same way, exports nothing, and its config load
//! warns once if an endpoint is set. The trace, the ledger, and `turn.trace`
//! are the record either way.

use serde::{Deserialize, Serialize};
use theseus_protocol::Span;
#[cfg(not(feature = "otel"))]
use theseus_protocol::TurnSubmitResult;

#[cfg(feature = "otel")]
mod otel;
#[cfg(feature = "otel")]
pub use otel::Telemetry;

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
}

/// A build without the `otel` feature: no pipeline, and every method does nothing.
#[cfg(not(feature = "otel"))]
pub struct Telemetry {
    pub endpoint: Option<String>,
}

#[cfg(not(feature = "otel"))]
impl Telemetry {
    pub fn disabled() -> Self {
        Self { endpoint: None }
    }

    pub fn enabled(&self) -> bool {
        false
    }

    /// Nothing to build. An endpoint set here was already warned about when
    /// the config loaded.
    pub fn from_config(
        cfg: &TelemetryConfig,
        _headers: Option<&crate::secrets::Secret>,
    ) -> anyhow::Result<Self> {
        match cfg.endpoint() {
            None => tracing::info!("telemetry: no otlp_endpoint configured; nothing is exported"),
            Some(_) => {
                tracing::info!("telemetry: this build has no OTLP export; nothing is exported")
            }
        }
        Ok(Self::disabled())
    }

    pub fn record_turn(&self, _: &TurnSubmitResult) {}

    pub fn record_failure(&self, _: &FailedTurn<'_>) {}

    pub fn flush(&self) {}
}
