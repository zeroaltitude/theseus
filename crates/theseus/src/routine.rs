//! `theseus routine …` (theseus-pvm5c): recurring work, each run a fresh task
//! with its own budget, from a month of its own. `list` (the default) and `runs`
//! read; `add`, `pause`, `resume`, `run`, `update` and `rm` are the owner's acts
//! (refused inside a job, `client::OPERATORS`).
//!
//! `theseus routine add "weekdays 08:00" "Summarize the overnight alerts" --budget 0.50 --to dm`

use anyhow::Result;
use clap::Subcommand;
use theseus_client::render::postmortem::usd;
use theseus_client::Conn;
use theseus_protocol::method;
use theseus_protocol::routines::{
    RoutineCreateParams, RoutineEditResult, RoutineInfo, RoutineListResult, RoutineNameParams,
    RoutinePauseParams, RoutineRunsParams, RoutineRunsResult, RoutineUpdateParams, RunRow,
};

use crate::cmd::output;

#[derive(Subcommand, Debug, Default)]
pub enum RoutineCmd {
    /// Every routine: when it runs next, what a run and a month may cost, and the last run.
    #[default]
    List,
    /// Make a routine. SCHEDULE is `weekdays 08:00`, `daily 7:30am`, `mon,thu 09:00`, `every 30m`,
    /// `monthly 09:00`, `the 1st 09:00`, or a cron expression, in the daemon's zone. Each run is a
    /// fresh task with the BRIEF and its own budget, carved from the month's.
    Add {
        schedule: String,
        /// What each run does: the whole instruction.
        #[arg(required = true)]
        brief: Vec<String>,
        /// What people call it (default: the brief's first words).
        #[arg(long)]
        title: Option<String>,
        /// The most one run may spend, in dollars (default `[routines] default_run_usd`, 0.50).
        #[arg(long)]
        budget: Option<f64>,
        /// The routine's month, in dollars, reset on the 1st (default 10).
        #[arg(long)]
        month: Option<f64>,
        /// Where reports go: `dm` (default), `none` (log only), or a place (`discord:channel:<id>`).
        #[arg(long)]
        to: Option<String>,
        /// After downtime: `one` (run once, saying how many were missed), `none`, or `auto`
        /// (default: `one` for spans of an hour or more).
        #[arg(long)]
        catch_up: Option<String>,
        /// `always` (default), or `when-notable`: a report ending in NOTHING NEW only logs.
        #[arg(long)]
        speak: Option<String>,
        /// The first run is at or after this time (`mon 9:00`, `in 2h`, `2026-11-03 08:00`);
        /// without it, the schedule's next time.
        #[arg(long)]
        start: Option<String>,
        /// Where the series ends: `after 3 runs`, or a time (`fri 17:00`). Without it the
        /// routine runs until it is paused.
        #[arg(long)]
        stop: Option<String>,
        /// What each run is given of the one before it: `last-report` (default), the whole of
        /// its report so this run reports the change, or `none`.
        #[arg(long)]
        carry: Option<String>,
        /// The tools its runs use without asking, comma-separated
        /// (`gog.gmail.search,web.fetch`; a post names its place: `"channel.post #digest"`). Any
        /// other call asks, and a run nobody answers by its deadline ends blocked, naming it.
        #[arg(long, value_delimiter = ',')]
        grant: Vec<String>,
    },
    /// A routine's runs, newest first: state, start, duration, cost, and the report's first line.
    Runs {
        id: String,
        #[arg(long, short = 'n')]
        limit: Option<u32>,
    },
    /// Hold a routine, or with `--until` (`mon`, `tomorrow 9:00`) skip to then.
    Pause {
        id: String,
        #[arg(long)]
        until: Option<String>,
    },
    /// Let a paused routine go on at its next time.
    Resume { id: String },
    /// Drop the next occurrence and go on at the one after it; the series keeps running.
    Skip { id: String },
    /// Run it once now, whether paused or not; the schedule is untouched.
    Run { id: String },
    /// Try it now: occurrence 0 at once, recorded as run 0, while you watch. A call outside the
    /// grant asks you here and now, so a grant that is too short shows today and not at 08:00.
    Try { id: String },
    /// Change what is given: schedule, brief, budget, month, destination, catch-up, speak, title.
    Update {
        id: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        schedule: Option<String>,
        #[arg(long)]
        brief: Option<String>,
        #[arg(long)]
        budget: Option<f64>,
        #[arg(long)]
        month: Option<f64>,
        #[arg(long)]
        to: Option<String>,
        #[arg(long)]
        catch_up: Option<String>,
        #[arg(long)]
        speak: Option<String>,
        /// Replace the grant (comma-separated tools).
        #[arg(long, value_delimiter = ',')]
        grant: Option<Vec<String>>,
        /// Add tools to the grant (comma-separated).
        #[arg(long, value_delimiter = ',')]
        add_grant: Vec<String>,
    },
    /// Delete a routine: its schedule and any run going end.
    Rm { id: String },
}

/// `routine skip`: the next run is dropped, the series goes on after it.
async fn skip(conn: &mut Conn, json: bool, id: String) -> Result<()> {
    let v = conn.request(method::ROUTINE_SKIP, named(id)).await?;
    output(json, v, |r: RoutineEditResult| {
        println!(
            "routine {} skips its next run: next {}",
            r.routine.short,
            next_of(&r.routine)
        );
        Ok(())
    })
}

/// A title from a brief's first words.
fn title_of(brief: &str) -> String {
    let first = brief
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim();
    let mut t: String = first.chars().take(48).collect();
    if first.chars().count() > 48 {
        t.push('…');
    }
    t
}

fn named(id: String) -> RoutineNameParams {
    RoutineNameParams {
        id,
        ..Default::default()
    }
}

pub async fn run(conn: &mut Conn, json: bool, cmd: RoutineCmd) -> Result<()> {
    match cmd {
        RoutineCmd::List => {
            let v = conn
                .request(method::ROUTINE_LIST, serde_json::json!({}))
                .await?;
            output(json, v, |r: RoutineListResult| {
                print_all(&list_lines(&r.routines));
                Ok(())
            })
        }
        RoutineCmd::Runs { id, limit } => {
            let v = conn
                .request(method::ROUTINE_RUNS, RoutineRunsParams { id, limit })
                .await?;
            output(json, v, |r: RoutineRunsResult| {
                print_all(&list_lines(std::slice::from_ref(&r.routine)));
                if r.runs.is_empty() {
                    println!("  no run yet");
                }
                for l in run_lines(&r.runs) {
                    println!("  {l}");
                }
                Ok(())
            })
        }
        RoutineCmd::Add { .. } | RoutineCmd::Update { .. } => write(conn, json, cmd).await,
        RoutineCmd::Pause { id, until } => {
            let p = RoutinePauseParams {
                id,
                until,
                ..Default::default()
            };
            let v = conn.request(method::ROUTINE_PAUSE, p).await?;
            output(json, v, |r: RoutineEditResult| {
                let i = &r.routine;
                match i.paused {
                    true => println!(
                        "routine {} paused: it keeps its place; `theseus routine resume {}`",
                        i.short, i.short
                    ),
                    false => println!("routine {} will run next {}", i.short, next_of(i)),
                }
                Ok(())
            })
        }
        RoutineCmd::Resume { id } => {
            let v = conn.request(method::ROUTINE_RESUME, named(id)).await?;
            output(json, v, |r: RoutineEditResult| {
                println!(
                    "routine {} goes on: next {}",
                    r.routine.short,
                    next_of(&r.routine)
                );
                Ok(())
            })
        }
        RoutineCmd::Skip { id } => skip(conn, json, id).await,
        RoutineCmd::Run { id } => {
            let v = conn.request(method::ROUTINE_RUN_NOW, named(id)).await?;
            output(json, v, |r: RoutineEditResult| {
                let task = r
                    .task
                    .as_deref()
                    .map_or("-", |t| &t[t.len().saturating_sub(6)..]);
                println!(
                    "routine {} runs now as task {task}; the series is as it was",
                    r.routine.short
                );
                Ok(())
            })
        }
        RoutineCmd::Try { id } => {
            let v = conn.request(method::ROUTINE_TRY, named(id)).await?;
            output(json, v, |r: RoutineEditResult| {
                let task = r
                    .task
                    .as_deref()
                    .map_or("-", |t| &t[t.len().saturating_sub(6)..]);
                println!(
                    "routine {} tries now as run 0, task {task}; a call outside its grant asks \
                     here (`theseus answer`, `theseus tasks`); the series is as it was",
                    r.routine.short
                );
                Ok(())
            })
        }
        RoutineCmd::Rm { id } => {
            let v = conn.request(method::ROUTINE_DELETE, named(id)).await?;
            output(json, v, |r: RoutineEditResult| {
                println!("routine {} ({}) deleted", r.routine.short, r.routine.title);
                Ok(())
            })
        }
    }
}

fn print_all(lines: &[String]) {
    for l in lines {
        println!("{l}");
    }
}

/// `, stops after 3 runs` for a routine that ends, and nothing for one that
/// runs until it is paused (theseus-97b8c).
fn stop_of(i: &RoutineInfo) -> String {
    match (&i.stopped, &i.stop) {
        (Some(s), _) => format!(", stopped: {s}"),
        (None, Some(s)) => format!(", stops {s}"),
        (None, None) => String::new(),
    }
}

fn next_of(i: &RoutineInfo) -> &str {
    i.next.as_deref().unwrap_or("-")
}

/// `add` and `update`: the two that carry a routine's fields.
async fn write(conn: &mut Conn, json: bool, cmd: RoutineCmd) -> Result<()> {
    match cmd {
        RoutineCmd::Add {
            schedule,
            brief,
            title,
            budget,
            month,
            to,
            catch_up,
            speak,
            start,
            stop,
            carry,
            grant,
        } => {
            let brief = brief.join(" ");
            let p = RoutineCreateParams {
                title: title.unwrap_or_else(|| title_of(&brief)),
                schedule,
                brief,
                budget_usd: budget,
                month_usd: month,
                deliver_to: to,
                catch_up,
                speak: speak.map(|s| s.replace('-', "_")),
                start,
                stop,
                carry: carry.map(|c| c.replace('-', "_")),
                grant,
                ..Default::default()
            };
            let v = conn.request(method::ROUTINE_CREATE, p).await?;
            output(json, v, |r: RoutineEditResult| {
                let i = &r.routine;
                println!(
                    "routine {} ({}) made: {}, next {}{}; up to {} a run and {} a month; \
                     `theseus routine runs {}`",
                    i.short,
                    i.title,
                    i.series,
                    next_of(i),
                    stop_of(i),
                    usd(i.run_usd),
                    usd(i.month_usd),
                    i.short
                );
                Ok(())
            })
        }
        RoutineCmd::Update {
            id,
            title,
            schedule,
            brief,
            budget,
            month,
            to,
            catch_up,
            speak,
            grant,
            add_grant,
        } => {
            let p = RoutineUpdateParams {
                id,
                title,
                schedule,
                brief,
                budget_usd: budget,
                month_usd: month,
                deliver_to: to,
                catch_up,
                speak: speak.map(|s| s.replace('-', "_")),
                grant,
                add_grant,
                ..Default::default()
            };
            let v = conn.request(method::ROUTINE_UPDATE, p).await?;
            output(json, v, |r: RoutineEditResult| {
                print_all(&list_lines(std::slice::from_ref(&r.routine)));
                Ok(())
            })
        }
        _ => unreachable!("`run` sends only add and update here"),
    }
}

/// `a41d04 weekdays 08:00 Morning digest  next Fri 08:00 · $0.50/run · $1.50 of $10 this month · dm`
pub fn list_lines(routines: &[RoutineInfo]) -> Vec<String> {
    if routines.is_empty() {
        return vec![
            "no routines: `theseus routine add \"weekdays 08:00\" \"what each run does\"`".into(),
        ];
    }
    routines
        .iter()
        .map(|i| {
            let state = if i.paused {
                "paused".to_string()
            } else {
                format!("next {}", i.next.as_deref().unwrap_or("-"))
            };
            let going = if i.running { " · running" } else { "" };
            let streak = match i.streak {
                0 => String::new(),
                1 => " · ✗ the last run failed or was blocked".to_string(),
                n => format!(" · ✗ {n} runs in a row failed or were blocked"),
            };
            let grant = match i.grant.is_empty() {
                true => String::new(),
                false => format!(" · grant: {}", i.grant.join(", ")),
            };
            let last = match &i.last {
                Some(r) => format!(" · last: {} {}", r.state, r.report.as_deref().unwrap_or("")),
                None => String::new(),
            };
            format!(
                "{} {} {}  {state}{going} · {}/run · {} of {} this month · {} · {} run{}{streak}{grant}{}",
                i.short,
                i.series,
                i.title,
                usd(i.run_usd),
                usd(i.month_spent_usd),
                usd(i.month_usd),
                i.deliver_to,
                i.runs,
                if i.runs == 1 { "" } else { "s" },
                last.trim_end()
            )
        })
        .collect()
}

/// One line a run: `#22 complete 2026-10-14 08:00:00 (41 s) $0.07 digest ready`.
pub fn run_lines(runs: &[RunRow]) -> Vec<String> {
    runs.iter()
        .map(|r| {
            format!(
                "#{} {} {} ({}) {} {}",
                r.run,
                r.state,
                r.started_local,
                duration(r.duration_ms),
                usd(r.cost_usd),
                r.report.as_deref().unwrap_or("")
            )
            .trim_end()
            .to_string()
        })
        .collect()
}

fn duration(ms: u64) -> String {
    match ms {
        m if m < 1_000 => format!("{m} ms"),
        m if m < 60_000 => format!("{} s", m / 1_000),
        m => format!("{} min {} s", m / 60_000, (m % 60_000) / 1_000),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_list_shows_the_grant_and_the_streak() {
        let mut i = info();
        i.grant = vec!["gog.gmail.search".into(), "channel.post #digest".into()];
        i.streak = 2;
        let l = &list_lines(&[i])[0];
        assert!(l.contains("2 runs in a row failed or were blocked"), "{l}");
        assert!(
            l.contains("grant: gog.gmail.search, channel.post #digest"),
            "{l}"
        );
        let plain = &list_lines(&[info()])[0];
        assert!(
            !plain.contains("grant") && !plain.contains("in a row"),
            "{plain}"
        );
    }

    use super::*;

    fn info() -> RoutineInfo {
        RoutineInfo {
            id: "ses_01a41d04".into(),
            short: "a41d04".into(),
            title: "Morning digest".into(),
            schedule: "weekdays 08:00".into(),
            series: "weekdays 08:00".into(),
            brief: "Summarize".into(),
            paused: false,
            next_ms: Some(1),
            next: Some("Fri 08:00".into()),
            run_usd: 0.5,
            month_usd: 10.0,
            month_spent_usd: 1.5,
            deliver_to: "dm".into(),
            catch_up: "auto".into(),
            speak: "always".into(),
            carry: "last_report".into(),
            stop: None,
            stop_ms: None,
            stopped: None,
            start_ms: None,
            runs: 3,
            grant: vec![],
            streak: 0,
            running: false,
            last: Some(RunRow {
                task: "ses_x".into(),
                short: "x".into(),
                run: 3,
                state: "complete".into(),
                started_ms: 1,
                started_local: "2026-10-14 08:00:00".into(),
                duration_ms: 41_000,
                cost_usd: 0.07,
                report: Some("digest ready".into()),
                blocked: None,
            }),
        }
    }

    #[test]
    fn a_routine_is_one_line_and_a_run_is_one_line() {
        let l = list_lines(&[info()]);
        assert_eq!(
            l,
            ["a41d04 weekdays 08:00 Morning digest  next Fri 08:00 · $0.50/run · $1.50 of $10.00 this month · dm · 3 runs · last: complete digest ready"]
        );
        let r = run_lines(&[info().last.unwrap()]);
        assert_eq!(
            r,
            ["#3 complete 2026-10-14 08:00:00 (41 s) $0.07 digest ready"]
        );
        assert!(list_lines(&[])[0].starts_with("no routines"));
    }

    #[test]
    fn a_title_comes_from_the_briefs_first_words() {
        assert_eq!(
            title_of("Summarize the alerts\nthen more"),
            "Summarize the alerts"
        );
        assert!(title_of(&"x".repeat(80)).ends_with('…'));
    }
}
