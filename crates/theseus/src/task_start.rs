//! `theseus task start` (theseus-7oq0w): start a task with no model turn. One
//! `task.start` call; the daemon opens the task in one frame and its first turn
//! runs on the next tick, or at the time asked. The task goes under your Home
//! conversation unless `--in` names another, and its budget is carved from there.
//! The method is yours alone (`client::OPERATORS`), refused inside a job.

use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use theseus_client::Conn;
use theseus_protocol::method;
use theseus_protocol::task_start::{ReportTo, TaskStartParams, TaskStartResult};

use crate::cmd::output;

#[derive(Subcommand, Debug)]
pub enum TaskCmd {
    /// Start a task now, or at a time, with no model turn: `theseus task start "Survey the north
    /// quay" --budget 2`. Prints the task's short id; `theseus tasks` shows it, `theseus watch ID`
    /// follows it. Only you may (refused inside a job).
    Start(StartArgs),
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
        start_when: a.at.or(a.in_time.map(|t| format!("in {t}"))),
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
}
