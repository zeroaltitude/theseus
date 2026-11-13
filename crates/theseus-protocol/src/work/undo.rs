//! The owner's override of a held call, on the wire (theseus-p5kln).
//!
//! `undo.v1` holds a call it calls `irreversible_reshapeable`: the call does
//! not run, the model is told to re-plan it, and **no card is made**, so the
//! hold costs the owner no attention. The owner's own word of 2026-11-12
//! asked for the other half of that: *"in the case that the irreversibility
//! flag is flipped and the model is notified, that i have the power to say
//! it's OK to run what was originally suggested"*. These are the types of
//! that power: a read of the holds this conversation left
//! ([`UndoListParams`]), and the allowance that lets ONE of them run as it
//! was first proposed ([`UndoAllowParams`]).
//!
//! **A hold is named by an id, and the allowance admits that call alone.**
//! [`HeldCall::input`] is the call's arguments exactly as the model proposed
//! them; the gate spends the allowance only on a call whose tool and input
//! are those bytes, so a *re-planned* call — which is a different input — is
//! held as it was. That is what makes this an override of the hold and not a
//! way past the classifier.
//!
//! **Not in the cockpit's TypeScript.** There is no cockpit reader yet, as
//! `flow.rs` has none: the generator's list in `ts.rs` does not name these,
//! so `cockpit/src/protocol.gen/` is unchanged by them. A cockpit surface
//! for the holds adds them to that list in its own commit.
//!
//! A submodule of `work.rs` rather than a module of its own, because
//! `lib.rs` has one line of headroom under `scripts/shape.sh`'s 2,500 and
//! `work/horizon.rs` set that precedent for exactly this reason.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The most holds kept, newest first. Past it the oldest are forgotten: the
/// holds are an offer of an override and not the record of what happened,
/// which is the `tool.held` row's and does not move.
pub const KEEP: usize = 64;

/// Who allowed a held call to run, and from where. The same three fields a
/// repository's grant keeps (`project_context::trust::Grant`), for the same
/// reason: an override of a safety hold is an act, and the record says whose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Allowance {
    pub by: String,
    pub via: String,
    pub at_ms: u64,
}

/// One call `undo.v1` held, as the store keeps it and a surface lists it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeldCall {
    /// Short, and what `undo.allow` names: `hld_<8 hex>`.
    pub id: String,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    pub tool: String,
    /// The call's arguments as the model first proposed them. The allowance
    /// is spent only on a call whose input is these bytes.
    pub input: Value,
    /// The call in one line, as the gate's plan summarised it.
    pub summary: String,
    /// The judgment that held it, so the `tool.held` row joins to this.
    pub judgment: String,
    /// The shape the pack chose, where it chose one above its own bar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<String>,
    pub held_ms: u64,
    /// Set by `undo.allow`; cleared by the gate when the call runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed: Option<Allowance>,
    /// When the gate spent the allowance. A hold with this set is history.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ran_ms: Option<u64>,
}

impl HeldCall {
    /// Whether an allowance on this hold is still waiting to be spent.
    pub fn waiting(&self) -> bool {
        self.allowed.is_some() && self.ran_ms.is_none()
    }

    /// The one line a surface shows: the id, the tool and what it was about.
    pub fn line(&self) -> String {
        let state = match (&self.allowed, self.ran_ms) {
            (_, Some(_)) => " · ran",
            (Some(_), None) => " · allowed",
            (None, None) => "",
        };
        format!("{} · {} · {}{state}", self.id, self.tool, self.summary)
    }
}

/// `undo.list`: the held calls, newest first. With `session_id` only that
/// conversation's.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndoListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// With this unset, only the holds no override has spent.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub all: bool,
}

/// `undo.list`'s answer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UndoListResult {
    pub held: Vec<HeldCall>,
}

/// `undo.allow`: let the held call `hold` run as it was first proposed. The
/// owner's act, judged as a repository's trust is (`places::owner_in_person`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndoAllowParams {
    /// The hold's id, whole or its last four characters or more.
    pub hold: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discord: Option<crate::DiscordOrigin>,
}

/// `undo.allow`'s answer: the hold as it now stands, and the words every
/// surface shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UndoAllowResult {
    pub hold: HeldCall,
    pub words: String,
}

/// The words an allowance is reported in, written once here so the CLI, the
/// chat route and a card cannot say different things. The model is told the
/// same sentence as a harness fact.
pub fn allowed_words(h: &HeldCall) -> String {
    format!(
        "{} is allowed to run as it was first proposed ({}). It runs when the model proposes it \
         again unchanged; a re-planned call is still held.",
        h.id, h.summary
    )
}

/// The shortest end of an id that names a hold, as `work.answer`'s
/// `question_in` takes.
pub const SHORTEST_ID: usize = 4;

/// The holds `name` picks out of `held`: the whole id, or an end of at least
/// [`SHORTEST_ID`] characters. More than one is ambiguous and the caller
/// refuses; the comparison is lower case, as an id is.
pub fn pick<'a>(held: &'a [HeldCall], name: &str) -> Vec<&'a HeldCall> {
    let want = name.trim().to_lowercase();
    if want.len() < SHORTEST_ID {
        return vec![];
    }
    let whole: Vec<&HeldCall> = held
        .iter()
        .filter(|h| h.id.to_lowercase() == want)
        .collect();
    if !whole.is_empty() {
        return whole;
    }
    held.iter()
        .filter(|h| h.id.to_lowercase().ends_with(&want))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn held(id: &str) -> HeldCall {
        HeldCall {
            id: id.into(),
            session_id: "ses_1".into(),
            turn_id: None,
            tool: "fs.write".into(),
            input: json!({"path": "/w/notes.txt"}),
            summary: "write notes.txt".into(),
            judgment: "jdg_1".into(),
            shape: Some("move_aside".into()),
            held_ms: 1,
            allowed: None,
            ran_ms: None,
        }
    }

    #[test]
    fn a_hold_is_named_by_its_whole_id_or_by_an_end_of_four() {
        let all = [held("hld_1a2b3c4d"), held("hld_5e6f7a8b")];
        assert_eq!(pick(&all, "hld_1a2b3c4d").len(), 1, "the whole id");
        assert_eq!(pick(&all, "3c4d").len(), 1, "an end of four");
        assert_eq!(pick(&all, "HLD_1A2B3C4D").len(), 1, "case does not matter");
        assert!(pick(&all, "4d").is_empty(), "an end of two names nothing");
        assert!(pick(&all, "zzzz").is_empty(), "no such hold");
        // Two holds whose ids end alike are ambiguous, and the caller refuses.
        let twins = [held("hld_aaaa1234"), held("hld_bbbb1234")];
        assert_eq!(pick(&twins, "1234").len(), 2, "ambiguous, not guessed");
        // A whole id beats an end: the exact hold, never both.
        let one = [held("hld_1234"), held("hld_aa1234")];
        assert_eq!(pick(&one, "hld_1234").len(), 1, "the exact id wins");
    }

    #[test]
    fn a_holds_state_is_in_its_line_and_waiting_is_allowed_and_unspent() {
        let mut h = held("hld_1a2b3c4d");
        assert!(!h.waiting(), "nothing allowed it yet");
        assert!(h.line().ends_with("write notes.txt"), "{}", h.line());
        h.allowed = Some(Allowance {
            by: "the owner".into(),
            via: "cli".into(),
            at_ms: 2,
        });
        assert!(h.waiting(), "allowed and not yet spent");
        assert!(h.line().ends_with(" · allowed"), "{}", h.line());
        h.ran_ms = Some(3);
        assert!(!h.waiting(), "spent");
        assert!(h.line().ends_with(" · ran"), "{}", h.line());
    }

    #[test]
    fn the_allowance_words_name_the_hold_and_say_a_replan_is_still_held() {
        let w = allowed_words(&held("hld_1a2b3c4d"));
        assert!(w.contains("hld_1a2b3c4d"), "{w}");
        assert!(w.contains("write notes.txt"), "{w}");
        assert!(w.contains("re-planned call is still held"), "{w}");
    }
}
