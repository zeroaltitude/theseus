//! `wake.cancel` and `wake.move`: the model changes the wakes it set
//! (theseus-lherl; the horizon row HZ1). `wake.at` could only set, so asked to
//! "make that 4 pm" the model set a second wake. Its pending wakes are in the
//! last block of every request (`commitments.rs`), and these two act on them
//! by the id the block shows.
//!
//! - **Its own wakes only.** A name is resolved among this conversation's
//!   pending wakes, and another conversation's wake is not found. The
//!   operator's cancel (`theseus cancel`, `/cancel`) reaches every wake; the
//!   model's reaches its own.
//! - **Private places only.** The tools are not public (`places::public_tool`),
//!   so a shared place is not offered them and its gate refuses them, and each
//!   asks the place rule again before it acts, with the same words.
//! - **One frame each, with its row**: `wake.cancelled { by: "model" }`, and
//!   `wake.moved { from, to }`; the wake keeps its id. The harness runs them
//!   (`Backend::Harness`), and each hands the turn the list it leaves
//!   (`TurnCtx::commitments`), so the next loop's block is right.
//! - **A series.** `wake.move` moves the next occurrence, and the series goes
//!   on from where it was; with `series: true` it moves the series' time, its
//!   occurrences falling on the new one, with its `every`, `days` and `until`
//!   kept. A cancel ends the series.

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_kernel::{MoveTo, PendingWake, Repeat};
use theseus_tools::{parse, Backend, Plan, Retry, Tool, ToolClass, ToolCtx};

use crate::toolrun::TurnCtx;
use crate::wake::{local, series_of, span, MIN_AHEAD_MS};

pub const CANCEL: &str = "wake.cancel";
pub const MOVE: &str = "wake.move";

/// The model's name in a wake's cancel row (`wake.cancelled`'s `by`).
const BY: &str = "model";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CancelInput {
    id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MoveInput {
    id: String,
    #[serde(default)]
    at: Option<String>,
    #[serde(default)]
    after: Option<String>,
    #[serde(default)]
    series: bool,
}

fn id_schema() -> Value {
    json!({
        "type": "string",
        "description": "The wake's id as the pending-wakes block shows it (six characters, such as a8cebb), or its whole id."
    })
}

pub struct WakeCancel;

impl Tool for WakeCancel {
    fn name(&self) -> &'static str {
        CANCEL
    }

    fn description(&self) -> &'static str {
        "Cancel one of this conversation's pending wakes, by the id the pending-wakes block in \
         the last block shows. A series ends with it. Only your own wakes, and only from a \
         private place."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"id": id_schema()},
            "required": ["id"],
            "additionalProperties": false
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Write
    }

    fn backend(&self) -> Backend {
        Backend::Harness
    }

    fn retry(&self) -> Retry {
        // Run again after a crash, the wake is gone: the repeat says so.
        Retry::NonRepeatable
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let i: CancelInput = parse(input)?;
        Ok(Plan {
            summary: format!("cancel wake {}", i.id.trim()),
            ..Default::default()
        })
    }
}

pub struct WakeMove;

impl Tool for WakeMove {
    fn name(&self) -> &'static str {
        MOVE
    }

    fn description(&self) -> &'static str {
        "Move one of this conversation's pending wakes, by the id the pending-wakes block shows, \
         to `at` (RFC 3339 with its offset) or `after` (a duration from now): it keeps its id and \
         note. For a series it moves the next one; with `series` true, the series' time. Only \
         your own wakes, and only from a private place."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": id_schema(),
                "at": {
                    "type": "string",
                    "description": "The new time, as an RFC 3339 time with its offset, such as 2026-09-30T15:00:00-07:00."
                },
                "after": {
                    "type": "string",
                    "description": "The new time as a duration from now, such as 10m, 2h, or 1d."
                },
                "series": {
                    "type": "boolean",
                    "description": "For a series: move its time, not only its next occurrence."
                }
            },
            "required": ["id"],
            "additionalProperties": false
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Write
    }

    fn backend(&self) -> Backend {
        Backend::Harness
    }

    fn retry(&self) -> Retry {
        // A relative `after` moved again would move it twice.
        Retry::NonRepeatable
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let i: MoveInput = parse(input)?;
        let to = match (&i.after, &i.at) {
            (Some(a), _) => format!("to in {a}"),
            (None, Some(at)) => format!("to {at}"),
            (None, None) => String::new(),
        };
        Ok(Plan {
            summary: format!("move wake {} {to}", i.id.trim())
                .trim_end()
                .to_string(),
            ..Default::default()
        })
    }
}

/// The place rule, asked again by the tool itself (its words are the gate's).
fn private_only(tc: &TurnCtx<'_>, name: &str) -> Result<(), String> {
    match crate::places::refusal(tc.class, name, &Plan::default(), &[]) {
        Some(why) => Err(format!("Refused: {why}.")),
        None => Ok(()),
    }
}

/// This conversation's pending wake that `name` names: its id, or the end
/// of it (four characters at least), when exactly one matches.
pub fn resolve_own<'a>(wakes: &'a [PendingWake], name: &str) -> Result<&'a PendingWake, String> {
    let n = name
        .trim()
        .trim_start_matches('…')
        .trim_start_matches("wake ");
    let own = || {
        if wakes.is_empty() {
            "this conversation has no pending wake".to_string()
        } else {
            let ids: Vec<String> = wakes.iter().map(|w| crate::task::short(&w.id)).collect();
            format!("this conversation's pending wakes are {}", ids.join(", "))
        }
    };
    if n.len() < 4 {
        return Err(format!(
            "`{name}` is too short to name a wake: give at least four characters of its id; {}",
            own()
        ));
    }
    if let Some(one) = wakes.iter().find(|w| w.id == n) {
        return Ok(one);
    }
    let ends: Vec<&PendingWake> = wakes.iter().filter(|w| w.id.ends_with(n)).collect();
    match ends.as_slice() {
        [one] => Ok(one),
        [] => Err(format!(
            "no pending wake of this conversation is named `{name}`: {}. Another conversation's \
             wake is not yours to change",
            own()
        )),
        many => Err(format!(
            "`{name}` names {} of this conversation's wakes: give more of its id",
            many.len()
        )),
    }
}

/// This turn's execution's pending wakes, soonest first.
fn pending_of(tc: &TurnCtx<'_>) -> Result<Vec<PendingWake>, String> {
    tc.kernel
        .execution(tc.execution_id)
        .map_err(|e| format!("Not changed: {e:#}"))?
        .map(|e| e.wakes)
        .ok_or_else(|| "Not changed: this conversation has no execution".to_string())
}

fn when_of(w: &PendingWake, now_ms: u64) -> String {
    format!(
        "{} (in {})",
        local(w.due_at_ms).full(),
        span(w.due_at_ms.saturating_sub(now_ms))
    )
}

/// Run `wake.cancel` for the turn `tc`: end the wake, and say what is left.
pub fn cancel(tc: &TurnCtx<'_>, input: &Value) -> Result<(String, Value), String> {
    private_only(tc, CANCEL)?;
    let i: CancelInput = parse(input)?;
    let pending = pending_of(tc)?;
    let w = resolve_own(&pending, &i.id)?.clone();
    let short = crate::task::short(&w.id);
    let Some((e, _)) = tc
        .kernel
        .cancel_wake(tc.execution_id, &w.id, BY)
        .map_err(|e| format!("Not cancelled: {e:#}"))?
    else {
        return Err(format!(
            "Wake {short} is no longer pending: it ran, or was cancelled, just now."
        ));
    };
    tc.commitments.set(e.wakes.clone());
    let now = tc.kernel.now_ms();
    tc.record(&crate::fact::tool::WakeCancelled {
        short: &short,
        note: &w.note,
        pending: e.wakes.len(),
    });
    let series = w.repeat.is_some();
    let max = tc.kernel.config().wakes.max_wakes;
    Ok((
        format!(
            "Cancelled wake {short}, due {}: \"{}\" will not run{}. {} of {max} wakes \
             are pending.",
            when_of(&w, now),
            crate::session::title_from(&w.note),
            if series {
                ", and its series is ended"
            } else {
                ""
            },
            crate::wake::checks(&e.wakes)
        ),
        json!({"wake_id": w.id, "short": short, "cancelled": true, "series": series,
               "pending": e.wakes.len()}),
    ))
}

/// Where `i` moves `w`, from `now_ms`, or why it cannot.
fn target(
    w: &PendingWake,
    i: &MoveInput,
    now_ms: u64,
    (zone, max_ahead_ms): (&theseus_kernel::TimeZone, u64),
) -> Result<MoveTo, String> {
    let due = match (&i.at, &i.after) {
        (None, None) => {
            return Err(
                "give `after` (a duration such as 10m or 2h) or `at` (an RFC 3339 time)".into(),
            )
        }
        (Some(_), Some(_)) => return Err("give `at` or `after`, not both".into()),
        (at, after) => {
            crate::wake::due_from(at.as_deref(), after.as_deref(), now_ms, max_ahead_ms)?
        }
    };
    if !i.series {
        // The next occurrence alone; a series ends at `until`.
        if let Some(until) = w.repeat.as_ref().and_then(|r| r.until_ms) {
            if due > until {
                return Err(format!(
                    "that is past the series' end ({}): move it earlier, or move the series",
                    local(until).full()
                ));
            }
        }
        return Ok(MoveTo {
            due_at_ms: due,
            repeat: None,
        });
    }
    let Some(r) = &w.repeat else {
        return Err(
            "this wake does not repeat, so it has no series to move: leave out \
                    `series` to move it"
                .into(),
        );
    };
    let moved = Repeat {
        first_ms: due,
        ..r.clone()
    };
    let Some(first) = moved.first(zone) else {
        return Err(format!(
            "the series' `until` ({}) comes before that time: nothing would run",
            r.until_ms.map(|u| local(u).full()).unwrap_or_default()
        ));
    };
    if first.saturating_sub(now_ms) > max_ahead_ms || first.saturating_sub(now_ms) < MIN_AHEAD_MS {
        return Err(format!(
            "a series' next time must be between 1 s and {} days from now",
            max_ahead_ms / 86_400_000
        ));
    }
    Ok(MoveTo {
        due_at_ms: first,
        repeat: Some(moved),
    })
}

/// Run `wake.move` for the turn `tc`: the wake keeps its id, and its next
/// time, or its series', is the new one.
pub fn move_to(tc: &TurnCtx<'_>, input: &Value) -> Result<(String, Value), String> {
    private_only(tc, MOVE)?;
    let i: MoveInput = parse(input)?;
    let pending = pending_of(tc)?;
    let w = resolve_own(&pending, &i.id)?.clone();
    let short = crate::task::short(&w.id);
    let now = tc.kernel.now_ms();
    let kc = tc.kernel.config();
    let to = target(&w, &i, now, (&kc.zone, kc.wakes.max_ahead_ms))?;
    let Some(moved) = tc
        .kernel
        .move_wake(tc.guard, &w.id, to, BY)
        .map_err(|e| format!("Not moved: {e:#}"))?
    else {
        return Err(format!(
            "Wake {short} is no longer pending: it ran, or was cancelled, just now."
        ));
    };
    tc.commitments.set(moved.list.clone());
    let (old, new) = (when_of(&moved.from, now), when_of(&moved.wake, now));
    let series = moved
        .wake
        .repeat
        .as_ref()
        .map(|r| format!(", {}", series_of(r)));
    tc.record(&crate::fact::tool::WakeMoved {
        short: &short,
        from: &old,
        to: &new,
        series: series.as_deref(),
    });
    let max = tc.kernel.config().wakes.max_wakes;
    Ok((
        format!(
            "Moved wake {short} from {old} to {new}; it keeps its id{}. {} of {max} \
             wakes are pending.",
            series.as_deref().unwrap_or(""),
            crate::wake::checks(&moved.list)
        ),
        json!({"wake_id": w.id, "short": short, "from_ms": moved.from.due_at_ms,
               "due_at_ms": moved.wake.due_at_ms, "due_local": local(moved.wake.due_at_ms).full(),
               "in_ms": moved.wake.due_at_ms.saturating_sub(now), "series": i.series,
               "pending": moved.list.len()}),
    ))
}
