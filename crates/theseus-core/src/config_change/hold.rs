//! The hold on a look, the settings read and the profile tools after outside text or with the
//! `injection` flag (issue 54), as `[content_based_security] outside` says it acts.
//!
//! - **`notify`, the default** (the owner, 2026-11-25: "I want way fewer of these"): the call is
//!   not held for it. The owner is told once a span, in an FYI note to him alone ([`tell`]): which
//!   conversation by its short id (never its title), where the outside text came from as a safe
//!   summary ([`source_words`]: a URL's scheme and host, else a fixed kind word; never the marker's
//!   own text or what was read), what kind of call it was in fixed words ([`call_words`]), that
//!   outside text added no hold to it, and the setting that makes Theseus ask. A span
//!   ends where the hold would end: the next user-role message, or a pressed "Approve and clear",
//!   for outside text; the flag's removal for the flag. Which spans were told is kept in memory,
//!   so after a restart a span is told once more.
//! - **`ask`**: each tool's own hold asks first (`show`, `look`, `profile_tool`), in part one's
//!   words, and the card offers "Approve and clear" (`policy::grants::hold_offer`). Its press
//!   removes the flag as `/trust` does (`content.unlabel`, judged as any removal) and marks the
//!   approved call's result as a boundary ([`CLEARED`]): outside text is counted from there on, as
//!   if the owner had just spoken.
//!
//! `vault.use` and `vault.keep` are held and told as the seven are (the owner, 2026-11-25:
//! "Notify, please"), but their card never offers "Approve and clear" ([`CLEARS`]): at `ask` each
//! keeps part one's two answers.

use std::collections::{BTreeSet, HashMap};
use std::sync::Mutex;

use serde_json::{json, Value};
use theseus_protocol::Plan;

use crate::content::{Flag, OutsideMode};
use crate::node::{Body, Node};
use crate::toolrun::{ToolRuntime, TurnCtx};

/// The tools whose held card offers "Approve and clear": the settings read, the three looks and
/// the three profile tools.
pub const CLEARS: [&str; 7] = [
    super::show::SHOW,
    crate::look::NAMES[0],
    crate::look::NAMES[1],
    crate::look::NAMES[2],
    crate::profile_tool::NAMES[0],
    crate::profile_tool::NAMES[1],
    crate::profile_tool::NAMES[2],
];

/// The tools whose hold `outside` decides: [`CLEARS`] and the vault's two.
pub const QUIET: [&str; 9] = [
    CLEARS[0],
    CLEARS[1],
    CLEARS[2],
    CLEARS[3],
    CLEARS[4],
    CLEARS[5],
    CLEARS[6],
    super::vault_use::USE,
    super::vault_keep::KEEP,
];

/// The key a result's `meta` carries when the owner's "Approve and clear" ran its call: outside
/// text is counted from that result on.
pub const CLEARED: &str = "hold_cleared";

/// How many sources the note names.
const SOURCES: usize = 3;

/// What the runtime keeps for the hold: the calls whose press cleared it, until their result is
/// written, and the span each conversation was last told of.
#[derive(Default)]
pub struct Holds {
    cleared: Mutex<BTreeSet<String>>,
    told: Mutex<HashMap<String, String>>,
}

impl Holds {
    /// The call `correlation_id` was approved with the hold cleared.
    pub fn clear_on(&self, correlation_id: &str) {
        self.cleared
            .lock()
            .unwrap()
            .insert(correlation_id.to_string());
    }

    /// A clear taken back: its answer was refused.
    pub fn unclear(&self, correlation_id: &str) {
        self.cleared.lock().unwrap().remove(correlation_id);
    }

    /// Whether the call's press cleared the hold, once: its result is the boundary.
    pub fn take_cleared(&self, correlation_id: &str) -> bool {
        self.cleared.lock().unwrap().remove(correlation_id)
    }

    /// Whether the conversation has not been told of the span `key` yet; it is, from now.
    fn first_in(&self, session: &str, key: &str) -> bool {
        let mut told = self.told.lock().unwrap();
        if told.get(session).is_some_and(|k| k == key) {
            return false;
        }
        told.insert(session.to_string(), key.to_string());
        true
    }
}

/// Mark a result `meta` as the boundary the hold counts from.
pub fn mark_cleared(meta: &mut Value) {
    if meta.is_null() {
        *meta = json!({});
    }
    if let Some(m) = meta.as_object_mut() {
        m.insert(CLEARED.into(), Value::Bool(true));
    }
}

/// Where the hold counts from in `nodes`: after the last user-role message, or at the last result
/// an "Approve and clear" ran (its own outside text still counts). With the node that starts the
/// span, for the note's once-a-span.
pub(crate) fn span(nodes: &[(u64, Node)]) -> (usize, Option<&str>) {
    let at = nodes.iter().rposition(|(_, n)| match &n.body {
        Body::UserMessage { .. } => true,
        Body::ToolResult { meta, .. } => meta[CLEARED].as_bool() == Some(true),
        _ => false,
    });
    match at {
        Some(i) if matches!(nodes[i].1.body, Body::UserMessage { .. }) => {
            (i + 1, Some(nodes[i].1.id.as_str()))
        }
        Some(i) => (i, Some(nodes[i].1.id.as_str())),
        None => (0, None),
    }
}

/// The hold on the nine tools at `[content_based_security] outside`: at `ask`, each tool's own;
/// at `notify`, none, and the owner is told ([`tell`]), unless `injection = "refuse"` refuses the
/// call of a flagged session after this, as it refuses every one of the nine there. Any other
/// tool's plan is returned as it came.
pub(crate) fn ask_or_tell(
    rt: &ToolRuntime,
    tc: &TurnCtx<'_>,
    name: &str,
    plan: Plan,
) -> Plan {
    if !QUIET.contains(&name) {
        return plan;
    }
    match rt.outside_mode() {
        OutsideMode::Ask => {
            let plan = super::show::ask_if_outside(tc, name, plan);
            let plan = crate::look::ask_if_outside(tc, name, plan);
            let plan = crate::profile_tool::ask_if_outside(tc, name, plan);
            super::vault_use::ask_if_outside(tc, name, plan)
        }
        OutsideMode::Notify => {
            tell(&rt.holds, tc, rt.frozen_refuses(), name);
            plan
        }
    }
}

/// What the note is about: the flag, with where it came from; or outside text, with its sources.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Cause {
    Flag(String),
    Outside(Vec<String>),
}

/// A marker as the note names it: a safe summary, never its own text. A URL anywhere in it is
/// named by its scheme and host alone (no userinfo, port, path, query or fragment); anything else
/// by a fixed word for its kind.
pub(crate) fn source_words(marker: &str) -> String {
    for word in marker.split_whitespace() {
        if let Some(host) = url_host(word) {
            return host;
        }
    }
    let kind = marker.split([':', ' ']).next().unwrap_or("");
    match kind {
        "gmail" => "a Gmail thread",
        "drive" => "a Drive file",
        "calendar" => "a calendar event",
        "task" => "a task's work",
        "history" => "another conversation's history",
        "attachment" => "an attachment",
        "mcp" => "a tool server",
        _ => "a source outside this conversation",
    }
    .to_string()
}

/// `scheme://host` of a URL, when `word` is one: lower-case letters before `://`, and a host of
/// letters, digits, `.` and `-`.
fn url_host(word: &str) -> Option<String> {
    let (scheme, rest) = word.split_once("://")?;
    if scheme.is_empty() || !scheme.chars().all(|c| c.is_ascii_lowercase() || c == '+') {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit('@').next().unwrap_or("");
    let host = host.split(':').next().unwrap_or("");
    let ok = !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    ok.then(|| format!("{scheme}://{}", host.to_ascii_lowercase()))
}

/// What kind of call it was, in fixed words: nothing of its input.
pub(crate) fn call_words(name: &str) -> &'static str {
    match name {
        n if n == super::show::SHOW => "a read of Theseus's settings",
        n if n == crate::look::NAMES[0] => "a look at what Theseus runs as",
        n if n == crate::look::NAMES[1] => "a look at the vault's names",
        n if n == crate::look::NAMES[2] => "a look at the work board",
        n if n == crate::profile_tool::NAMES[0] => "a question to another profile",
        n if n == crate::profile_tool::NAMES[1] => "a change of this conversation's profile",
        n if n == crate::profile_tool::NAMES[2] => "work started on another profile",
        n if n == super::vault_use::USE => "a use of a secret from the vault",
        n if n == super::vault_keep::KEEP => "a request to keep a secret in the vault",
        _ => "a call",
    }
}

/// The note: FYI-headed, the conversation by its short id, the cause and its source, the kind of
/// call, that outside text added no hold to it, and the setting that asks. True when it is
/// written, before the call's other rules decide whether it runs.
pub(crate) fn note_text(session_id: &str, cause: &Cause, what: &str) -> String {
    let here = format!("In conversation {}", crate::narrative::short(session_id));
    let (trigger, until) = match cause {
        Cause::Flag(source) => (
            format!(
                "{here}, Theseus's check flagged text this conversation read as steering it (from \
                 {}).",
                source_words(source)
            ),
            "until the flag is cleared",
        ),
        Cause::Outside(sources) => {
            let mut named: Vec<String> = sources.iter().take(SOURCES).cloned().collect();
            if sources.len() > SOURCES {
                named.push(format!("{} more", sources.len() - SOURCES));
            }
            let from = match named.is_empty() {
                true => "a source it could not name".to_string(),
                false => named.join(", "),
            };
            (
                format!(
                    "{here}, Theseus read text from outside since your last message (from {from})."
                ),
                "until your next message",
            )
        }
    };
    crate::owner_note::note(
        crate::owner_note::Kind::Fyi,
        "Outside text did not hold a call",
        &trigger,
        &format!(
            "It added no hold to {what}; the call's other rules still apply. Later calls of this \
             kind in this conversation are not told again {until}.\nTo be asked first instead: \
             `[content_based_security] outside = \"ask\"`."
        ),
    )
}

/// Tell the owner, once a span, that a call the hold would have asked about went on: one FYI note
/// to him alone, never a node of the session. A note that cannot be staged is logged, and the
/// call goes on as it would.
pub(crate) fn tell(holds: &Holds, tc: &TurnCtx<'_>, freeze: bool, name: &str) {
    // A call the tools' wall refuses (a task, a job, a shared place) does not go on: nothing to tell.
    if super::wall::refusal(tc).is_some() {
        return;
    }
    let rec = tc
        .store
        .get_session::<crate::session::SessionRecord>(tc.session_id)
        .ok()
        .flatten();
    let flag = rec
        .as_ref()
        .and_then(|s| s.data.as_ref())
        .and_then(|d| d.label(Flag::Injection))
        .cloned();
    let (key, cause) = match flag {
        // The freeze refuses the call: it does not go on, so nothing is told.
        Some(_) if freeze => return,
        Some(l) => (format!("flag:{}", l.at_ms), Cause::Flag(l.source)),
        None => match tc.store.session_nodes(tc.session_id) {
            Ok(nodes) => {
                let (from, anchor) = span(&nodes);
                let sources: Vec<String> = nodes[from..]
                    .iter()
                    .filter_map(|(_, n)| match &n.body {
                        Body::ToolResult {
                            external: Some(e), ..
                        } => Some(source_words(&e.url)),
                        _ => None,
                    })
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();
                if sources.is_empty() {
                    return;
                }
                (
                    format!("outside:{}", anchor.unwrap_or("")),
                    Cause::Outside(sources),
                )
            }
            Err(_) => ("unread".to_string(), Cause::Outside(Vec::new())),
        },
    };
    if !holds.first_in(tc.session_id, &key) {
        return;
    }
    tell_owner(tc, &note_text(tc.session_id, &cause, call_words(name)), name);
}

/// One note to the owner alone, riding the turn's next frame; never a node of the session. A note
/// that cannot be staged is logged by the tool's name, and the call goes on.
pub(crate) fn tell_owner(tc: &TurnCtx<'_>, text: &str, tool: &str) {
    match tc
        .outbox
        .stage_to_operator(Some(tc.session_id), json!({"kind": "notice", "text": text}))
    {
        Ok((post, records)) => {
            for r in records {
                if let Err(e) = tc.store.defer(r) {
                    tracing::warn!(error = %format!("{e:#}"), tool, "a note's record was not kept");
                }
            }
            tc.posts.lock().unwrap().push(post);
        }
        Err(e) => {
            tracing::warn!(error = %format!("{e:#}"), tool, "the owner's note was not staged");
        }
    }
}
