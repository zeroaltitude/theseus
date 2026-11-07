//! `undo.v1`'s state (theseus-p5kln): one tool call, and what code found about
//! putting back what it would change.
//!
//! The pack asks whether a call is reversible. That question cannot be answered
//! from the call alone, and this module is why. `fs.write` on the same path is
//! reversible in a clean tree and not in a dirty one; `rm -rf build/` is
//! reversible when the project rebuilds it; an object delete is reversible on a
//! versioned bucket and final on an unversioned one. In each pair the tool and
//! the arguments are identical and the answer is opposite, so the fact that
//! settles it has to be computed and stated.
//!
//! The loader forbids a question that asks Jev to look something up, and the
//! design forbids it too: Jev is never asked whether a copy exists. Code
//! resolves every target the call names and gives each one a [`Recovery`] word,
//! and the pack's instructions say what each word means. A version of this
//! classifier without these fields is a keyword matcher with a language model
//! attached.
//!
//! Three fields carry reversibility that lives outside the arguments entirely:
//! `sandbox`, because a program call under L1 writes into a scratch layer that
//! is thrown away when the run ends; `cwd_kind`, because `rm -rf *` means
//! something different in a scratch directory and in a working tree; and
//! `terminal_tail`, because `term.send` of one letter is a confirmation whose
//! whole meaning is the prompt waiting for it.
//!
//! `steps` is carried apart from `argv` on purpose. A batch `proc.run` puts its
//! commands in `plan.steps` and leaves `plan.argv` empty, so a state that read
//! only `argv` would show a batch with no command line at all and call it
//! harmless. The gate takes the strictest step's posture for a batch and the
//! pack is told to answer the same way.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{compact, left_out, share, Clipper, Prepared, UNDO_VERSION};
use crate::pack::Dynamic;
use crate::state::{Keep, Scrub, StateBuilder};

/// The argv entries, targets and batch steps the state shows at most.
pub const UNDO_ARGV: usize = 24;
pub const UNDO_TARGETS: usize = 12;
pub const UNDO_STEPS: usize = 16;

/// The field names `undo.v1`'s state can carry, in the order the builder adds
/// them. A test holds this list against the built state.
pub const UNDO_STATE_KEYS: [&str; 12] = [
    "tool",
    "class",
    "posture",
    "sandbox",
    "cwd",
    "cwd_kind",
    "argv",
    "steps",
    "targets",
    "url",
    "remote_keeps",
    "terminal_tail",
];

/// What code found about putting one target back. Serialized as the word the
/// pack's instructions name; `Unknown` is the honest answer when the resolver
/// could not tell, and the pack is told to treat it as no copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Recovery {
    /// In the repository and matching HEAD: a checkout restores it.
    TrackedClean,
    /// In the repository with uncommitted changes: the committed bytes come
    /// back, the uncommitted ones do not.
    TrackedDirty,
    /// Inside a repository and not in it: nothing holds a copy.
    Untracked,
    /// Not under any repository: nothing holds a copy.
    OutsideRepo,
    /// Not there yet, so creating it is undone by deleting it.
    Absent,
    /// This session read these bytes and its own record still holds them.
    ReadThisSession,
    /// The project's own build makes it again.
    Regenerable,
    /// The resolver could not tell.
    Unknown,
}

impl Recovery {
    pub fn as_str(self) -> &'static str {
        match self {
            Recovery::TrackedClean => "tracked_clean",
            Recovery::TrackedDirty => "tracked_dirty",
            Recovery::Untracked => "untracked",
            Recovery::OutsideRepo => "outside_repo",
            Recovery::Absent => "absent",
            Recovery::ReadThisSession => "read_this_session",
            Recovery::Regenerable => "regenerable",
            Recovery::Unknown => "unknown",
        }
    }
}

/// What a remote store keeps of what this call would replace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteKeeps {
    /// The service keeps previous versions of an object.
    Versions,
    /// The service keeps previous revisions of a document.
    Revisions,
    /// The service keeps nothing of what this replaces.
    None,
    /// Not known, or the call reaches no such service.
    Unknown,
}

/// Where a program call runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sandbox {
    /// The operator's own process, with its reach.
    L0,
    /// The L1 sandbox, whose workspace writes go to a scratch layer that is
    /// discarded when the run ends.
    L1,
    /// Not a program call.
    NotAProgram,
}

/// What kind of place a program call's working directory is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CwdKind {
    RepoClean,
    RepoDirty,
    NotARepo,
    /// A directory made for this run, outside any repository.
    Scratch,
    Unknown,
}

/// One path the call names, and what code found about putting it back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub path: String,
    /// `read`, `write` or `exec`, as the plan's resource says.
    pub access: String,
    pub recovery: Recovery,
}

/// `undo.v1`'s input: the call as the toollet planned it, and the recovery
/// facts beside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UndoInput {
    /// The canonical tool name (`proc.run`).
    pub tool: String,
    /// `read`, `write` or `run`, the call's own class where it has one.
    pub class: String,
    /// The gate's posture so far, for context only: this pack may raise it and
    /// never lowers it.
    #[serde(default)]
    pub posture: String,
    /// A program call's argv, empty for a batch and for every other tool.
    #[serde(default)]
    pub argv: Vec<String>,
    /// A batch program call's steps, one argv each.
    #[serde(default)]
    pub steps: Vec<Vec<String>>,
    /// Every path the call names, with its recovery word.
    #[serde(default)]
    pub targets: Vec<Target>,
    /// A network tool's address.
    #[serde(default)]
    pub url: Option<String>,
    /// Everything else the model wrote, as it wrote it.
    #[serde(default)]
    pub other_args: Option<Value>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub cwd_kind: Option<CwdKind>,
    #[serde(default)]
    pub sandbox: Option<Sandbox>,
    #[serde(default)]
    pub remote_keeps: Option<RemoteKeeps>,
    /// The last lines of the terminal a `term.send` types into.
    #[serde(default)]
    pub terminal_tail: Option<String>,
}

/// The share of the cap each list may take. They sum below one.
const ARGV_SHARE: u64 = 14;
const STEPS_SHARE: u64 = 22;
const TARGETS_SHARE: u64 = 22;
const OTHER_ARGS_SHARE: u64 = 16;
const TAIL_SHARE: u64 = 14;

pub fn undo(i: &UndoInput, cap: u64, scrub: &dyn Scrub) -> Prepared {
    let c = Clipper::new(scrub);
    let mut b = StateBuilder::new("undo", UNDO_VERSION, cap, scrub);

    b.scalar("tool", c.clip(&i.tool, 60))
        .cut_if("tool", c.cut())
        .scalar("class", c.clip(&i.class, 30))
        .cut_if("class", c.cut());
    if !i.posture.is_empty() {
        b.scalar("posture", c.clip(&i.posture, 30))
            .cut_if("posture", c.cut());
    }
    if let Some(s) = i.sandbox {
        b.scalar("sandbox", serde_json::to_value(s).unwrap_or_default());
    }
    if let Some(cwd) = &i.cwd {
        b.scalar("cwd", c.clip(cwd, 200)).cut_if("cwd", c.cut());
    }
    if let Some(k) = i.cwd_kind {
        b.scalar("cwd_kind", serde_json::to_value(k).unwrap_or_default());
    }

    // An argv's first words matter most (the program, then its flags), so a
    // cut keeps the head. So for the steps and the targets.
    if !i.argv.is_empty() {
        let argv: Vec<Value> = i
            .argv
            .iter()
            .take(UNDO_ARGV)
            .map(|a| Value::String(c.clip(a, 200)))
            .collect();
        let later = left_out(&i.argv, argv.len());
        b.list_head("argv", 9, share(cap, ARGV_SHARE), argv, later)
            .cut_if("argv", c.cut());
    }
    if !i.steps.is_empty() {
        let steps: Vec<Value> = i
            .steps
            .iter()
            .take(UNDO_STEPS)
            .map(|s| {
                Value::Array(
                    s.iter()
                        .take(UNDO_ARGV)
                        .map(|a| Value::String(c.clip(a, 200)))
                        .collect(),
                )
            })
            .collect();
        let later = left_out(&i.steps, steps.len());
        b.list_head("steps", 9, share(cap, STEPS_SHARE), steps, later)
            .cut_if("steps", c.cut());
    }
    if !i.targets.is_empty() {
        let targets: Vec<Value> = i
            .targets
            .iter()
            .take(UNDO_TARGETS)
            .map(|t| {
                serde_json::json!({
                    "path": c.clip(&t.path, 200),
                    "access": c.clip(&t.access, 20),
                    "recovery": t.recovery.as_str(),
                })
            })
            .collect();
        let later = left_out(&i.targets, targets.len());
        b.list_head("targets", 9, share(cap, TARGETS_SHARE), targets, later)
            .cut_if("targets", c.cut());
    }
    if let Some(u) = &i.url {
        b.scalar("url", c.clip(u, 400)).cut_if("url", c.cut());
    }
    if let Some(k) = i.remote_keeps {
        b.scalar(
            "remote_keeps",
            serde_json::to_value(k).unwrap_or_default(),
        );
    }
    if let Some(t) = &i.terminal_tail {
        // A prompt is the last thing on the screen, so a cut keeps the tail.
        b.text("terminal_tail", 8, share(cap, TAIL_SHARE), Keep::Tail, t);
    }
    if let Some(a) = &i.other_args {
        let cap_bytes = share(cap, OTHER_ARGS_SHARE) as usize * 4;
        b.text(
            "other_args",
            7,
            share(cap, OTHER_ARGS_SHARE),
            Keep::Head,
            &compact(a, cap_bytes + 8192),
        );
    }

    Prepared {
        state: Arc::new(b.build()),
        dynamic: Dynamic::default(),
    }
}
