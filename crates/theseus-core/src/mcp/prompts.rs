//! The board's prompts (M7 §2.1, step 36c): each server's `prompts/list`,
//! kept with a digest of every definition, and run as a turn's input.
//!
//! - **The stored list.** Each server's last good `prompts/list` is a META
//!   record of its own, `mcp.prompts.<server>`, written when it changes, so a
//!   start lists prompts at once, as 36b's tools are (no format bump: a new
//!   key). `notifications/prompts/list_changed` lists again.
//! - **Running one.** [`McpBoard::resolve_prompt`] checks the arguments
//!   against the definition (a missing required one is the operator's
//!   mistake, said before anything is sent), waits for that server alone as a
//!   tool call does, calls `prompts/get`, and turns its messages into
//!   [`PromptInput`]: text stays text, an image an attachment, an embedded
//!   resource text with its URI. Nothing is written here: a failure is an
//!   error before any node exists.
//! - **A changed definition.** The definition each prompt had when it was
//!   last used is kept (`mcp.prompt_used.<server>`); a use whose definition
//!   differs is `mcp.prompt_changed`, ledgered and narrated, with an
//!   operator notice, and the use goes ahead.
//! - **Outside text.** The messages are the server's words: a server with
//!   `external = true` (the default) gives the session T1's hold, in the
//!   frame of the input's own nodes (`PromptInput::hold`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use theseus_mcp::types::{ContentView, Prompt};
use theseus_protocol::mcp::{McpPromptArgument, McpPromptInfo, McpPromptRef};

use super::{Live, McpBoard, Server, State};
use crate::fact::mcp::McpPromptChanged;

/// The META key of a server's stored prompt list.
pub const STORED_PREFIX: &str = "mcp.prompts.";
/// The META key of each prompt's definition as it was last used.
pub const USED_PREFIX: &str = "mcp.prompt_used.";
/// The most prompt text one run brings (all its messages), in characters:
/// the server's words go into the turn's context whole.
pub const TEXT_MAX: usize = 200_000;

/// What the store keeps of a server's prompts.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StoredPrompts {
    pub digest: String,
    pub prompts: Vec<Prompt>,
}

/// A prompt's definition as it was when last used.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Used {
    pub digest: String,
    pub prompt: Prompt,
}

/// What a server's prompts are now, held in its `Live`.
#[derive(Debug, Clone, Default)]
pub struct PromptState {
    pub list: Vec<Prompt>,
    pub digest: String,
    /// Whether `list` is the stored one.
    pub stored: bool,
    pub used: BTreeMap<String, Used>,
}

/// The digest of one definition: its name, title, description, and
/// arguments.
pub fn prompt_digest(p: &Prompt) -> String {
    let args: Vec<Value> = p
        .arguments
        .iter()
        .map(|a| serde_json::json!([a.name, a.description, a.required]))
        .collect();
    let one = serde_json::json!([p.name, p.title, p.description, args]);
    let mut h = Sha256::new();
    h.update(one.to_string().as_bytes());
    hex::encode(&h.finalize()[..8])
}

/// The digest of a list: each definition's, in the server's order.
pub fn list_digest(list: &[Prompt]) -> String {
    let mut h = Sha256::new();
    for p in list {
        h.update(prompt_digest(p).as_bytes());
        h.update(b"\n");
    }
    hex::encode(&h.finalize()[..8])
}

/// What changed between a definition and the one now: "description changed;
/// arguments: added x, removed y, changed z".
pub fn summary(before: &Prompt, after: &Prompt) -> String {
    let mut parts = Vec::new();
    if before.title != after.title {
        parts.push("title changed".to_string());
    }
    if before.description != after.description {
        parts.push("description changed".to_string());
    }
    let old: BTreeMap<&str, &theseus_mcp::types::PromptArgument> = before
        .arguments
        .iter()
        .map(|a| (a.name.as_str(), a))
        .collect();
    let new: BTreeMap<&str, &theseus_mcp::types::PromptArgument> = after
        .arguments
        .iter()
        .map(|a| (a.name.as_str(), a))
        .collect();
    let mut args = Vec::new();
    let names = |f: &dyn Fn(&str) -> bool, m: &BTreeMap<&str, _>| -> Vec<String> {
        m.keys().filter(|k| f(k)).map(|k| k.to_string()).collect()
    };
    let added = names(&|k| !old.contains_key(k), &new);
    let removed = names(&|k| !new.contains_key(k), &old);
    let changed: Vec<String> = new
        .iter()
        .filter(|(k, a)| {
            old.get(*k)
                .is_some_and(|o| o.required != a.required || o.description != a.description)
        })
        .map(|(k, _)| k.to_string())
        .collect();
    for (w, v) in [("added", added), ("removed", removed), ("changed", changed)] {
        if !v.is_empty() {
            args.push(format!("{w} {}", v.join(", ")));
        }
    }
    if !args.is_empty() {
        parts.push(format!("arguments: {}", args.join("; ")));
    }
    if parts.is_empty() {
        "its definition changed".into()
    } else {
        parts.join("; ")
    }
}

/// The wire's view of one prompt.
pub fn info(server: &str, p: &Prompt, stored: bool) -> McpPromptInfo {
    McpPromptInfo {
        server: server.into(),
        prompt: p.name.clone(),
        name: format!("{server}/{}", p.name),
        title: p.title.clone(),
        description: p.description.clone(),
        arguments: p
            .arguments
            .iter()
            .map(|a| McpPromptArgument {
                name: a.name.clone(),
                description: a.description.clone(),
                required: a.required,
            })
            .collect(),
        digest: prompt_digest(p),
        stored,
    }
}

/// One message of a prompt, ready to be a node: its text, and its images.
#[derive(Debug, Clone, PartialEq)]
pub struct PromptMessage {
    pub text: String,
    pub files: Vec<theseus_protocol::Attachment>,
}

/// A prompt as a turn's input: its messages, who wrote them, and whether the
/// server's words hold the session.
#[derive(Debug, Clone, PartialEq)]
pub struct PromptInput {
    /// `prompt:<server>/<name>`.
    pub author: String,
    pub messages: Vec<PromptMessage>,
    /// Outside text (`external = true`): the hold's source, `mcp:<server>/<name>`.
    pub hold: Option<String>,
}

impl PromptInput {
    /// The messages as one text, for the turn's announcement and title.
    pub fn text(&self) -> String {
        self.messages
            .iter()
            .map(|m| m.text.as_str())
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

/// Why a prompt did not run: before any node is written.
#[derive(Debug, Clone, PartialEq)]
pub enum Refusal {
    /// The operator's mistake: no such server or prompt, a missing or an
    /// unknown argument.
    Invalid(String),
    /// The server could not be asked, or did not answer.
    Unavailable(String),
}

impl Refusal {
    pub fn message(&self) -> &str {
        match self {
            Refusal::Invalid(m) | Refusal::Unavailable(m) => m,
        }
    }
}

/// A prompt message's content as the turn's text and attachments.
fn convert(
    role: &str,
    c: &theseus_mcp::types::Content,
) -> (String, Vec<theseus_protocol::Attachment>) {
    let mut files = Vec::new();
    let text = match c.view() {
        ContentView::Image { data, mime_type } => {
            files.push(theseus_protocol::Attachment {
                name: format!(
                    "prompt-image.{}",
                    mime_type.rsplit('/').next().unwrap_or("png")
                ),
                media_type: mime_type.into(),
                size: 0,
                data: Some(data.into()),
                ..Default::default()
            });
            String::new()
        }
        _ => c.as_model_text(),
    };
    // A prompt's assistant message is the server's, read as the user's: it
    // is said so rather than passed off as the model's own words.
    let text = match (role, text.is_empty()) {
        ("user", _) | (_, true) => text,
        (r, false) => format!("[the prompt's {r} message]\n{text}"),
    };
    (text, files)
}

impl McpBoard {
    /// Seed each server's prompts from the store: one META key each, before
    /// anything starts.
    pub fn seed_prompts(
        &self,
        stored: impl Fn(&str) -> Option<StoredPrompts>,
        used: impl Fn(&str) -> BTreeMap<String, Used>,
    ) {
        for s in self.servers.values() {
            let mut l = s.live();
            if let Some(p) = stored(&s.name) {
                l.prompts = p.prompts.len() as u64;
                l.prompt_state.digest = p.digest;
                l.prompt_state.list = p.prompts;
                l.prompt_state.stored = true;
            }
            l.prompt_state.used = used(&s.name);
        }
    }

    /// A list the server gave: kept in the server, and stored when it
    /// differs from what the store has.
    pub(super) fn apply_prompts(&self, s: &Server, list: Vec<Prompt>) {
        let digest = list_digest(&list);
        let old = s.live().prompt_state.digest.clone();
        if digest != old {
            self.put_meta(
                format!("{STORED_PREFIX}{}", s.name),
                StoredPrompts {
                    digest: digest.clone(),
                    prompts: list.clone(),
                },
            );
        }
        s.set(|l: &mut Live| {
            l.prompts = list.len() as u64;
            l.prompt_state.list = list;
            l.prompt_state.digest = digest;
            l.prompt_state.stored = false;
        });
    }

    /// The server's prompts again, after `list_changed`.
    pub(super) async fn relist_prompts(&self, s: &Server, client: &theseus_mcp::Client) {
        if client.server_info().capabilities.prompts.is_none() {
            return;
        }
        match client.list_prompts().await {
            Ok(list) => self.apply_prompts(s, list),
            Err(e) => tracing::warn!(server = %s.name, error = %e,
                "an MCP server's changed prompt list could not be read"),
        }
    }

    /// `mcp.prompt.list`: one server's prompts, or every server's.
    pub fn prompt_infos(&self, server: Option<&str>) -> Vec<McpPromptInfo> {
        self.servers
            .values()
            .filter(|s| server.is_none_or(|n| n == s.name))
            .filter(|s| s.cfg.enabled)
            .flat_map(|s| {
                let l = s.live();
                l.prompt_state
                    .list
                    .iter()
                    .map(|p| info(&s.name, p, l.prompt_state.stored))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// Run a prompt: check it, ask its server, and give its messages as the
    /// turn's input. Nothing is written.
    pub async fn resolve_prompt(&self, r: &McpPromptRef) -> Result<PromptInput, Refusal> {
        let Some(s) = self.servers.get(&r.server) else {
            let known: Vec<&str> = self.servers.keys().map(String::as_str).collect();
            return Err(Refusal::Invalid(format!(
                "no MCP server {:?} is configured (the servers: {})",
                r.server,
                if known.is_empty() {
                    "none".into()
                } else {
                    known.join(", ")
                }
            )));
        };
        if s.state() == State::Disabled {
            return Err(Refusal::Invalid(format!(
                "MCP server {} is disabled",
                s.name
            )));
        }
        // The definition: the live list, or the stored one until the server
        // lists. A server that lists none yet is asked all the same.
        let (def, listed) = {
            let l = s.live();
            (
                l.prompt_state
                    .list
                    .iter()
                    .find(|p| p.name == r.name)
                    .cloned(),
                !l.prompt_state.list.is_empty() || !l.prompt_state.digest.is_empty(),
            )
        };
        if def.is_none() && listed {
            let names: Vec<String> = s
                .live()
                .prompt_state
                .list
                .iter()
                .map(|p| p.name.clone())
                .collect();
            return Err(Refusal::Invalid(format!(
                "MCP server {} lists no prompt {:?} (its prompts: {})",
                s.name,
                r.name,
                if names.is_empty() {
                    "none".into()
                } else {
                    names.join(", ")
                }
            )));
        }
        if let Some(def) = &def {
            check_arguments(&s.name, def, r)?;
        }
        let wait = std::time::Duration::from_secs(s.cfg.start_timeout_secs);
        let client = s.client(wait).await.map_err(|why| {
            Refusal::Unavailable(format!("mcp_unavailable: {why}. Nothing was sent."))
        })?;
        // The definition may have been listed again while this waited.
        let def = s
            .live()
            .prompt_state
            .list
            .iter()
            .find(|p| p.name == r.name)
            .cloned()
            .or(def);
        s.calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let got = match client.get_prompt(&r.name, &r.arguments).await {
            Ok(g) => g,
            Err(e) => {
                s.errors.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                return Err(Refusal::Unavailable(format!(
                    "mcp:{}/{} prompts/get: {e}",
                    s.name, r.name
                )));
            }
        };
        let messages = messages_of(&got);
        if messages.is_empty() {
            return Err(Refusal::Invalid(format!(
                "mcp:{}/{} gave no messages",
                s.name, r.name
            )));
        }
        if let Some(def) = def {
            self.note_use(s, &def);
        }
        Ok(PromptInput {
            author: format!("prompt:{}/{}", s.name, r.name),
            messages,
            hold: s.cfg.external.then(|| format!("mcp:{}/{}", s.name, r.name)),
        })
    }

    /// A use of `def`: when its definition differs from the one at its last
    /// use, a row and a notice, once; then it is the one on record.
    fn note_use(&self, s: &Server, def: &Prompt) {
        let digest = prompt_digest(def);
        let (before, used) = {
            let mut l = s.live();
            let before = l.prompt_state.used.get(&def.name).cloned();
            if before.as_ref().is_some_and(|b| b.digest == digest) {
                return;
            }
            l.prompt_state.used.insert(
                def.name.clone(),
                Used {
                    digest: digest.clone(),
                    prompt: def.clone(),
                },
            );
            (before, l.prompt_state.used.clone())
        };
        self.put_meta(format!("{USED_PREFIX}{}", s.name), used);
        let Some(before) = before else { return };
        let fact = McpPromptChanged {
            server: s.name.clone(),
            prompt: def.name.clone(),
            before: before.digest,
            after: digest,
            summary: summary(&before.prompt, def),
        };
        tracing::warn!(server = %s.name, prompt = %def.name, summary = %fact.summary,
            "an MCP prompt's definition changed since its last use; the use goes ahead");
        self.notice(
            serde_json::json!({"kind": "mcp_prompt_changed", "server": s.name,
            "prompt": def.name, "summary": fact.summary}),
        );
        self.record(fact);
    }

    /// A META record written off the runtime's workers.
    fn put_meta<T: Serialize + Send + 'static>(&self, key: String, value: T) {
        let Some(core) = self.core.get().and_then(std::sync::Weak::upgrade) else {
            return;
        };
        let write = move || {
            if let Err(e) = core.store.put_meta(&key, &value) {
                tracing::warn!(error = %e, key, "an MCP prompt record was not stored");
            }
        };
        match tokio::runtime::Handle::try_current() {
            Ok(rt) => {
                rt.spawn_blocking(write);
            }
            Err(_) => write(),
        }
    }
}

/// A prompt's messages as nodes' text and attachments: each cut to what is
/// left of [`TEXT_MAX`], and one with nothing in it left out.
fn messages_of(got: &theseus_mcp::types::GetPromptResult) -> Vec<PromptMessage> {
    let mut messages = Vec::new();
    let mut room = TEXT_MAX;
    for m in &got.messages {
        let (mut text, files) = convert(&m.role, &m.content);
        if let Some((i, _)) = text.char_indices().nth(room) {
            text.truncate(i);
            text.push_str(" …[cut]");
        }
        room = room.saturating_sub(text.chars().count());
        if text.is_empty() && files.is_empty() {
            continue;
        }
        messages.push(PromptMessage { text, files });
    }
    messages
}

/// The arguments against the definition: a required one that is missing or
/// empty, and one the prompt does not take, are the operator's to fix.
fn check_arguments(server: &str, def: &Prompt, r: &McpPromptRef) -> Result<(), Refusal> {
    let missing: Vec<&str> = def
        .arguments
        .iter()
        .filter(|a| a.required)
        .filter(|a| r.arguments.get(&a.name).is_none_or(|v| v.trim().is_empty()))
        .map(|a| a.name.as_str())
        .collect();
    if !missing.is_empty() {
        return Err(Refusal::Invalid(format!(
            "prompt {server}/{} needs {}: {}",
            def.name,
            if missing.len() == 1 {
                "its argument"
            } else {
                "its arguments"
            },
            missing.join(", ")
        )));
    }
    let unknown: Vec<&str> = r
        .arguments
        .keys()
        .filter(|k| !def.arguments.iter().any(|a| &a.name == *k))
        .map(String::as_str)
        .collect();
    if !unknown.is_empty() {
        let takes: Vec<&str> = def.arguments.iter().map(|a| a.name.as_str()).collect();
        return Err(Refusal::Invalid(format!(
            "prompt {server}/{} takes no argument {} (it takes: {})",
            def.name,
            unknown.join(", "),
            if takes.is_empty() {
                "none".into()
            } else {
                takes.join(", ")
            }
        )));
    }
    Ok(())
}

/// What a server's stored prompts read as, when they read.
pub fn read_stored(store: &crate::store::Store, server: &str) -> Option<StoredPrompts> {
    match store.get_meta::<StoredPrompts>(&format!("{STORED_PREFIX}{server}")) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, server, "an MCP server's stored prompt list did not read");
            None
        }
    }
}

/// The definitions each prompt had when last used, as the store has them.
pub fn read_used(store: &crate::store::Store, server: &str) -> BTreeMap<String, Used> {
    match store.get_meta::<BTreeMap<String, Used>>(&format!("{USED_PREFIX}{server}")) {
        Ok(v) => v.unwrap_or_default(),
        Err(e) => {
            tracing::warn!(error = %e, server, "an MCP server's used prompts did not read");
            BTreeMap::new()
        }
    }
}

#[cfg(test)]
pub(super) fn convert_for_tests(
    role: &str,
    c: &theseus_mcp::types::Content,
) -> (String, Vec<theseus_protocol::Attachment>) {
    convert(role, c)
}
