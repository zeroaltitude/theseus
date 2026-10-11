//! The harness's overhead per turn (theseus-4w1h), from each turn's trace:
//! **the turn's time less its model's calls and its tools' runs**, the one
//! definition the README's speed table and the cockpit's speed wall read. It
//! is everything a person waits for that is neither the model nor their own
//! work, so the disk's commits are in it: a frame's `fdatasync` is the
//! harness's choice to make the turn durable, and a person waits for it.
//! Split as the speed wall splits it: the commits (`store` spans), the
//! compiles (`compile`), the admission (`lock`), and the rest.

use serde::Serialize;
use serde_json::Value;

use crate::lifecycle::Summary;

/// One turn's overhead, part by part, in ms.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Split {
    pub total: f64,
    pub model: f64,
    pub tools: f64,
    pub harness: f64,
    pub commits: f64,
    pub compile: f64,
    pub admission: f64,
    pub rest: f64,
}

impl Split {
    /// A turn's split, from its trace's root span (`TurnSubmitResult.trace`):
    /// the speed wall's `costOf`, line for line.
    pub fn of(root: &Value) -> Self {
        let mut s = Self::default();
        let (Some(start), Some(end)) = (root["start_us"].as_f64(), root["end_us"].as_f64()) else {
            return s;
        };
        walk(root, &mut s);
        s.total = (end - start) / 1000.0;
        s.harness = (s.total - s.model - s.tools).max(0.0);
        s.rest = (s.harness - s.commits - s.compile - s.admission).max(0.0);
        s
    }
}

fn walk(span: &Value, s: &mut Split) {
    let start = span["start_us"].as_f64().unwrap_or(0.0);
    let d = (span["end_us"].as_f64().unwrap_or(start) - start).max(0.0) / 1000.0;
    let kind = span["kind"].as_str().unwrap_or("");
    if kind == "provider" {
        s.model += d;
        return;
    }
    if kind == "tool"
        && span["name"]
            .as_str()
            .is_some_and(|n| n.starts_with("tool "))
    {
        s.tools += d;
        return;
    }
    match kind {
        "store" => s.commits += d,
        "compile" => s.compile += d,
        "lock" => s.admission += d,
        _ => {}
    }
    for c in span["children"].as_array().into_iter().flatten() {
        walk(c, s);
    }
}

/// A kind's turns: each part's median, and the whole overhead's summary.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Overhead {
    pub harness: Option<Summary>,
    /// Each part's median, in ms.
    pub p50: Split,
}

impl Overhead {
    pub fn of(splits: &[Split]) -> Self {
        let med = |f: fn(&Split) -> f64| {
            Summary::of(&splits.iter().map(f).collect::<Vec<_>>()).map_or(0.0, |s| s.p50)
        };
        Self {
            harness: Summary::of(&splits.iter().map(|s| s.harness).collect::<Vec<_>>()),
            p50: Split {
                total: med(|s| s.total),
                model: med(|s| s.model),
                tools: med(|s| s.tools),
                harness: med(|s| s.harness),
                commits: med(|s| s.commits),
                compile: med(|s| s.compile),
                admission: med(|s| s.admission),
                rest: med(|s| s.rest),
            },
        }
    }

    pub fn line(&self, kind: &str) -> String {
        let p = &self.p50;
        match self.harness {
            None => format!("  {kind}: no traced turns, so no harness overhead"),
            Some(h) => format!(
                "  {kind}: harness overhead (the turn less its model and tools) p50 {:.1} ms, p95 {:.1} ms; \
                 of it, the commits {:.1}, the compiles {:.1}, the admission {:.1}, the rest {:.1} \
                 (the turn {:.1}, the model {:.1}, the tools {:.1}; part medians)",
                h.p50, h.p95, p.commits, p.compile, p.admission, p.rest, p.total, p.model, p.tools
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The model and the tools are the turn's own waits, and nothing under
    /// them counts again; the harness is the rest, part by part.
    #[test]
    fn a_turns_overhead_is_its_time_less_its_model_and_tools() {
        let trace = json!({"kind": "turn", "start_us": 0, "end_us": 50_000, "children": [
            {"kind": "lock", "start_us": 0, "end_us": 1_000},
            {"kind": "store", "start_us": 1_000, "end_us": 4_000},
            {"kind": "loop", "start_us": 4_000, "end_us": 49_000, "children": [
                {"kind": "compile", "start_us": 4_000, "end_us": 6_000},
                {"kind": "provider", "start_us": 6_000, "end_us": 36_000,
                 "children": [{"kind": "store", "start_us": 7_000, "end_us": 8_000}]},
                {"kind": "tool", "name": "tool proc.run", "start_us": 36_000, "end_us": 46_000},
                {"kind": "store", "start_us": 46_000, "end_us": 49_000}
            ]}
        ]});
        let s = Split::of(&trace);
        assert_eq!((s.total, s.model, s.tools), (50.0, 30.0, 10.0));
        assert_eq!(s.harness, 10.0);
        assert_eq!((s.commits, s.compile, s.admission), (6.0, 2.0, 1.0));
        assert_eq!(s.rest, 1.0);
        let o = Overhead::of(&[s, Split { harness: 20.0, ..s }]);
        assert_eq!(o.harness.unwrap().max, 20.0);
        assert!(o.line("plain").contains("the commits 6.0"));
        assert_eq!(Split::of(&json!({})), Split::default(), "no trace, nothing");
        assert!(Overhead::of(&[]).line("plain").contains("no traced turns"));
    }
}
