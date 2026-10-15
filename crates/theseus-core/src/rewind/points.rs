//! The person's messages of a session with the files the session's edits
//! changed since each: what the picker lists, newest first.

use anyhow::Result;
use theseus_protocol::{FileChange, RewindPoint};

use super::{rows, Before};
use crate::node::{Body, Origin};
use crate::store::Store;
use crate::stub::Kind;

/// The newest `limit` messages.
pub(crate) fn points(
    store: &Store,
    session_id: &str,
    (limit, only): (usize, Option<&str>),
) -> Result<Vec<RewindPoint>> {
    let transcript = store.transcript(session_id)?;
    let edits = rows(store, session_id)?;
    let mut out = Vec::new();
    for (i, (pos, stub)) in transcript.iter().enumerate().rev() {
        if out.len() >= limit {
            break;
        }
        if stub.kind != Kind::UserMessage || stub.origin == Origin::Harness {
            continue;
        }
        if only.is_some_and(|id| id != stub.id) {
            continue;
        }
        let Body::UserMessage { text, .. } = &stub.body else {
            continue;
        };
        out.push(RewindPoint {
            node_id: stub.id.clone(),
            position: *pos,
            at_ms: stub.created_at_ms,
            text: first_line(text),
            message: text
                .chars()
                .take(theseus_protocol::rewind::MESSAGE_CAP)
                .collect(),
            files: changed_since(&edits, *pos),
            commands: commands_since(&transcript[i + 1..]),
            fork_at: fork_at(&transcript[..i]),
        });
    }
    Ok(out)
}

fn first_line(text: &str) -> String {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    match line.char_indices().nth(100) {
        Some((i, _)) => format!("{}…", &line[..i]),
        None => line.to_string(),
    }
}

/// One entry for each file the edits after `since` changed.
fn changed_since(edits: &[(u64, super::PreImage)], since: u64) -> Vec<FileChange> {
    let mut files: Vec<FileChange> = Vec::new();
    for (_, p) in edits.iter().filter(|(pos, _)| *pos > since) {
        match files.iter_mut().find(|f| f.path == p.path) {
            Some(f) => {
                f.added += p.added;
                f.removed += p.removed;
            }
            None => files.push(FileChange {
                path: p.path.clone(),
                added: p.added,
                removed: p.removed,
                created: p.before == Before::Absent,
            }),
        }
    }
    files
}

fn commands_since(after: &[(u64, crate::stub::Stub)]) -> u32 {
    after
        .iter()
        .filter(|(_, s)| s.kind == Kind::ToolCall)
        .filter(|(_, s)| matches!(&s.body, Body::ToolCall { tool, .. } if tool == "proc.run"))
        .count() as u32
}

/// The node a fork ends at to hold everything before a message: the nearest
/// earlier one that leaves no call without its result.
fn fork_at(before: &[(u64, crate::stub::Stub)]) -> Option<String> {
    before.iter().rev().find_map(|(_, s)| {
        let closed = match &s.body {
            Body::UserMessage { .. } => s.origin != Origin::Harness,
            Body::AssistantMessage { blocks, .. } => !blocks
                .iter()
                .any(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_use")),
            _ => return None,
        };
        closed.then(|| s.id.clone())
    })
}

/// How many programs the session ran after the position `since`.
pub(crate) fn commands_after(store: &Store, session_id: &str, since: u64) -> Result<u32> {
    let t = store.transcript(session_id)?;
    let i = t.partition_point(|(pos, _)| *pos <= since);
    Ok(commands_since(&t[i..]))
}
