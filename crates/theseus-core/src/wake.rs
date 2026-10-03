//! Wakes into the current session (DD8, theseus-cff; spec §3.3, §3.15):
//! `wake.at { at | after, note }` asks for a turn in this conversation at a
//! time, whose input is the note.
//!
//! - **The tool.** The harness runs it (`Backend::Harness`), since it needs
//!   the turn's kernel. It records a pending wake on the session's execution
//!   (`Kernel::set_wake`) and returns at once.
//! - **When it fires.** Once the wake is due and its session is free, the
//!   due scan (the driver's tick, and the heartbeat) queues the execution,
//!   and the driver takes a continuation turn. The turn's catch-up takes the
//!   due wakes, each as a node the model reads as the user's: `⏰ wake (set
//!   13:05): check the build`, with how late it ran when it ran late. It is
//!   an ordinary turn: it spends from the session's limit, under its
//!   authority, and its reply posts where the session posts.
//! - **Few.** At most `MAX_PENDING` per session. A task cannot set one: it
//!   reports when it is done.
//! - **Repeating** (37a, theseus-d4pt). `every` (`30m`, `1d`, `1w`; `[kernel]
//!   min_repeat_minutes` at the shortest), with `days` for a daily one and
//!   `until`, makes the wake a series: one pending wake, counted once, that
//!   the turn taking it puts back at its next occurrence in the daemon's
//!   zone (`theseus_kernel::repeat`). Its node says which occurrence it is,
//!   and how many were passed over while it waited: `⏰ wake (every 1d, #4;
//!   2 missed while the daemon was down): …`. A repeating wake waits for
//!   approval in a session holding external text, where a one-shot one keeps
//!   its posture (T1b): set once under a page's influence, it would run every
//!   day (`repeats`, read by the gate).
//! - **Seeing and stopping.** `wake.list` and `wake.cancel`, `theseus wakes`
//!   and `theseus cancel`, Discord's `/wakes` and `/cancel`, health, and the
//!   Observatory.

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_kernel::{
    Day, Every, Execution, FiredWake, KernelError, PendingWake, Repeat, TimeZone, MAX_PENDING,
};
use theseus_tools::{parse, Backend, Plan, Retry, Tool, ToolClass, ToolCtx};

use crate::toolrun::TurnCtx;

pub const AT: &str = "wake.at";
/// The tools this module adds, for the template's `[policy.tools]` list.
pub const NAMES: [&str; 1] = [AT];
/// The longest note a wake takes, in characters.
pub const MAX_NOTE_CHARS: usize = 2_000;
/// The soonest and the latest a wake may be set for, from now.
pub const MIN_AHEAD_MS: u64 = 1_000;
pub const MAX_AHEAD_MS: u64 = 30 * 86_400_000;
/// A wake that runs more than this after its due time says how late it ran.
pub const LATE_AFTER_MS: u64 = 5_000;
/// The longest span a repeating wake may take.
pub const MAX_EVERY_MS: u64 = 365 * 86_400_000;

/// What a task says to a model that asks it to set a wake.
pub const TASK_REFUSAL: &str = "Refused: this session is a task, and a task cannot set wakes; it \
    reports when it finishes. Wait on the work itself, or say in your report when it should be \
    checked again; the conversation that started you can set the wake.";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    #[serde(default)]
    at: Option<String>,
    #[serde(default)]
    after: Option<String>,
    #[serde(default)]
    every: Option<String>,
    #[serde(default)]
    days: Option<Vec<String>>,
    #[serde(default)]
    until: Option<String>,
    note: String,
}

/// Whether a `wake.at` call's input asks for a repeating wake (37a): the
/// gate holds it in a session holding external text.
pub fn repeats(input: &Value) -> bool {
    input.get("every").is_some_and(|v| !v.is_null())
}

fn input_of(input: &Value) -> Result<Input, String> {
    let i: Input = parse(input)?;
    if i.note.trim().is_empty() {
        return Err("the note is empty: say what the wake's turn should do".into());
    }
    let n = i.note.chars().count();
    if n > MAX_NOTE_CHARS {
        return Err(format!(
            "the note is {n} characters, over the {MAX_NOTE_CHARS} a wake takes"
        ));
    }
    if i.every.is_none() && (i.days.is_some() || i.until.is_some()) {
        return Err("`days` and `until` belong to a repeating wake: give `every` too".into());
    }
    match (&i.at, &i.after) {
        (Some(_), Some(_)) => Err("give `at` or `after`, not both".into()),
        (None, None) if i.every.is_none() => {
            Err("give `after` (a duration such as 10m or 2h) or `at` (an RFC 3339 time)".into())
        }
        _ => Ok(i),
    }
}

/// When `i`'s wake is first due, from `now_ms`, and its series if it
/// repeats, in `zone`, with `floor_ms` the shortest span it may take; or why
/// it cannot be.
fn plan_of(
    i: &Input,
    now_ms: u64,
    zone: &TimeZone,
    floor_ms: u64,
) -> Result<(u64, Option<Repeat>), String> {
    let Some(every) = &i.every else {
        return Ok((due_of(i, now_ms)?, None));
    };
    let every = Every::parse(every)?;
    if every.nominal_ms() < floor_ms {
        return Err(format!(
            "a wake may repeat every {} at the most often, not every {every}",
            span(floor_ms)
        ));
    }
    if every.nominal_ms() > MAX_EVERY_MS {
        return Err(format!(
            "a wake may repeat every 365 days at the longest, not every {every}"
        ));
    }
    let days = match &i.days {
        None => vec![],
        Some(d) if d.is_empty() => {
            return Err("`days` is empty: leave it out to repeat every day".into())
        }
        Some(d) => {
            if every.to_string() != "1d" {
                return Err(format!(
                    "`days` limits a daily wake: give `every = \"1d\"` with it, not `{every}`"
                ));
            }
            let mut days = d
                .iter()
                .map(|s| {
                    Day::parse(s).ok_or_else(|| {
                        format!("`days` takes weekdays such as mon, tue, …, sun, not `{s}`")
                    })
                })
                .collect::<Result<Vec<Day>, String>>()?;
            days.sort();
            days.dedup();
            days
        }
    };
    // The first time: `at` or `after`, or one span from now.
    let first_ms = if i.at.is_some() || i.after.is_some() {
        due_of(i, now_ms)?
    } else {
        let from_now = Repeat {
            every,
            first_ms: now_ms,
            days: vec![],
            until_ms: None,
        };
        from_now
            .at(zone, 1)
            .ok_or_else(|| format!("`every` ({every}) runs past what a time can be"))?
    };
    let until_ms = match &i.until {
        None => None,
        Some(u) => Some(parse_at(u).map_err(|e| e.replace("`at`", "`until`"))?),
    };
    let repeat = Repeat {
        every,
        first_ms,
        days,
        until_ms,
    };
    let Some(first) = repeat.first(zone) else {
        return Err(format!(
            "`until` ({}) comes before the series' first time: nothing would run",
            i.until.as_deref().unwrap_or("")
        ));
    };
    if first.saturating_sub(now_ms) > MAX_AHEAD_MS {
        return Err(format!(
            "a wake's first time may be at most 30 days from now, and this one is {}",
            span(first.saturating_sub(now_ms))
        ));
    }
    Ok((first, Some(repeat)))
}

/// The due time `i` asks for, from `now_ms`, or why it cannot be.
fn due_of(i: &Input, now_ms: u64) -> Result<u64, String> {
    let due = match (&i.at, &i.after) {
        (_, Some(a)) => now_ms.saturating_add(parse_after(a)?),
        (Some(at), None) => {
            let due = parse_at(at)?;
            if due <= now_ms {
                return Err(format!(
                    "`at` ({at}) is {} ago; it is {} now. Give a later time, or use `after`",
                    span(now_ms - due),
                    local(now_ms).full()
                ));
            }
            due
        }
        (None, None) => unreachable!("input_of requires one"),
    };
    let ahead = due.saturating_sub(now_ms);
    if ahead < MIN_AHEAD_MS {
        return Err("a wake must be at least 1 s from now".into());
    }
    if ahead > MAX_AHEAD_MS {
        return Err(format!(
            "a wake may be at most 30 days from now, and this one is {}",
            span(ahead)
        ));
    }
    Ok(due)
}

/// The toollet side of `wake.at`: its name, description, and schema, and the
/// plan the gate reads. The harness runs it (`set`).
pub struct WakeAt;

impl Tool for WakeAt {
    fn name(&self) -> &'static str {
        AT
    }

    fn description(&self) -> &'static str {
        "Wake this conversation later: at that time it gets a turn of its own, whose input is \
         `note`, marked as a wake (`⏰ wake (set 13:05): <note>`), and your reply posts where \
         this conversation posts. Use it for \"remind me in 10 minutes\" or \"check the build at \
         3 pm\". Give `after`, a duration such as `90s`, `10m`, `2h`, or `1h30m`, or `at`, an \
         RFC 3339 time with its offset such as `2026-09-30T15:00:00-07:00`; at least 1 second \
         and at most 30 days ahead. Write the note as the instruction you will want then. It \
         fires once. Messages that come before it are answered as usual, and it still fires \
         later; if a turn is running at its time, it runs when that turn ends. A session holds \
         at most 5 pending wakes. To repeat it, give `every` (`30m`, `2h`, `1d`, `1w`): it then \
         runs at each occurrence, from `at` or `after` (or one `every` from now), in the \
         daemon's time zone, so a daily wake keeps its clock time; `days` limits a daily one \
         to weekdays (`[\"mon\", \"fri\"]`), and `until` (RFC 3339) ends it. A series counts \
         as one wake, and a cancel ends it."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "after": {
                    "type": "string",
                    "description": "How long from now: a duration such as 90s, 10m, 2h, 1h30m, or 1d."
                },
                "at": {
                    "type": "string",
                    "description": "When, as an RFC 3339 time with its offset, such as 2026-09-30T15:00:00-07:00."
                },
                "every": {
                    "type": "string",
                    "description": "Repeat it at this span: Nm (5 minutes at least), Nh, Nd, or Nw, such as 30m, 1d, or 1w."
                },
                "days": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "With every = 1d: the weekdays it runs on, such as [\"mon\", \"tue\", \"wed\", \"thu\", \"fri\"]."
                },
                "until": {
                    "type": "string",
                    "description": "With every: no occurrence after this RFC 3339 time."
                },
                "note": {
                    "type": "string",
                    "description": "The wake's turn's input: what to do or check then."
                }
            },
            "required": ["note"],
            "additionalProperties": false
        })
    }

    fn class(&self) -> ToolClass {
        // It changes this session's state, and runs alone in a batch.
        ToolClass::Write
    }

    fn backend(&self) -> Backend {
        Backend::Harness
    }

    fn retry(&self) -> Retry {
        // Run again after a crash, it finds the wake it set (`wake_id`).
        Retry::NonRepeatable
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let i = input_of(input)?;
        let mut when = match (&i.after, &i.at) {
            (Some(a), _) => format!("in {a}"),
            (None, Some(at)) => format!("at {at}"),
            (None, None) => String::new(),
        };
        if let Some(every) = &i.every {
            when = format!(
                "every {every}{}{when}",
                if when.is_empty() { "" } else { ", first " }
            );
        }
        Ok(Plan {
            summary: format!("wake {when}: {}", crate::session::title_from(&i.note)),
            ..Default::default()
        })
    }
}

/// Run `wake.at` for the call `correlation_id` of the turn `tc` (the
/// harness's side): set the wake, and say when it fires. An error is the
/// result the model reads.
pub fn set(
    tc: &TurnCtx<'_>,
    input: &Value,
    correlation_id: &str,
) -> Result<(String, Value), String> {
    let i = input_of(input)?;
    if tc.task.is_some() {
        return Err(TASK_REFUSAL.into());
    }
    let now = tc.kernel.now_ms();
    let kc = tc.kernel.config();
    let (due, repeat) = plan_of(&i, now, &kc.zone, kc.min_repeat_ms)?;
    let target = tc.outbox.target(tc.session_id);
    let set = match tc.kernel.set_wake(
        tc.guard,
        correlation_id,
        due,
        &i.note,
        target.clone(),
        repeat,
    ) {
        Ok(s) => s,
        Err(e) => {
            return Err(match e.downcast_ref::<KernelError>() {
                Some(KernelError::TooManyWakes { max }) => {
                    let pending = tc
                        .kernel
                        .execution(tc.execution_id)
                        .ok()
                        .flatten()
                        .map(|e| e.wakes)
                        .unwrap_or_default();
                    too_many(*max, &pending, now)
                }
                _ => format!("Not set: {e:#}"),
            })
        }
    };
    let w = &set.wake;
    let s = crate::task::short(&w.id);
    let when = format!(
        "{} (in {})",
        local(w.due_at_ms).full(),
        span(w.due_at_ms.saturating_sub(now))
    );
    let series = w.repeat.as_ref().map(series_of);
    if set.set {
        tc.record(&crate::fact::tool::WakeSet {
            short: &s,
            when: &when,
            note: &w.note,
            pending: set.pending,
            series: series.as_deref(),
        });
    }
    let text = if set.set && w.repeat.is_some() {
        format!(
            "Set wake {s}, {}, first at {when}; {} of {MAX_PENDING} wakes are pending (a series \
             counts once), and its id is {}. This conversation gets a turn at each occurrence, \
             whose input is this line:\n{}",
            series.as_deref().unwrap_or(""),
            set.pending,
            w.id,
            line(w, None, w.note.as_str())
        )
    } else if set.set {
        format!(
            "Set wake {s} for {when}; {} of {MAX_PENDING} wakes are pending, and its id is {}. \
             This conversation gets a turn then, whose input is this line:\n\
             ⏰ wake (set {}): {}",
            set.pending,
            w.id,
            local(w.set_at_ms).hm(),
            w.note
        )
    } else {
        format!(
            "Wake {s} was already set by this call, for {when}. Its id is {}.",
            w.id
        )
    };
    let mut meta = json!({
        "wake_id": w.id,
        "short": s,
        "due_at_ms": w.due_at_ms,
        "due_local": local(w.due_at_ms).full(),
        "in_ms": w.due_at_ms.saturating_sub(now),
        "pending": set.pending,
        "set": set.set,
        "target": target,
    });
    if let Some(r) = &w.repeat {
        meta["every"] = json!(r.every.to_string());
        meta["days"] = json!(r.days);
        meta["until_ms"] = json!(r.until_ms);
    }
    Ok((text, meta))
}

/// A series as people say it: `every 1d`, `every 1d on mon–fri`, `every 1h
/// until 2026-10-09 18:00:00 -07:00`.
pub fn series_of(r: &Repeat) -> String {
    let mut out = format!("every {}", r.every);
    if !r.days.is_empty() {
        let days: Vec<&str> = r.days.iter().map(|d| d.as_str()).collect();
        out.push_str(&format!(" on {}", days.join(", ")));
    }
    if let Some(u) = r.until_ms {
        out.push_str(&format!(" until {}", local(u).full()));
    }
    out
}

/// A repeating wake's line: `⏰ wake (every 1d, #4): <note>`, with what
/// `extra` adds after the number.
fn line(w: &PendingWake, extra: Option<String>, note: &str) -> String {
    let every = w
        .repeat
        .as_ref()
        .map(|r| r.every.to_string())
        .unwrap_or_default();
    format!(
        "⏰ wake (every {every}, #{}{}): {note}",
        w.occurrence,
        extra.unwrap_or_default()
    )
}

/// The refusal at the cap: what is pending, soonest first.
fn too_many(max: usize, pending: &[PendingWake], now_ms: u64) -> String {
    let list = pending
        .iter()
        .map(|w| {
            format!(
                "{} at {} (in {}): {}",
                crate::task::short(&w.id),
                local(w.due_at_ms).full(),
                span(w.due_at_ms.saturating_sub(now_ms)),
                crate::session::title_from(&w.note)
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    format!(
        "Refused: this session already has {max} pending wakes, the most it may hold ({list}). \
         Wait for one to fire, or ask the operator to cancel one (`theseus cancel <id>`, or \
         `/cancel` on Discord)."
    )
}

/// The node a fired wake writes into its session: `⏰ wake (set 13:05):
/// <note>`, and, when it ran late, when it was due and by how much. A
/// repeating one says its span and which occurrence it is, and how many it
/// passed over (37a): `⏰ wake (every 1d, #4; 2 missed while the daemon was
/// down): <note>`.
pub fn fired_text(f: &FiredWake) -> String {
    let set = local(f.wake.set_at_ms);
    let due = local(f.wake.due_at_ms);
    if f.wake.repeat.is_some() {
        let why = if f.while_down {
            " while the daemon was down"
        } else {
            ""
        };
        let extra = if f.missed > 0 {
            Some(format!("; {} missed{why}", f.missed))
        } else if f.late_ms > LATE_AFTER_MS {
            let why = if f.while_down {
                ": the daemon was not running then"
            } else {
                ""
            };
            Some(format!(
                ", due {}, {} late{why}",
                due.hms_on(&set),
                span(f.late_ms)
            ))
        } else {
            None
        };
        return line(&f.wake, extra, &f.wake.note);
    }
    let late = if f.late_ms > LATE_AFTER_MS {
        let why = if f.while_down {
            ": the daemon was not running then"
        } else {
            ""
        };
        format!(", due {}, {} late{why}", due.hms_on(&set), span(f.late_ms))
    } else {
        String::new()
    };
    format!("⏰ wake (set {}{late}): {}", set.hm(), f.wake.note)
}

/// A pending wake as the protocol shows it (`wake.list`, health, `/wakes`,
/// the Observatory). Its target is the place it was set from, which it
/// records: health only peeks, and never warms the outbox's index for it.
pub fn info(e: &Execution, w: &PendingWake, title: Option<String>) -> theseus_protocol::WakeInfo {
    theseus_protocol::WakeInfo {
        wake_id: w.id.clone(),
        short: crate::task::short(&w.id),
        session_id: e.session_id.clone(),
        execution_id: e.id.clone(),
        session_title: title,
        due_at_ms: w.due_at_ms,
        due_local: local(w.due_at_ms).full(),
        note: w.note.clone(),
        set_at_ms: w.set_at_ms,
        target: w.target.clone(),
        state: e.state.as_str().into(),
    }
}

/// No wake, or more than one, answers to a name; the message says which.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct NoSuchWake(pub String);

/// The wake a person means by `name`: its id, or the end of it (`a1b2c3`),
/// when exactly one pending wake matches.
pub fn resolve<'a>(
    wakes: &'a [(Execution, PendingWake)],
    name: &str,
) -> Result<&'a (Execution, PendingWake), String> {
    let n = name
        .trim()
        .trim_start_matches('…')
        .trim_start_matches("wake ");
    if n.len() < 4 {
        return Err(format!(
            "`{name}` is too short to name a wake: give at least four characters of its id"
        ));
    }
    if let Some(one) = wakes.iter().find(|(_, w)| w.id == n) {
        return Ok(one);
    }
    let ends: Vec<&(Execution, PendingWake)> =
        wakes.iter().filter(|(_, w)| w.id.ends_with(n)).collect();
    match ends.as_slice() {
        [one] => Ok(one),
        [] => Err(format!("no pending wake is named `{name}`")),
        many => Err(format!(
            "`{name}` names {} wakes ({}): give more of its id",
            many.len(),
            many.iter()
                .map(|(_, w)| crate::task::short(&w.id))
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

// ------------------------------------------------------------ time

/// `after`: whole numbers with units, such as `90s`, `10m`, `2h`, `1h30m`,
/// `1d`, or `10 minutes`. Returns milliseconds.
pub fn parse_after(s: &str) -> Result<u64, String> {
    let bad = || format!("`after` must be a duration such as 90s, 10m, 2h, or 1h30m, not `{s}`");
    let mut rest = s.trim();
    if rest.is_empty() {
        return Err(bad());
    }
    let mut total: u64 = 0;
    while !rest.is_empty() {
        let digits = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        if digits == 0 {
            return Err(bad());
        }
        let n: u64 = rest[..digits].parse().map_err(|_| bad())?;
        rest = rest[digits..].trim_start();
        let len = rest
            .find(|c: char| !c.is_ascii_alphabetic())
            .unwrap_or(rest.len());
        let unit = match rest[..len].to_ascii_lowercase().as_str() {
            "s" | "sec" | "secs" | "second" | "seconds" => 1_000,
            "m" | "min" | "mins" | "minute" | "minutes" => 60_000,
            "h" | "hr" | "hrs" | "hour" | "hours" => 3_600_000,
            "d" | "day" | "days" => 86_400_000,
            _ => return Err(bad()),
        };
        total = n
            .checked_mul(unit)
            .and_then(|ms| total.checked_add(ms))
            .ok_or_else(bad)?;
        rest = rest[len..].trim_start();
    }
    Ok(total)
}

/// `at`: an RFC 3339 time with its offset (`2026-09-30T15:00:00-07:00`,
/// `…Z`; seconds and a fraction optional). Returns Unix milliseconds.
pub fn parse_at(s: &str) -> Result<u64, String> {
    let bad = || {
        format!(
            "`at` must be an RFC 3339 time with its offset, such as 2026-09-30T15:00:00-07:00, \
             not `{s}`"
        )
    };
    let b = s.trim().as_bytes();
    let num = |from: usize, len: usize| -> Option<i64> {
        let d = b.get(from..from + len)?;
        d.iter()
            .all(u8::is_ascii_digit)
            .then(|| std::str::from_utf8(d).ok()?.parse().ok())
            .flatten()
    };
    let at = |i: usize, c: &[u8]| b.get(i).is_some_and(|x| c.contains(x));
    let (y, mo, d) = (num(0, 4), num(5, 2), num(8, 2));
    let (h, mi) = (num(11, 2), num(14, 2));
    let (Some(y), Some(mo), Some(d), Some(h), Some(mi)) = (y, mo, d, h, mi) else {
        return Err(bad());
    };
    if !(at(4, b"-") && at(7, b"-") && at(10, b"Tt ") && at(13, b":")) {
        return Err(bad());
    }
    let mut i = 16;
    let mut sec = 0;
    let mut millis = 0;
    if at(i, b":") {
        sec = num(i + 1, 2).ok_or_else(bad)?;
        i += 3;
        if at(i, b".") {
            i += 1;
            let start = i;
            while b.get(i).is_some_and(u8::is_ascii_digit) {
                if i - start < 3 {
                    millis = millis * 10 + i64::from(b[i] - b'0');
                }
                i += 1;
            }
            if i == start {
                return Err(bad());
            }
            for _ in (i - start)..3 {
                millis *= 10;
            }
        }
    }
    let offset_min = if at(i, b"Zz") {
        i += 1;
        0
    } else if at(i, b"+-") {
        let sign = if b[i] == b'-' { -1 } else { 1 };
        let oh = num(i + 1, 2).ok_or_else(bad)?;
        let (om, len) = if at(i + 3, b":") {
            (num(i + 4, 2).ok_or_else(bad)?, 6)
        } else {
            (num(i + 3, 2).ok_or_else(bad)?, 5)
        };
        i += len;
        sign * (oh * 60 + om)
    } else {
        return Err(bad());
    };
    if i != b.len()
        || !(1..=12).contains(&mo)
        || d < 1
        || d > days_in_month(y, mo)
        || h > 23
        || mi > 59
        || sec > 60
    {
        return Err(bad());
    }
    let days = days_from_civil(y, mo as u32, d as u32);
    let secs = days * 86_400 + h * 3_600 + mi * 60 + sec.min(59) - offset_min * 60;
    if secs < 0 {
        return Err(bad());
    }
    Ok(secs as u64 * 1000 + millis as u64)
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

// Howard Hinnant's days-from-civil, days since 1970-01-01.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// A time on the daemon's clock, in its time zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Local {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    /// East of UTC.
    pub offset_secs: i64,
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

impl Local {
    /// `13:05`.
    pub fn hm(&self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }
    /// `13:05:07`.
    pub fn hms(&self) -> String {
        format!("{:02}:{:02}:{:02}", self.hour, self.minute, self.second)
    }
    /// `13:05:07` on the same day as `other`, else `Sep 30 13:05:07`.
    pub fn hms_on(&self, other: &Local) -> String {
        if (self.year, self.month, self.day) == (other.year, other.month, other.day) {
            self.hms()
        } else {
            format!(
                "{} {} {}",
                MONTHS[(self.month.clamp(1, 12) - 1) as usize],
                self.day,
                self.hms()
            )
        }
    }
    /// `2026-09-30 13:05:00 -07:00`.
    pub fn full(&self) -> String {
        let sign = if self.offset_secs < 0 { '-' } else { '+' };
        let off = self.offset_secs.unsigned_abs();
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02} {sign}{:02}:{:02}",
            self.year,
            self.month,
            self.day,
            self.hour,
            self.minute,
            self.second,
            off / 3600,
            off % 3600 / 60
        )
    }
}

/// `unix_ms` in the daemon's time zone (the system's, as `localtime_r` reads
/// it), or UTC if that fails.
pub fn local(unix_ms: u64) -> Local {
    let t = (unix_ms / 1000) as libc::time_t;
    // SAFETY: `localtime_r` writes only the `tm` it is given, which lives
    // on this stack frame; a null return leaves it unread.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let ok = !unsafe { libc::localtime_r(&t, &mut tm) }.is_null();
    if !ok {
        let days = (unix_ms / 86_400_000) as i64;
        let (y, m, d) = civil_from_days(days);
        let s = unix_ms / 1000 % 86_400;
        return Local {
            year: y as i32,
            month: m,
            day: d,
            hour: (s / 3600) as u32,
            minute: (s / 60 % 60) as u32,
            second: (s % 60) as u32,
            offset_secs: 0,
        };
    }
    Local {
        year: tm.tm_year + 1900,
        month: (tm.tm_mon + 1) as u32,
        day: tm.tm_mday as u32,
        hour: tm.tm_hour as u32,
        minute: tm.tm_min as u32,
        second: tm.tm_sec as u32,
        offset_secs: tm.tm_gmtoff,
    }
}

// Hinnant's civil-from-days, the inverse of `days_from_civil`.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// A span as people say it: `45 s`, `10 min`, `1 min 30 s`, `2 h 5 min`,
/// `3 d 4 h`.
pub fn span(ms: u64) -> String {
    let s = ms.div_ceil(1000);
    match s {
        0..=59 => format!("{s} s"),
        60..=599 if !s.is_multiple_of(60) => format!("{} min {} s", s / 60, s % 60),
        60..=3599 => format!("{} min", (s + 30) / 60),
        3600..=86_399 => {
            let m = s % 3600 / 60;
            if m == 0 {
                format!("{} h", s / 3600)
            } else {
                format!("{} h {m} min", s / 3600)
            }
        }
        _ => {
            let h = s % 86_400 / 3600;
            if h == 0 {
                format!("{} d", s / 86_400)
            } else {
                format!("{} d {h} h", s / 86_400)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_parse_in_their_units() {
        assert_eq!(parse_after("2s"), Ok(2_000));
        assert_eq!(parse_after("10m"), Ok(600_000));
        assert_eq!(parse_after("2h"), Ok(7_200_000));
        assert_eq!(parse_after("1h30m"), Ok(5_400_000));
        assert_eq!(parse_after(" 1d 2h "), Ok(93_600_000));
        assert_eq!(parse_after("10 minutes"), Ok(600_000));
        assert_eq!(parse_after("90 SEC"), Ok(90_000));
        for bad in [
            "",
            "10",
            "m",
            "ten minutes",
            "10x",
            "1.5h",
            "-5m",
            "99999999999999999999s",
        ] {
            let e = parse_after(bad).unwrap_err();
            assert!(e.contains("such as 90s"), "{bad}: {e}");
        }
    }

    #[test]
    fn rfc3339_times_parse_with_their_offsets() {
        let z = parse_at("2026-09-30T20:05:00Z").unwrap();
        assert_eq!(parse_at("2026-09-30T13:05:00-07:00"), Ok(z));
        assert_eq!(
            parse_at("2026-09-30 13:05-07:00"),
            Ok(z),
            "a space, no seconds"
        );
        assert_eq!(parse_at("2026-09-30t22:05:00+0200"), Ok(z));
        assert_eq!(parse_at("2026-09-30T20:05:00.250Z"), Ok(z + 250));
        assert_eq!(parse_at("1970-01-01T00:00:01Z"), Ok(1_000));
        // 2026-09-30 is day 20726 since the epoch.
        assert_eq!(z, (20_726 * 86_400 + 20 * 3_600 + 5 * 60) * 1000);
        for bad in [
            "2026-09-30T13:05:00",
            "2026-09-30",
            "13:05",
            "2026-02-30T00:00:00Z",
            "2026-13-01T00:00:00Z",
            "2026-09-30T24:00:00Z",
            "2026-09-30T13:05:00Zjunk",
            "tomorrow at noon",
        ] {
            let e = parse_at(bad).unwrap_err();
            assert!(e.contains("RFC 3339"), "{bad}: {e}");
        }
    }

    #[test]
    fn civil_days_round_trip() {
        for d in [-1, 0, 1, 11_017, 20_361, 60_000] {
            let (y, m, day) = civil_from_days(d);
            assert_eq!(days_from_civil(y, m, day), d);
        }
    }

    #[test]
    fn spans_read_as_people_say_them() {
        assert_eq!(span(0), "0 s");
        assert_eq!(span(2_000), "2 s");
        assert_eq!(span(90_000), "1 min 30 s");
        assert_eq!(span(600_000), "10 min");
        assert_eq!(span(599_400), "10 min");
        assert_eq!(span(7_500_000), "2 h 5 min");
        assert_eq!(span(3 * 86_400_000 + 4 * 3_600_000), "3 d 4 h");
    }

    #[test]
    fn the_input_takes_one_of_at_and_after_and_a_note() {
        let now = parse_at("2026-09-30T20:05:00Z").unwrap();
        let due = |v: Value| input_of(&v).and_then(|i| due_of(&i, now));
        assert_eq!(due(json!({"after": "10m", "note": "n"})), Ok(now + 600_000));
        assert_eq!(
            due(json!({"at": "2026-09-30T13:15:00-07:00", "note": "n"})),
            Ok(now + 600_000)
        );
        let err = |v: Value| due(v).unwrap_err();
        assert!(err(json!({"note": "n"})).contains("give `after`"));
        assert!(err(json!({"after": "1m", "at": "x", "note": "n"})).contains("not both"));
        assert!(err(json!({"after": "1m", "note": " "})).contains("note is empty"));
        assert!(err(json!({"after": "1m", "note": "x".repeat(2_001)})).contains("2001 characters"));
        assert!(err(json!({"after": "31d", "note": "n"})).contains("at most 30 days"));
        assert!(err(json!({"after": "0s", "note": "n"})).contains("at least 1 s"));
        let past = err(json!({"at": "2026-09-30T13:00:00-07:00", "note": "n"}));
        assert!(past.contains("is 5 min ago"), "{past}");
        assert!(err(json!({"after": "1m", "note": "n", "when": 1})).contains("when"));
    }

    #[test]
    fn a_late_wake_says_when_it_was_due_and_why() {
        let set_at = parse_at("2026-09-30T20:05:00Z").unwrap();
        let w = PendingWake {
            id: "wak_x".into(),
            due_at_ms: set_at + 60_000,
            note: "check the build".into(),
            set_at_ms: set_at,
            by: "act_x".into(),
            target: None,
            repeat: None,
            occurrence: 0,
        };
        let set = local(set_at).hm();
        let on_time = FiredWake {
            wake: w.clone(),
            late_ms: 300,
            while_down: false,
            missed: 0,
            next_due_at_ms: None,
        };
        assert_eq!(
            fired_text(&on_time),
            format!("⏰ wake (set {set}): check the build")
        );
        let late = FiredWake {
            wake: w.clone(),
            late_ms: 36_000,
            while_down: true,
            missed: 0,
            next_due_at_ms: None,
        };
        let due = local(set_at + 60_000).hms();
        assert_eq!(
            fired_text(&late),
            format!(
                "⏰ wake (set {set}, due {due}, 36 s late: the daemon was not running then): \
                 check the build"
            )
        );
        let busy = FiredWake {
            wake: w,
            late_ms: 90_000,
            while_down: false,
            missed: 0,
            next_due_at_ms: None,
        };
        assert!(fired_text(&busy).contains("1 min 30 s late):"));
    }

    #[test]
    fn local_time_reads_the_same_instant() {
        let ms = parse_at("2026-09-30T20:05:07Z").unwrap();
        let l = local(ms);
        let back = days_from_civil(l.year as i64, l.month, l.day) * 86_400
            + i64::from(l.hour) * 3_600
            + i64::from(l.minute) * 60
            + i64::from(l.second)
            - l.offset_secs;
        assert_eq!(back as u64 * 1000, ms);
        assert!(l.full().ends_with(&format!(
            "{}{:02}:{:02}",
            if l.offset_secs < 0 { '-' } else { '+' },
            l.offset_secs.unsigned_abs() / 3600,
            l.offset_secs.unsigned_abs() % 3600 / 60
        )));
    }

    #[test]
    fn a_wake_is_named_by_the_end_of_its_id() {
        let e: Execution = serde_json::from_value(json!({
            "id": "exe_1", "schema": 2, "session_id": "ses_1", "kind": "conversation",
            "state": "waiting", "authority": {"principal": "operator"},
            "budget": {"limit_micros": 1, "spent_micros": 0, "reserved_micros": 0, "held_unknown_micros": 0},
            "turns": 0, "interrupted": 0, "created_at_ms": 0, "updated_at_ms": 0
        }))
        .unwrap();
        let w = |id: &str| PendingWake {
            id: id.into(),
            due_at_ms: 0,
            note: "n".into(),
            set_at_ms: 0,
            by: "act".into(),
            target: None,
            repeat: None,
            occurrence: 0,
        };
        let all = vec![
            (e.clone(), w("wak_0199aaaa1111")),
            (e, w("wak_0199bbbb1111")),
        ];
        assert_eq!(resolve(&all, "aaaa1111").unwrap().1.id, "wak_0199aaaa1111");
        assert_eq!(
            resolve(&all, "wak_0199bbbb1111").unwrap().1.id,
            "wak_0199bbbb1111"
        );
        assert_eq!(resolve(&all, " …bb1111").unwrap().1.id, "wak_0199bbbb1111");
        assert!(resolve(&all, "a11").unwrap_err().contains("too short"));
        assert!(resolve(&all, "bbb1111x")
            .unwrap_err()
            .contains("no pending wake"));
        let two = resolve(&all, "1111").unwrap_err();
        assert!(two.contains("names 2 wakes (aa1111, bb1111)"), "{two}");
    }

    /// `every`, `days`, and `until` (37a): the first time is `at` or
    /// `after`, or one span from now in the zone; the floor, the year, the
    /// days' daily span, and an `until` before the first time are refused.
    #[test]
    fn the_input_takes_a_series_and_refuses_what_cannot_run() {
        let ny = TimeZone::posix("EST5EDT,M3.2.0,M11.1.0").unwrap();
        // Saturday 2026-10-03, 16:05 EDT.
        let now = parse_at("2026-10-03T20:05:00Z").unwrap();
        let floor = 5 * 60_000;
        let plan = |v: Value| input_of(&v).and_then(|i| plan_of(&i, now, &ny, floor));
        let (first, r) = plan(json!({"every": "1d", "note": "n"})).unwrap();
        assert_eq!(first, now + 86_400_000, "one day from now");
        assert_eq!(r.unwrap().every.to_string(), "1d");
        let (first, r) = plan(
            json!({"after": "10m", "every": "30m", "until": "2026-10-04T00:00:00Z", "note": "n"}),
        )
        .unwrap();
        let r = r.unwrap();
        assert_eq!((first, r.first_ms), (now + 600_000, now + 600_000));
        assert_eq!(r.until_ms, Some(parse_at("2026-10-04T00:00:00Z").unwrap()));
        // A weekday series set on a Saturday starts on Monday.
        let (first, r) = plan(json!({"at": "2026-10-03T21:00:00-04:00", "every": "1d",
                                     "days": ["mon", "Tuesday", "fri", "mon"], "note": "n"}))
        .unwrap();
        assert_eq!(first, parse_at("2026-10-05T21:00:00-04:00").unwrap());
        assert_eq!(r.unwrap().days, vec![Day::Mon, Day::Tue, Day::Fri]);
        let (_, none) = plan(json!({"after": "1m", "note": "n"})).unwrap();
        assert!(none.is_none(), "no every: one-shot");

        let err = |v: Value| plan(v).unwrap_err();
        assert!(err(json!({"every": "4m", "note": "n"})).contains("every 5 min at the most often"));
        assert!(err(json!({"every": "53w", "note": "n"})).contains("365 days at the longest"));
        assert!(err(json!({"every": "1x", "note": "n"})).contains("such as 30m"));
        assert!(
            err(json!({"every": "1w", "days": ["mon"], "note": "n"})).contains("limits a daily")
        );
        assert!(err(json!({"every": "1d", "days": ["someday"], "note": "n"})).contains("weekdays"));
        assert!(err(json!({"every": "1d", "days": [], "note": "n"})).contains("is empty"));
        assert!(
            err(json!({"after": "1m", "until": "2026-10-04T00:00:00Z", "note": "n"}))
                .contains("give `every` too")
        );
        assert!(err(
            json!({"after": "1h", "every": "1d", "until": "2026-10-03T20:30:00Z", "note": "n"})
        )
        .contains("comes before the series' first time"));
        assert!(
            err(json!({"every": "1d", "until": "tomorrow", "note": "n"}))
                .contains("`until` must be")
        );
        assert!(
            err(json!({"after": "31d", "every": "1d", "note": "n"})).contains("at most 30 days")
        );
        assert!(repeats(&json!({"every": "1d", "note": "n"})));
        assert!(!repeats(&json!({"after": "1d", "note": "n"})));
        assert!(!repeats(
            &json!({"after": "1d", "every": null, "note": "n"})
        ));
    }

    /// A repeating wake's node says its span and occurrence, and what it
    /// passed over and why, or how late it ran (37a).
    #[test]
    fn a_repeating_wakes_line_says_its_occurrence_and_what_it_missed() {
        let set_at = parse_at("2026-09-30T20:05:00Z").unwrap();
        let w = PendingWake {
            id: "wak_x".into(),
            due_at_ms: set_at + 60_000,
            note: "write me a haiku".into(),
            set_at_ms: set_at,
            by: "act_x".into(),
            target: None,
            repeat: Some(Repeat {
                every: Every::parse("1d").unwrap(),
                first_ms: set_at + 60_000,
                days: vec![],
                until_ms: None,
            }),
            occurrence: 4,
        };
        let fired = |late_ms: u64, while_down: bool, missed: u64| FiredWake {
            wake: w.clone(),
            late_ms,
            while_down,
            missed,
            next_due_at_ms: Some(set_at + 86_460_000),
        };
        assert_eq!(
            fired_text(&fired(300, false, 0)),
            "⏰ wake (every 1d, #4): write me a haiku"
        );
        assert_eq!(
            fired_text(&fired(2 * 86_400_000, true, 2)),
            "⏰ wake (every 1d, #4; 2 missed while the daemon was down): write me a haiku"
        );
        assert_eq!(
            fired_text(&fired(2 * 86_400_000, false, 2)),
            "⏰ wake (every 1d, #4; 2 missed): write me a haiku"
        );
        let due = local(set_at + 60_000).hms();
        assert_eq!(
            fired_text(&fired(36_000, true, 0)),
            format!(
                "⏰ wake (every 1d, #4, due {due}, 36 s late: the daemon was not running then): \
                 write me a haiku"
            )
        );
        let r = w.repeat.clone().unwrap();
        assert_eq!(series_of(&r), "every 1d");
        let weekdays = Repeat {
            days: vec![Day::Mon, Day::Fri],
            until_ms: Some(set_at),
            ..r
        };
        assert_eq!(
            series_of(&weekdays),
            format!("every 1d on mon, fri until {}", local(set_at).full())
        );
    }
}
