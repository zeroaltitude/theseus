//! The sessions the self loop opens for a job other than a build (issue 36, round 2): a fixer, the
//! approver of a fix, and a reviewer. Each is a `"self"`-principal session, so without this it was
//! offered everything a builder is, `self.propose_change` among it: a fixer whose item's tree was open
//! could build the branch itself, with no fix commit, no bounds and no approval (review round 1, B1).
//!
//! Two layers, either of which holds alone:
//!
//! - **What it is offered.** `TurnRunner::view_of` gives a role's session the role's own ceiling
//!   ([`Role::ceiling`]), the mechanism a place's ceiling already is: the model is offered only those
//!   tools, and the gate refuses any other call in words that name the role.
//!   - a fixer: its tree's file and command tools (`fs`, `proc`, `git`, `job`), and the history tool
//!     (`context.history`), which reads and acts on nothing;
//!   - an approver: no tool (it is given the fix's diff; a tool would only reach beyond it);
//!   - a reviewer: the read-only file and git tools, and `proc.run` with `job.*`, which its
//!     instructions use for a checkout and a build under its own scratch.
//! - **What the builder's verb does.** `self.propose_change` refuses a role's session before
//!   anything else (`rsi::pr::propose`), whatever it was offered.
//!
//! The role is kept in memory and in the store (a META record per session), so a session whose turn
//! a restart cut short keeps its role when the daemon serves it again.
//!
//! The role also decides what a self session's view reads (`view_gate::Verified::own_place_only`):
//! the judging roles (an approver, a reviewer) their own place only, a builder and a fixer the widest
//! ([`Role::own_place_only`], read by [`crate::turn::TurnRunner::role_read`]).

use std::sync::LazyLock;

use serde::{Deserialize, Serialize};
use theseus_store::{kinds, NewRecord};

use crate::ceiling::Ceiling;

/// The META key of a session's role.
pub(crate) fn key(session: &str) -> String {
    format!("self.role.{session}")
}

/// The job a self session was opened for, when it is not a build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Fixer,
    Approver,
    Reviewer,
}

const FIXER_TOOLS: &[&str] = &[
    "fs",
    "proc",
    "git",
    "job",
    crate::view::history::HISTORY_TOOL,
];
const REVIEWER_TOOLS: &[&str] = &[
    "fs.read", "fs.list", "fs.glob", "fs.grep", "git.diff", "git.log", "proc.run", "job.wait",
    "job.read", "job.stop",
];

fn ceiling(place: &str, tools: &[&str]) -> Ceiling {
    let wire = theseus_protocol::PlaceCeiling {
        tools: Some(tools.iter().map(|t| (*t).to_string()).collect()),
        ..Default::default()
    };
    let mut c = Ceiling::new(place, &wire).unwrap_or_else(|| unreachable!("a tools list narrows"));
    c.source = "the self loop's rule for this one job";
    c
}

static FIXER: LazyLock<Ceiling> = LazyLock::new(|| ceiling("a fixer's session", FIXER_TOOLS));
static APPROVER: LazyLock<Ceiling> = LazyLock::new(|| ceiling("an approver's session", &[]));
static REVIEWER: LazyLock<Ceiling> =
    LazyLock::new(|| ceiling("a reviewer's session", REVIEWER_TOOLS));

impl Role {
    /// As a person reads it.
    pub fn words(self) -> &'static str {
        match self {
            Role::Fixer => "a fixer",
            Role::Approver => "the approver of a fix",
            Role::Reviewer => "an independent reviewer",
        }
    }

    /// Whether its session reads its own place only (`view_gate::Verified::own_place_only`): a judging role,
    /// whose independence from the builder's words and everyone else's is its job. A fixer builds, and reads
    /// at the widest, as a builder does.
    pub fn own_place_only(self) -> bool {
        matches!(self, Role::Approver | Role::Reviewer)
    }

    /// What its session is offered, as a place's ceiling.
    pub fn ceiling(self) -> &'static Ceiling {
        match self {
            Role::Fixer => &FIXER,
            Role::Approver => &APPROVER,
            Role::Reviewer => &REVIEWER,
        }
    }

    /// The record that keeps it.
    pub(crate) fn record(self, session: &str) -> anyhow::Result<NewRecord> {
        NewRecord::json(kinds::META, Some(&key(session)), &self)
    }
}

impl crate::turn::TurnRunner {
    /// `session`'s role, if the self loop opened it for one.
    pub(crate) fn role_of(&self, session: &str) -> Option<Role> {
        let board = &self.tools.self_pr;
        board.role_ceilings().then(|| board.role(session)).flatten()
    }

    /// `session`'s role as its view's scope reads it (`view_gate::scope_of`): the role the self loop gave it,
    /// or, for a fork, the role of the session it was forked from (a fork keeps its source's scope, through
    /// every fork between); none for a session opened for no role (a builder). An error when a role record or
    /// a fork's origin cannot be read, so that the scope is unread and never the widest by default.
    pub(crate) fn role_read(&self, session: &str) -> anyhow::Result<Option<Role>> {
        let mut at = session.to_string();
        for _ in 0..=crate::fork::MAX_DEPTH {
            if let Some(role) = self.tools.self_pr.role_kept(&at) {
                return Ok(Some(role));
            }
            if let Some(role) = self.store.get_meta::<Role>(&key(&at))? {
                return Ok(Some(role));
            }
            match crate::fork::origin_of(&self.store, &at)? {
                Some(o) => at = o.session,
                None => return Ok(None),
            }
        }
        anyhow::bail!("{session} is forked from a chain deeper than any a build writes")
    }
}
