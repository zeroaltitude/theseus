//! What the follower says to herdr, and what it hears (theseus-fuw5d): the params of
//! `workspace.create`, `layout.apply` (one tab a call), `pane.report_metadata`,
//! `workspace.report_metadata`, `agent.view.set` and `events.subscribe`, the answer of
//! `layout.apply` read for its ids, and the event stream. herdr's `herdr api schema --json` is
//! the contract, and `herdr_contract_tests.rs` checks every shape built here against a
//! snapshot of it (theseus-6ifz1); each shape is built in one function so a change of the
//! schema is a change in one place.

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

/// What one tab runs: this binary's watch of a session, its chat, or the follower itself, on
/// this daemon's socket. A session's tab carries its id; the `main` and `board` tabs have none.
pub struct TabSpec {
    pub sid: String,
    pub label: String,
    pub kind: &'static str,
    pub pinned: bool,
}

impl TabSpec {
    /// The `main` tab: `theseus chat`, the home conversation.
    pub fn main() -> Self {
        Self::base("main", MAIN_KIND)
    }

    /// The `board` tab: the follower.
    pub fn board() -> Self {
        Self::base("board", BOARD_KIND)
    }

    fn base(label: &str, kind: &'static str) -> Self {
        Self {
            sid: String::new(),
            label: label.to_string(),
            kind,
            pinned: false,
        }
    }

    /// What its pane runs, as an argv: nothing is typed into a shell.
    fn argv(&self, exe: &Path, socket: &str) -> Vec<String> {
        let rest: Vec<&str> = match self.kind {
            BOARD_KIND => vec!["herdr", "up", "--follow", "--here"],
            MAIN_KIND => vec!["chat"],
            _ => vec!["watch", &self.sid, "--interactive"],
        };
        let mut v = vec![
            exe.to_string_lossy().into_owned(),
            "--socket".into(),
            socket.into(),
        ];
        v.extend(rest.iter().map(|s| (*s).to_string()));
        v
    }

    fn tokens(&self) -> Value {
        let mut tokens = json!({"kind": self.kind});
        if !self.sid.is_empty() {
            tokens["session"] = json!(self.sid);
        }
        if self.pinned {
            tokens["pinned"] = json!("1");
        }
        tokens
    }
}

/// Where `layout.apply` puts the tab: a new tab in a workspace, or in place of a tab (a new
/// workspace's blank shell, which the first tab replaces).
pub enum Where<'a> {
    Workspace(&'a str),
    Replacing(&'a str),
}

/// `workspace.create`'s params for the `theseus` workspace.
pub fn workspace_params() -> Value {
    json!({"label": WORKSPACE, "focus": false})
}

/// `layout.apply`'s params for one tab: a single pane running the spec's argv. herdr's
/// `layout.apply` makes one tab a call; a pane's tokens are set after, by `pane_tokens_params`.
pub fn tab_params(at: &Where, exe: &Path, socket: &str, t: &TabSpec, focus: bool) -> Value {
    let root = json!({"type": "pane", "command": t.argv(exe, socket)});
    let mut p = json!({"tab_label": t.label, "focus": focus, "root": root});
    match at {
        Where::Workspace(id) => p["workspace_id"] = json!(id),
        Where::Replacing(id) => p["tab_id"] = json!(id),
    }
    p
}

/// The tab `layout.apply` made: its id, and its pane (the first leaf of the layout it describes).
pub fn made_tab(answer: &Value) -> Option<Tab> {
    let l = &answer["layout"];
    let mut node = &l["root"];
    while node["type"] == "split" {
        node = &node["first"];
    }
    Some(Tab {
        tab_id: l["tab_id"].as_str()?.to_string(),
        pane_id: node["pane_id"]
            .as_str()
            .or_else(|| l["focused_pane_id"].as_str())?
            .to_string(),
    })
}

/// A pane's tokens, set by id (`layout.apply` takes none). A session's pane is also named as
/// our agent, so herdr shows its state.
pub fn pane_tokens_params(pane_id: &str, t: &TabSpec) -> Value {
    let mut p = json!({"pane_id": pane_id, "source": SOURCE,
                       "tokens": t.tokens(), "seq": herdr::next_seq()});
    if !t.sid.is_empty() {
        p["agent"] = json!(herdr::AGENT);
    }
    p
}

/// Where herdr's `theseus` workspace stands for this process: its id once known, and the blank
/// tab a workspace made here starts with, until a tab replaces it.
#[derive(Default)]
pub struct Space {
    pub workspace_id: Option<String>,
    blank_tab: Option<String>,
}

impl Herdr {
    /// The workspace's id: the one herdr has, else a new one (with its blank tab noted).
    pub async fn workspace(&mut self, space: &mut Space) -> Result<String> {
        if let Some(id) = &space.workspace_id {
            return Ok(id.clone());
        }
        let list = self.call("workspace.list", json!({})).await?;
        let found = list["workspaces"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|w| w["label"] == WORKSPACE)
            .and_then(|w| w["workspace_id"].as_str());
        let id = match found {
            Some(id) => id.to_string(),
            None => {
                let r = self.call("workspace.create", workspace_params()).await?;
                space.blank_tab = r["tab"]["tab_id"].as_str().map(str::to_string);
                r["workspace"]["workspace_id"]
                    .as_str()
                    .ok_or_else(|| anyhow!("herdr's answer has no workspace id"))?
                    .to_string()
            }
        };
        space.workspace_id = Some(id.clone());
        Ok(id)
    }

    /// Open a tab for the spec, with its tokens. A workspace or blank tab that went away since
    /// it was noted is found or made again, once.
    pub async fn open_tab(
        &mut self,
        space: &mut Space,
        (exe, socket): (&Path, &str),
        t: &TabSpec,
        focus: bool,
    ) -> Result<Tab> {
        let mut again = true;
        loop {
            let ws = self.workspace(space).await?;
            let blank = space.blank_tab.take();
            let at = blank
                .as_deref()
                .map_or(Where::Workspace(&ws), Where::Replacing);
            match self
                .call("layout.apply", tab_params(&at, exe, socket, t, focus))
                .await
            {
                Ok(answer) => {
                    let tab = made_tab(&answer)
                        .ok_or_else(|| anyhow!("herdr's layout answer names no tab"))?;
                    self.call("pane.report_metadata", pane_tokens_params(&tab.pane_id, t))
                        .await?;
                    return Ok(tab);
                }
                Err(e) if again && gone(&e) => {
                    again = false;
                    *space = Space::default();
                }
                Err(e) => {
                    space.blank_tab = blank;
                    return Err(e);
                }
            }
        }
    }
}

/// Whether herdr said the workspace or tab it was given is gone.
fn gone(e: &anyhow::Error) -> bool {
    e.downcast_ref::<herdr::HerdrError>()
        .is_some_and(|h| matches!(h.code.as_str(), "workspace_not_found" | "tab_not_found"))
}

/// The Space row: `needs ●2`, `working ◐5`, `done ◆1`, `paused ⏸2`, `spend $1.24`, and the
/// horizon's: `since ↺3` (what happened since you looked), `next ⏰16:52` (the next
/// reminder) and `routines 🔁2` (due now). The four are empty when there is nothing
/// to say, so a token herdr kept from before is cleared.
pub fn counts_params(workspace_id: &str, c: &Counts) -> Value {
    let since = if c.since > 0 {
        format!("↺{}", c.since)
    } else {
        String::new()
    };
    let next = c.next_ms.map_or_else(String::new, |ms| {
        format!("⏰{}", crate::status::local_hm(ms))
    });
    let paused = if c.paused > 0 {
        format!("⏸{}", c.paused)
    } else {
        String::new()
    };
    let routines = if c.routines_due > 0 {
        format!("🔁{}", c.routines_due)
    } else {
        String::new()
    };
    json!({"workspace_id": workspace_id, "source": SOURCE, "seq": herdr::next_seq(),
           "tokens": {"needs": format!("●{}", c.needs), "working": format!("◐{}", c.working),
                      "done": format!("◆{}", c.done),
                      "spend": format!("${}.{:02}", c.spend_cents / 100, c.spend_cents % 100),
                      "paused": paused, "since": since, "next": next, "routines": routines}})
}

/// Needs-you first in herdr's agent view (the view is the server's, not a workspace's).
pub fn view_params() -> Value {
    json!({"source": SOURCE, "sort": [{"field": "attention", "order": "desc"}]})
}

/// `events.subscribe`'s params: the three events the follower acts on.
pub fn subscribe_params() -> Value {
    json!({"subscriptions": [{"type": "pane.exited"}, {"type": "tab.focused"},
                             {"type": "pane.focused"}]})
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
                         "method": "events.subscribe", "params": subscribe_params()});
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
                paused: 2,
                spend_cents: 124,
                since: 3,
                next_ms: Some(0),
                routines_due: 2,
            },
        );
        assert_eq!(p["tokens"]["needs"], "●2");
        assert_eq!(p["tokens"]["working"], "◐5");
        assert_eq!(p["tokens"]["done"], "◆1");
        assert_eq!(p["tokens"]["paused"], "⏸2");
        assert_eq!(p["tokens"]["spend"], "$1.24");
        assert_eq!(p["tokens"]["since"], "↺3");
        assert_eq!(
            p["tokens"]["next"],
            format!("⏰{}", crate::status::local_hm(0))
        );
        assert_eq!(p["tokens"]["routines"], "🔁2");
        let quiet = counts_params("w1", &Counts::default());
        for k in ["paused", "since", "next", "routines"] {
            assert_eq!(
                quiet["tokens"][k], "",
                "{k} is cleared when there is nothing"
            );
        }
    }
}
