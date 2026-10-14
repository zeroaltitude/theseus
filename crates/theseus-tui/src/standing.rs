//! The owner's standing records where the TUI shows them (theseus-slvl.4): a
//! pill in the header, `📌 3`, counting the live records that hold in the
//! focused session on this surface (its session's, its place's, everywhere's,
//! and the ones scoped to the terminal), and `p`, which lists them. Read from
//! `standing.list` when a session is focused, after a turn of this TUI's, and
//! when the list opens: nothing polls.

use serde_json::{json, Value};
use theseus_protocol::{StandingInfo, StandingListResult};

/// The surface this client speaks through, as a record's scope names it.
pub const SURFACE: &str = "terminal";

/// The live records of one session's scope, as the daemon last said.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Standing {
    pub session: String,
    pub records: Vec<StandingInfo>,
}

/// `standing.list`'s params for `session` on this surface.
pub fn params(session: &str) -> Value {
    json!({ "session_id": session, "surface": SURFACE })
}

impl Standing {
    pub fn read(session: &str, v: Value) -> Option<Self> {
        let r: StandingListResult = serde_json::from_value(v).ok()?;
        Some(Self {
            session: session.to_string(),
            records: r.records,
        })
    }

    /// `📌 3`, or none while no record holds.
    pub fn pill(&self) -> Option<String> {
        (!self.records.is_empty()).then(|| format!("📌 {}", self.records.len()))
    }

    /// The list `p` opens: a title, a line a record (id, kind, scope, text),
    /// and where to change them.
    pub fn lines(&self) -> Vec<String> {
        let mut out = vec![format!(" standing here ({})", self.records.len())];
        if self.records.is_empty() {
            out.push(" no records hold in this session".to_string());
        }
        for i in &self.records {
            let r = &i.record;
            out.push(format!(
                " {}  {:<10} {}  {}",
                r.id,
                r.kind.name(),
                r.scope.words(),
                r.text
            ));
        }
        out.push(String::new());
        out.push(" theseus standing add|revoke changes them; any key closes this".to_string());
        out
    }
}
