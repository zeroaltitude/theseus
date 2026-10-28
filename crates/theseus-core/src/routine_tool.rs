//! `routine.create`: the model sets up recurring work (theseus-pvm5c; the
//! owner's decision D-H3: yes, behind one card). A routine spends money on a
//! schedule with no one there, so the call always waits for the owner's yes:
//! its plan names the authority it needs (`Plan::authority`), which asks at
//! every posture as a floor call does, and its card reads
//!
//! `Create routine "Morning digest": weekdays at 08:00 (next: Fri 08:00,
//! Mon 08:00, Tue 08:00) · up to $0.50 a run, $10 a month · posts to your DM`
//!
//! Approved, the harness makes the routine (`routine::create`, the same
//! function the owner's `routine.create` verb calls) in the conversation's
//! name: its commitments block lists it. A run asks before it uses a tool, as
//! any task does, except the tools in its `grant` (theseus-52tut): those run
//! unasked, even after the run reads outside text, and the card names them.
//!
//! Private places only, like the other tools that change what the model
//! committed to (`places::public_tool` does not list it).

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_kernel::TimeZone;
use theseus_tools::{parse, Backend, Plan, Retry, Tool, ToolClass, ToolCtx};

use crate::config::RoutinesConfig;
use crate::routine::{create, prepare, usd_ok, NewRoutine};
use crate::toolrun::TurnCtx;

pub const CREATE: &str = "routine.create";
/// Why it always asks: the authority its plan names (`policy.explain` probes with it too).
pub const ASKS: &str = "a routine spends money on a schedule, so it always waits for your yes";
/// The tools this module adds, for the template's `[policy.tools]` list.
pub const NAMES: [&str; 1] = [CREATE];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    title: String,
    schedule: String,
    brief: String,
    #[serde(default)]
    budget_usd: Option<f64>,
    #[serde(default)]
    month_usd: Option<f64>,
    #[serde(default)]
    deliver_to: Option<String>,
    #[serde(default)]
    speak: Option<String>,
    #[serde(default)]
    start: Option<String>,
    #[serde(default)]
    stop: Option<String>,
    #[serde(default)]
    carry: Option<String>,
    #[serde(default)]
    grant: Vec<String>,
}

impl Input {
    fn new_routine(self) -> NewRoutine {
        NewRoutine {
            title: self.title,
            schedule: self.schedule,
            brief: self.brief,
            budget_usd: self.budget_usd,
            month_usd: self.month_usd,
            deliver_to: self.deliver_to,
            catch_up: None,
            speak: self.speak,
            start: self.start,
            stop: self.stop,
            carry: self.carry,
            grant: self.grant,
        }
    }
}

/// The tool: its plan words the card, so it holds the zone and the defaults.
pub struct RoutineCreate {
    pub zone: TimeZone,
    pub cfg: RoutinesConfig,
    pub floor_ms: u64,
    /// How far ahead a start or a stop may be, as a wake's bound.
    pub max_ahead_ms: u64,
}

impl Tool for RoutineCreate {
    fn name(&self) -> &'static str {
        CREATE
    }

    fn description(&self) -> &'static str {
        "Set up recurring, deferred or standing work. Reach for it on the shape of what is \
         asked, not on any one word: watch this, keep an eye on that, remind me, check it \
         hourly, every morning tell me, at 3 today, each Monday until Friday, keep me posted. A \
         routine runs `brief` on `schedule` (`weekdays 08:00`, `every 30m`, `monthly 09:00`, or \
         cron), each run a fresh task that does not see this conversation, reporting to the \
         owner's DM. `start` moves the first run (`mon 9:00`, `in 2h`); `stop` ends the series \
         (`after 3 runs`, or a time such as `fri 17:00`), and with no stop it runs until the \
         owner pauses it. Each run is given the whole of the last run's report, so a brief that \
         says what to look at reports what changed rather than the state again (`carry: none` \
         turns that off). It waits for the owner's yes on one card. A run may spend `budget_usd` \
         (default $0.50), the routine `month_usd` a month (default $10); a spent month skips \
         runs. To wait on one thing a command can check, with no model and no money, use `watch`."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "title": {"type": "string"},
                "schedule": {"type": "string"},
                "brief": {"type": "string", "description": "The complete instruction each run follows."},
                "budget_usd": {"type": "number", "exclusiveMinimum": 0},
                "month_usd": {"type": "number", "exclusiveMinimum": 0},
                "deliver_to": {"type": "string", "description": "`dm` (default), `none`, or a place."},
                "speak": {"type": "string", "enum": ["always", "when_notable"], "description": "`when_notable`: a report ending NOTHING NEW is not posted."},
                "start": {"type": "string", "description": "The first run is at or after this time (`mon 9:00`, `in 2h`, `2026-11-03 08:00`). Without one it is the schedule's next time."},
                "stop": {"type": "string", "description": "Where the series ends: `after 3 runs`, or a time (`fri 17:00`). Without one it runs until the owner pauses it."},
                "carry": {"type": "string", "enum": ["last_report", "none"], "description": "What each run is given of the one before it. `last_report` (default): the whole of its report, so this run reports the change."},
                "grant": {"type": "array", "items": {"type": "string"}}
            },
            "required": ["title", "schedule", "brief"],
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
        // Run again after a crash it would make a second routine.
        Retry::NonRepeatable
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let i: Input = parse(input)?;
        let mut new = i.new_routine();
        // A grant that cannot be read, or names what no grant may, fails
        // here, before a card is raised; the tools it names exist is checked
        // when the owner's yes lets the call run.
        new.grant = crate::routine::grant::check(&new.grant, &|_| true)?;
        // A schedule, a start or a stop that does not read fails here, before a
        // card is raised; the card then says where the series ends, which is
        // what the owner's yes is really about (theseus-97b8c).
        let now = theseus_protocol::now_unix_ms();
        let ahead = self.max_ahead_ms;
        let start_ms = crate::routine::start_of(&new, &self.zone, now, ahead)?;
        let mut parsed = theseus_kernel::cron::parse_schedule(
            &new.schedule,
            &self.zone,
            start_ms.unwrap_or(now),
            self.floor_ms,
        )?;
        let bounds = crate::routine::Bounds {
            start_ms,
            stop: crate::routine::stop_of(&new, &self.zone, now, ahead, parsed.first_ms)?,
        };
        parsed.repeat.until_ms = bounds.stop.as_ref().and_then(|s| s.at_ms);
        if let Some(c) = &new.carry {
            theseus_protocol::routines::Carry::parse(c).ok_or("carry is last_report or none")?;
        }
        let next = crate::routine::occurrences(&parsed, &self.zone, 3);
        let run = new.budget_usd.unwrap_or(self.cfg.default_run_usd);
        let month = new.month_usd.unwrap_or(self.cfg.default_month_usd);
        if !(usd_ok(run) && usd_ok(month)) || run > month {
            return Err(format!(
                "a run's budget (${run}) must be above 0 and no more than the month's (${month})"
            ));
        }
        let card = crate::routine::Prepared::words(
            &new,
            &parsed.repeat,
            &next,
            run,
            month,
            &self.zone,
            &bounds,
        );
        Ok(Plan {
            summary: card,
            authority: Some(ASKS.into()),
            ..Default::default()
        })
    }
}

/// Run `routine.create` (the harness's side), once the owner said yes.
pub fn run(
    tc: &TurnCtx<'_>,
    book: &crate::routine::Book,
    input: &Value,
    known: &dyn Fn(&str) -> bool,
) -> Result<(String, Value), String> {
    // A task may propose one: the owner's yes on its card is the gate, as for a
    // conversation's (the owner, 2026-10-25 13:58: "precious few hard gates").
    if let Some(why) = crate::places::refusal(tc.class, CREATE, &Plan::default(), &[]) {
        return Err(format!("Refused: {why}."));
    }
    let i: Input = parse(input)?;
    let by = format!("model:{}", tc.session_id);
    let mut new = i.new_routine();
    new.grant = crate::routine::grant::check(&new.grant, known)?;
    let prepared = prepare(tc.kernel, &book.cfg, &new, &by, Some(tc.session_id))?;
    let spec = create(tc.kernel, tc.store, book, prepared, &by).map_err(|e| format!("{e:#}"))?;
    let zone = &tc.kernel.config().zone;
    let first = tc
        .kernel
        .execution(&spec.execution_id)
        .ok()
        .flatten()
        .and_then(|e| {
            e.wakes
                .first()
                .map(|w| theseus_kernel::cron::when(zone, w.due_at_ms))
        })
        .unwrap_or_default();
    Ok((
        format!(
            "Created routine {} (\"{}\"), id {}: {}. Its first run is {first}. Each run is a \
             fresh task with up to ${:.2}, from a month of ${:.2}; the owner can pause it with \
             `theseus routine pause {}`; `theseus routine try {}` runs it once now, while \
             they watch.",
            crate::routine::short(&spec.session_id),
            spec.title,
            spec.session_id,
            spec.schedule,
            theseus_kernel::micros_to_usd(spec.run_micros),
            theseus_kernel::micros_to_usd(spec.month_micros),
            crate::routine::short(&spec.session_id),
            crate::routine::short(&spec.session_id),
        ),
        json!({"routine": spec.session_id, "short": crate::routine::short(&spec.session_id),
               "execution_id": spec.execution_id, "wake_id": spec.wake_id}),
    ))
}
