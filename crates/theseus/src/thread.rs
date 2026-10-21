//! `theseus thread …` (theseus-nhew): the threads on virtual places
//! `thread:<slug>`. `list` and `show` read; `new`, `edit`, `pause`, `resume`,
//! `done`, `reopen`, `merge`, `unmerge`, `split` and `unsplit` are the owner's
//! acts (refused inside a job, `client::OPERATORS`). Address a thread with
//! `theseus ask --thread <slug>`. A merge or a split deletes nothing:
//! `unmerge` undoes a merge and `unsplit` a split (theseus-vt9k,
//! theseus-q9b85; `thread/split.rs`).

use anyhow::{bail, Result};
use clap::Subcommand;
use theseus_client::current::{self, Pick};
use theseus_client::render::postmortem::usd;
use theseus_client::render::time::fmt_stamp;
use theseus_client::Conn;
use theseus_protocol::method;
use theseus_protocol::thread_route::{
    ThreadAnswerParams, ThreadAnswerResult, ThreadMoveParams, ThreadMoveResult,
};
use theseus_protocol::threads::{
    crossed_words, ThreadEditParams, ThreadLinkParams, ThreadListParams, ThreadListResult,
    ThreadMergeParams, ThreadMergeResult, ThreadNameParams, ThreadNewParams, ThreadRow,
    ThreadSealParams, ThreadSealResult, ThreadState, WorkingSet,
};

use crate::cmd::output;

pub(crate) mod ask;
pub(crate) mod split;

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
    /// Make a thread this shell's current one: prints the `export` line that sets
    /// `THESEUS_THREAD`, which `ask` reads (`eval "$(theseus thread switch SLUG)"`), and tells the
    /// interactive loop and the TUI on this terminal. `home` clears it. A slug, a number from
    /// `theseus thread list`, or a unique start of a slug or title (four characters) names it.
    Switch { thread: String },
    /// Candidate threads from your earlier harness: `--from openclaw` is the dry
    /// run (a table and a plan file to tick, nothing written); `--apply 1,3,5` or `--apply
    /// all-ticked` makes those threads, each with a card written from its episodes (a Haiku call,
    /// under `[threads] import_day_usd`). `--vestige PATH` imports a Vestige dump as notes, once.
    Import(crate::thread_import::Args),
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
    /// Answer a question the router asked ("Is this about the pr round or the security fix?"):
    /// `1`, `2`, `new` (`n`) or `home` (`h`). The turn that waits for it goes on.
    Answer { ask: String, choice: String },
    /// "That was for the security fix": run the last message the router placed in a session again in
    /// another thread (a slug) or at `home`, and tell the thread it first ran in to disregard it.
    Move {
        to: String,
        /// The session the message was sent to (default: the newest session).
        #[arg(long)]
        session: Option<String>,
        /// The turn to move (default: the session's last placed message).
        #[arg(long)]
        turn: Option<String>,
    },
    /// Fold thread FROM into thread INTO. Nothing is deleted: FROM is retired (`merged_into`), its
    /// card is quoted at the end of INTO's, the working sets and topics are joined (at most three
    /// topics: the rest are named, and `thread edit --topic` picks), and a seal on either stays on
    /// INTO. INTO's session takes the new card at its next cold resume, never mid-warm.
    /// `theseus thread unmerge FROM` undoes it.
    Merge { from: String, into: String },
    /// Undo a merge: FROM is as it was before (its state, its surfaces; its session opens again).
    /// INTO keeps what it took and gains a card line saying so.
    Unmerge { slug: String },
    /// Link thread SLUG with OTHER: each one's notes reach the other's recall with no distance.
    /// Neither may be sealed, and a seal set later holds the notes out again. At most 8 links a
    /// thread. `theseus thread unlink SLUG OTHER` undoes it.
    Link { slug: String, other: String },
    /// Undo a link: the distance applies between SLUG and OTHER again. A sealed thread may be
    /// unlinked.
    Unlink { slug: String, other: String },
    /// Move whole turns into a thread, and take them out of the source's compile. The source is a
    /// thread's slug, a place's own session (its home) by its whole id (`ses_…`, `theseus
    /// places`), or `home@<place>`. The turns are quoted verbatim, in order, into a new thread
    /// NEW, or at the tail of an existing one (`--into SLUG`); the source keeps every node in the
    /// store but its compile holds one line, `moved to thread «X»: <gist>`, in their place.
    /// Choose them by the `@` positions `theseus history` prints: `--first @A --last @B`, `--range
    /// @A-@B` (repeat it), or `--at @N`. A part over 400 messages or 400,000 characters moves in
    /// several splits into the same thread. `--dry-run` writes nothing and prints the next
    /// request's tokens before and after. The source's cache prefix changes once. `theseus thread
    /// unsplit SPLIT` undoes it; nothing is ever deleted.
    Split(split::SplitArgs),
    /// Undo a split by its id (`spl_…`, printed by `thread split`): the source's compile reads the
    /// range again in its own place, the quoted turns leave the target's compile, and a thread the
    /// split opened is done.
    Unsplit { split: String },
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
        for l in list_lines(&r) {
            println!("{l}");
        }
        Ok(())
    })
}

pub async fn run(conn: &mut Conn, json: bool, cmd: ThreadCmd) -> Result<()> {
    let show = |r: ThreadRow| {
        for l in show_lines(&r) {
            println!("{l}");
        }
        Ok(())
    };
    match cmd {
        ThreadCmd::List { state } => list(conn, json, state).await,
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
        ThreadCmd::Switch { thread } => switch(conn, json, &thread).await,
        ThreadCmd::Import(a) => crate::thread_import::run(conn, json, a).await,
        s @ (ThreadCmd::Seal { .. } | ThreadCmd::Unseal { .. }) => sealing(conn, json, s).await,
        ThreadCmd::Answer { ask, choice } => answer(conn, json, ask, choice).await,
        ThreadCmd::Move { to, session, turn } => move_message(conn, json, to, session, turn).await,
        cmd @ (ThreadCmd::Merge { .. } | ThreadCmd::Unmerge { .. }) => join(conn, json, cmd).await,
        ThreadCmd::Link { slug, other } => linking(conn, json, true, slug, other).await,
        ThreadCmd::Unlink { slug, other } => linking(conn, json, false, slug, other).await,
        ThreadCmd::Split(a) => split::split(conn, json, a).await,
        ThreadCmd::Unsplit { split } => split::unsplit(conn, json, split).await,
    }
}

/// `link` and `unlink` (theseus-98bz.11).
async fn linking(
    conn: &mut Conn,
    json: bool,
    link: bool,
    slug: String,
    other: String,
) -> Result<()> {
    let p = ThreadLinkParams {
        slug,
        other,
        ..Default::default()
    };
    let m = if link {
        method::THREAD_LINK
    } else {
        method::THREAD_UNLINK
    };
    let v = conn.request(m, serde_json::to_value(&p)?).await?;
    output(json, v, |r: ThreadRow| {
        println!("{}", link_line(&r, &p.other, link));
        Ok(())
    })
}

/// What a link or an unlink left, in a line.
fn link_line(r: &ThreadRow, other: &str, link: bool) -> String {
    let slug = &r.thread.slug;
    if link {
        format!("Linked {slug} with {other}: each one's notes reach the other's recall with no distance; a seal still holds them out. `theseus thread unlink {slug} {other}` undoes it.")
    } else {
        format!("Unlinked {slug} from {other}: the distance applies between them again.")
    }
}

/// `thread answer`: the owner's word on a question the router asked.
async fn answer(conn: &mut Conn, json: bool, ask: String, choice: String) -> Result<()> {
    let p = ThreadAnswerParams {
        ask_id: ask,
        choice,
        author: None,
        discord: None,
    };
    let v = conn
        .request(method::THREAD_ANSWER, serde_json::to_value(&p)?)
        .await?;
    output(json, v, |r: ThreadAnswerResult| {
        match r.routed {
            Some(s) => println!("The message goes to the thread {s}."),
            None => println!("The message goes home, or to a new thread."),
        }
        Ok(())
    })
}

/// `thread move`: the last message the router placed, run again elsewhere.
async fn move_message(
    conn: &mut Conn,
    json: bool,
    to: String,
    session: Option<String>,
    turn: Option<String>,
) -> Result<()> {
    let session_id = match session {
        Some(s) => s,
        None => newest(conn).await?,
    };
    let p = ThreadMoveParams {
        session_id,
        to,
        turn_id: turn,
        author: None,
        discord: None,
    };
    let v = conn
        .request(method::THREAD_MOVE, serde_json::to_value(&p)?)
        .await?;
    output(json, v, |r: ThreadMoveResult| {
        println!(
            "Moved from {} to {}: it ran again in turn {}{}.",
            r.from,
            r.to,
            r.turn_id,
            if r.redirected {
                ", and the first thread was told to disregard it"
            } else {
                ""
            }
        );
        Ok(())
    })
}

/// The newest session's id: where a message was sent when no session is named.
async fn newest(conn: &mut Conn) -> Result<String> {
    let v = conn
        .request(
            method::SESSION_LIST,
            serde_json::to_value(crate::cmd::NEWEST)?,
        )
        .await?;
    let l: theseus_protocol::SessionListResult = serde_json::from_value(v)?;
    match l.sessions.into_iter().next() {
        Some(s) => Ok(s.session_id),
        None => bail!("there is no session to move a message from"),
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

/// `thread switch`: resolve the name among the live threads, remember it for
/// this terminal, and print the line that makes it this shell's.
async fn switch(conn: &mut Conn, json: bool, given: &str) -> Result<()> {
    let v = conn
        .request(
            method::THREAD_LIST,
            serde_json::to_value(ThreadListParams::default())?,
        )
        .await?;
    let list: ThreadListResult = serde_json::from_value(v)?;
    let pick = current::pick(given, &list.threads)?;
    let (slug, title) = match pick {
        Pick::Home => (None, "home".to_string()),
        Pick::Thread(r) => (Some(r.thread.slug.as_str()), r.thread.title.clone()),
    };
    current::remember(slug);
    if json {
        println!("{}", serde_json::json!({"thread": slug}));
    } else {
        match slug {
            Some(s) => println!("export {}={s}", current::ENV),
            None => println!("unset {}", current::ENV),
        }
        eprintln!("current thread: {title}");
    }
    Ok(())
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

/// `merge`, `unmerge` and `split` (theseus-vt9k).
async fn join(conn: &mut Conn, json: bool, cmd: ThreadCmd) -> Result<()> {
    match cmd {
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
        _ => unreachable!("run sends only the joins here"),
    }
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

    #[test]
    fn link_and_unlink_parse_and_say_how_to_undo() {
        use clap::Parser;
        #[derive(Parser)]
        struct Top {
            #[command(subcommand)]
            cmd: ThreadCmd,
        }
        let link = Top::try_parse_from(["t", "link", "io", "tides"])
            .unwrap()
            .cmd;
        assert!(
            matches!(link, ThreadCmd::Link { slug, other } if slug == "io" && other == "tides")
        );
        let un = Top::try_parse_from(["t", "unlink", "io", "tides"])
            .unwrap()
            .cmd;
        assert!(
            matches!(un, ThreadCmd::Unlink { slug, other } if slug == "io" && other == "tides")
        );
        assert!(Top::try_parse_from(["t", "link", "io"]).is_err());
        let r = row("io", ThreadState::Active, None, 0.0);
        let l = link_line(&r, "tides", true);
        assert!(
            l.contains("Linked io with tides") && l.contains("`theseus thread unlink io tides`"),
            "{l}"
        );
        assert!(link_line(&r, "tides", false).contains("Unlinked io from tides"));
    }

    fn args(first: Option<&str>, last: Option<&str>, at: &[&str]) -> split::SplitArgs {
        split::SplitArgs {
            from: "a".into(),
            new: Some("b".into()),
            into: None,
            title: None,
            goal: None,
            gist: None,
            first: first.map(Into::into),
            last: last.map(Into::into),
            ranges: vec![],
            at: at.iter().map(|s| s.to_string()).collect(),
            dry_run: false,
        }
    }

    #[test]
    fn split_takes_a_range_or_positions_and_never_both() {
        let p = split::params(args(Some("@10"), Some("20"), &[])).unwrap();
        assert_eq!((p.range, p.positions), (Some([10, 20]), None));
        let p = split::params(args(None, None, &["@3", "9"])).unwrap();
        assert_eq!((p.range, p.positions), (None, Some(vec![3, 9])));
        let mut many = args(None, None, &[]);
        many.ranges = vec!["@3-@9".into(), "20-30".into()];
        assert_eq!(split::params(many).unwrap().ranges, [[3, 9], [20, 30]]);
        for (first, last, at) in [
            (None, None, vec![]),
            (Some("@1"), None, vec![]),
            (Some("@1"), Some("@2"), vec!["@3"]),
        ] {
            let e = split::params(args(first, last, &at)).unwrap_err();
            let words = e.to_string();
            assert!(
                words.contains("name the turns") || words.contains("come together"),
                "{e}"
            );
        }
        let e = split::params(args(Some("x"), Some("2"), &[])).unwrap_err();
        assert!(e.to_string().contains("no position"), "{e}");
        // Where the turns go: a new thread or an existing one, not both and not neither.
        let mut both = args(Some("1"), Some("2"), &[]);
        both.into = Some("dev".into());
        assert!(split::params(both)
            .unwrap_err()
            .to_string()
            .contains("name where"));
        let mut neither = args(Some("1"), Some("2"), &[]);
        neither.new = None;
        assert!(split::params(neither).is_err());
        let mut into = args(Some("1"), Some("2"), &[]);
        (into.new, into.into) = (None, Some("dev".into()));
        let p = split::params(into).unwrap();
        assert_eq!((p.slug.as_str(), p.into.as_deref()), ("", Some("dev")));
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
        let mut s = theseus_protocol::threads::ThreadSplitResult {
            thread: row("disk", ThreadState::Active, None, 0.0),
            from: "harbour".into(),
            quoted: 6,
            first_pos: 10,
            last_pos: 30,
            noted: false,
            split: "spl_1".into(),
            moved: 14,
            into: false,
        };
        let l = split::split_lines(&s).join("\n");
        assert!(l.contains("6 messages quoted, @10 to @30"), "{l}");
        assert!(l.contains("had a turn running"), "{l}");
        s.noted = true;
        let l = split::split_lines(&s).join("\n");
        assert!(l.contains("14 nodes left harbour's compile"), "{l}");
        assert!(
            l.contains("prefix changed once") && l.contains("no other"),
            "the cache: {l}"
        );
        assert!(l.contains("`theseus thread unsplit spl_1`"), "{l}");
    }
}
