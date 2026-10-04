//! The card (design `stage2` §2.9, "Answering"): the focused session's first
//! question, whole, at the foot of the detail pane, with the keys that answer
//! it. A tool's question shows the tool, the input (clipped), the reason, the
//! floor, the external text (with `t` to trust), and the expiry as a
//! countdown; a budget question shows spent, limit, needed, and lifetime.

use serde_json::Value;
use theseus_client::render::{held_what, Tag};
use theseus_protocol::usd;

use crate::board::Question;

/// The card's lines, each with its tag. `refused`: why the last answer to it
/// did not count. `more`: how many questions wait behind it.
pub fn lines(
    q: Question<'_>,
    now_ms: u64,
    refused: Option<&str>,
    more: usize,
) -> Vec<(Tag, String)> {
    let mut out = Vec::new();
    match q {
        Question::Whole(c) if c.budget.is_some() => {
            let b = c.budget.as_ref().expect("a budget question");
            out.push((
                Tag::Ask,
                format!(
                    "$ budget: spent {} of {} · needs {} · lifetime {}",
                    usd(b.spent_usd),
                    usd(b.limit_usd),
                    usd(b.needed_usd),
                    usd(b.lifetime_usd)
                ),
            ));
            out.push((Tag::Dim, "  approve resets its spend to $0".to_string()));
            push_refusal(&mut out, refused);
            out.push((
                Tag::Plain,
                "  [y] reset and continue  [n] keep waiting".to_string(),
            ));
        }
        Question::Whole(c) => {
            out.push((
                Tag::Ask,
                format!("⏸ confirm {}: {}", c.tool, summary(&c.input)),
            ));
            // The countdown and the floor on a line of their own, before the
            // reason: a long reason (a path, a command) wraps over rows of its
            // own (`ui::card_rows`), and would push them down.
            out.extend(when(c.floor, c.expires_at_ms, now_ms));
            out.push((Tag::Dim, format!("  why: {}", c.reason)));
            if let Some(t) = &c.task {
                out.push((
                    Tag::Dim,
                    format!(
                        "  asked by task {}{}",
                        t.short,
                        t.title
                            .as_deref()
                            .map(|x| format!(": {x}"))
                            .unwrap_or_default()
                    ),
                ));
            }
            if let Some(h) = &c.external_text {
                out.push((
                    Tag::Warn,
                    format!("  it waits because the session read {}", held_what(h)),
                ));
            }
            push_refusal(&mut out, refused);
            out.push((
                Tag::Plain,
                if c.external_text.is_some() {
                    "  [y] approve  [t] approve + trust  [n] decline".to_string()
                } else {
                    "  [y] approve  [n] decline".to_string()
                },
            ));
        }
        Question::Brief(p) if p.budget => {
            out.push((
                Tag::Ask,
                "$ budget: the session reached its spend limit".to_string(),
            ));
            out.push((Tag::Dim, "  approve resets its spend to $0".to_string()));
            push_refusal(&mut out, refused);
            out.push((
                Tag::Plain,
                "  [y] reset and continue  [n] keep waiting".to_string(),
            ));
        }
        Question::Brief(p) => {
            out.push((Tag::Ask, format!("⏸ confirm {}: {}", p.tool, p.reason)));
            out.extend(when(p.floor, p.expires_at_ms, now_ms));
            out.push((Tag::Dim, "  reading the whole question…".to_string()));
            push_refusal(&mut out, refused);
            out.push((Tag::Plain, "  [y] approve  [n] decline".to_string()));
        }
    }
    if more > 0 {
        out.push((Tag::Dim, format!("  +{more} more after it")));
    }
    out
}

fn push_refusal(out: &mut Vec<(Tag, String)>, refused: Option<&str>) {
    if let Some(r) = refused {
        out.push((Tag::Bad, format!("  refused: {r}")));
    }
}

/// `  expires in 4:12 · FLOOR`: none for a question with neither.
fn when(floor: bool, expires_at_ms: u64, now_ms: u64) -> Option<(Tag, String)> {
    let mut parts = Vec::new();
    if expires_at_ms > 0 {
        parts.push(countdown(expires_at_ms, now_ms));
    }
    if floor {
        parts.push("FLOOR".to_string());
    }
    (!parts.is_empty()).then(|| (Tag::Dim, format!("  {}", parts.join(" · "))))
}

/// `expires in 4:12`, or `expired`.
pub fn countdown(expires_at_ms: u64, now_ms: u64) -> String {
    if expires_at_ms <= now_ms {
        return "expired".to_string();
    }
    let s = (expires_at_ms - now_ms).div_ceil(1000);
    format!("expires in {}:{:02}", s / 60, s % 60)
}

/// A call's input in one line: a command's argv, a path, else its JSON.
pub fn summary(input: &Value) -> String {
    if let Some(argv) = input.get("argv").and_then(Value::as_array) {
        let words: Vec<String> = argv
            .iter()
            .map(|a| {
                a.as_str()
                    .map(String::from)
                    .unwrap_or_else(|| a.to_string())
            })
            .collect();
        return words.join(" ");
    }
    for key in ["command", "path", "url", "query"] {
        if let Some(s) = input.get(key).and_then(Value::as_str) {
            return s.to_string();
        }
    }
    theseus_client::render::clip(&input.to_string(), 200)
}
