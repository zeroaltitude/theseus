//! A turn's tool calls on its trace (theseus-yf1, theseus-8pei): a span per
//! call, which telemetry's tool metrics read. The calls `run_tools` ran are
//! traced in the turn whose model proposed them; the calls a continuation
//! answered (`ToolRuntime::resume`) and the late results a turn took
//! (`ToolRuntime::absorb`) are traced in the turn that answered them, built
//! after they ran, from what they returned: nothing here waits or reads the
//! store.

use std::collections::BTreeMap;

use serde_json::json;
use theseus_protocol::Span;

use super::{Turn, TurnRunner};
use crate::provider::ToolUse;
use crate::toolrun::{CallOutcome, LateCall, Ran, ToolRuntime};
use crate::trace::Trace;

/// A span per call, in the order the calls ran, under the trace's innermost
/// open span. The calls of a group that ran together sit under one `tools`
/// span, so their spans overlap there.
pub(super) fn trace_calls(trace: &mut Trace, tools: &ToolRuntime, uses: &[ToolUse], ran: &[Ran]) {
    for span in call_spans(trace, tools, uses, ran) {
        trace.push(span);
    }
}

/// The spans `trace_calls` pushes, for a parent of the caller's (the
/// continuation's). Each names its tool, family, and backend (`none` for a
/// tool that is not registered), and the call's result. An AWS call's
/// requests (AWS design §3.8), a language server's, and the call's judged
/// marks are spans under its own; a call's requests are taken once.
pub(super) fn call_spans(
    trace: &Trace,
    tools: &ToolRuntime,
    uses: &[ToolUse],
    ran: &[Ran],
) -> Vec<Span> {
    let (aws, lsp) = (tools.aws.as_deref(), tools.lsp.as_deref());
    let span = |r: &Ran| {
        let wire = uses[r.index].name.as_str();
        let tool = tools.tool_by_wire(wire);
        let tool = tool.as_deref();
        let mut attrs = json!({"tool_use_id": uses[r.index].id, "outcome": format!("{:?}", r.outcome),
            "tool": tool.map_or(wire, |t| t.name()),
            "family": tool.map_or("unknown", |t| t.family()),
            "backend": tool.map_or("none", |t| t.backend().as_str()),
            "result": call_result(&r.outcome)});
        crate::mcp::span_attrs(tool, &mut attrs);
        Span {
            name: format!("tool {wire}"),
            kind: "tool".into(),
            start_us: trace.at(r.started),
            end_us: Some(trace.at(r.ended)),
            attrs,
            children: aws
                .map(|a| a.spans(&uses[r.index].id, |i| trace.at(i)))
                .into_iter()
                .chain(lsp.map(|l| l.spans(&uses[r.index].id, |i| trace.at(i))))
                .flatten()
                .chain(crate::judge::gate::marks(&r.judged, |i| trace.at(i)))
                .collect(),
        }
    };
    let mut groups: BTreeMap<usize, Vec<Span>> = BTreeMap::new();
    for r in ran {
        groups.entry(r.group).or_default().push(span(r));
    }
    let mut out = Vec::new();
    for (_, mut spans) in groups {
        if spans.len() == 1 {
            out.push(spans.remove(0));
            continue;
        }
        let start_us = spans.iter().map(|s| s.start_us).min().unwrap_or(0);
        let end_us = spans.iter().filter_map(|s| s.end_us).max();
        out.push(Span {
            name: "tools".into(),
            kind: "tools".into(),
            start_us,
            end_us,
            attrs: json!({"calls": spans.len(), "together": true}),
            children: spans,
        });
    }
    out
}

/// A span per late result, at its absorption: the job ran before this trace
/// began, and a span cannot start before the trace's origin, so it is a
/// point with `late: true` and the job's run (`run_ms`, its dispatch to its
/// settle), which the tool metrics take as the call's time.
pub(super) fn late_spans(trace: &Trace, tools: &ToolRuntime, late: &[LateCall]) -> Vec<Span> {
    let at = trace.now_us();
    late.iter()
        .map(|l| {
            let tool = tools.tool_by_wire(&l.wire);
            let tool = tool.as_deref();
            let mut attrs = json!({"tool_use_id": l.tool_use_id, "late": true, "run_ms": l.run_ms,
                "tool": tool.map_or(l.wire.as_str(), |t| t.name()),
                "family": tool.map_or("unknown", |t| t.family()),
                "backend": tool.map_or("none", |t| t.backend().as_str()),
                "result": l.status.as_str()});
            crate::mcp::span_attrs(tool, &mut attrs);
            Span {
                name: format!("tool {}", l.wire),
                kind: "tool".into(),
                start_us: at,
                end_us: Some(at),
                attrs,
                children: Vec::new(),
            }
        })
        .collect()
}

impl TurnRunner {
    /// The late results that settled while the turn ran (`finish`), each
    /// traced as it is taken: how many were written.
    pub(super) fn take_late(&self, t: &mut Turn<'_>) -> anyhow::Result<u32> {
        let (_, late) = self.tools.absorb(&t.tc)?;
        for span in late_spans(&t.trace, &self.tools, &late) {
            t.trace.push(span);
        }
        Ok(late.len() as u32)
    }
}

/// What became of a call, as its trace span says it (theseus-yf1): its result
/// node's status, or that it waits for the operator or runs in the
/// background.
pub(crate) fn call_result(o: &CallOutcome) -> &'static str {
    match o {
        CallOutcome::Done { status } => status.as_str(),
        CallOutcome::AwaitingConfirm { .. } => "awaiting_confirm",
        CallOutcome::Background { .. } => "background",
    }
}
