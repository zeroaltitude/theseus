//! `theseus thread …` (theseus-nhew): the threads on virtual places
//! `thread:<slug>`. `list` and `show` read; `new`, `edit`, `pause`, `resume`,
//! `done` and `reopen` are the owner's acts (refused inside a job,
//! `client::OPERATORS`). Address a thread with `theseus ask --thread <slug>`.

use anyhow::{bail, Result};
use clap::Subcommand;
use theseus_client::render::postmortem::usd;
use theseus_client::render::time::fmt_stamp;
use theseus_client::Conn;
use theseus_protocol::method;
use theseus_protocol::threads::{
    crossed_words, ThreadEditParams, ThreadListParams, ThreadListResult, ThreadNameParams,
    ThreadNewParams, ThreadRow, ThreadSealParams, ThreadSealResult, ThreadState, WorkingSet,
};

use crate::cmd::output;

#[derive(Subcommand, Debug)]
pub enum ThreadCmd {
    /// The active and paused threads (default), newest activity first, with each one's session's
    /// context size and today's spend.
    List {
        /// Only threads in this state: `active`, `paused`, or `done`.
        #[arg(long)]
        state: Option<String>,
    },
    /// Make a thread: a line of work with a session of its own, which `theseus ask --thread SLUG`
    /// opens at its first message. Only you, from a private place (the CLI is one), may.
    New {
        /// Lower-case letters, digits and hyphens, at most 40.
        slug: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        goal: Option<String>,
        /// A topic (at most three; repeat the flag).
        #[arg(long = "topic")]
        topics: Vec<String>,
        /// Make it sealed: its notes never come into another thread.
        #[arg(long)]
        sealed: bool,
        /// The thread this one splits from; a split from a sealed thread starts sealed.
        #[arg(long = "split-from")]
        split_from: Option<String>,
    },
    /// One thread: its record, its session, its context and today's spend.
    Show { slug: String },
    /// Edit a thread: each flag given replaces what the thread held. A list flag (`--topic`,
    /// `--root`, `--repo`, `--branch`, `--ref`, `--file`, `--host`, `--card`) replaces that list
    /// with the values given; `--persona ''` clears the persona.
    Edit {
        slug: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        goal: Option<String>,
        #[arg(long = "topic")]
        topics: Vec<String>,
        #[arg(long = "root")]
        roots: Vec<String>,
        #[arg(long = "repo")]
        repos: Vec<String>,
        #[arg(long = "branch")]
        branches: Vec<String>,
        /// A PR, issue or Beads id.
        #[arg(long = "ref")]
        refs: Vec<String>,
        #[arg(long = "file")]
        files: Vec<String>,
        #[arg(long = "host")]
        hosts: Vec<String>,
        /// A card line: where the thread stands (at most twelve; repeat the flag).
        #[arg(long = "card")]
        card: Vec<String>,
        #[arg(long)]
        persona: Option<String>,
    },
    /// Pause a thread: it takes no message until resumed.
    Pause { slug: String },
    /// Let a paused thread take messages again.
    Resume { slug: String },
    /// Finish a thread: its session retires and its card goes to memory.
    Done { slug: String },
    /// Take a finished thread up again: its next message opens a successor session.
    Reopen { slug: String },
    /// Seal a thread: its notes never come into another thread's recall, and `thread.read` of it
    /// asks you first. Says what had already crossed into other threads. Only you, from a private
    /// place. `--not-needed` answers a proposed seal "not needed" instead.
    Seal {
        slug: String,
        #[arg(long)]
        reason: Option<String>,
        /// Also leave a note in each session the thread's notes crossed into: they are withdrawn.
        #[arg(long)]
        redirect: bool,
        /// Dismiss the seal Theseus proposed: not proposed again until the thread's content changes.
        #[arg(long = "not-needed")]
        not_needed: bool,
    },
    /// Open a sealed thread again. Only you, from a private place.
    Unseal { slug: String },
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

pub async fn run(conn: &mut Conn, json: bool, cmd: ThreadCmd) -> Result<()> {
    let show = |r: ThreadRow| {
        for l in show_lines(&r) {
            println!("{l}");
        }
        Ok(())
    };
    match cmd {
        ThreadCmd::List { state } => {
            let p = ThreadListParams {
                state: state.as_deref().map(state_of).transpose()?,
                ..Default::default()
            };
            let v = conn
                .request(method::THREAD_LIST, serde_json::to_value(&p)?)
                .await?;
            output(json, v, |r: ThreadListResult| {
                for l in list_lines(&r) {
                    println!("{l}");
                }
                Ok(())
            })
        }
        ThreadCmd::New {
            slug,
            title,
            goal,
            topics,
            sealed,
            split_from,
        } => {
            let p = ThreadNewParams {
                slug,
                title,
                goal,
                topics: (!topics.is_empty()).then_some(topics),
                sealed,
                split_from,
                ..Default::default()
            };
            let v = conn
                .request(method::THREAD_NEW, serde_json::to_value(&p)?)
                .await?;
            output(json, v, |r: ThreadRow| {
                println!(
                    "Made the thread {}: `theseus ask --thread {}` opens its session.",
                    r.thread.slug, r.thread.slug
                );
                Ok(())
            })
        }
        ThreadCmd::Show { slug } => {
            let v = conn
                .request(method::THREAD_SHOW, serde_json::to_value(name(slug))?)
                .await?;
            output(json, v, show)
        }
        ThreadCmd::Edit {
            slug,
            title,
            goal,
            topics,
            roots,
            repos,
            branches,
            refs,
            files,
            hosts,
            card,
            persona,
        } => {
            let working_set =
                patched_set(conn, &slug, [roots, repos, branches, refs, files, hosts]).await?;
            let p = ThreadEditParams {
                slug,
                title,
                goal,
                topics: (!topics.is_empty()).then_some(topics),
                working_set,
                card: (!card.is_empty()).then_some(card),
                persona,
                ..Default::default()
            };
            let v = conn
                .request(method::THREAD_EDIT, serde_json::to_value(&p)?)
                .await?;
            output(json, v, show)
        }
        ThreadCmd::Pause { slug } => moved(conn, json, method::THREAD_PAUSE, slug).await,
        ThreadCmd::Resume { slug } => moved(conn, json, method::THREAD_RESUME, slug).await,
        ThreadCmd::Done { slug } => moved(conn, json, method::THREAD_DONE, slug).await,
        ThreadCmd::Reopen { slug } => moved(conn, json, method::THREAD_REOPEN, slug).await,
        seal @ (ThreadCmd::Seal { .. } | ThreadCmd::Unseal { .. }) => {
            sealing(conn, json, seal).await
        }
    }
}

/// `thread seal` and `thread unseal`.
async fn sealing(conn: &mut Conn, json: bool, cmd: ThreadCmd) -> Result<()> {
    match cmd {
        ThreadCmd::Seal {
            slug,
            not_needed: true,
            ..
        } => {
            let v = conn
                .request(
                    method::THREAD_SEAL_DISMISS,
                    serde_json::to_value(name(slug))?,
                )
                .await?;
            output(json, v, |r: ThreadRow| {
                println!(
                    "Not needed: the thread {} is not proposed for sealing again until its content changes.",
                    r.thread.slug
                );
                Ok(())
            })
        }
        ThreadCmd::Seal {
            slug,
            reason,
            redirect,
            ..
        } => {
            let p = ThreadSealParams {
                slug,
                reason,
                redirect,
                ..Default::default()
            };
            let v = conn
                .request(method::THREAD_SEAL, serde_json::to_value(&p)?)
                .await?;
            output(json, v, |r: ThreadSealResult| {
                for l in seal_lines(&r) {
                    println!("{l}");
                }
                Ok(())
            })
        }
        ThreadCmd::Unseal { slug } => {
            let v = conn
                .request(method::THREAD_UNSEAL, serde_json::to_value(name(slug))?)
                .await?;
            output(json, v, |r: ThreadRow| {
                println!("The thread {} is unsealed.", r.thread.slug);
                Ok(())
            })
        }
        _ => unreachable!("only the seal commands come here"),
    }
}

/// The working set with the lists given put in place of the thread's own,
/// the rest as they are; none when no list was given.
async fn patched_set(
    conn: &mut Conn,
    slug: &str,
    lists: [Vec<String>; 6],
) -> Result<Option<WorkingSet>> {
    if lists.iter().all(Vec::is_empty) {
        return Ok(None);
    }
    let v = conn
        .request(
            method::THREAD_SHOW,
            serde_json::to_value(name(slug.to_string()))?,
        )
        .await?;
    let mut w: WorkingSet = serde_json::from_value::<ThreadRow>(v)?.thread.working_set;
    let [roots, repos, branches, refs, files, hosts] = lists;
    let put = |dst: &mut Vec<String>, src: Vec<String>| {
        if !src.is_empty() {
            *dst = src;
        }
    };
    put(&mut w.roots, roots);
    put(&mut w.repos, repos);
    put(&mut w.branches, branches);
    put(&mut w.refs, refs);
    put(&mut w.files, files);
    put(&mut w.hosts, hosts);
    Ok(Some(w))
}

async fn moved(conn: &mut Conn, json: bool, m: &str, slug: String) -> Result<()> {
    let v = conn.request(m, serde_json::to_value(name(slug))?).await?;
    output(json, v, |r: ThreadRow| {
        println!(
            "The thread {} is {}.",
            r.thread.slug,
            r.thread.state.as_str()
        );
        Ok(())
    })
}

/// What a seal said: the thread sealed, then each place its notes had crossed
/// into, with the offer to leave a redirect note there.
pub fn seal_lines(r: &ThreadSealResult) -> Vec<String> {
    let t = &r.thread.thread;
    let mut out = vec![match &t.sealed {
        Some(s) => format!("🔒 The thread {} is sealed ({}).", t.slug, s.reason),
        None => format!("The thread {} is not sealed.", t.slug),
    }];
    if r.crossed.is_empty() {
        out.push("None of its notes had crossed into another session.".into());
        return out;
    }
    for (c, words) in r.crossed.iter().zip(crossed_words(&r.crossed)) {
        out.push(format!(
            "  {words}{}",
            if c.redirected {
                " (a redirect note is there now)"
            } else {
                ""
            }
        ));
    }
    if r.crossed.iter().any(|c| !c.redirected) {
        out.push(format!(
            "  `theseus thread seal {} --redirect` leaves a note in each, withdrawing them.",
            t.slug
        ));
    }
    out
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
        return vec![format!(
            "No threads here ({} in all). `theseus thread new SLUG` makes one.",
            r.total
        )];
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
        out.push(format!(
            "  🔒 sealed by {}: {} (`theseus thread unseal {}` opens it)",
            s.by, s.reason, t.slug
        ));
    }
    if let Some(why) = &t.seal_pending {
        out.push(format!(
            "  🔒? a seal is proposed: {why} (`theseus thread seal {0}` seals it; `theseus thread seal {0} --not-needed` dismisses it)",
            t.slug
        ));
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
        None => "  session: none yet; its first message opens one".into(),
    });
    out.push(format!("  last active: {}", fmt_stamp(t.last_active_ms)));
    if !t.card.is_empty() {
        out.push("  card:".into());
        out.extend(t.card.iter().map(|l| format!("    - {l}")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::threads::ThreadInfo;

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
                session_id: None,
                last_active_ms: 0,
                created_at_ms: 0,
            },
            context_tokens: tokens,
            today_usd: usd,
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
        assert!(
            card.contains("🔒 sealed by the CLI: embargoed (`theseus thread unseal fix` opens it)"),
            "{card}"
        );
        let card = show_lines(&maybe).join("\n");
        assert!(
            card.contains("a seal is proposed: looks embargoed"),
            "{card}"
        );
        assert!(card.contains("seal maybe --not-needed"), "{card}");
    }

    /// The seal's report names where its facts had crossed, and offers the
    /// redirect until it is taken.
    #[test]
    fn the_seals_report_names_the_crossed_facts_and_offers_a_redirect() {
        let mut r = row("fix", ThreadState::Active, None, 0.0);
        r.thread.sealed = Some(theseus_protocol::threads::Seal {
            by: "the CLI".into(),
            at_ms: 1,
            reason: "embargoed".into(),
        });
        let crossed = |redirected| theseus_protocol::threads::ThreadCrossed {
            session_id: "ses_pr".into(),
            thread: Some("pr-round".into()),
            title: Some("pr round".into()),
            facts: 2,
            redirected,
        };
        let lines = seal_lines(&ThreadSealResult {
            thread: r.clone(),
            crossed: vec![crossed(false)],
        });
        assert_eq!(lines[0], "🔒 The thread fix is sealed (embargoed).");
        assert_eq!(lines[1], "  2 facts were already used in «pr round»");
        assert!(lines[2].contains("seal fix --redirect"), "{lines:?}");
        let done = seal_lines(&ThreadSealResult {
            thread: r.clone(),
            crossed: vec![crossed(true)],
        });
        assert!(
            done[1].ends_with("(a redirect note is there now)") && done.len() == 2,
            "{done:?}"
        );
        let none = seal_lines(&ThreadSealResult {
            thread: r,
            crossed: vec![],
        });
        assert!(
            none[1].contains("None of its notes had crossed"),
            "{none:?}"
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
            .contains("session: none yet"));
    }
}
