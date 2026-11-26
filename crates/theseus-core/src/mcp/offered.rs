//! What a server that signs in may offer (issue 55): a tool whose public
//! metadata holds a value Theseus keeps is not offered at all. Its name, its
//! canonical and wire names, its description, its schema, and everything
//! else it says of itself (title, annotations, `_meta`: every key and every
//! value) are read by the daemon's scrubber; if the scrubber would change
//! any of them, the tool is dropped before the list is stored, published,
//! listed or sent. It is not masked and kept: a masked name cannot be
//! called, and a masked schema is not the server's. The owner is told a
//! count and the server's name, in fixed words, once per change. A server
//! with no sign-in offers its list as it came.

use serde_json::Value;
use theseus_mcp::names;

use super::{Listed, McpBoard, Server};
use crate::fact::mcp::McpToolsWithheld;
use crate::scrub::Scrubber;

/// The fixed words for `count` tools of `server` that are not offered.
pub fn withheld_words(server: &str, count: usize) -> String {
    match count {
        1 => format!(
            "1 tool of {server} is not offered: what it says of itself holds a value Theseus \
             keeps secret"
        ),
        n => format!(
            "{n} tools of {server} are not offered: what they say of themselves holds a value \
             Theseus keeps secret"
        ),
    }
}

/// Whether anything `t` says of itself, or a name it would be offered
/// under, holds a value the scrubber knows.
pub(super) fn says_a_kept_value(scrubber: &Scrubber, server: &str, t: &Listed) -> bool {
    let whole = serde_json::to_value(t).unwrap_or(Value::Null);
    let mut texts = vec![
        names::canonical(server, &t.name),
        names::wire_name(server, &t.name),
        whole.to_string(),
    ];
    strings_of(&whole, &mut texts);
    texts.iter().any(|text| scrubber.scrub(text).1 > 0)
}

/// Every key and every scalar of `v`, as text: a value is found whether
/// the server put it in a key, a string, or a number.
fn strings_of(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => out.push(s.clone()),
        Value::Number(n) => out.push(n.to_string()),
        Value::Array(a) => a.iter().for_each(|x| strings_of(x, out)),
        Value::Object(m) => {
            for (k, x) in m {
                out.push(k.clone());
                strings_of(x, out);
            }
        }
        Value::Null | Value::Bool(_) => {}
    }
}

impl McpBoard {
    /// The tools of `tools` that `s` may offer: all of them for a server
    /// with no sign-in; for one that signs in, those that say no kept
    /// value. A change in what is dropped is said once: a count and the
    /// server's name, in the log, a fact and a notice.
    pub(super) fn offered(&self, s: &Server, tools: Vec<Listed>) -> Vec<Listed> {
        if s.cfg.oauth.is_none() {
            return tools;
        }
        let scrubber = Scrubber::from_board(self.secrets.clone());
        let (dropped, kept): (Vec<Listed>, Vec<Listed>) = tools
            .into_iter()
            .partition(|t| says_a_kept_value(&scrubber, &s.name, t));
        let now = match dropped.is_empty() {
            true => String::new(),
            false => super::digest(&dropped),
        };
        let was = std::mem::replace(&mut s.live().withheld, now.clone());
        if now != was && !dropped.is_empty() {
            let words = withheld_words(&s.name, dropped.len());
            tracing::warn!(server = %s.name, count = dropped.len(), "{words}");
            self.notice(serde_json::json!({"kind": "mcp_changed", "server": s.name,
                "summary": words}));
            self.record(McpToolsWithheld {
                server: s.name.clone(),
                count: dropped.len() as u64,
            });
        }
        kept
    }

    /// Whether the catalog may offer `t` of `s` under `wire`: the last
    /// check, where a wire name made unique is known.
    pub(super) fn may_offer(&self, s: &Server, t: &Listed, wire: &str) -> bool {
        if s.cfg.oauth.is_none() {
            return true;
        }
        let scrubber = Scrubber::from_board(self.secrets.clone());
        !says_a_kept_value(&scrubber, &s.name, t) && scrubber.scrub(wire).1 == 0
    }
}
