//! The tools note's postures: the exceptions and a default, not every tool.
//!
//! A tool's posture is its policy line (`open` runs, `notify` runs and tells
//! the operator, `approve` waits). Most tools share one, so the note names it
//! once ("every other tool: notify") and lists by name only the tools whose
//! posture differs; the tools a request does not carry cost it nothing.
//! Enforcement is `policy`'s: the note only tells the model what to expect.

use crate::policy::Posture;

/// "open: a, b; approve: c; every other tool: notify": `tools` are wire names
/// with their posture. The default is the posture most of them have (the
/// lowest on the ladder when two tie), so it is the same for the same tools.
pub(super) fn postures_line(tools: &[(String, Posture)]) -> String {
    let count = |p: Posture| tools.iter().filter(|(_, q)| *q == p).count();
    let default = Posture::ALL
        .into_iter()
        .rev()
        .max_by_key(|p| count(*p))
        .unwrap_or_default();
    let mut parts: Vec<String> = Posture::ALL
        .iter()
        .filter(|p| **p != default)
        .filter_map(|p| {
            let names: Vec<&str> = tools
                .iter()
                .filter(|(_, q)| q == p)
                .map(|(n, _)| n.as_str())
                .collect();
            (!names.is_empty()).then(|| format!("{}: {}", p.as_str(), names.join(", ")))
        })
        .collect();
    parts.push(format!("every other tool: {}", default.as_str()));
    parts.join("; ")
}
