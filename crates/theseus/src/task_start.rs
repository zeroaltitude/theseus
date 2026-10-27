//! `theseus task start` (theseus-7oq0w): start a task with no model turn. One
//! `task.start` call; the daemon opens the task in one frame and its first turn
//! runs on the next tick, or at the time asked. The task goes under your Home
//! conversation unless `--in` names another, and its budget is carved from there.
//! The method is yours alone (`client::OPERATORS`), refused inside a job.

use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use theseus_client::render::{task_history, task_tree};
use theseus_client::Conn;
use theseus_protocol::task_not_this::{TaskNotThisParams, TaskNotThisResult};
use theseus_protocol::task_pause::{TaskPauseParams, TaskPauseResult};
use theseus_protocol::task_start::{ReportTo, TaskStartParams, TaskStartResult};
use theseus_protocol::{
    method, LedgerTailParams, LedgerTailResult, TaskListParams, TaskListResult,
};

use crate::cmd::output;

#[derive(Subcommand, Debug)]
pub enum TaskCmd {
    /// Start a task now, or at a time, with no model turn: `theseus task start "Survey the north
    /// quay" --budget 2`. Prints the task's short id; `theseus tasks` shows it, `theseus watch ID`
    /// follows it. Only you may (refused inside a job).
    Start(StartArgs),
    /// One task whole: its state, where it reports, its brief and plan, what it has spent, when it
    /// started and ended, its children and its whole report, and the commands that act on it now.
    /// ID is its short id (the last six characters, or four or more of its id). `--json` is the
    /// record.
    Show {
        /// The task: its id, or the end of it (four characters at least).
        id: String,
    },
    /// Hold a running task at its next loop, so it takes no more turns until you resume it (a
    /// model call already going finishes). Yours alone (refused inside a job).
    Pause {
        /// The task: its id, or the end of it (four characters at least).
        id: String,
    },
    /// Let a held task go on. Yours alone (refused inside a job).
    Resume {
        /// The task: its id, or the end of it (four characters at least).
        id: String,
    },
    /// Say "not this" to a task: it is cancelled, one standing preference (the last one's replacement)
    /// tells Theseus not to start work like it on its own, and the offer that produced it is labelled
    /// wrong. `theseus task not-this a1b2c3 "too noisy"`. Yours alone (refused inside a job).
    NotThis {
        /// The task: its id, or the end of it (four characters at least).
        id: String,
        /// Why, in your words; it rides the preference.
        reason: Option<String>,
    },
    /// Send a task's report to another home, no model turn: a conversation, or `thread:<slug>`.
    /// Its budget stays carved from its original parent. Yours alone (refused inside a job).
    Rehome {
        /// The task: its id, or the end of it (four characters at least).
        id: String,
        /// The new home: a conversation (its id, or the end of it), or `thread:<slug>`.
        home: String,
    },
    /// Move a deferred task's start, no model turn: `--at fri 9:00` or `--in-time 2h`. Yours alone
    /// (refused inside a job).
    #[command(group(clap::ArgGroup::new("when").required(true).args(["at", "in_time"])))]
    Reschedule {
        /// The task: its id, or the end of it (four characters at least).
        id: String,
        /// Start then, as `task start --at` reads it.
        #[arg(long = "at", value_name = "WHEN")]
        at: Option<String>,
        /// Start after this long: `90s`, `2h`, `1d`.
        #[arg(long = "in-time", value_name = "SPAN")]
        in_time: Option<String>,
    },
}

/// `theseus task start`'s arguments.
#[derive(Args, Debug)]
pub struct StartArgs {
    /// The work the task does, as you would brief it.
    #[arg(required = true)]
    pub brief: Vec<String>,
    /// Its title (the brief's first line by default).
    #[arg(long)]
    pub title: Option<String>,
    /// The conversation it goes under, which its budget is carved from (your Home by default).
    #[arg(long = "in", value_name = "SESSION")]
    pub in_session: Option<String>,
    /// Its budget in dollars (a quarter of what the conversation has left by default).
    #[arg(long, value_name = "USD")]
    pub budget: Option<f64>,
    /// The profile its turns run on (routed from the brief by default).
    #[arg(long)]
    pub profile: Option<String>,
    /// Start then, asleep until it: `fri 9:00`, `tomorrow 9:00`, `17:30`, `2026-10-14 09:00`, or
    /// an RFC 3339 time, in the daemon's zone.
    #[arg(long = "at", value_name = "WHEN", conflicts_with = "in_time")]
    pub at: Option<String>,
    /// Start after this long: `90s`, `2h`, `1d`.
    #[arg(long = "in-time", value_name = "SPAN")]
    pub in_time: Option<String>,
    /// A file whose text the task reads verbatim (`-` is stdin); repeat for more. It is outside
    /// text: the task's calls that act wait for your approval, as after web text.
    #[arg(long = "quote-file", value_name = "FILE")]
    pub quote_files: Vec<String>,
    /// Where the report goes: `home` (the conversation's place; the default) or `board` (nobody is
    /// told; `theseus tasks` shows it).
    #[arg(long = "report-to", value_name = "home|board")]
    pub report_to: Option<String>,
}

/// `theseus task …`.
pub async fn run(conn: &mut Conn, json: bool, cmd: TaskCmd) -> Result<()> {
    match cmd {
        TaskCmd::Start(a) => start(conn, json, a).await,
        TaskCmd::Show { id } => show(conn, json, &id).await,
        TaskCmd::Pause { id } => hold(conn, json, &id, true).await,
        TaskCmd::Resume { id } => hold(conn, json, &id, false).await,
        TaskCmd::NotThis { id, reason } => not_this(conn, json, &id, reason).await,
        TaskCmd::Rehome { id, home } => crate::ops_cmd::rehome(conn, json, &id, &home).await,
        TaskCmd::Reschedule { id, at, in_time } => {
            crate::ops_cmd::reschedule(conn, json, &id, start_when(at, in_time)).await
        }
    }
}

/// `theseus task show ID`: one `task.list` read of the task by `id`.
async fn show(conn: &mut Conn, json: bool, id: &str) -> Result<()> {
    let p = TaskListParams {
        id: Some(id.to_string()),
        ..Default::default()
    };
    let v = conn.request(method::TASK_LIST, p).await?;
    let models = match json {
        true => Vec::new(),
        false => models_of(conn, &v).await,
    };
    output(json, v, |r: TaskListResult| {
        for l in task_history::show_with(&r, &models)? {
            println!("{l}");
        }
        Ok(())
    })
}

/// The task's model lines: where its brief put it and each move a checkpoint made, read from its
/// session's `route.decided` rows (one `ledger.tail`, asked only here). A task with no session of
/// its own, or an answer that does not come, has none: the lines are a help, never the point.
async fn models_of(conn: &mut Conn, task: &serde_json::Value) -> Vec<String> {
    let Some(session) = task["records"][0]["session"].as_str() else {
        return Vec::new();
    };
    let p = LedgerTailParams {
        n: Some(200),
        kind: Some("route.decided".into()),
        session_id: Some(session.to_string()),
        ..LedgerTailParams::default()
    };
    match conn.request(method::LEDGER_TAIL, p).await {
        Ok(v) => serde_json::from_value::<LedgerTailResult>(v)
            .map(|r| task_history::model_lines(&r.rows))
            .unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// `theseus task pause|resume ID`: `task.pause` or `task.resume`. A task that has ended is refused
/// with the daemon's words.
async fn hold(conn: &mut Conn, json: bool, id: &str, pause: bool) -> Result<()> {
    let m = if pause {
        method::TASK_PAUSE
    } else {
        method::TASK_RESUME
    };
    let p = TaskPauseParams {
        task: id.to_string(),
        author: None,
        discord: None,
    };
    let v = conn.request(m, p).await?;
    output(json, v, |r: TaskPauseResult| {
        println!("{}", hold_line(&r, id));
        Ok(())
    })
}

/// `theseus task not-this ID [REASON]`: `task.not_this`. A task that has ended is answered the same.
async fn not_this(conn: &mut Conn, json: bool, id: &str, reason: Option<String>) -> Result<()> {
    let p = TaskNotThisParams {
        task: id.to_string(),
        reason,
        author: None,
        discord: None,
    };
    let v = conn.request(method::TASK_NOT_THIS, p).await?;
    output(json, v, |r: TaskNotThisResult| {
        println!("{}", not_this_line(&r));
        Ok(())
    })
}

/// What `not-this` did, in words.
fn not_this_line(r: &TaskNotThisResult) -> String {
    let short = task_tree::short_id(&r.task.task_id);
    let mut line = format!(
        "Not this: {short} is {}. Preference {} will keep Theseus from starting work like it",
        r.task.state, r.preference.id
    );
    if let Some(old) = &r.replaced {
        line.push_str(&format!(" (it replaces {old})"));
    }
    line.push('.');
    if r.labeled.is_some() {
        line.push_str(" The offer that produced it is labelled wrong.");
    }
    line
}

/// What `pause` or `resume` did, in words.
fn hold_line(r: &TaskPauseResult, id: &str) -> String {
    let short = task_tree::short_id(&r.task_id);
    match (r.paused, r.changed) {
        (true, true) => format!(
            "Pausing {short}: it stops at its next loop. `theseus task resume {id}` lets it go on."
        ),
        (true, false) => format!("{short} is paused already."),
        (false, true) => format!("Resuming {short}: it goes on."),
        (false, false) => format!("{short} is not paused."),
    }
}

/// The report's place as a person types it.
fn report_to(given: Option<&str>) -> Result<Option<ReportTo>> {
    match given {
        None => Ok(None),
        Some("home") => Ok(Some(ReportTo::Home)),
        Some("board") => Ok(Some(ReportTo::Board)),
        Some(other) => bail!("--report-to is `home` or `board`, not `{other}`"),
    }
}

/// A quote file's text; `-` reads stdin.
fn quote_of(path: &str) -> Result<String> {
    let text = if path == "-" {
        std::io::read_to_string(std::io::stdin()).context("reading the quote from stdin")?
    } else {
        std::fs::read_to_string(path).with_context(|| format!("reading the quote file {path}"))?
    };
    Ok(text)
}

/// The `start_when` a pair of time flags makes: `--at` as typed, or `--in-time 2h` as `in 2h`.
/// `task reschedule` takes the same flags (`ops_cmd`).
pub fn start_when(at: Option<String>, in_time: Option<String>) -> Option<String> {
    at.or(in_time.map(|t| format!("in {t}")))
}

/// The call's params from the arguments (the quote files read, the times kept as
/// typed: the daemon reads them in its zone).
fn params_of(a: StartArgs, home: Option<String>) -> Result<TaskStartParams> {
    let quotes = a
        .quote_files
        .iter()
        .map(|f| quote_of(f))
        .collect::<Result<Vec<_>>>()?;
    Ok(TaskStartParams {
        brief: a.brief.join(" "),
        title: a.title,
        home,
        budget_usd: a.budget,
        profile: a.profile,
        start_when: start_when(a.at, a.in_time),
        quotes,
        report_to: report_to(a.report_to.as_deref())?,
        ..Default::default()
    })
}

async fn start(conn: &mut Conn, json: bool, a: StartArgs) -> Result<()> {
    let home = match a.in_session.as_deref() {
        Some(s) => Some(theseus_client::resolve::session(conn, s).await?),
        None => None,
    };
    let p = params_of(a, home)?;
    let v = conn.request(method::TASK_START, p).await?;
    output(json, v, |r: TaskStartResult| {
        for line in lines(&r) {
            println!("{line}");
        }
        Ok(())
    })
}

/// What a start says: the short id first, then where to follow it.
pub fn lines(r: &TaskStartResult) -> Vec<String> {
    let when = match r.starts_at_ms {
        Some(at) => format!(
            "asleep until {}",
            theseus_client::render::time::fmt_stamp(at)
        ),
        None => "running".to_string(),
    };
    vec![
        format!(
            "task {} started ({when}), ${:.2} carved from session {}: no model turn was spent",
            r.short,
            r.limit_usd,
            theseus_client::render::short_id(&r.home_session)
        ),
        format!(
            "follow it with `theseus watch {}`; its report reaches that conversation; `theseus \
             tasks` lists it, `theseus cancel {}` stops it",
            r.short, r.short
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(brief: &str) -> StartArgs {
        StartArgs {
            brief: vec![brief.into()],
            title: None,
            in_session: None,
            budget: None,
            profile: None,
            at: None,
            in_time: None,
            quote_files: vec![],
            report_to: None,
        }
    }

    #[test]
    fn the_times_go_to_the_daemon_as_typed() {
        let mut a = args("Survey the north quay");
        a.in_time = Some("2h".into());
        let p = params_of(a, None).unwrap();
        assert_eq!(p.start_when.as_deref(), Some("in 2h"));
        let mut a = args("x y");
        a.at = Some("fri 9:00".into());
        a.budget = Some(2.0);
        let p = params_of(a, Some("ses_0000a1b2c3".into())).unwrap();
        assert_eq!(p.start_when.as_deref(), Some("fri 9:00"));
        assert_eq!(p.home.as_deref(), Some("ses_0000a1b2c3"));
        assert_eq!(p.budget_usd, Some(2.0));
        assert!(p.start_at_ms.is_none() && p.start_after_ms.is_none());
    }

    #[test]
    fn a_quote_file_is_read_and_a_report_place_is_checked() {
        let dir = std::env::temp_dir().join(format!("theseus-quote-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("log.txt");
        std::fs::write(&f, "the ferry was late\n").unwrap();
        let mut a = args("Why was the ferry late?");
        a.quote_files = vec![f.display().to_string()];
        a.report_to = Some("board".into());
        let p = params_of(a, None).unwrap();
        assert_eq!(p.quotes, vec!["the ferry was late\n".to_string()]);
        assert_eq!(p.report_to, Some(ReportTo::Board));
        assert!(report_to(Some("dm")).is_err());
        let mut a = args("x");
        a.quote_files = vec![dir.join("missing.txt").display().to_string()];
        assert!(params_of(a, None).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_start_names_the_short_id_and_says_no_turn_was_spent() {
        let r = TaskStartResult {
            task_id: "ses_0000a1b2c3".into(),
            session_id: "ses_0000a1b2c3".into(),
            short: "a1b2c3".into(),
            home_session: "ses_0000ffee001".into(),
            limit_usd: 0.5,
            starts_at_ms: None,
        };
        let l = lines(&r);
        assert!(
            l[0].starts_with("task a1b2c3 started (running), $0.50"),
            "{l:?}"
        );
        assert!(l[0].contains("no model turn"), "{l:?}");
    }

    #[test]
    fn not_this_names_the_task_the_preference_and_what_it_replaced() {
        let rec = |id: &str| theseus_protocol::StandingRecord {
            id: id.into(),
            kind: theseus_protocol::StandingKind::Preference,
            text: "Not this".into(),
            scope: theseus_protocol::StandingScope::Everywhere,
            since_ms: 1,
            until_ms: None,
            source: "task.not_this".into(),
            ends: None,
        };
        let mut r = TaskNotThisResult {
            task: serde_json::from_value(serde_json::json!({
                "task_id": "ses_0000a1b2c3", "short": "a1b2c3", "execution_id": "exe_1",
                "parent_session_id": "ses_p", "parent_execution_id": "exe_p",
                "state": "cancelled", "spent_usd": 0.0, "limit_usd": 1.0, "cost_usd": 0.0,
                "turns": 1, "created_at_ms": 1, "updated_at_ms": 2
            }))
            .unwrap(),
            preference: rec("std_0001"),
            replaced: None,
            labeled: None,
        };
        let l = not_this_line(&r);
        assert!(
            l.contains("a1b2c3 is cancelled") && l.contains("std_0001"),
            "{l}"
        );
        assert!(!l.contains("replaces") && !l.contains("labelled"), "{l}");
        r.replaced = Some("std_0000".into());
        r.labeled = Some("jdg_1".into());
        let l = not_this_line(&r);
        assert!(
            l.contains("replaces std_0000") && l.contains("labelled wrong"),
            "{l}"
        );
    }
}
