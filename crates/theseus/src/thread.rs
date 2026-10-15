//! `theseus thread …` (theseus-nhew): the threads on virtual places
//! `thread:<slug>`. `list` and `show` read; `new`, `edit`, `pause`, `resume`,
//! `done`, `reopen`, `merge`, `unmerge` and `split` are the owner's acts
//! (refused inside a job, `client::OPERATORS`). Address a thread with
//! `theseus ask --thread <slug>`. A merge or a split deletes nothing:
//! `unmerge` undoes a merge and `done` on the new thread undoes a split
//! (theseus-vt9k).

use anyhow::{bail, Result};
use clap::Subcommand;
use theseus_client::render::postmortem::usd;
use theseus_client::render::time::fmt_stamp;
use theseus_client::Conn;
use theseus_protocol::method;
use theseus_protocol::threads::{
    ThreadEditParams, ThreadListParams, ThreadListResult, ThreadMergeParams, ThreadMergeResult,
    ThreadNameParams, ThreadNewParams, ThreadRow, ThreadSplitParams, ThreadSplitResult,
    ThreadState, WorkingSet,
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
    /// Fold thread FROM into thread INTO. Nothing is deleted: FROM is retired (`merged_into`), its
    /// card is quoted at the end of INTO's, the working sets and topics are joined (at most three
    /// topics: the rest are named, and `thread edit --topic` picks), and a seal on either stays on
    /// INTO. INTO's session takes the new card at its next cold resume, never mid-warm.
    /// `theseus thread unmerge FROM` undoes it.
    Merge { from: String, into: String },
    /// Undo a merge: FROM is as it was before (its state, its surfaces; its session opens again).
    /// INTO keeps what it took and gains a card line saying so.
    Unmerge { slug: String },
    /// Move part of a thread into a thread of its own. NEW opens with the chosen turns of FROM
    /// quoted verbatim (an arrangement) and FROM keeps every node: it gains one note at its tail,
    /// "split to «NEW» at @pos". Choose the turns by the `@` positions `theseus history` prints:
    /// `--first @A --last @B` (inclusive), or `--at @N` for single messages (repeat the flag).
    /// There is no delete: `theseus thread done NEW` ends the new thread, and FROM is unchanged.
    Split {
        from: String,
        /// The new thread's name.
        new: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        goal: Option<String>,
        /// The first position of the range to quote (`@1234` or `1234`).
        #[arg(long)]
        first: Option<String>,
        /// The last position of the range.
        #[arg(long)]
        last: Option<String>,
        /// One message to quote, by position; repeat the flag.
        #[arg(long = "at")]
        at: Vec<String>,
    },
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
        } => {
            let p = ThreadNewParams {
                slug,
                title,
                goal,
                topics: (!topics.is_empty()).then_some(topics),
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
        ThreadCmd::Merge { from, into } => {
            let p = ThreadMergeParams {
                from,
                into,
                ..Default::default()
            };
            let v = conn
                .request(method::THREAD_MERGE, serde_json::to_value(&p)?)
                .await?;
            output(json, v, |r: ThreadMergeResult| {
                for l in merge_lines(&r, true) {
                    println!("{l}");
                }
                Ok(())
            })
        }
        ThreadCmd::Unmerge { slug } => {
            let v = conn
                .request(method::THREAD_UNMERGE, serde_json::to_value(name(slug))?)
                .await?;
            output(json, v, |r: ThreadMergeResult| {
                for l in merge_lines(&r, false) {
                    println!("{l}");
                }
                Ok(())
            })
        }
        ThreadCmd::Split {
            from,
            new,
            title,
            goal,
            first,
            last,
            at,
        } => {
            let p = split_params(from, new, title, goal, first, last, at)?;
            let v = conn
                .request(method::THREAD_SPLIT, serde_json::to_value(&p)?)
                .await?;
            output(json, v, |r: ThreadSplitResult| {
                for l in split_lines(&r) {
                    println!("{l}");
                }
                Ok(())
            })
        }
    }
}

/// A position as `@1234` or `1234`.
fn position(s: &str) -> Result<u64> {
    s.trim().trim_start_matches('@').parse().map_err(|_| {
        anyhow::anyhow!("`{s}` is no position: write `@1234` as `theseus history` shows it")
    })
}

/// `thread split`'s flags as the request: a range or a list, one of the two.
fn split_params(
    from: String,
    new: String,
    title: Option<String>,
    goal: Option<String>,
    first: Option<String>,
    last: Option<String>,
    at: Vec<String>,
) -> Result<ThreadSplitParams> {
    let (range, positions) = match (first, last, at.is_empty()) {
        (Some(a), Some(b), true) => (Some([position(&a)?, position(&b)?]), None),
        (None, None, false) => (
            None,
            Some(at.iter().map(|p| position(p)).collect::<Result<Vec<_>>>()?),
        ),
        _ => bail!("name the turns: `--first @A --last @B`, or `--at @N` for each message"),
    };
    Ok(ThreadSplitParams {
        from,
        slug: new,
        title,
        goal,
        range,
        positions,
        ..Default::default()
    })
}

/// What a merge or an unmerge did, in lines.
pub fn merge_lines(r: &ThreadMergeResult, merged: bool) -> Vec<String> {
    let (a, b) = (&r.from.thread, &r.into.thread);
    let mut out = match merged {
        true => vec![format!(
            "Merged {} into {}: {} is retired and nothing of it is deleted; its card is quoted \
             in {}'s. `theseus thread unmerge {}` undoes it.",
            a.slug, b.slug, a.slug, b.slug, a.slug
        )],
        false => vec![format!(
            "Unmerged {} from {}: {} is {} again.",
            a.slug,
            b.slug,
            a.slug,
            a.state.as_str()
        )],
    };
    if !r.topics_left_out.is_empty() {
        out.push(format!(
            "{} holds three topics; left out: {}. `theseus thread edit {} --topic …` picks.",
            b.slug,
            r.topics_left_out.join(", "),
            b.slug
        ));
    }
    if merged && b.sealed.is_some() {
        out.push(format!("{} is sealed.", b.slug));
    }
    out
}

/// What a split made, in lines.
pub fn split_lines(r: &ThreadSplitResult) -> Vec<String> {
    let t = &r.thread.thread;
    let mut out = vec![format!(
        "Split {} out of {}: {} quoted, @{} to @{}. `theseus ask --thread {}` goes on from them; \
         `theseus thread done {}` ends it. Nothing of {} moved.",
        t.slug,
        r.from,
        match r.quoted {
            1 => "1 message".to_string(),
            n => format!("{n} messages"),
        },
        r.first_pos,
        r.last_pos,
        t.slug,
        t.slug,
        r.from
    )];
    if !r.noted {
        out.push(format!(
            "{} had a turn running, so its note \"split to {}\" was not written.",
            r.from, t.slug
        ));
    }
    out
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
        .map(|t| t.thread.slug.len())
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
            t.thread.slug,
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
    let mut out = vec![format!("{} ({}): {}", t.slug, t.state.as_str(), t.title)];
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
    for (what, v) in [
        (
            "merged into",
            t.merged_into.iter().cloned().collect::<Vec<_>>(),
        ),
        ("merged from", r.merged_from.clone()),
        ("split from", t.split_from.iter().cloned().collect()),
        ("split to", r.split_to.clone()),
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
    fn split_takes_a_range_or_positions_and_never_both() {
        let p = split_params(
            "a".into(),
            "b".into(),
            None,
            None,
            Some("@10".into()),
            Some("20".into()),
            vec![],
        )
        .unwrap();
        assert_eq!((p.range, p.positions), (Some([10, 20]), None));
        let p = split_params(
            "a".into(),
            "b".into(),
            None,
            None,
            None,
            None,
            vec!["@3".into(), "9".into()],
        )
        .unwrap();
        assert_eq!((p.range, p.positions), (None, Some(vec![3, 9])));
        for (first, last, at) in [
            (None, None, vec![]),
            (Some("@1".to_string()), None, vec![]),
            (
                Some("@1".to_string()),
                Some("@2".to_string()),
                vec!["@3".to_string()],
            ),
        ] {
            let e = split_params("a".into(), "b".into(), None, None, first, last, at).unwrap_err();
            assert!(e.to_string().contains("name the turns"), "{e}");
        }
        let e = split_params(
            "a".into(),
            "b".into(),
            None,
            None,
            Some("x".into()),
            Some("2".into()),
            vec![],
        )
        .unwrap_err();
        assert!(e.to_string().contains("no position"), "{e}");
    }

    #[test]
    fn a_merge_and_a_split_say_what_was_kept_and_how_to_undo() {
        let r = ThreadMergeResult {
            from: row("io", ThreadState::Done, None, 0.0),
            into: row("tides", ThreadState::Active, None, 0.0),
            topics_left_out: vec!["gauges".into()],
        };
        let m = merge_lines(&r, true).join("\n");
        assert!(
            m.contains("nothing of it is deleted") && m.contains("`theseus thread unmerge io`"),
            "{m}"
        );
        assert!(m.contains("left out: gauges"), "{m}");
        let u = merge_lines(&r, false).join("\n");
        assert!(u.contains("Unmerged io from tides"), "{u}");
        let s = ThreadSplitResult {
            thread: row("disk", ThreadState::Active, None, 0.0),
            from: "harbour".into(),
            quoted: 6,
            first_pos: 10,
            last_pos: 30,
            noted: false,
        };
        let l = split_lines(&s).join("\n");
        assert!(
            l.contains("6 messages quoted, @10 to @30") && l.contains("Nothing of harbour moved"),
            "{l}"
        );
        assert!(l.contains("note \"split to disk\" was not written"), "{l}");
    }
}
