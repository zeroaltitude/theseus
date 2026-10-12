//! A developer's history from the coding harnesses (theseus-o73y): the
//! session files of Claude Code and Codex read into episodes of the import's
//! format (format 1), one episode a session file, which `import.episodes`
//! takes as it takes the openclaw pipeline's.
//!
//! What a file becomes (the same for both, each source's reader in its own
//! module):
//!
//! - **Messages.** The person's text is a message of integrity `operator`
//!   (author `user`), the assistant's text one of `agent`
//!   (author `agent:<source>`), each at the line's own timestamp. A tool call
//!   is a line of the assistant's (`[tool_use] <name> <input, cut at
//!   TOOL_INPUT_MAX>`); its result is a line of author `tool`, integrity
//!   `outside` (a result is text from outside the conversation: a page, a
//!   file, a command's output), so recall keeps results out unless the
//!   config admits outside text. A result is cut at `RESULT_MAX` characters
//!   and any other text at `TEXT_MAX`, each cut said in the text with how
//!   much was kept.
//! - **Left out, and counted.** Thinking blocks; a line of a type the reader
//!   does not know, and the harness's own bookkeeping (summaries, snapshots,
//!   meta lines, injected context), each counted by its type in the file's
//!   report; sidechain (subagent) lines and files, skipped whole, since the
//!   parent session's own line already holds what the subagent returned and
//!   the subagent's working would only repeat it at length.
//! - **Rejected, with its number**: a line that is not JSON, not UTF-8, not an
//!   object, or a message line with no readable time.
//! - **The place** is the project directory (`cli`, named by the line's
//!   `cwd`), its id the git branch when the file names one; the topic label
//!   `projects/<directory name>`; the book hint `casebook`.
//! - **Idempotent by construction.** An episode's id is the digest of its
//!   source and the file's stem, so the same file is the same episode in any
//!   tag, and its hash is over the episode's own content, so a file that is
//!   unchanged is skipped, and one that has grown since it was imported
//!   (Claude Code appends to an open session) has another hash: the daemon
//!   appends its new messages when the stored ones are an exact prefix
//!   (theseus-pjmj7), and reports a file whose stored messages differ,
//!   never overwriting it. To take such a file's newer text, erase the tag
//!   and import again.
//!
//! The text is not scrubbed here: the CLI holds no vault value. The daemon
//! scrubs every message as it writes the episode (`import::write::scrubbed`).

use std::collections::BTreeMap;
use std::io::BufRead;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use theseus_protocol::import::{canonical, EPISODE_FORMAT};

pub mod claude_code;
pub mod codex;
pub mod run;
#[cfg(test)]
mod tests;

/// A message's text, in characters, past which it is cut.
pub const TEXT_MAX: usize = 32_000;
/// A tool result's, past which it is cut.
pub const RESULT_MAX: usize = 2_000;
/// A tool call's input, past which it is cut.
pub const TOOL_INPUT_MAX: usize = 400;

/// Which harness a file is from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    ClaudeCode,
    Codex,
}

impl Source {
    /// The episode's `source`, and the default tag's prefix.
    pub fn name(self) -> &'static str {
        match self {
            Source::ClaudeCode => "claude-code",
            Source::Codex => "codex",
        }
    }
}

/// What one file held, for its line of the report.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct Counts {
    /// Lines read, blank ones not counted.
    pub read: u64,
    /// Messages made: text, tool calls, and tool results.
    pub messages: u64,
    /// Of them, the tool calls and results.
    pub tool_lines: u64,
    /// Results and texts cut at their bound.
    pub cut: u64,
    /// Lines and blocks left out, by what they were.
    pub skipped: BTreeMap<String, u64>,
    /// Lines that did not read: their number, and why.
    pub rejected: Vec<(u64, String)>,
}

impl Counts {
    fn skip(&mut self, what: &str) {
        *self.skipped.entry(what.to_string()).or_default() += 1;
    }

    /// Lines and blocks left out, all kinds.
    pub fn skipped_total(&self) -> u64 {
        self.skipped.values().sum()
    }
}

/// A file read: its counts, and its episode when it held any message.
#[derive(Debug, Clone, PartialEq)]
pub struct Converted {
    pub counts: Counts,
    /// The episode's JSON line, hash and all; none for an empty file or one
    /// with no message in it.
    pub episode: Option<String>,
    /// The episode's id, when there is one.
    pub episode_id: Option<String>,
}

/// One message as a reader hands it over.
pub(crate) struct Msg {
    pub author: String,
    pub integrity: &'static str,
    pub text: String,
    pub at_ms: i64,
    pub line: u64,
}

/// What a reader collects from a file.
#[derive(Default)]
pub(crate) struct Draft {
    pub msgs: Vec<Msg>,
    pub cwd: Option<String>,
    pub branch: Option<String>,
}

/// A text cut to `max` characters, with the cut said (`None` when it fits).
pub(crate) fn cut(text: &str, max: usize) -> Option<String> {
    let total = text.chars().count();
    if total <= max {
        return None;
    }
    let kept: String = text.chars().take(max).collect();
    Some(format!("{kept}\n[cut: {max} of {total} characters kept]"))
}

/// `text` within `max` characters, counting a cut in `counts`.
pub(crate) fn bounded(text: &str, max: usize, counts: &mut Counts) -> String {
    match cut(text, max) {
        Some(c) => {
            counts.cut += 1;
            c
        }
        None => text.to_string(),
    }
}

/// A line's time, as unix ms (RFC 3339, any offset), or none.
pub(crate) fn time_ms(v: &Value) -> Option<i64> {
    let s = v.as_str()?;
    s.parse::<jiff::Timestamp>()
        .ok()
        .map(|t| t.as_millisecond())
}

fn rfc3339(ms: i64) -> String {
    jiff::Timestamp::from_millisecond(ms)
        .map(|t| t.strftime("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
        .unwrap_or_else(|_| "1970-01-01T00:00:00.000Z".into())
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A tool call's input, as the model sent it, in a line, cut.
pub(crate) fn short_input(input: &Value, counts: &mut Counts) -> String {
    let text = match input {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    };
    bounded(&text, TOOL_INPUT_MAX, counts)
}

/// The tool line of a call.
pub(crate) fn call_line(name: &str, input: &Value, counts: &mut Counts) -> String {
    let input = short_input(input, counts);
    if input.is_empty() {
        format!("[tool_use] {name}")
    } else {
        format!("[tool_use] {name} {input}")
    }
}

/// The tool line of a result, cut.
pub(crate) fn result_line(text: &str, error: bool, counts: &mut Counts) -> String {
    let head = if error {
        "[tool_result error]"
    } else {
        "[tool_result]"
    };
    format!("{head} {}", bounded(text, RESULT_MAX, counts))
}

/// A project's directory name as a topic's path part.
fn topic_part(cwd: &str) -> Option<String> {
    let name = cwd.trim_end_matches('/').rsplit('/').next()?;
    let part: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "._-".contains(c) {
                c
            } else {
                '-'
            }
        })
        .collect();
    let part = part.trim_matches('-').to_string();
    (!part.is_empty()).then_some(part)
}

/// Read `reader`, a session file's lines, as `source`'s. `stem` is the
/// file's name without its extension and `file` its name for the messages'
/// units, so a message is found again in its file. Never fails on a line: a
/// line that does not read is in `counts.rejected`.
pub fn convert(
    source: Source,
    mut reader: impl BufRead,
    stem: &str,
    file: &str,
    tag: &str,
) -> std::io::Result<Converted> {
    let mut counts = Counts::default();
    let mut draft = Draft::default();
    let mut number = 0u64;
    let mut buf = Vec::new();
    let mut state = codex::State::default();
    loop {
        buf.clear();
        if reader.read_until(b'\n', &mut buf)? == 0 {
            break;
        }
        number += 1;
        let Ok(text) = std::str::from_utf8(&buf) else {
            counts.read += 1;
            counts.rejected.push((number, "not UTF-8".into()));
            continue;
        };
        if text.trim().is_empty() {
            continue;
        }
        counts.read += 1;
        let v: Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(e) => {
                counts.rejected.push((number, format!("not JSON: {e}")));
                continue;
            }
        };
        if !v.is_object() {
            counts.rejected.push((number, "not a JSON object".into()));
            continue;
        }
        match source {
            Source::ClaudeCode => claude_code::line(&v, number, &mut draft, &mut counts),
            Source::Codex => codex::line(&v, number, &mut draft, &mut counts, &mut state),
        }
    }
    counts.messages = draft.msgs.len() as u64;
    counts.tool_lines = draft
        .msgs
        .iter()
        .filter(|m| m.text.starts_with("[tool_"))
        .count() as u64;
    if draft.msgs.is_empty() {
        return Ok(Converted {
            counts,
            episode: None,
            episode_id: None,
        });
    }
    let (id, line) = episode(source, &draft, stem, file, tag);
    Ok(Converted {
        counts,
        episode: Some(line),
        episode_id: Some(id),
    })
}

/// The episode of a draft, as the JSON line `import.episodes` reads, hash
/// and all, and its id.
fn episode(source: Source, d: &Draft, stem: &str, file: &str, tag: &str) -> (String, String) {
    let id = format!(
        "ep_{}",
        sha256_hex(format!("{}\0{stem}", source.name()).as_bytes())
    );
    let start = d.msgs.iter().map(|m| m.at_ms).min().unwrap_or(0);
    let end = d.msgs.iter().map(|m| m.at_ms).max().unwrap_or(0);
    let messages: Vec<Value> = d
        .msgs
        .iter()
        .enumerate()
        .map(|(i, m)| {
            json!({
                "idx": i,
                "time": rfc3339(m.at_ms),
                "author": m.author,
                "integrity": m.integrity,
                "text": m.text,
                "unit": format!("{file}#L{}", m.line),
                "sha256": sha256_hex(m.text.as_bytes()),
            })
        })
        .collect();
    let topic: Vec<String> = d
        .cwd
        .as_deref()
        .and_then(topic_part)
        .map(|p| format!("projects/{p}"))
        .into_iter()
        .collect();
    let mut v = json!({
        "format": EPISODE_FORMAT,
        "import_tag": tag,
        "episode_id": id,
        "source": source.name(),
        "agent": source.name(),
        "place": {"kind": "cli", "name": d.cwd, "id": d.branch},
        "as_of": {"start": rfc3339(start), "end": rfc3339(end)},
        "labels": {
            "sensitivity": "company-confidential",
            "partner": null,
            "topic": topic,
            "book_hint": "casebook",
            "credential_redacted": false,
        },
        "summary": null,
        "messages": messages,
    });
    let mut out = String::new();
    canonical(&v, true, &mut out);
    let hash = sha256_hex(out.as_bytes());
    v["hash"] = json!(hash);
    (id, v.to_string())
}
