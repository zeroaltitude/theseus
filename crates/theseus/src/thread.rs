//! `theseus thread list` and `thread show` (theseus-nhew), read-only: threads were retired on
//! 2026-11-22 (issue 44), and these read the stored records. Every other `thread` verb is gone.
//! `thread import --vestige PATH` is a note import and moved to `theseus import vestige PATH`;
//! the old spelling still runs it, and says so.

use anyhow::{bail, Result};
use clap::Subcommand;
use theseus_client::render::postmortem::usd;
use theseus_client::render::time::fmt_stamp;
use theseus_client::Conn;
use theseus_protocol::method;
use theseus_protocol::threads::{
    ThreadListParams, ThreadListResult, ThreadNameParams, ThreadRow, ThreadState,
};

use crate::cmd::output;

/// The heading `list` and `show` print above the records.
pub const RETIRED: &str =
    "(read-only: threads were retired on 2026-11-22; these are the stored records)";

#[derive(Subcommand, Debug)]
pub enum ThreadCmd {
    /// The stored active and paused threads (default), newest activity first. Read-only: threads
    /// were retired on 2026-11-22.
    List {
        /// Only threads in this state: `active`, `paused`, or `done`.
        #[arg(long)]
        state: Option<String>,
    },
    /// One stored thread: its record, its session and its card. Read-only.
    Show { slug: String },
    /// Moved: `theseus import vestige PATH` imports a Vestige dump as notes.
    #[command(hide = true)]
    Import(crate::thread_import::Args),
}

impl Default for ThreadCmd {
    fn default() -> Self {
        Self::List { state: None }
    }
}

fn state_of(s: &str) -> Result<ThreadState> {
    Ok(match s {
        "active" => ThreadState::Active,
        "paused" => ThreadState::Paused,
        "done" => ThreadState::Done,
        _ => bail!("`{s}` is no state: write active, paused, or done"),
    })
}

fn name(slug: String) -> ThreadNameParams {
    ThreadNameParams {
        slug,
        ..Default::default()
    }
}

/// `thread list [--state S]`.
async fn list(conn: &mut Conn, json: bool, state: Option<String>) -> Result<()> {
    let p = ThreadListParams {
        state: state.as_deref().map(state_of).transpose()?,
        ..Default::default()
    };
    let v = conn
        .request(method::THREAD_LIST, serde_json::to_value(&p)?)
        .await?;
    output(json, v, |r: ThreadListResult| {
        println!("{RETIRED}");
        for l in list_lines(&r) {
            println!("{l}");
        }
        Ok(())
    })
}

pub async fn run(conn: &mut Conn, json: bool, cmd: ThreadCmd) -> Result<()> {
    match cmd {
        ThreadCmd::List { state } => list(conn, json, state).await,
        ThreadCmd::Show { slug } => {
            let v = conn
                .request(method::THREAD_SHOW, serde_json::to_value(name(slug))?)
                .await?;
            output(json, v, |r: ThreadRow| {
                println!("{RETIRED}");
                for l in show_lines(&r) {
                    println!("{l}");
                }
                Ok(())
            })
        }
        ThreadCmd::Import(a) => crate::thread_import::run(conn, json, a).await,
    }
}

/// The title of a thread for a reply's tag, the slug when it cannot be read.
pub async fn title_of(conn: &mut Conn, slug: &str) -> String {
    let Ok(p) = serde_json::to_value(name(slug.to_string())) else {
        return slug.to_string();
    };
    conn.request(method::THREAD_SHOW, p)
        .await
        .ok()
        .and_then(|v| serde_json::from_value::<ThreadRow>(v).ok())
        .map_or_else(|| slug.to_string(), |r| r.thread.title)
}

/// A thread's name in a list, with its seal: `🔒 tides`, and `🔒? tides` while a seal is proposed.
fn marked(t: &theseus_protocol::threads::ThreadInfo) -> String {
    match (&t.sealed, &t.seal_pending) {
        (Some(_), _) => format!("🔒 {}", t.slug),
        (None, Some(_)) => format!("🔒? {}", t.slug),
        _ => t.slug.clone(),
    }
}

/// A context size as `12.4k`.
fn tokens(t: Option<u64>) -> String {
    match t {
        None => "-".into(),
        Some(t) if t >= 1_000_000 => format!("{:.1}M", t as f64 / 1e6),
        Some(t) if t >= 1_000 => format!("{:.1}k", t as f64 / 1e3),
        Some(t) => t.to_string(),
    }
}

/// `thread list`'s lines: a header and one line for each thread.
pub fn list_lines(r: &ThreadListResult) -> Vec<String> {
    if r.threads.is_empty() {
        return vec![format!("No threads here ({} in all).", r.total)];
    }
    let w = r
        .threads
        .iter()
        .map(|t| marked(&t.thread).chars().count())
        .max()
        .unwrap_or(4)
        .max(4);
    let mut out = vec![format!(
        "{:<w$}  {:<6}  {:<16}  {:>7}  {:>7}  TITLE",
        "SLUG", "STATE", "LAST ACTIVE", "CONTEXT", "TODAY"
    )];
    for t in &r.threads {
        out.push(format!(
            "{:<w$}  {:<6}  {:<16}  {:>7}  {:>7}  {}",
            marked(&t.thread),
            t.thread.state.as_str(),
            fmt_stamp(t.thread.last_active_ms),
            tokens(t.context_tokens),
            usd(t.today_usd),
            t.thread.title,
        ));
    }
    out
}

/// `thread show`'s lines.
pub fn show_lines(r: &ThreadRow) -> Vec<String> {
    let t = &r.thread;
    let mut out = vec![format!("{} ({}): {}", marked(t), t.state.as_str(), t.title)];
    if let Some(s) = &t.sealed {
        out.push(format!("  🔒 sealed by {}: {}", s.by, s.reason));
    }
    if let Some(why) = &t.seal_pending {
        out.push(format!("  🔒? a seal was proposed: {why}"));
    }
    if !t.goal.is_empty() {
        out.push(format!("  goal: {}", t.goal));
    }
    if !t.topics.is_empty() {
        out.push(format!("  topics: {}", t.topics.join(", ")));
    }
    let w = &t.working_set;
    for (what, v) in [
        ("roots", &w.roots),
        ("repos", &w.repos),
        ("branches", &w.branches),
        ("refs", &w.refs),
        ("files", &w.files),
        ("hosts", &w.hosts),
    ] {
        if !v.is_empty() {
            out.push(format!("  {what}: {}", v.join(", ")));
        }
    }
    if let Some(p) = &t.persona {
        out.push(format!("  persona: {p}"));
    }
    out.push(match &t.session_id {
        Some(s) => format!(
            "  session: {s} (context {}, {} today)",
            tokens(r.context_tokens),
            usd(r.today_usd)
        ),
        None => "  session: none".into(),
    });
    out.push(format!("  last active: {}", fmt_stamp(t.last_active_ms)));
    if let Some(c) = &r.cache {
        out.push(format!("  cache today: {}", c.line()));
    }
    for (what, v) in [
        (
            "merged into",
            t.merged_into.iter().cloned().collect::<Vec<_>>(),
        ),
        ("merged from", r.merged_from.clone()),
        ("split from", t.split_from.iter().cloned().collect()),
        ("split to", r.split_to.clone()),
        ("linked with", t.links.clone()),
    ] {
        if !v.is_empty() {
            out.push(format!("  {what}: {}", v.join(", ")));
        }
    }
    if !t.card.is_empty() {
        out.push("  card:".into());
        out.extend(t.card.iter().map(|l| format!("    - {l}")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::threads::{ThreadInfo, WorkingSet};

    fn row(slug: &str, state: ThreadState, tokens: Option<u64>, usd: f64) -> ThreadRow {
        ThreadRow {
            thread: ThreadInfo {
                slug: slug.into(),
                title: format!("Title of {slug}"),
                goal: String::new(),
                topics: vec![],
                working_set: WorkingSet::default(),
                card: vec![],
                persona: None,
                state,
                sealed: None,
                seal_pending: None,
                surfaces: vec![],
                parent: None,
                merged_into: None,
                split_from: None,
                links: vec![],
                session_id: None,
                last_active_ms: 0,
                created_at_ms: 0,
            },
            context_tokens: tokens,
            today_usd: usd,
            cache_ttl_secs: None,
            cache: None,
            merged_from: vec![],
            split_to: vec![],
        }
    }

    /// The list shows title, state, last activity, context tokens and
    /// today's dollars for each thread.
    #[test]
    fn the_list_shows_each_threads_state_context_and_spend() {
        theseus_client::render::time::pin_for_tests();
        let r = ThreadListResult {
            threads: vec![
                row("tides", ThreadState::Active, Some(12_400), 0.31),
                row("moons", ThreadState::Paused, None, 0.0),
            ],
            total: 3,
        };
        let lines = list_lines(&r);
        assert!(
            lines[0].starts_with("SLUG ") && lines[0].ends_with("TITLE"),
            "{lines:?}"
        );
        assert!(
            lines[1].contains("tides") && lines[1].contains("active"),
            "{lines:?}"
        );
        assert!(
            lines[1].contains("12.4k") && lines[1].contains("$0.31"),
            "{lines:?}"
        );
        assert!(lines[1].ends_with("Title of tides"), "{lines:?}");
        assert!(
            lines[2].contains("paused") && lines[2].contains(" -"),
            "{lines:?}"
        );
        let none = list_lines(&ThreadListResult {
            threads: vec![],
            total: 2,
        });
        assert!(none[0].contains("No threads here (2 in all)"), "{none:?}");
    }

    /// A sealed thread's name carries a lock in the list and its card says
    /// who sealed it and why; a proposal carries a lock and a question mark.
    #[test]
    fn a_seal_shows_in_the_list_and_on_the_card() {
        theseus_client::render::time::pin_for_tests();
        let mut sealed = row("fix", ThreadState::Active, None, 0.0);
        sealed.thread.sealed = Some(theseus_protocol::threads::Seal {
            by: "the CLI".into(),
            at_ms: 1,
            reason: "embargoed".into(),
        });
        let mut maybe = row("maybe", ThreadState::Active, None, 0.0);
        maybe.thread.seal_pending = Some("looks embargoed".into());
        let list = list_lines(&ThreadListResult {
            threads: vec![sealed.clone(), maybe.clone()],
            total: 2,
        })
        .join("\n");
        assert!(
            list.contains("🔒 fix") && list.contains("🔒? maybe"),
            "{list}"
        );
        let card = show_lines(&sealed).join("\n");
        assert!(card.contains("🔒 sealed by the CLI: embargoed"), "{card}");
        let card = show_lines(&maybe).join("\n");
        assert!(
            card.contains("a seal was proposed: looks embargoed"),
            "{card}"
        );
    }

    #[test]
    fn show_names_the_session_the_card_and_the_working_set() {
        let mut r = row("tides", ThreadState::Active, Some(900), 1.5);
        r.thread.session_id = Some("ses_abc".into());
        r.thread.card = vec!["gauge 4 is drifting".into()];
        r.thread.working_set.repos = vec!["tide-log".into()];
        let s = show_lines(&r).join("\n");
        assert!(
            s.contains("session: ses_abc (context 900, $1.50 today)"),
            "{s}"
        );
        assert!(
            s.contains("- gauge 4 is drifting") && s.contains("repos: tide-log"),
            "{s}"
        );
        assert!(show_lines(&row("a", ThreadState::Paused, None, 0.0))
            .join("\n")
            .contains("session: none"));
    }

    #[test]
    fn show_names_the_threads_it_was_merged_into_or_split_from() {
        let mut r = row("io", ThreadState::Done, None, 0.0);
        r.thread.merged_into = Some("tides".into());
        r.thread.split_from = Some("harbour".into());
        r.merged_from = vec!["a".into(), "b".into()];
        r.split_to = vec!["disk".into()];
        let s = show_lines(&r).join("\n");
        for want in [
            "merged into: tides",
            "merged from: a, b",
            "split from: harbour",
            "split to: disk",
        ] {
            assert!(s.contains(want), "{want} in {s}");
        }
        assert!(!show_lines(&row("x", ThreadState::Active, None, 0.0))
            .join("\n")
            .contains("merged"));
    }

    #[test]
    fn show_names_the_threads_it_is_linked_with() {
        let mut r = row("io", ThreadState::Active, None, 0.0);
        assert!(!show_lines(&r).join("\n").contains("linked"));
        r.thread.links = vec!["harbour".into(), "tides".into()];
        let s = show_lines(&r).join("\n");
        assert!(s.contains("linked with: harbour, tides"), "{s}");
    }
}
