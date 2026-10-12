//! `memory.lookup`'s reads by session (theseus-8c8s): one session's
//! messages in order, and the harness's own sessions in a span's listing,
//! beside the imported episodes.
//!
//! - **A session's messages** (`session`): its people's and the model's
//!   messages, oldest first, each with its date, time and author, paged by
//!   `cursor` within the call's token budget. An imported one leads with its
//!   episode's summary on its first page. Tool calls and results are left
//!   out, and so is an imported message of outside text: the answer counts
//!   what it left out, so it never reads as the whole.
//! - **Veiled as the rest is.** An imported session takes its episode's
//!   sensitivity, and a personal or partner-confidential one, or one whose
//!   labels cannot be read, shows its dates and authors and never its text.
//!   The place rule is the caller's: only a private session asks.
//! - **A span's own sessions.** With no words, book or topic, a span lists
//!   the sessions this harness held in it (not the imported ones, which the
//!   catalog lists) before the imported episodes, each with its title.

use std::sync::Arc;

use serde_json::json;
use theseus_protocol::import::ImportSessionsParams;
use theseus_tools::ToolOutput;

use super::{catalog, day, some, span_words, veils, Args, Item, UNREAD};
use crate::node::{Body, Node};
use crate::recall::when::When;
use crate::session::SessionRecord;
use crate::Core;

/// A message's text at most, in tokens, before it is cut.
pub(super) const MESSAGE_MAX: u64 = 1_500;

/// One session's messages, a page of them: what `session` asks.
pub(super) async fn session(
    core: &Arc<Core>,
    a: &Args,
    when: Option<&When>,
    (offset, limit, budget): (u64, u64, u64),
) -> Result<ToolOutput, String> {
    let id = some(&a.session).unwrap_or_default().to_string();
    let c = core.clone();
    let sid = id.clone();
    let nodes = tokio::task::spawn_blocking(move || c.store.session_nodes(&sid))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("session {id} could not be read: {e:#}"))?;
    if nodes.is_empty() {
        return Err(format!(
            "no session is named {id}: give a session id an earlier answer named (ses_…)"
        ));
    }
    let (source, sensitivity, span, summary) = labels_of(core, &id, &nodes).await?;
    let veiled = veils(sensitivity.as_deref());
    let (messages, left) = readable(&nodes, when);
    let total = messages.len() as u64;
    let mut text =
        format!(
        "memory.lookup: session {id} · {source} · {span}{} — {total} message{}{}, oldest first.",
        sensitivity.as_deref().map(|s| format!(" · {s}")).unwrap_or_default(),
        if total == 1 { "" } else { "s" },
        when.map(|w| format!(" in {}", w.said)).unwrap_or_default(),
    );
    if let Some(s) = summary.filter(|_| offset == 0) {
        text.push_str(&match veiled {
            true => "\nIts summary is veiled.".to_string(),
            false => format!("\nIts summary: {}", one_line(&s)),
        });
    }
    let mut used = text.len() as u64 / 4;
    let mut shown = 0u64;
    let cut = budget.min(MESSAGE_MAX) * 4;
    for (i, n) in messages
        .iter()
        .skip(offset as usize)
        .take(limit as usize)
        .enumerate()
    {
        let body = match veiled {
            true => format!(
                "[{} · veiled: its text stays veiled here, as on the cockpit's Context page; the owner opens it there]",
                sensitivity.as_deref().unwrap_or_default().replace('-', " ")
            ),
            false => clipped(&one_line(&crate::recall::text_of(n)), cut as usize),
        };
        let line = format!(
            "\n{}. {} · {}\n   {body}",
            offset + i as u64 + 1,
            moment(n.created_at_ms),
            n.author.as_deref().unwrap_or(match &n.body {
                Body::AssistantMessage { .. } => "the model",
                _ => "someone",
            }),
        );
        let cost = line.len() as u64 / 4;
        if shown > 0 && used + cost > budget {
            break;
        }
        text.push_str(&line);
        used += cost;
        shown += 1;
    }
    let next = offset + shown;
    let more = next < total;
    text.push_str(&match (shown, more) {
        (0, _) => "\nNothing on this page.".to_string(),
        (_, true) => format!(
            "\nShown {}–{next} of {total}. More: call again with this session and cursor {next}.",
            offset + 1
        ),
        (_, false) => format!("\nShown {}–{next} of {total}; that is all.", offset + 1),
    });
    if let Some(left) = left {
        text.push_str(&format!(" Left out: {left}."));
    }
    Ok(ToolOutput {
        text,
        meta: json!({
            "mode": "session", "session": id,
            "when": when.map(theseus_protocol::memory::RecallWhen::from),
            "total": total, "shown": shown, "cursor": offset,
            "next": more.then_some(next), "veiled": veiled, "tokens": used,
        }),
    })
}

/// A session's messages a reader reads, in `when` when it is given, and
/// what it leaves out, said: tool calls and results, and an imported message
/// of outside text.
fn readable<'n>(nodes: &'n [(u64, Node)], when: Option<&When>) -> (Vec<&'n Node>, Option<String>) {
    let (messages, left) = readable_at(nodes, when);
    (messages.into_iter().map(|(_, n)| n).collect(), left)
}

/// `readable`, each message with its WAL position (the MCP tools cite it).
pub(super) fn readable_at<'n>(
    nodes: &'n [(u64, Node)],
    when: Option<&When>,
) -> (Vec<(u64, &'n Node)>, Option<String>) {
    let (mut calls, mut outside) = (0u64, 0u64);
    let messages = nodes
        .iter()
        .map(|(p, n)| (*p, n))
        .filter(|(_, n)| when.is_none_or(|w| w.holds(n.created_at_ms)))
        .filter(|(_, n)| match &n.body {
            Body::UserMessage { .. } | Body::AssistantMessage { .. } => true,
            Body::Imported { integrity, .. } => {
                let out = *integrity == crate::import::Integrity::Outside;
                outside += u64::from(out);
                !out
            }
            Body::ToolCall { .. } | Body::ToolResult { .. } => {
                calls += 1;
                false
            }
            _ => false,
        })
        .filter(|(_, n)| !crate::recall::text_of(n).trim().is_empty())
        .collect();
    let mut left = Vec::new();
    if calls > 0 {
        left.push(crate::narrative::count(
            calls,
            "tool call or result",
            "tool calls and results",
        ));
    }
    if outside > 0 {
        left.push(crate::narrative::count(
            outside,
            "message of outside text",
            "messages of outside text",
        ));
    }
    (messages, (!left.is_empty()).then(|| left.join(" and ")))
}

/// A session's source, sensitivity, span and summary: an imported one's
/// from its episode (one read of the catalog), its labels unread when the
/// catalog does not name it; the harness's own from its record.
async fn labels_of(
    core: &Arc<Core>,
    id: &str,
    nodes: &[(u64, Node)],
) -> Result<(String, Option<String>, String, Option<String>), String> {
    let first = nodes.first().map_or(0, |(_, n)| n.created_at_ms);
    let last = nodes.last().map_or(0, |(_, n)| n.created_at_ms);
    if !crate::import::is_imported(id) {
        let r: Option<SessionRecord> = core.store.get_session(id).map_err(|e| format!("{e:#}"))?;
        let what = r
            .as_ref()
            .and_then(|r| r.label.clone().or_else(|| r.title.clone()))
            .map(|l| format!("this harness ({l})"))
            .unwrap_or_else(|| "this harness".into());
        return Ok((what, None, span_words(first, last), None));
    }
    let p = ImportSessionsParams {
        ids: vec![id.to_string()],
        erased: true,
        limit: Some(1),
        summaries: true,
        ..Default::default()
    };
    let r = catalog(core, p).await?;
    Ok(match r.episodes.into_iter().next() {
        Some(e) => (
            format!("imported from {}", e.source),
            Some(e.sensitivity),
            span_words(e.start_ms, e.end_ms),
            e.summary_text,
        ),
        None => (
            "imported".into(),
            Some(UNREAD.into()),
            span_words(first, last),
            None,
        ),
    })
}

/// The sessions this harness held in `span`, newest first: those with a
/// turn, whose time from their start to their last activity meets it.
pub(super) async fn own_sessions(core: &Arc<Core>, span: &When) -> Result<Vec<Item>, String> {
    let c = core.clone();
    let mut all: Vec<SessionRecord> = tokio::task::spawn_blocking(move || c.store.live_sessions())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("the sessions could not be read: {e:#}"))?;
    all.retain(|r| {
        let end = r.last_active_ms.max(r.created_at_unix_ms);
        r.turns > 0
            && span.from_ms.is_none_or(|f| end >= f)
            && span.to_ms.is_none_or(|t| r.created_at_unix_ms < t)
    });
    all.sort_by_key(|r| std::cmp::Reverse(r.last_active_ms.max(r.created_at_unix_ms)));
    Ok(all
        .into_iter()
        .map(|r| Item {
            when: span_words(
                r.created_at_unix_ms,
                r.last_active_ms.max(r.created_at_unix_ms),
            ),
            source: format!(
                "this harness ({}, {})",
                r.label.as_deref().unwrap_or(r.kind.as_str()),
                crate::narrative::count(r.turns, "turn", "turns")
            ),
            session_id: r.session_id,
            sensitivity: None,
            book: None,
            text: r.title.unwrap_or_default(),
            ..Item::default()
        })
        .collect())
}

/// A time as a page says it: `2026-03-14 09:30`, in the daemon's zone.
pub(super) fn moment(ms: u64) -> String {
    let off = crate::wake::local(ms).offset_secs;
    let secs = (ms / 1000) as i64 + off;
    let (h, m) = (secs.rem_euclid(86_400) / 3600, secs.rem_euclid(3600) / 60);
    format!("{} {h:02}:{m:02}", day(ms))
}

pub(super) fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `s` cut to `max` bytes at a char's edge, saying how much more it held.
pub(super) fn clipped(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut at = max;
    while !s.is_char_boundary(at) {
        at -= 1;
    }
    format!(
        "{} […{} more characters: ask with a larger budget_tokens, up to 6000, to read more of it]",
        &s[..at],
        s[at..].chars().count()
    )
}
