//! Health's lines about holds and waiting executions, said for what they do
//! (theseus-g00vv). Apart from `render.rs`, whose length the shape budget caps.

use theseus_protocol::{ExternalTextInfo, KernelStatus};

use super::{held_since, held_what, short_id};

/// The sessions that hold external text (theseus-9bp), as health and `policy
/// list` say them: how many, the first three with what each read, and the way
/// to trust one again. `mode` is the daemon's `[policy] external_text`: under
/// `notify` a hold's first call that acts tells the owner and none waits, so
/// the line says that; `ask` (and a daemon that sends none) keeps the words
/// "their calls that act wait".
pub fn line(held: &[ExternalTextInfo], mode: &str) -> Option<String> {
    if held.is_empty() {
        return None;
    }
    let each: Vec<String> = held
        .iter()
        .take(3)
        .map(|i| {
            let name = match &i.task {
                Some(t) => format!("task {t}"),
                None => short_id(&i.session_id),
            };
            format!(
                "{name} since {} ({}{})",
                held_since(&i.since_local, &i.held),
                held_what(&i.held),
                match i.held.via.as_deref() {
                    Some("task.create") => ", from the session that started it",
                    Some("task.report") => ", from a task's report",
                    Some("job") => ", from the session whose job reached it",
                    _ => "",
                }
            )
        })
        .collect();
    let more = match held.len() {
        n if n > 3 => format!(", and {} more", n - 3),
        _ => String::new(),
    };
    let s = if held.len() == 1 { "" } else { "s" };
    let then = if mode == "notify" {
        "under notify each one's first call that acts tells you"
    } else {
        "so their calls that act wait"
    };
    let verb = if mode == "notify" { ";" } else { "," };
    Some(format!(
        "external text: {} session{s} read it{verb} {then}: {}{more} · trust one again: theseus \
         policy trust <session>",
        held.len(),
        each.join("; ")
    ))
}

/// The kernel's executions by state, `4 cancelled, 189 waiting: 180 at rest on
/// input, 9 retired, 0 on work, 0 on a time`: the waiting count says what each
/// waits on when the daemon sends the split.
pub fn executions(k: &KernelStatus) -> String {
    if k.executions_by_state.is_empty() {
        return "none".to_string();
    }
    k.executions_by_state
        .iter()
        .map(|(state, n)| match state.as_str() {
            "waiting" if k.waiting_split != Default::default() => {
                format!("{n} {state}: {}", k.waiting_split.words())
            }
            _ => format!("{n} {state}"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// What a dismissed notice says: the hold stays, and tells again when a new
/// one comes.
pub fn notice_seen_line(r: &theseus_protocol::notice::NoticeSeenResult) -> String {
    let what = held_what(&r.held);
    if r.wrote {
        format!(
            "dismissed the notice of session {} · it had read {what} · the hold stays on record, \
             and a later hold tells you again",
            r.session_id
        )
    } else {
        format!(
            "session {}'s notice was dismissed already · it had read {what}",
            r.session_id
        )
    }
}
