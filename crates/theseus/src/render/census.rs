//! `theseus census` (theseus-7m7w.1): the lines that show a request's census, part by part down to each tool.
//! Pure: the answer in, plain lines out; the times are the clock's zone (`time`).

use theseus_protocol::census::{CensusTool, ContextCensusResult};

use super::thousands;
use super::time::{fmt_date, fmt_day};

/// `1,234 tok`.
fn tok(n: u64) -> String {
    format!("{} tok", thousands(n))
}

/// The share of `of` that `n` is, to a tenth of a percent: `12.5%`.
fn share(n: u64, of: u64) -> String {
    match of {
        0 => "0%".into(),
        _ => format!("{:.1}%", n as f64 * 100.0 / of as f64),
    }
}

/// The census as lines: the request's total, the system's blocks and sections, the tools (largest first) with
/// their use, and what the census could not say.
pub fn census_lines(r: &ContextCensusResult) -> Vec<String> {
    let mut out = Vec::new();
    let what = match &r.session_id {
        Some(s) => format!("the next request of {s}"),
        None => format!("a fresh session's first request, place {}", r.place),
    };
    let window = r
        .window
        .map_or(String::new(), |w| format!(" · window {}", thousands(w)));
    out.push(format!(
        "census · {what} · {} {} · {} · {}{window}",
        r.model, r.profile, r.class, r.compile
    ));
    out.push(format!(
        "request: {} = system {} + tools {} + messages {} + images {}  ({} of the window)",
        tok(r.total_tokens),
        thousands(r.system_tokens),
        thousands(r.tools_tokens),
        thousands(r.messages_tokens),
        thousands(r.images_tokens),
        r.window
            .map_or("-".to_string(), |w| share(r.total_tokens, w)),
    ));
    if r.compile_tokens != r.total_tokens {
        out.push(format!(
            "  the compile counts {} (the provider's count of what the last request repeated)",
            tok(r.compile_tokens)
        ));
    }
    if r.recall_tokens > 0 {
        out.push(format!(
            "  recalled notes in the messages: {}",
            tok(r.recall_tokens)
        ));
    }
    if let Some(e) = &r.exact {
        out.push(format!(
            "exact (the provider's count): {} = system {} + tools {} + messages {}; the estimate reads {}",
            tok(e.total),
            thousands(e.system),
            thousands(e.tools),
            thousands(e.messages),
            signed(r.total_tokens, e.total),
        ));
    }
    if let Some(why) = &r.exact_error {
        out.push(format!("exact: not counted ({why})"));
    }
    out.push(String::new());
    out.push(format!(
        "system · {} blocks · {}",
        r.system.len(),
        tok(r.system_tokens)
    ));
    for b in &r.system {
        out.push(format!(
            "  {:<8} {:>9}  {} B  (framing {})",
            b.block,
            tok(b.tokens),
            thousands(b.bytes),
            b.framing
        ));
        for p in &b.parts {
            out.push(format!(
                "    {:<34} {:>9}  {} B",
                p.name,
                tok(p.tokens),
                thousands(p.bytes)
            ));
        }
    }
    out.push(String::new());
    out.extend(tool_lines(r));
    for n in &r.notes {
        out.push(format!("note: {n}"));
    }
    out
}

/// `-6.9%`: how far the estimate `est` is from the provider's count `exact`.
fn signed(est: u64, exact: u64) -> String {
    if exact == 0 {
        return "-".into();
    }
    let d = (est as f64 - exact as f64) * 100.0 / exact as f64;
    format!("{d:+.1}% against it")
}

fn tool_lines(r: &ContextCensusResult) -> Vec<String> {
    let mut tools: Vec<&CensusTool> = r.tools.iter().collect();
    tools.sort_by(|a, b| b.tokens.cmp(&a.tokens).then(a.name.cmp(&b.name)));
    let since = fmt_day(r.use_since_ms);
    let mut out = vec![format!(
        "tools · {} · {}  (use: calls planned since {since}, {} days, across every session)",
        r.tools.len(),
        tok(r.tools_tokens),
        r.use_days,
    )];
    if let Some(why) = &r.use_unread {
        out.push(format!("  use not read: {why}"));
    }
    for t in tools {
        let used = match (r.use_unread.is_some(), t.last_call_ms) {
            (true, _) => String::new(),
            (false, Some(at)) => format!("{:>6} calls, last {}", thousands(t.calls), fmt_date(at)),
            (false, None) => "     0 calls".to_string(),
        };
        out.push(format!(
            "  {:<26} {:>8}  description {:>6} · schema {:>6}  {used}",
            t.name,
            thousands(t.tokens),
            thousands(t.description_tokens),
            thousands(t.schema_tokens),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::census::{CensusBlock, CensusPart};

    fn sample() -> ContextCensusResult {
        ContextCensusResult {
            place: "cli".into(),
            class: "private".into(),
            profile: "bench".into(),
            model: "claude-sonnet-5-5".into(),
            window: Some(1_000_000),
            compile: "fresh".into(),
            system: vec![CensusBlock {
                block: "header".into(),
                bytes: 400,
                tokens: 103,
                framing: 3,
                parts: vec![CensusPart {
                    name: "persona".into(),
                    bytes: 400,
                    tokens: 100,
                }],
            }],
            system_tokens: 103,
            tools: vec![
                CensusTool {
                    name: "fs_list".into(),
                    tokens: 30,
                    description_tokens: 10,
                    schema_tokens: 15,
                    ..CensusTool::default()
                },
                CensusTool {
                    name: "fs_read".into(),
                    tokens: 120,
                    description_tokens: 70,
                    schema_tokens: 40,
                    calls: 14,
                    last_call_ms: Some(1_760_000_000_000),
                    ..CensusTool::default()
                },
            ],
            tools_tokens: 150,
            total_tokens: 253,
            compile_tokens: 253,
            use_since_ms: 1_757_000_000_000,
            use_days: 30,
            ..ContextCensusResult::default()
        }
    }

    #[test]
    fn the_largest_tool_comes_first_with_its_use() {
        super::super::time::pin_for_tests();
        let text = census_lines(&sample()).join("\n");
        let read = text.find("fs_read").unwrap();
        let list = text.find("fs_list").unwrap();
        assert!(read < list, "{text}");
        assert!(text.contains("14 calls, last 2025-10-09"), "{text}");
        assert!(text.contains("0 calls"), "{text}");
        assert!(
            text.contains("request: 253 tok = system 103 + tools 150"),
            "{text}"
        );
        assert!(text.contains("persona"), "{text}");
    }

    #[test]
    fn an_unread_use_says_why_and_prints_no_counts() {
        super::super::time::pin_for_tests();
        let mut r = sample();
        r.use_unread = Some("the index's shape is being built".into());
        let text = census_lines(&r).join("\n");
        assert!(text.contains("use not read: the index's shape is being built"));
        assert!(!text.contains("calls, last"), "{text}");
    }
}
