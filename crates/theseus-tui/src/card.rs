//! The card (design `stage2` §2.9, "Answering"): the focused session's first
//! question, whole, at the foot of the detail pane, with the keys that answer
//! it. A tool's question shows the tool, the input (clipped), the reason, the
//! floor, the external text (with `t` to trust), and the expiry as a
//! countdown; a budget question shows spent, limit, needed, and lifetime.

use theseus_client::render::{self, held_what, Tag};
use theseus_protocol::{usd, ConfirmRequest};

use crate::board::Question;
use crate::held;

/// An approval's first line: who wants to do what in plain words
/// (theseus-cudar), else the tool and its call.
fn confirm_head(c: &ConfirmRequest) -> String {
    match &c.plain {
        Some(p) => format!("⏸ {} wants to {}", p.who, p.what),
        None => format!(
            "⏸ confirm {}: {}",
            c.tool,
            theseus_protocol::term::words::summarize(&c.tool, &c.input)
        ),
    }
}

/// Its risk, the model's reason and what Yes and No do, else the gate's reason.
fn confirm_why(c: &ConfirmRequest) -> Vec<(Tag, String)> {
    match &c.plain {
        Some(p) => p
            .lines()
            .split_off(1)
            .into_iter()
            .map(|l| (Tag::Dim, format!("  {l}")))
            .collect(),
        None => vec![(Tag::Dim, format!("  why: {}", c.reason))],
    }
}

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
        // A question with options (`owner_ask`, theseus-j7cje).
        Question::Whole(c) if q.is_ask() => ask_lines(c, now_ms, refused, &mut out),
        Question::Whole(c) => {
            out.push((Tag::Ask, confirm_head(c)));
            // The countdown and the floor on a line of their own, before the
            // reason: a long reason (a path, a command) wraps over rows of its
            // own (`ui::card_rows`), and would push them down.
            out.extend(when(
                c.floor,
                held::deadline_of(c),
                held::waited_label(c, now_ms),
                now_ms,
            ));
            out.extend(confirm_why(c));
            out.extend(preview(c));
            if let Some(t) = c.task.as_ref().filter(|_| c.plain.is_none()) {
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
            // "Yes, and don't ask again" (theseus-drjs): what it covers on a
            // line of its own, short, so it stays whole on a narrow pane.
            if let Some(o) = &c.grant_offer {
                out.push((Tag::Plain, offer_line(o)));
            }
            push_refusal(&mut out, refused);
            out.push((
                Tag::Plain,
                match (&c.grant_offer, c.external_text.is_some()) {
                    (_, true) => "  [y] approve  [t] approve + trust  [n] decline".to_string(),
                    (Some(o), false) => format!(
                        "  [y] approve  [{}] approve + don't ask again  [n] decline",
                        o.key()
                    ),
                    (None, false) => "  [y] approve  [n] decline".to_string(),
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
            out.extend(when(p.floor, p.expires_at_ms, None, now_ms));
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

/// What the call would do, under its question (theseus-z6au): an edit's hunk,
/// numbered, its changed words marked, or a command with its directory; at
/// most `PREVIEW_ROWS` rows, so the keys that answer stay on the screen.
fn preview(c: &theseus_protocol::ConfirmRequest) -> Vec<(Tag, String)> {
    let mut lines = render::diffview::preview_lines(c);
    if lines.len() > PREVIEW_ROWS {
        let more = lines.len() - PREVIEW_ROWS + 1;
        lines.truncate(PREVIEW_ROWS - 1);
        lines.push(render::Line::new(
            Tag::Dim,
            format!("      … {more} more rows (theseus answer shows them)"),
        ));
    }
    lines.into_iter().map(|l| (l.tag, l.text)).collect()
}

/// The most rows of a call's preview the card shows.
const PREVIEW_ROWS: usize = 12;

/// `  [a] don't ask again: python3 -m unittest, this conversation`.
pub fn offer_line(o: &theseus_protocol::grants::GrantOffer) -> String {
    // A plan's other yes is a mode, not a grant (theseus-lyxy).
    if o.kind == theseus_protocol::grants::KIND_MODE {
        return format!(
            "  [{}] approve the plan, and accept edits: file edits here run with a notice",
            o.key()
        );
    }
    let what = match o.kind.as_str() {
        theseus_protocol::grants::KIND_EDITS => format!("edits under {}", o.pattern),
        _ => o.pattern.clone(),
    };
    format!("  [{}] don't ask again: {what}, this conversation", o.key())
}

/// A failure's card (theseus-7zgsm): what failed, what it cost, and the keys
/// that retry it (a task only), dismiss it, or open it. `refused`: why the
/// last try did not count.
pub fn failure_lines(
    v: &theseus_protocol::ExecutionView,
    refused: Option<&str>,
) -> Vec<(Tag, String)> {
    let what = match v.state.as_str() {
        "blocked" => "blocked",
        "budget_exhausted" => "out of budget",
        _ => "failed",
    };
    let mut out = vec![(
        Tag::Bad,
        match v.ended_reason.as_deref().filter(|r| !r.is_empty()) {
            Some(r) => format!("✗ {what}: {}", theseus_client::render::clip(r, 200)),
            None => format!("✗ {what}"),
        },
    )];
    out.push((
        Tag::Dim,
        format!(
            "  spent {} of {} · turn {}",
            usd(v.spent_usd),
            usd(v.limit_usd),
            v.turns
        ),
    ));
    push_refusal(&mut out, refused);
    out.push((
        Tag::Plain,
        if v.kind == theseus_protocol::SessionKind::Task {
            "  [r] retry  [d] dismiss  [o] open".to_string()
        } else {
            "  [d] dismiss  [o] open".to_string()
        },
    ));
    out
}

/// The card of a question with options: the question, each option numbered
/// (the keys 1 to 6), what happens if no one answers, and who asks.
fn ask_lines(
    c: &theseus_protocol::ConfirmRequest,
    now_ms: u64,
    refused: Option<&str>,
    out: &mut Vec<(Tag, String)>,
) {
    let ask = theseus_protocol::work::ask::AskInput::parse(&c.input).ok();
    out.push((
        Tag::Ask,
        format!(
            "? {}",
            ask.as_ref().map_or("a question", |a| a.question.as_str())
        ),
    ));
    out.extend(when(false, c.expires_at_ms, None, now_ms));
    for (i, o) in ask.iter().flat_map(|a| a.options.iter()).enumerate() {
        let detail = o.detail.as_deref().map(|d| format!(" — {d}"));
        out.push((
            Tag::Plain,
            format!("  [{}] {}{}", i + 1, o.label, detail.unwrap_or_default()),
        ));
    }
    if let Some(a) = &ask {
        out.push((Tag::Dim, format!("  if no one answers: {}", a.on_expiry())));
    }
    if let Some(t) = &c.task {
        out.push((Tag::Dim, format!("  asked by task {}", t.short)));
    }
    push_refusal(out, refused);
}

fn push_refusal(out: &mut Vec<(Tag, String)>, refused: Option<&str>) {
    if let Some(r) = refused {
        out.push((Tag::Bad, format!("  refused: {r}")));
    }
}

/// `  expires in 4:12 · FLOOR`, or `  waiting 12 min · FLOOR` for a question
/// held past its life (`held`): none for a question with neither.
fn when(
    floor: bool,
    expires_at_ms: u64,
    waited: Option<String>,
    now_ms: u64,
) -> Option<(Tag, String)> {
    let mut parts = Vec::new();
    if expires_at_ms > 0 {
        parts.push(countdown(expires_at_ms, now_ms));
    }
    parts.extend(waited);
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
