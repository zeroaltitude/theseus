//! How full the context is (theseus-wy6v.2), as every surface says it: the
//! status line's `ctx 7%`, and the short form of `theseus why` that `/context`
//! in the chat and `theseus why --short` print. Pure: numbers in, text out. The
//! parts and their tokens are `context.explain`'s, the words of `why.rs`'s
//! (`num`, the block names); this is the short form beside its full one, never
//! a second reading of the compile.

use crate::context::{ContextExplainResult, ContextPart};
use crate::why::num;

/// The share of the window at which the status line starts to warn that a
/// compaction is near. The compaction itself comes when the request passes
/// the compiler's limit (the window less the reply's cap and a margin), and a
/// clearing pass of old results comes at half of that limit.
pub const COMPACT_WARN_PCT: f64 = 80.0;

/// The width of a part's bar, in cells.
const BAR: usize = 20;

/// `tokens` as a percentage of `window`; none with no window.
pub fn share(tokens: u64, window: u64) -> Option<f64> {
    (window > 0).then(|| tokens as f64 * 100.0 / window as f64)
}

/// The status line's segment: `ctx 7%`, and past the warn band `ctx 83% ·
/// compacts soon`. None before a request has been compiled, or with no
/// window to measure by.
pub fn status_words(tokens: Option<u64>, window: Option<u64>) -> Option<String> {
    let pct = share(tokens?, window?)?;
    let shown = pct.round() as u64;
    Some(if pct >= COMPACT_WARN_PCT {
        format!("ctx {shown}% · compacts soon")
    } else {
        format!("ctx {shown}%")
    })
}

impl crate::TurnSubmitResult {
    /// `ctx 7%` for the turn's last request, in the footer after its cost;
    /// none from a daemon that does not send the pair.
    pub fn context_words(&self) -> Option<String> {
        status_words(self.context_tokens, self.context_window)
    }
}

/// The tokens of the parts of one kind of block.
fn tokens_of(parts: &[ContextPart], blocks: &[&str]) -> u64 {
    parts
        .iter()
        .filter(|p| blocks.contains(&p.block.as_str()))
        .map(|p| p.tokens)
        .sum()
}

/// How many tools the tools part names: its name leads with the count, else
/// its note lists them.
fn tool_count(parts: &[ContextPart]) -> Option<u64> {
    let p = parts.iter().find(|p| p.block == "tools")?;
    p.name
        .split_whitespace()
        .next()
        .and_then(|w| w.parse().ok())
        .or_else(|| p.note.as_deref().map(|n| n.split(", ").count() as u64))
}

fn bar(tokens: u64, window: u64) -> String {
    let cells = if window == 0 {
        0
    } else {
        ((tokens as f64 / window as f64) * BAR as f64).ceil() as usize
    };
    let cells = cells.min(BAR);
    format!("{}{}", "█".repeat(cells), "░".repeat(BAR - cells))
}

fn row(label: &str, tokens: u64, window: Option<u64>) -> String {
    match window {
        Some(w) => format!(
            "  {label:<26} {:>9}  {}  {}",
            num(tokens),
            bar(tokens, w),
            pct(tokens, w)
        ),
        None => format!("  {label:<26} {:>9}", num(tokens)),
    }
}

fn pct(tokens: u64, window: u64) -> String {
    share(tokens, window).map_or_else(String::new, |p| format!("{p:.1}%"))
}

/// `/context` and `theseus why --short`: the total against the window, a row for
/// each kind of part with its tokens and a bar, the free space, and where the
/// rest is. The parts and their tokens are the full `why`'s own.
pub fn context_short(x: &ContextExplainResult) -> Vec<String> {
    if x.turn_id.is_none() {
        return vec!["no turn of this session has compiled a context yet".into()];
    }
    if let Some(u) = &x.unbuilt {
        return vec![format!("the parts could not be built: {u}")];
    }
    let total: u64 = x.parts.iter().map(|p| p.tokens).sum();
    let window = x.window.filter(|w| *w > 0);
    let mut out = vec![match window {
        Some(w) => format!("{} of {} tokens ({})", num(total), num(w), pct(total, w)),
        None => format!("{} tokens (the model's window is not known)", num(total)),
    }];
    // A detour's request is its own (theseus-r5ydi): its parts are the ones its row names.
    let detour = x.detour.as_ref();
    out.extend(detour.map(crate::why::detour_words));
    let recalled = x.parts.iter().filter(|p| p.block == "recall").count();
    let tools = tool_count(&x.parts).map_or(String::new(), |n| format!(" ({n})"));
    let messages = x
        .compiled
        .as_ref()
        .map(|c| c.messages)
        .filter(|m| *m > 0)
        .map_or(String::new(), |m| format!(" ({m} messages)"));
    out.push(row(
        "system and persona",
        tokens_of(
            &x.parts,
            &["header", "guidance", "session", "persona", "standing"],
        ),
        window,
    ));
    out.push(row(
        &format!("tools{tools}"),
        tokens_of(&x.parts, &["tools"]),
        window,
    ));
    out.push(row(
        "context files",
        tokens_of(&x.parts, &["context"]),
        window,
    ));
    out.push(row(
        &format!("recalled ({recalled})"),
        tokens_of(&x.parts, &["recall"]),
        window,
    ));
    out.push(row(
        &format!("conversation{messages}"),
        tokens_of(&x.parts, &["conversation", "exchanges", "message"]),
        window,
    ));
    if detour.is_some() {
        out.push(row(
            "project slice",
            tokens_of(&x.parts, &["project"]),
            window,
        ));
    }
    if let Some(w) = window {
        out.push(row("free space", w.saturating_sub(total), Some(w)));
    }
    out.push("`theseus why` names every part and the rule that put it there".into());
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::why::{why_lines, WhyInput};

    /// A private place's turn: persona, two context files, guidance, 31 tools,
    /// two recalled notes and a conversation of 9 messages, against 200,000.
    fn explain() -> ContextExplainResult {
        serde_json::from_value(json!({
            "session_id": "ses_reef42", "kind": "conversation", "class": "private", "place": "CLI",
            "turns": [], "turn_id": "trn_k3m9x2",
            "compiled": {"session_id": "ses_reef42", "turn_id": "trn_k3m9x2", "loop": 0,
                "decision": "append", "messages": 9},
            "profile": "deep", "model": "glm-x", "window": 200000,
            "parts": [
                {"block": "header", "name": "persona", "bytes": 4800, "tokens": 1200},
                {"block": "context", "name": "AGENTS.md", "bytes": 9000, "tokens": 2250},
                {"block": "context", "name": "NOTES.md", "bytes": 80, "tokens": 20},
                {"block": "guidance", "name": "preamble", "bytes": 400, "tokens": 100},
                {"block": "tools", "name": "31 tools", "bytes": 12000, "tokens": 3000,
                 "note": "fs.read, proc.run"},
                {"block": "recall", "name": "ses_ep9 · chat", "bytes": 800, "tokens": 210},
                {"block": "recall", "name": "ses_reef42 · note", "bytes": 300, "tokens": 90},
                {"block": "conversation", "name": "6 nodes in the prefix, 3 in the tail",
                 "bytes": 20000, "tokens": 5000}],
            "digest_now": "d1", "recalls": [], "sources": {}, "ms": 4.0
        }))
        .unwrap()
    }

    #[test]
    fn the_share_is_of_the_window_and_warns_past_the_band() {
        assert_eq!(status_words(Some(14_210), Some(200_000)).unwrap(), "ctx 7%");
        assert_eq!(
            status_words(Some(166_000), Some(200_000)).unwrap(),
            "ctx 83% · compacts soon"
        );
        assert_eq!(status_words(None, Some(200_000)), None);
        assert_eq!(status_words(Some(5), None), None);
        assert_eq!(status_words(Some(5), Some(0)), None);
        // The wrong window would say 14%: the share is of the window given.
        assert_eq!(
            status_words(Some(14_210), Some(100_000)).unwrap(),
            "ctx 14%"
        );
    }

    #[test]
    fn the_pair_round_trips_and_an_older_peer_decodes() {
        let r = crate::TurnSubmitResult {
            context_tokens: Some(14_210),
            context_window: Some(200_000),
            ..Default::default()
        };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["context_tokens"], 14_210);
        let back: crate::TurnSubmitResult = serde_json::from_value(v).unwrap();
        assert_eq!(back.context_words().unwrap(), "ctx 7%");
        // A daemon from before it sends neither, and none is written back.
        let old = serde_json::to_value(crate::TurnSubmitResult::default()).unwrap();
        assert!(old.get("context_tokens").is_none() && old.get("context_window").is_none());
        let back: crate::TurnSubmitResult = serde_json::from_value(old).unwrap();
        assert_eq!((back.context_tokens, back.context_window), (None, None));
        assert_eq!(back.context_words(), None);
    }

    #[test]
    fn the_short_form_names_the_parts_with_amounts_and_the_free_space() {
        let lines = context_short(&explain());
        assert_eq!(lines[0], "11,870 of 200,000 tokens (5.9%)");
        let text = lines.join("\n");
        for part in [
            "system and persona",
            "tools (31)",
            "context files",
            "recalled (2)",
            "conversation (9 messages)",
            "free space",
        ] {
            assert!(text.contains(part), "{part} is missing:\n{text}");
        }
        assert!(text.contains("188,130"), "the free space:\n{text}");
        assert!(lines
            .last()
            .unwrap()
            .contains("`theseus why` names every part"));
    }

    /// The tokens a row of the short form gives for `label`: its fixed column.
    fn shown(lines: &[String], label: &str) -> u64 {
        let l = lines
            .iter()
            .find(|l| l.trim_start().starts_with(label))
            .unwrap();
        let col: String = l.chars().skip(2 + 26 + 1).take(9).collect();
        col.trim().replace(',', "").parse().unwrap()
    }

    /// `theseus why`'s lines add up, block by block, to the same tokens.
    #[test]
    fn the_short_form_and_why_agree_on_every_parts_tokens() {
        let x = explain();
        let why = why_lines(&WhyInput {
            explain: &x,
            policy: None,
            route: None,
            tools: &[],
            hide_sources: false,
        });
        let of = |blocks: &[&str]| -> u64 {
            why.iter()
                .filter_map(|l| {
                    let t = l.trim_start();
                    let block = t.split_whitespace().next()?;
                    blocks.contains(&block).then_some(())?;
                    let tok = t.split(" · ").find(|f| f.ends_with(" tok"))?;
                    tok.trim_end_matches(" tok")
                        .replace(',', "")
                        .parse::<u64>()
                        .ok()
                })
                .sum()
        };
        let short = context_short(&x);
        for (label, blocks) in [
            ("system and persona", &["header", "guidance"][..]),
            ("tools", &["tools"]),
            ("context files", &["context"]),
            ("recalled", &["recall"]),
            ("conversation", &["conversation"]),
        ] {
            let w = of(blocks);
            assert!(w > 0, "why shows no tokens for {label}");
            assert_eq!(shown(&short, label), w, "{label}\n{short:#?}\n{why:#?}");
        }
    }

    #[test]
    fn a_turn_with_no_compile_or_no_window_says_so() {
        let mut x = explain();
        x.window = None;
        let lines = context_short(&x);
        assert!(lines[0].contains("window is not known"), "{lines:?}");
        assert!(!lines.iter().any(|l| l.contains("free space")));
        x.turn_id = None;
        assert!(context_short(&x)[0].contains("no turn"));
    }
}
