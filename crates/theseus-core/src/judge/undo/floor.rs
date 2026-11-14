//! **THE ONE-WAY FLOOR** (`p5kln-location-facts` piece 4; the owner,
//! 2026-11-13 12:22 MST, Twenty-fifth set, S25-1, whose whole message was
//! *"Yes"*).
//!
//! WHAT HE SAID YES TO, in the words it was put to him in: *for a plain file
//! write or edit whose resolved fact says nothing holds a copy, the call is
//! held whatever the model answers*. It applies **only where code knows the
//! whole effect of the call**; shell commands, sends and everything else stay
//! with the model's judgment; the model may still hold more than the facts
//! require; and a hold costs one model loop and does not reach him.
//!
//! **ONE DIRECTION ONLY, and that is the whole of why this is safe.** Code can
//! force a hold. It can never force `reversible`, never lower a verdict, and
//! never turn an `irreversible_final` — which reaches the owner — into a hold,
//! which does not. Everything here answers one question, `forces_hold`, and a
//! `false` from it means "the model decides", exactly as before.
//!
//! **WHY A CLOSED TABLE OF TOOL NAMES AND NOT A SHAPE TEST.** The condition
//! the owner set is that code knows the WHOLE EFFECT of the call. A structural
//! test — "not a program, class `write`, no argv, no url, and it names a write
//! resource" — reads the plan's own declaration of what it touches, and a
//! plan's resources are what a tool SAYS it will touch. That is true of the
//! three tools below because this repository implements them; it is not
//! something that can be known of an MCP tool whose server is somebody else's
//! process, and an MCP tool with a write resource would walk straight into
//! this floor. So the table is the same instrument `undo_regen` uses for
//! build paths, for the same reason: three lines, each saying what the tool's
//! whole effect is, and a reader can argue with all three at once. Growing it
//! is a decision with a reviewer.
//!
//! The structural test is kept as well, under the table rather than instead of
//! it: a call that carries an argv, steps, a url or a program backend is not a
//! plain file write however it is named, and a tool that grew a second effect
//! would stop matching here rather than go on being floored for the wrong
//! reason.
//!
//! **WHY `unknown` IS NOT ONE OF THE WORDS.** The owner named three —
//! `untracked`, `outside_repo`, `tracked_dirty` — and `unknown` is not among
//! them. The pack is told to READ `unknown` as no copy, and the model may hold
//! on it; that is the model's judgment and it stays the model's. A floor is
//! code overriding a model, so it runs on the facts the owner named and not on
//! the resolver's admission that it could not tell.
//!
//! **WHY `absent` IS NOT EITHER, which is what keeps the floor off ordinary
//! work.** Creating a file loses nothing, so a write to a path that is not
//! there is never floored. With piece 1 (a file this session wrote reads as
//! `read_this_session`) and piece 2 (a build output reads as `regenerable`),
//! the calls left for the floor are the ones that overwrite bytes that this
//! session has not seen and that neither git nor a build holds — which is the
//! case the eval set calls `f4`.

use theseus_judge::builders::{Recovery, UndoInput};

use super::state::UndoCall;

/// One line of the CLOSED table of plain file tools: a tool whose whole effect
/// is the file targets its plan names, so code knows what the call would do.
#[derive(Debug, Clone, Copy)]
struct Plain {
    /// The canonical tool name, as `Tool::name` gives it.
    tool: &'static str,
    /// What the whole effect is. Kept for the reader of this table, and
    /// asserted non-empty by `the_table_is_closed_and_every_line_says_its_whole_effect`,
    /// so a new line cannot be added without saying this.
    effect: &'static str,
}

/// **THE CLOSED TABLE.** One line per tool; each says what its whole effect is.
/// `fs.read`, `fs.glob`, `fs.grep` and `fs.list` are absent because they change
/// nothing; `proc.run` and every other program, every network tool and every
/// MCP tool are absent because their whole effect is not the paths they name.
const PLAIN_FILE_TOOLS: &[Plain] = &[
    Plain {
        tool: "fs.write",
        effect: "it writes the one path it names, and nothing else",
    },
    Plain {
        tool: "fs.edit",
        effect: "it replaces a string inside the one path it names, and nothing else",
    },
    Plain {
        tool: "fs.patch",
        effect: "it applies a diff to the paths its own headers name, all or none",
    },
];

/// Whether a resolved word says nothing holds a copy, in the owner's own three
/// words. The match is exhaustive on purpose: a word added to [`Recovery`]
/// later has to be decided here rather than falling quietly to one side.
fn nothing_holds(r: Recovery) -> bool {
    match r {
        // The three the owner named (S25-1).
        Recovery::Untracked | Recovery::OutsideRepo | Recovery::TrackedDirty => true,
        // A way back exists: a checkout, this session's own record, or the
        // project's own build.
        Recovery::TrackedClean | Recovery::ReadThisSession | Recovery::Regenerable => false,
        // Nothing is there to lose.
        Recovery::Absent => false,
        // Not one of the three he named: the model's judgment, as before.
        Recovery::Unknown => false,
    }
}

/// Whether one call of a table tool carries nothing but its file targets. The
/// structural test sits UNDER the table, never instead of it: the table says
/// this tool's whole effect is its targets, and this says THIS call of it is
/// the shape the table was written about.
fn carries_only_its_targets(call: &UndoCall) -> bool {
    !call.is_program && call.argv.is_empty() && call.steps.is_empty() && call.url.is_none()
}

/// **The floor itself.** Whether code forces a hold for this call.
pub(crate) fn forces_hold(call: &UndoCall, input: &UndoInput) -> bool {
    floored_because(call, input).is_some()
}

/// **Why** the floor holds this call, in the table's own words for the tool's
/// whole effect; `None` is "the model decides", which is every call this floor
/// does not reach. It reads the same resolved words the request carries — they
/// come from one `UndoInput` — so what code saw and what the model was shown
/// cannot differ.
///
/// Only `write` targets count: a plain file call may name a path it merely
/// reads, and nothing is lost from a path the call does not change.
///
/// This is the reader of each line's `effect` (the reader rule, AGENTS.md:
/// nothing is declared without its reader), and it is what a report or a row
/// would say if it ever names why a call was floored.
pub(crate) fn floored_because(call: &UndoCall, input: &UndoInput) -> Option<&'static str> {
    let effect = whole_effect_of(&call.tool)?;
    let loses = input
        .targets
        .iter()
        .any(|t| t.access == "write" && nothing_holds(t.recovery));
    (carries_only_its_targets(call) && loses).then_some(effect)
}

/// The table's own words for what a tool's whole effect is, or `None` for a
/// tool it does not name.
pub(crate) fn whole_effect_of(tool: &str) -> Option<&'static str> {
    PLAIN_FILE_TOOLS
        .iter()
        .find(|p| p.tool == tool)
        .map(|p| p.effect)
}

#[cfg(test)]
#[path = "floor/tests.rs"]
mod tests;
