//! A Vestige dump, read as imported notes (theseus-71ib): a JSON file of
//! memories, each made one episode of the import's format (source `vestige`,
//! a `file` place named by the dump, one message of the note's text), which
//! `import.episodes` takes as it takes any other. No Vestige connection: a
//! file read.
//!
//! - **The shapes read**: an array of memories, or an object holding one
//!   under `memories`, `items`, `nodes`, `entries` or `data`. A memory is a
//!   string, or an object whose text is the first of `content`, `text`,
//!   `memory`, `note`, `body`, `summary`; its time the first of `created_at`,
//!   `createdAt`, `created`, `timestamp`, `date`, `time`, `updated_at` (RFC
//!   3339, or a number of seconds or milliseconds), else the file's
//!   modification time; its `tags` (or `tag`) are kept as a last line.
//! - **Once**: an episode's id is the digest of its text, so a memory seen
//!   again, in this dump or a later one, is the same episode and is skipped
//!   (the daemon says so; a changed time or tag set with the same text is
//!   the same note and also skipped).
//! - **Scrubbed by the daemon**, as every harness's text is: the CLI holds
//!   no vault value (`source` `vestige` is in its scrubbed sources).

use std::path::{Path, PathBuf};

use anyhow::{bail, Context as _, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use theseus_protocol::import::{canonical, EPISODE_FORMAT};

/// The episode's source and the tag the notes land in.
pub const SOURCE: &str = "vestige";
pub const TAG: &str = "vestige";
/// A note's text, in characters, past which it is cut.
pub const TEXT_MAX: usize = 32_000;

const TEXT_KEYS: &[&str] = &["content", "text", "memory", "note", "body", "summary"];
const TIME_KEYS: &[&str] = &[
    "created_at",
    "createdAt",
    "created",
    "timestamp",
    "date",
    "time",
    "updated_at",
];
const LIST_KEYS: &[&str] = &["memories", "items", "nodes", "entries", "data"];

/// One memory of a dump.
#[derive(Debug, Clone, PartialEq)]
pub struct Memory {
    pub text: String,
    pub at_ms: i64,
    pub tags: Vec<String>,
}

/// The dump files under `path`: the file itself, or a directory's
/// `vestige*.json` files (not recursive), sorted.
pub fn dumps_at(path: &Path) -> Result<Vec<PathBuf>> {
    if path.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }
    if !path.is_dir() {
        bail!("{} is no file or directory", path.display());
    }
    let mut found = Vec::new();
    for e in std::fs::read_dir(path).with_context(|| format!("reading {}", path.display()))? {
        let p = e?.path();
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_lowercase();
        if p.is_file() && name.starts_with("vestige") && name.ends_with(".json") {
            found.push(p);
        }
    }
    found.sort();
    Ok(found)
}

fn time_of(v: &Value) -> Option<i64> {
    match v {
        Value::String(s) => s
            .parse::<jiff::Timestamp>()
            .ok()
            .map(|t| t.as_millisecond()),
        Value::Number(n) => {
            let f = n.as_f64()?;
            Some(if f > 1e11 {
                f as i64
            } else {
                (f * 1000.0) as i64
            })
        }
        _ => None,
    }
}

fn cut(text: &str) -> String {
    match text.char_indices().nth(TEXT_MAX) {
        Some((at, _)) => format!(
            "{}\n[cut: {TEXT_MAX} of {} characters kept]",
            &text[..at],
            text.chars().count()
        ),
        None => text.to_string(),
    }
}

/// The memories of a dump's text; `fallback_ms` is the time of one that names
/// none. Nothing readable is an error naming what was expected.
pub fn parse(text: &str, fallback_ms: i64) -> Result<Vec<Memory>> {
    let v: Value = serde_json::from_str(text).context("not JSON")?;
    let list = match &v {
        Value::Array(a) => a,
        Value::Object(o) => LIST_KEYS
            .iter()
            .find_map(|k| o.get(*k).and_then(Value::as_array))
            .context(
                "an object with no list of memories (memories, items, nodes, entries, data)",
            )?,
        _ => bail!("not an array of memories or an object holding one"),
    };
    let mut out = Vec::new();
    for item in list {
        let (text, at_ms, tags) = match item {
            Value::String(s) => (s.clone(), fallback_ms, Vec::new()),
            Value::Object(o) => {
                let Some(text) = TEXT_KEYS
                    .iter()
                    .find_map(|k| o.get(*k).and_then(Value::as_str))
                else {
                    continue;
                };
                let at = TIME_KEYS
                    .iter()
                    .find_map(|k| o.get(*k).and_then(time_of))
                    .unwrap_or(fallback_ms);
                let tags = match o.get("tags").or_else(|| o.get("tag")) {
                    Some(Value::Array(a)) => a
                        .iter()
                        .filter_map(|t| t.as_str().map(str::to_string))
                        .collect(),
                    Some(Value::String(s)) => vec![s.clone()],
                    _ => Vec::new(),
                };
                (text.to_string(), at, tags)
            }
            _ => continue,
        };
        let text = text.trim();
        if !text.is_empty() {
            out.push(Memory {
                text: text.to_string(),
                at_ms,
                tags,
            });
        }
    }
    Ok(out)
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn rfc3339(ms: i64) -> String {
    jiff::Timestamp::from_millisecond(ms)
        .map(|t| t.strftime("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
        .unwrap_or_else(|_| "1970-01-01T00:00:00.000Z".into())
}

/// A memory as the JSON line `import.episodes` reads, and its id: the
/// digest of its text.
pub fn episode(m: &Memory, file: &str, index: usize) -> (String, String) {
    let id = format!(
        "ep_{}",
        sha256_hex(format!("{SOURCE}\0{}", m.text).as_bytes())
    );
    let mut body = cut(&m.text);
    if !m.tags.is_empty() {
        body.push_str(&format!("\n[tags: {}]", m.tags.join(", ")));
    }
    let at = rfc3339(m.at_ms);
    let mut v = json!({
        "format": EPISODE_FORMAT,
        "import_tag": TAG,
        "episode_id": id,
        "source": SOURCE,
        "agent": SOURCE,
        "place": {"kind": "file", "name": file, "id": null},
        "as_of": {"start": at, "end": at},
        "labels": {
            "sensitivity": "personal",
            "partner": null,
            "topic": [],
            "book_hint": null,
            "credential_redacted": false,
        },
        "summary": null,
        "messages": [{
            "idx": 0,
            "time": at,
            "author": SOURCE,
            "integrity": "agent",
            "text": body,
            "unit": format!("{file}#{index}"),
            "sha256": sha256_hex(body.as_bytes()),
        }],
    });
    let mut out = String::new();
    canonical(&v, true, &mut out);
    v["hash"] = json!(sha256_hex(out.as_bytes()));
    (id, v.to_string())
}

#[cfg(test)]
mod tests;
