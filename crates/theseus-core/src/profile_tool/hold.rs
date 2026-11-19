//! `profile.hold`'s act: this conversation put on a named profile, or let go (issue 28).
//!
//! It acts on one of two words, each the owner's: the message that started the turn (`said`,
//! when the owner's own typed input started it), or, when the turn had read outside text and the
//! call was asked first, the owner's own yes to the hold's card. A continuation is offered
//! nothing to read an owner's message from (`chat_acts::Board::offered`, by design), so the yes
//! is kept at the answer (`Board::keep_yes`, the answerer `confirm_action` judged) and taken
//! here, once. What acts is what the card showed: the profile is read from the plan the card
//! was made from (the action's proposal), never from what the model says after the card.

use super::*;

/// Said when no word of the owner's stands behind the call.
const NOT_SPOKEN: &str = "profile.hold acts only in a turn the owner's own message started, or on \
     the owner's own yes to its card, and neither started this one. Nothing was done.";

/// Put this conversation on the profile the owner chose, as the owner who spoke or pressed:
/// through `session_set_profile`, which the command calls, so it is judged as the command is.
pub(super) fn hold(
    board: &Board,
    core: &Core,
    tc: &TurnCtx<'_>,
    input: &Value,
    call: &str,
    said: Option<Said>,
) -> Result<String, String> {
    let (to, who) = match said.filter(|s| s.owner_input) {
        Some(s) => (hold_of(input)?, s.who),
        None => pressed(board, tc, input, call)?,
    };
    let done = core
        .session_set_profile(
            &SessionProfileParams {
                session_id: tc.session_id.to_string(),
                profile: to,
                author: None,
                discord: None,
            },
            &who,
        )
        .map_err(|e| format!("{e:#}"))?;
    if let Some(why) = done.refused {
        return Err(format!("Nothing was changed: {why}."));
    }
    let now = match &done.profile {
        Some(p) => format!("This conversation's next turns run on {p}"),
        None => {
            "This conversation's profile hold is let go: its next turns route as the default does"
                .into()
        }
    };
    let was = match (&done.previous, &done.profile) {
        (p, n) if p == n => " (it already was)".to_string(),
        (Some(p), _) => format!(" (it was held to {p})"),
        (None, _) => String::new(),
    };
    let note = done
        .note
        .map(|n| format!(" Note: {n}."))
        .unwrap_or_default();
    Ok(format!(
        "{now}{was}. This turn keeps the model it started on.{note}"
    ))
}

/// The profile its card named, and who pressed the yes, for the approved call `call` of this
/// conversation; or why there is none. The yes is taken, so it is one hold's.
fn pressed(
    board: &Board,
    tc: &TurnCtx<'_>,
    input: &Value,
    call: &str,
) -> Result<(Option<String>, Answerer), String> {
    let who = board.take_yes(call).ok_or(NOT_SPOKEN)?;
    // The confirm refuses an agent's process; this is the second look, at the act.
    if who.agent.is_some() {
        return Err(NOT_SPOKEN.into());
    }
    let action = tc
        .kernel
        .action(call)
        .ok()
        .flatten()
        .filter(|a| a.tool == HOLD && a.session_id == tc.session_id)
        .ok_or(NOT_SPOKEN)?;
    let kept = crate::toolrun::confirm_proposal(tc.store, &action, None).map_err(|_| NOT_SPOKEN)?;
    Ok((kept_plan(&kept.args, input)?, who))
}

/// The profile of the kept plan, which is what the card showed: refused when the call about to
/// run is not that plan.
pub(super) fn kept_plan(kept: &Value, input: &Value) -> Result<Option<String>, String> {
    let shown = hold_of(kept)?;
    match shown == hold_of(input)? {
        true => Ok(shown),
        false => Err("profile.hold is not what its card showed. Nothing was done.".into()),
    }
}
