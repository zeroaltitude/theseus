//! `undo.v1`'s state from a planned call (theseus-p5kln): what the gate
//! holds about the call, and the tree probed for what it would destroy.
//!
//! [`UndoCall`] is built on the gate's own path from what `gate` already has
//! — no store read, no probe, no allocation past cloning the plan's own
//! strings. [`input_of`] is the part that costs: it runs the probe, so it
//! runs in the dispatch's blocking half (`judge/undo.rs`'s `prepare_undo`),
//! never on a runtime worker.
//!
//! It is split from the probe on purpose: [`input_of`] takes a
//! [`Probe`], so a test states
//! what a tree says instead of making one, and the *mapping* — which plan
//! field becomes which state field — is readable without a repository.
//!
//! FOUR fields a plan does not carry are passed in by the gate because only
//! it knows them: `is_program` (the tool's backend), `l1` (what
//! `Sandbox::l1_for` would bind for this argv, which the pack reads as
//! losing nothing on this machine), `cwd`, and -- since
//! `p5kln-location-facts` -- [`UndoCall::held`], the targets whose whole
//! current bytes this session's own read record holds. The fourth is here for
//! the same reason as the first three: the record is the core's
//! (`toolrun::reads`), so the judge cannot ask it and the resolver must be
//! told. See [`UndoCall::held`] for why it is resolved before the probe runs
//! rather than inside it.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;
use theseus_judge::builders::{self as jb, Probe, Sandbox, UndoInput};
use theseus_tools::{Access, Plan, Tool, ToolClass};

/// One planned call, as the gate holds it at 0d.
#[derive(Debug, Clone, PartialEq)]
pub struct UndoCall {
    pub session_id: String,
    pub turn_id: String,
    /// The canonical tool name (`proc.run`).
    pub tool: String,
    /// `read`, `write` or `run`: the call's own class where it has one, else
    /// the tool's.
    pub class: String,
    /// A program call's argv, empty for a batch and for every other tool.
    pub argv: Vec<String>,
    /// A batch program call's steps, one argv each.
    pub steps: Vec<Vec<String>>,
    /// Every path the call names, with its access word.
    pub targets: Vec<(String, String)>,
    /// A network tool's address.
    pub url: Option<String>,
    /// Everything else the model wrote, as it wrote it.
    pub other_args: Option<Value>,
    /// Where a program call runs.
    pub cwd: Option<String>,
    /// Whether this call runs a program (the tool's backend is a job).
    pub is_program: bool,
    /// Whether the gate would bind L1 for it.
    pub l1: bool,
    /// The targets whose whole CURRENT bytes this session's own record holds
    /// (`p5kln-location-facts`): the paths, out of `targets`, that
    /// `toolrun::reads`'s board answers `holds` for -- read in full or
    /// written by this session, and unchanged by anyone else since.
    ///
    /// **Resolved here, with the call, and not inside the probe.** The board
    /// lives on the `ToolRuntime` and the probe lives in the judge, which
    /// knows nothing of sessions; handing the judge a reference to the board
    /// would put a core type through a crate that deliberately holds none.
    /// One `stat` per target is what it costs, which the 300 ms budget buys.
    pub held: BTreeSet<String>,
}

impl UndoCall {
    /// What the gate holds about `plan`, read with no probe and no store.
    /// `cwd` is the directory the plan's own `Exec` resource names, which is
    /// what a program call runs in.
    pub(crate) fn of(
        session_id: &str,
        turn_id: &str,
        tool: &dyn Tool,
        plan: &Plan,
        input: &Value,
        knows: Knows<'_>,
    ) -> Self {
        let Knows {
            is_program,
            l1,
            holds,
        } = knows;
        let class = plan.class.unwrap_or(tool.class());
        let cwd = plan
            .resources
            .iter()
            .find(|r| r.access == Access::Exec)
            .map(|r| r.path.display().to_string());
        let targets: Vec<(String, String)> = plan
            .resources
            .iter()
            .filter(|r| r.access != Access::Exec)
            .map(|r| (r.path.display().to_string(), access_word(r.access)))
            .collect();
        // One pass over the same paths the state will carry, so the word and
        // the path cannot diverge: the board is asked about the plan's own
        // resource path, which is what `undo_find` is given.
        let held = plan
            .resources
            .iter()
            .filter(|r| r.access != Access::Exec)
            .filter(|r| holds(&r.path))
            .map(|r| r.path.display().to_string())
            .collect();
        Self {
            session_id: session_id.into(),
            turn_id: turn_id.into(),
            tool: tool.name().into(),
            class: class.as_str().into(),
            argv: plan.argv.clone().unwrap_or_default(),
            steps: plan.steps.clone().unwrap_or_default(),
            targets,
            url: plan.url.clone(),
            other_args: super::other_args(input),
            cwd,
            is_program,
            l1,
            held,
        }
    }

    /// Whether the pack can say anything useful about this call at all: a
    /// read that names no target, no command and no address has nothing to
    /// lose, so asking would spend money to be told `reversible`. A read
    /// that names a target is still asked, because the question is about
    /// what the call CHANGES and a tool's class is not always its call's.
    pub(crate) fn worth_asking(&self) -> bool {
        if self.class != ToolClass::Read.as_str() {
            return true;
        }
        !self.targets.is_empty() || !self.argv.is_empty() || self.url.is_some()
    }
}

/// What the GATE knows about a call and a plan does not, passed in because
/// only it can know it (the module header). It is one struct rather than
/// three parameters so [`UndoCall::of`] stays inside the shape budget, and
/// so the next such fact lands here rather than widening a signature again.
pub(crate) struct Knows<'a> {
    /// Whether this call runs a program (the tool's backend is a job).
    pub is_program: bool,
    /// Whether the gate would bind L1 for it.
    pub l1: bool,
    /// Whether this session's own record holds one path's whole current
    /// bytes (`toolrun::reads`'s `holds`). A closure, so `of` stays testable
    /// without a board.
    pub holds: &'a dyn Fn(&Path) -> bool,
}

/// The access word the state shows for one resource, as `Target::access`
/// says: `read`, `write` or `exec`.
fn access_word(a: Access) -> String {
    match a {
        Access::Read => "read",
        Access::Write => "write",
        Access::Exec => "exec",
    }
    .into()
}

/// `undo.v1`'s input for one call: the call as the gate holds it, and each
/// target's recovery word as the probe found it. Runs the probe, so it is
/// the blocking half's.
pub(crate) fn input_of(call: &UndoCall, probe: &mut dyn Probe) -> UndoInput {
    let found: Vec<_> = call
        .targets
        .iter()
        // `read_this_session` is the session's own read record, resolved
        // with the call (`p5kln-location-facts`, `UndoCall::held`): the
        // board holds this path's whole current bytes, because this session
        // read them in full or wrote them and nobody has changed them since.
        // It was a hard `false` until that row, and a false reads as
        // "nothing holds a copy", which errs toward holding the call -- which
        // is also what an empty board after a restart still comes to.
        .map(|(path, access)| jb::undo_find(probe, path, access, call.held.contains(path)))
        .collect();
    let (root, dirty) = match &call.cwd {
        Some(d) => probe.repo(std::path::Path::new(d)),
        None => (None, None),
    };
    UndoInput {
        tool: call.tool.clone(),
        class: call.class.clone(),
        // 0d runs before the order, so no layer has decided a posture yet;
        // the field is context only and the builder leaves an empty one out.
        posture: String::new(),
        argv: call.argv.clone(),
        steps: call.steps.clone(),
        targets: jb::targets_of(&found),
        url: call.url.clone(),
        other_args: call.other_args.clone(),
        cwd: call.cwd.clone(),
        cwd_kind: call
            .cwd
            .is_some()
            .then(|| jb::cwd_kind_of(root.as_deref(), dirty)),
        sandbox: match jb::sandbox_of(call.is_program, call.l1) {
            // Not a program: the field says nothing rather than saying it is
            // not a program, which the builder would render as a fact.
            Sandbox::NotAProgram => None,
            s => Some(s),
        },
        // An API call the gate's path cannot make (`p5kln-remote-keeps`):
        // unknown, which the pack reads as keeping nothing.
        remote_keeps: None,
        terminal_tail: None,
    }
}
