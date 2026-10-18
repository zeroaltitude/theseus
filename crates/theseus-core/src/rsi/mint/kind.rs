//! The rule for choosing a tool's kind (RSI loop 1, §1.3), in order:
//!
//! 1. **Flow**: every step of the motif is a call to a tool a flow may name
//!    (`flow::spec::ALLOWED_TOOLS`). A flow adds no capability: each step is
//!    gated as the model's own call, so a `git push` in one is still asked
//!    about, and the flow itself asks for neither network nor a secret.
//! 2. **An owner card**, never a mint: any other tool the motif needs that
//!    reaches the network or holds a secret. A minted tool of the other kinds
//!    never asks for either on its own (as `graduate.ext.net` is filed today).
//! 3. **Rust**: the tool needs the daemon's internals, is hot, needs a
//!    primitive no tool has, is a graduation, or writes (an MCP extension runs
//!    in L1 with a discarded overlay and cannot).
//! 4. **MCP**: computation the existing tools cannot do in one call, reading
//!    only, with no network and no secret.
//!
//! `[self.mint] kinds` limits the choice: a kind the owner did not allow is
//! not minted, and the answer says which one it would have been.

use crate::flow::spec::ALLOWED_TOOLS;
use crate::rsi::mine::{Motif, SlotType};

/// A kind of minted tool, cheapest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Flow,
    Mcp,
    Rust,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Flow => "flow",
            Kind::Mcp => "mcp",
            Kind::Rust => "rust",
        }
    }
}

/// What a motif's tool needs beyond the existing tools, as far as the
/// skeleton shows it. [`Needs::of`] reads it from the steps; a test, or the
/// writer's own declaration at the next step, sets the rest.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Needs {
    /// A tool outside the allow-list is named.
    pub other_tools: bool,
    /// The tool itself would reach the network.
    pub network: bool,
    /// The tool itself would hold a secret.
    pub secret: bool,
    /// Computation the existing tools cannot do in one call.
    pub computation: bool,
    /// It writes (a file, the repository, Theseus's own state).
    pub writes: bool,
    /// The daemon's internals, a hot path, or a primitive no tool has.
    pub internals: bool,
    /// An extension or flow that graduated.
    pub graduation: bool,
}

/// Tools that reach the network or a credential, by canonical-name prefix.
const NETWORK: &[&str] = &["http.", "web.", "aws."];
const SECRET: &[&str] = &["aws.", "cred.", "secret."];
/// Tools that change Theseus or the world.
const WRITES: &[&str] = &["fs.write", "fs.edit", "fs.patch", "task.", "memory."];

impl Needs {
    /// What the skeleton's steps show. A tool of the allow-list needs nothing.
    pub fn of(m: &Motif) -> Needs {
        let mut n = Needs::default();
        for s in &m.steps {
            let t = s.shape.tool.as_str();
            if ALLOWED_TOOLS.contains(&t) {
                continue;
            }
            n.other_tools = true;
            n.network |= NETWORK.iter().any(|p| t.starts_with(p))
                || s.shape.slots.iter().any(|x| x.ty == SlotType::UrlHost);
            n.secret |= SECRET.iter().any(|p| t.starts_with(p));
            n.writes |= WRITES.iter().any(|p| t.starts_with(p));
        }
        n
    }
}

/// What the rule decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// Mint a tool of this kind.
    Mint(Kind),
    /// The tool would need the network or a secret: an owner's card, never
    /// a mint. `why` names which.
    OwnerCard { why: &'static str },
    /// The rule chose a kind `[self.mint] kinds` does not allow.
    NotAllowed(Kind),
}

/// The rule, then the owner's list.
pub fn choose(needs: &Needs, allowed: &[String]) -> Choice {
    let kind = if !needs.other_tools && !needs.graduation && !needs.internals && !needs.computation
    {
        Kind::Flow
    } else if needs.network || needs.secret {
        let why = match (needs.network, needs.secret) {
            (true, true) => "the tool would need the network and a secret",
            (true, false) => "the tool would need the network",
            _ => "the tool would need a secret",
        };
        return Choice::OwnerCard { why };
    } else if needs.graduation || needs.internals || needs.writes || !needs.computation {
        Kind::Rust
    } else {
        Kind::Mcp
    };
    if allowed.iter().any(|k| k == kind.name()) {
        Choice::Mint(kind)
    } else {
        Choice::NotAllowed(kind)
    }
}
