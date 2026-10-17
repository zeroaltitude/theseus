//! What the follower says to herdr, and what it hears (theseus-fuw5d): the params of
//! `layout.apply`, `workspace.report_metadata`, `agent.view.set`, `tab.close` and
//! `notification.show`, the answer of `layout.apply` read for its ids, and the
//! `events.subscribe` stream. herdr's `herdr api schema --json` is the contract; each
//! shape is built in one function here so a change of the schema is a change in one
//! place.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::state::{Counts, Tab};
use crate::herdr::{self, SOURCE};
use crate::herdr_sync::WORKSPACE;

/// The kind token of the follower's own pane.
pub const BOARD_KIND: &str = "board";

/// The kind token of the `main` tab's pane (`herdr_main`).
pub const MAIN_KIND: &str = "main";

/// herdr's socket, and how many requests went to it.
pub struct Herdr {
    pub socket: PathBuf,
    pub n: u64,
}

impl Herdr {
    pub fn new(socket: PathBuf) -> Self {
        Self { socket, n: 0 }
    }

    pub async fn call(&mut self, m: &str, params: Value) -> Result<Value> {
        self.n += 1;
        let id = format!("theseus-up:{}:{}", std::process::id(), self.n);
        herdr::request(&self.socket, &id, m, params).await
    }
}

/// What one tab runs: this binary's watch of a session, on this daemon's socket.
pub struct TabSpec {
    pub sid: String,
    pub label: String,
    pub kind: &'static str,
    pub pinned: bool,
}

fn argv_of(exe: &Path, socket: &str, rest: &[&str]) -> Vec<String> {
    let mut v = vec![
        exe.to_string_lossy().into_owned(),
        "--socket".into(),
        socket.into(),
    ];
    v.extend(rest.iter().map(|s| (*s).to_string()));
    v
}

fn tokens_of(t: &TabSpec) -> Value {
    let mut tokens = json!({"session": t.sid, "kind": t.kind});
    if t.pinned {
        tokens["pinned"] = json!("1");
    }
    tokens
}

/// `layout.apply`'s params for tabs that each run a session's watch: no typing
/// into a shell, an argv pane per tab.
pub fn tabs_params(exe: &Path, socket: &str, tabs: &[TabSpec]) -> Value {
    let tabs: Vec<Value> = tabs
        .iter()
        .map(|t| {
            json!({"label": t.label, "focus": false, "panes": [{
                "argv": argv_of(exe, socket, &["watch", &t.sid, "--interactive"]),
                "tokens": tokens_of(t)}]})
        })
        .collect();
    json!({"workspace": {"label": WORKSPACE, "create": true}, "focus": false, "tabs": tabs})
}

/// The board tab: a pane that runs the follower itself.
pub fn board_params(exe: &Path, socket: &str) -> Value {
    json!({"workspace": {"label": WORKSPACE, "create": true}, "focus": false,
           "tabs": [{"label": "board", "focus": false, "panes": [{
               "argv": argv_of(exe, socket, &["herdr", "up", "--follow", "--here"]),
               "tokens": {"kind": BOARD_KIND}}]}]})
}

/// The tabs and panes `layout.apply` made, in the order asked for.
pub fn made_tabs(answer: &Value) -> Vec<Tab> {
    answer["tabs"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|t| Tab {
            tab_id: t["tab_id"].as_str().unwrap_or_default().to_string(),
            pane_id: t["panes"][0]["pane_id"]
                .as_str()
                .or_else(|| t["root_pane"]["pane_id"].as_str())
                .unwrap_or_default()
                .to_string(),
        })
        .collect()
}

/// A pane's tokens, set by id (herdr reads the watch's own report over them).
pub fn pane_tokens_params(pane_id: &str, t: &TabSpec) -> Value {
    json!({"pane_id": pane_id, "source": SOURCE, "agent": herdr::AGENT,
           "tokens": tokens_of(t), "seq": herdr::next_seq()})
}

/// The Space row: `needs ●2`, `working ◐5`, `done ◆1`, `spend $1.24`.
pub fn counts_params(workspace_id: &str, c: &Counts) -> Value {
    json!({"workspace_id": workspace_id, "source": SOURCE, "seq": herdr::next_seq(),
           "tokens": {"needs": format!("●{}", c.needs), "working": format!("◐{}", c.working),
                      "done": format!("◆{}", c.done),
                      "spend": format!("${}.{:02}", c.spend_cents / 100, c.spend_cents % 100)}})
}

/// Needs-you first in herdr's agent view.
pub fn view_params(workspace_id: &str) -> Value {
    json!({"workspace_id": workspace_id, "sort": "attention"})
}

/// herdr's events, one connection.
pub struct Events {
    reader: BufReader<tokio::net::UnixStream>,
    line: Vec<u8>,
}

/// One event, its name normalised (`pane.exited`, `events.lost`) and its payload.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub name: String,
    pub data: Value,
}

impl Event {
    /// The pane or tab id the event names.
    pub fn id(&self, key: &str) -> Option<&str> {
        self.data[key]
            .as_str()
            .or_else(|| self.data["data"][key].as_str())
    }
}

/// An event line, read leniently: herdr's name for the event may sit under `event`,
/// `type` or `method`, and its payload under `data` or `params`.
pub fn parse_event(line: &str) -> Option<Event> {
    let v: Value = serde_json::from_str(line.trim()).ok()?;
    if v.get("result").is_some() && v.get("event").is_none() {
        return None;
    }
    let name = ["event", "type", "method"]
        .iter()
        .find_map(|k| v[*k].as_str())?
        .replace(['_', '-'], ".");
    let data = v
        .get("data")
        .or_else(|| v.get("params"))
        .cloned()
        .unwrap_or(v);
    Some(Event { name, data })
}

impl Events {
    /// Subscribe to the three events the follower acts on.
    pub async fn connect(socket: &Path) -> Result<Self> {
        let mut stream = tokio::net::UnixStream::connect(socket)
            .await
            .with_context(|| format!("connecting to herdr at {}", socket.display()))?;
        let req = json!({"id": format!("theseus-up:{}:events", std::process::id()),
                         "method": "events.subscribe",
                         "params": {"events": ["pane.exited", "tab.focused", "pane.focused"]}});
        stream.write_all(format!("{req}\n").as_bytes()).await?;
        Ok(Self {
            reader: BufReader::new(stream),
            line: Vec::new(),
        })
    }

    /// The next event; an error when herdr closes the stream. Cancel-safe: what was
    /// read of a line waits for the next call.
    pub async fn next(&mut self) -> Result<Event> {
        loop {
            let n = self.reader.read_until(b'\n', &mut self.line).await?;
            if n == 0 && self.line.is_empty() {
                return Err(anyhow!("herdr closed its event stream"));
            }
            let bytes = std::mem::take(&mut self.line);
            if let Some(e) = parse_event(&String::from_utf8_lossy(&bytes)) {
                return Ok(e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// herdr's names are read whichever way the line carries them.
    #[test]
    fn events_are_read_leniently() {
        let e = parse_event(r#"{"event":"pane_exited","data":{"pane_id":"w1:p2"}}"#).unwrap();
        assert_eq!(
            (e.name.as_str(), e.id("pane_id")),
            ("pane.exited", Some("w1:p2"))
        );
        let e = parse_event(r#"{"type":"events_lost"}"#).unwrap();
        assert_eq!(e.name, "events.lost");
        assert!(parse_event(r#"{"id":"x","result":{"type":"ok"}}"#).is_none());
        assert!(parse_event("nonsense").is_none());
    }

    #[test]
    fn the_space_row_words_its_counts() {
        let p = counts_params(
            "w1",
            &Counts {
                needs: 2,
                working: 5,
                done: 1,
                spend_cents: 124,
            },
        );
        assert_eq!(p["tokens"]["needs"], "●2");
        assert_eq!(p["tokens"]["working"], "◐5");
        assert_eq!(p["tokens"]["done"], "◆1");
        assert_eq!(p["tokens"]["spend"], "$1.24");
    }
}
