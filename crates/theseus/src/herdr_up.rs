//! `theseus herdr up [--follow] [--boot]` (theseus-fuw5d): herdr kept in step with
//! Theseus's tasks by itself. `up` lays out herdr's `theseus` workspace for the
//! tasks there are, a tab each, and exits; run again, it sends herdr only reads.
//! `--follow` keeps it so: one follower process, in the workspace's `board` tab,
//! with one `work.watch` connection to the daemon and one `events.subscribe` to
//! herdr. `--boot` (herdr's startup hook) first closes what a herdr restart left, the
//! panes whose `kind` token is gone, then follows. Run from anywhere but the board
//! tab, `--follow` makes the board tab (its pane runs `up --follow --here`) and
//! exits; `--here` is the follower itself.
//!
//! A tab runs `theseus watch <session> --interactive` as the pane's argv, so nothing
//! is typed into a shell. The rules of which tab exists and when it closes are
//! `state.rs`'s; herdr's request shapes are `wire.rs`'s. The follower sets no timer
//! while nothing awaits closing, and one deadline otherwise.
//!
//! The Space row also says what happened since the owner last looked (`work.digest`
//! from the machine's seen position, read on every connect and when a task ends) and
//! the horizon's two numbers, the next reminder and the routines due. They get no tabs.
//!
//! A session is pinned (`--pin`) by a `pinned` token on its pane, so a later `up`
//! keeps and restarts its tab (theseus-d5oc).

mod horizon;
mod state;
pub(crate) mod wire;

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::Args;
use serde_json::{json, Value};
use theseus_client::{seen, Conn};
use theseus_protocol::work::{WorkKind, WorkState};
use theseus_protocol::{
    method, notify, DigestKind, Event, Message, SessionKind, SessionListResult, WorkDigestParams,
    WorkDigestResult, WorkWatchParams, WorkWatchResult,
};
use tokio::time::Instant;

use self::state::{Act, Counts, Follower, PaneRow};
use self::wire::{Events, Herdr, TabSpec, BOARD_KIND};
use crate::herdr;
use crate::herdr_sync::{default_socket, resolve_pins, WORKSPACE};

/// How long a finished task's tab stays after it was seen.
const FINISHED_GRACE: u64 = 600;

/// `theseus herdr up`.
#[derive(Args, Debug)]
pub struct UpArgs {
    /// Stay and keep herdr in step: a tab for each task as it starts, closed after it ends
    /// and is seen. Starts the board tab's follower unless this is it (`--here`).
    #[arg(long)]
    pub follow: bool,
    /// herdr's startup hook: first close the panes a herdr restart left in the `theseus`
    /// workspace (no `kind` token), then lay out and follow.
    #[arg(long)]
    pub boot: bool,
    /// Keep a tab for this session whatever its state (repeatable); remembered by a token
    /// on its pane. SESSION is its id, or at least its last four characters.
    #[arg(long = "pin", value_name = "SESSION")]
    pub pin: Vec<String>,
    /// herdr's API socket. Default: $HERDR_SOCKET_PATH, else herdr's own.
    #[arg(long, value_name = "PATH")]
    pub herdr_socket: Option<PathBuf>,
    /// Seconds a finished task's tab stays after it was seen.
    #[arg(long, value_name = "SECS", default_value_t = FINISHED_GRACE)]
    pub finished_grace: u64,
    /// Run the follower in this process (what the board tab runs).
    #[arg(long)]
    pub here: bool,
}

/// What `up` knows of its two sockets.
struct Ctx {
    exe: PathBuf,
    socket: String,
    h: Herdr,
    workspace_id: Option<String>,
    json: bool,
}

pub async fn run(socket: &str, spawn: Option<&str>, json: bool, a: UpArgs) -> Result<()> {
    if spawn.is_some() {
        bail!(
            "`theseus herdr up` needs a running theseusd, not --spawn: the watches it starts \
             connect to the daemon's socket"
        );
    }
    let socket = theseus_client::client::tilde(socket, std::env::var("HOME").ok());
    let herdr_socket = match &a.herdr_socket {
        Some(p) => p.clone(),
        None => default_socket()?,
    };
    let mut ctx = Ctx {
        exe: std::env::current_exe().context("finding this theseus binary")?,
        socket,
        h: Herdr::new(herdr_socket),
        workspace_id: None,
        json,
    };
    let follow = a.follow || a.boot;
    if a.boot {
        let closed = boot_close(&mut ctx).await?;
        say(
            &ctx,
            &format!("closed {closed} pane(s) a herdr restart left"),
        );
    }
    if follow && !a.here {
        return board_tab(&mut ctx).await;
    }
    let grace = Duration::from_secs(a.finished_grace);
    let mut f = Follower::new(grace);
    pin(&mut ctx, &mut f, &a.pin).await?;
    if follow {
        return follow_loop(ctx, f).await;
    }
    one_shot(&mut ctx, f).await
}

fn say(ctx: &Ctx, line: &str) {
    if !ctx.json {
        println!("{line}");
    }
}

/// The panes of the `theseus` workspace, and the workspace's id.
async fn panes_of(ctx: &mut Ctx) -> Result<Vec<Value>> {
    let w = ctx.h.call("workspace.list", json!({})).await?;
    ctx.workspace_id = w["workspaces"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|w| w["label"] == WORKSPACE)
        .and_then(|w| w["workspace_id"].as_str())
        .map(str::to_string);
    let Some(ws) = ctx.workspace_id.clone() else {
        return Ok(Vec::new());
    };
    let p = ctx.h.call("pane.list", json!({})).await?;
    Ok(p["panes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["workspace_id"] == ws.as_str())
        .cloned()
        .collect())
}

/// Close what a herdr restart left: a pane in our workspace with no `kind` token is a
/// restored shell. The pane this process runs in is never closed.
async fn boot_close(ctx: &mut Ctx) -> Result<usize> {
    let own = std::env::var("HERDR_PANE_ID").ok();
    let mut n = 0;
    for p in panes_of(ctx).await? {
        let (Some(id), None) = (p["pane_id"].as_str(), p["tokens"]["kind"].as_str()) else {
            continue;
        };
        if own.as_deref() != Some(id) {
            ctx.h.call("pane.close", json!({ "pane_id": id })).await?;
            n += 1;
        }
    }
    Ok(n)
}

/// Make the board tab unless there is one, and say where the follower runs.
async fn board_tab(ctx: &mut Ctx) -> Result<()> {
    let have = panes_of(ctx)
        .await?
        .iter()
        .any(|p| p["tokens"]["kind"] == BOARD_KIND);
    if have {
        say(
            ctx,
            "the board tab is there: its follower keeps herdr in step",
        );
        return Ok(());
    }
    ctx.h
        .call("layout.apply", wire::board_params(&ctx.exe, &ctx.socket))
        .await?;
    say(
        ctx,
        "made the board tab: it follows the tasks and keeps herdr in step",
    );
    Ok(())
}

/// Pins from `--pin`, as sessions the follower keeps a tab for.
async fn pin(ctx: &mut Ctx, f: &mut Follower, pins: &[String]) -> Result<()> {
    if pins.is_empty() {
        return Ok(());
    }
    let mut conn = Conn::socket(&ctx.socket).await?;
    let sessions = serde_json::from_value::<SessionListResult>(
        conn.request(method::SESSION_LIST, Value::Null).await?,
    )?
    .sessions;
    for sid in resolve_pins(pins, &sessions)? {
        let Some(s) = sessions.iter().find(|s| s.session_id == sid) else {
            continue;
        };
        let kind = match s.kind {
            SessionKind::Task => WorkKind::Task,
            SessionKind::Conversation => WorkKind::Conversation,
        };
        f.pin(&sid, kind, theseus_client::names::of(s));
    }
    Ok(())
}

/// The board as the daemon holds it now: `work.watch`'s snapshot, with the
/// notifications that came before it, then the connection kept.
async fn snapshot(conn: &mut Conn) -> Result<(WorkWatchResult, Vec<(String, Value)>)> {
    let mut early = Vec::new();
    let params = WorkWatchParams {
        limit: Some(0),
        roots: None,
        interactive: Some(false),
    };
    let v = conn
        .call(method::WORK_WATCH, serde_json::to_value(params)?, |m, p| {
            early.push((m.to_string(), p.clone()));
        })
        .await?;
    Ok((serde_json::from_value(v)?, early))
}

/// How many things happened since the owner last looked at the board on this machine:
/// the digest's lines after the seen position, bar the owner's own answers. Read only
/// (the look is not recorded), on a connection of its own so the watch's notifications
/// are not read past. 0 on a first start, and when the daemon does not answer.
async fn since_looked(socket: &str) -> u32 {
    let looked = seen::Seen::default_path()
        .as_deref()
        .map(seen::looked)
        .unwrap_or_default();
    if looked.position == 0 {
        return 0;
    }
    let ask = async {
        let mut conn = Conn::socket(socket).await?;
        let params = WorkDigestParams {
            since: Some(looked.position),
            ..Default::default()
        };
        let v = conn
            .call(
                method::WORK_DIGEST,
                serde_json::to_value(params)?,
                |_, _| {},
            )
            .await?;
        anyhow::Ok(serde_json::from_value::<WorkDigestResult>(v)?)
    };
    match tokio::time::timeout(Duration::from_secs(2), ask).await {
        Ok(Ok(res)) => res
            .lines
            .iter()
            .filter(|l| l.kind != DigestKind::Answered)
            .map(|l| l.count)
            .sum(),
        _ => 0,
    }
}

/// Herdr's tabs read into the follower.
async fn refresh(ctx: &mut Ctx, f: &mut Follower, now: Instant) -> Result<()> {
    let rows: Vec<PaneRow> = panes_of(ctx)
        .await?
        .iter()
        .filter_map(PaneRow::from_value)
        .collect();
    f.set_panes(&rows, &HashMap::new(), now);
    Ok(())
}

fn spec_of(f: &Follower, sid: &str) -> TabSpec {
    let t = &f.tasks[sid];
    let name = if t.name.is_empty() {
        let tail = &sid[sid.len().saturating_sub(6)..];
        format!("{} {tail}", t.kind.as_str())
    } else {
        t.name.clone()
    };
    TabSpec {
        sid: sid.to_string(),
        label: crate::herdr_slots::fit(&name),
        kind: if t.kind == WorkKind::Task {
            "task"
        } else {
            "conversation"
        },
        pinned: t.pinned,
    }
}

/// Do the plan: open what is missing in one `layout.apply`, close what is due. The
/// lines say what was done.
async fn execute(ctx: &mut Ctx, f: &mut Follower, now: Instant) -> Result<Vec<String>> {
    let mut lines = Vec::new();
    for act in f.plan(now) {
        match act {
            Act::Open(sids) => {
                let specs: Vec<TabSpec> = sids.iter().map(|s| spec_of(f, s)).collect();
                let params = wire::tabs_params(&ctx.exe, &ctx.socket, &specs);
                let answer = ctx.h.call("layout.apply", params).await?;
                if let Some(ws) = answer["workspace"]["workspace_id"].as_str() {
                    ctx.workspace_id = Some(ws.to_string());
                }
                let made = wire::made_tabs(&answer);
                for ((spec, tab), sid) in specs.iter().zip(made).zip(&sids) {
                    // The pane's tokens by id too, so a layout that ignores them is mended.
                    let _ = ctx
                        .h
                        .call(
                            "pane.report_metadata",
                            wire::pane_tokens_params(&tab.pane_id, spec),
                        )
                        .await;
                    lines.push(format!("opened a tab for {} ({sid})", spec.label));
                    f.opened(sid, tab, now);
                }
            }
            Act::Close { tab_id, sid } => {
                ctx.h.call("tab.close", json!({ "tab_id": tab_id })).await?;
                lines.push(format!("closed the tab of {sid}"));
                f.closed(&sid);
            }
        }
    }
    Ok(lines)
}

/// `up` without `--follow`: one read of the board, one lay-out, done.
async fn one_shot(ctx: &mut Ctx, mut f: Follower) -> Result<()> {
    let mut conn = Conn::socket(&ctx.socket).await?;
    let (snap, _) = snapshot(&mut conn).await?;
    let _ = conn.request(method::WORK_UNWATCH, json!({})).await;
    let now = Instant::now();
    f.apply(&snap.views, &[], true, now, herdr::now_ms());
    refresh(ctx, &mut f, now).await?;
    let lines = execute(ctx, &mut f, now).await?;
    if ctx.json {
        println!("{}", json!({"steps": lines, "herdr_requests": ctx.h.n}));
    } else if lines.is_empty() {
        println!("herdr is in step: nothing changed");
    } else {
        lines.iter().for_each(|l| println!("{l}"));
    }
    Ok(())
}

// ---------------------------------------------------------------- the follower

/// The follower's two sockets, either of which may be away.
struct Link {
    daemon: Option<Conn>,
    events: Option<Events>,
    /// The daemon was lost and said so; the next connect says it is back.
    lost_said: bool,
    sent: Option<Counts>,
    view_set: Option<String>,
    /// A task ended since the digest was read: read it again.
    digest_stale: bool,
}

const RETRY_FROM: Duration = Duration::from_millis(250);
const RETRY_TO: Duration = Duration::from_secs(5);

async fn follow_loop(mut ctx: Ctx, mut f: Follower) -> Result<()> {
    mark_board_pane(&mut ctx).await;
    let mut link = Link {
        daemon: None,
        events: None,
        lost_said: false,
        sent: None,
        view_set: None,
        digest_stale: false,
    };
    let mut wait = RETRY_FROM;
    loop {
        connect_both(&mut ctx, &mut f, &mut link).await;
        settle(&mut ctx, &mut f, &mut link).await;
        let away = link.daemon.is_none() || link.events.is_none();
        let deadline = deadline_of(&f);
        tokio::select! {
            m = next_daemon(&mut link.daemon) => daemon_said(&mut ctx, &mut f, &mut link, m).await,
            e = next_event(&mut link.events) => herdr_said(&mut ctx, &mut f, &mut link, e).await,
            () = until(deadline) => {}
            () = retry(away, &mut wait) => {}
            () = stop_signal() => return Ok(()),
        }
        if !away {
            wait = RETRY_FROM;
        }
    }
}

/// The one timer the follower keeps: a tab's close or reopen, or a routine coming due.
fn deadline_of(f: &Follower) -> Option<Instant> {
    let now = Instant::now();
    [f.deadline(), f.horizon_deadline(now, herdr::now_ms())]
        .into_iter()
        .flatten()
        .min()
}

/// The pane this process runs in is the board's: a token says so, for a later boot.
async fn mark_board_pane(ctx: &mut Ctx) {
    if let Ok(pane) = std::env::var("HERDR_PANE_ID") {
        let tokens = json!({"kind": BOARD_KIND});
        let p = json!({"pane_id": pane, "source": herdr::SOURCE, "agent": "theseus-board",
                       "tokens": tokens, "seq": herdr::next_seq()});
        let _ = ctx.h.call("pane.report_metadata", p).await;
    }
}

async fn connect_both(ctx: &mut Ctx, f: &mut Follower, link: &mut Link) {
    if link.daemon.is_none() {
        if let Ok(mut conn) = Conn::socket(&ctx.socket).await {
            if let Ok((snap, early)) = snapshot(&mut conn).await {
                let now = Instant::now();
                f.apply(&snap.views, &[], true, now, herdr::now_ms());
                f.forget_missing(&snap.views);
                apply_early(f, &early, now);
                f.since = since_looked(&ctx.socket).await;
                print_board(f);
                if link.lost_said {
                    link.lost_said = false;
                    notice(ctx, "daemon is back").await;
                }
                link.daemon = Some(conn);
            }
        }
    }
    if link.events.is_none() {
        if let Ok(e) = Events::connect(&ctx.h.socket).await {
            let _ = refresh(ctx, f, Instant::now()).await;
            link.events = Some(e);
        }
    }
}

fn apply_early(f: &mut Follower, early: &[(String, Value)], now: Instant) {
    for (m, p) in early {
        if let Ok(Some(Event::WorkChanged(c))) = Event::from_notification(m, p) {
            f.apply(&c.views, &c.gone, false, now, herdr::now_ms());
        }
    }
}

/// Lay out what the board and herdr say, and put the counts on the Space row.
async fn settle(ctx: &mut Ctx, f: &mut Follower, link: &mut Link) {
    if link.events.is_none() {
        return;
    }
    match execute(ctx, f, Instant::now()).await {
        Ok(lines) => lines.iter().for_each(|l| println!("· {l}")),
        Err(e) => eprintln!("theseus: herdr: {e:#}; laid out again at the next change"),
    }
    let Some(ws) = ctx.workspace_id.clone() else {
        return;
    };
    if link.view_set.as_deref() != Some(&ws) {
        let _ = ctx.h.call("agent.view.set", wire::view_params(&ws)).await;
        link.view_set = Some(ws.clone());
    }
    if std::mem::take(&mut link.digest_stale) {
        f.since = since_looked(&ctx.socket).await;
    }
    let counts = f.counts(herdr::now_ms());
    if link.sent.as_ref() != Some(&counts) {
        let _ = ctx
            .h
            .call(
                "workspace.report_metadata",
                wire::counts_params(&ws, &counts),
            )
            .await;
        link.sent = Some(counts);
    }
}

async fn next_daemon(c: &mut Option<Conn>) -> Result<Option<Message>> {
    match c {
        Some(c) => c.next().await,
        None => std::future::pending().await,
    }
}

async fn next_event(e: &mut Option<Events>) -> Result<wire::Event> {
    match e {
        Some(e) => e.next().await,
        None => std::future::pending().await,
    }
}

async fn until(at: Option<Instant>) {
    match at {
        Some(t) => tokio::time::sleep_until(t).await,
        None => std::future::pending().await,
    }
}

/// A timer only while a socket is away: the tries double to 5 s.
async fn retry(away: bool, wait: &mut Duration) {
    if away {
        tokio::time::sleep(*wait).await;
        *wait = (*wait * 2).min(RETRY_TO);
    } else {
        std::future::pending::<()>().await;
    }
}

async fn daemon_said(ctx: &mut Ctx, f: &mut Follower, link: &mut Link, m: Result<Option<Message>>) {
    let now = Instant::now();
    match m {
        Ok(Some(Message::Notification(n))) if n.method == notify::EVENTS_LOST => {
            if let Some(c) = link.daemon.as_mut() {
                if let Ok((snap, early)) = snapshot(c).await {
                    f.apply(&snap.views, &[], true, now, herdr::now_ms());
                    f.forget_missing(&snap.views);
                    apply_early(f, &early, now);
                    f.since = since_looked(&ctx.socket).await;
                    print_board(f);
                }
            }
        }
        Ok(Some(Message::Notification(n))) => {
            if let Ok(Some(Event::WorkChanged(c))) = Event::from_notification(&n.method, &n.params)
            {
                let applied = f.apply(&c.views, &c.gone, false, now, herdr::now_ms());
                for v in &applied.changed {
                    println!("{}", crate::work::change_line(v, herdr::now_ms()));
                }
                link.digest_stale |= applied.changed.iter().any(|v| v.state.ended());
            }
        }
        Ok(Some(_)) => {}
        Ok(None) | Err(_) => {
            link.daemon = None;
            if !link.lost_said {
                link.lost_said = true;
                notice(ctx, "daemon restarting").await;
            }
        }
    }
}

async fn herdr_said(ctx: &mut Ctx, f: &mut Follower, link: &mut Link, e: Result<wire::Event>) {
    let now = Instant::now();
    let Ok(e) = e else {
        link.events = None;
        return;
    };
    match e.name.as_str() {
        "pane.exited" => {
            if let Some(p) = e.id("pane_id") {
                f.pane_exited(p);
            }
        }
        "tab.focused" => {
            if let Some(t) = e.id("tab_id") {
                f.focus(t, now);
            }
        }
        "pane.focused" => {
            if let Some(p) = e.id("pane_id") {
                f.focus(p, now);
            }
        }
        "events.lost" => {
            let _ = refresh(ctx, f, now).await;
        }
        _ => {}
    }
}

/// One notice of the follower's own: herdr already toasts every needs-you.
async fn notice(ctx: &mut Ctx, body: &str) {
    let _ = ctx
        .h
        .call(
            "notification.show",
            herdr::notice_params("Theseus", body, false),
        )
        .await;
}

/// The queue the board tab shows: what needs you, what works, what finished unseen.
fn print_board(f: &Follower) {
    let line = |t: &state::Task| match &t.last {
        Some(v) => crate::work::view_line(v, herdr::now_ms()),
        None => format!("{} {}", t.kind.as_str(), t.name),
    };
    let mut sections: [(&str, Vec<String>); 3] = [
        ("NEEDS YOU", Vec::new()),
        ("WORKING", Vec::new()),
        ("DONE NOT YET SEEN", Vec::new()),
    ];
    for t in f.tasks.values() {
        let i = if t.state == WorkState::NeedsYou || t.level == theseus_protocol::Level::NeedsYou {
            0
        } else if t.state.working() {
            1
        } else if t.state.ended() && t.seen_at.is_none() && t.news {
            2
        } else {
            continue;
        };
        sections[i].1.push(line(t));
    }
    for (title, lines) in sections.iter().filter(|(_, l)| !l.is_empty()) {
        println!("{title}");
        lines.iter().for_each(|l| println!("  {l}"));
    }
}

/// Until this process is asked to end.
async fn stop_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let (Ok(mut term), Ok(mut int), Ok(mut hup)) = (
        signal(SignalKind::terminate()),
        signal(SignalKind::interrupt()),
        signal(SignalKind::hangup()),
    ) else {
        return std::future::pending().await;
    };
    tokio::select! {
        _ = term.recv() => {}
        _ = int.recv() => {}
        _ = hup.recv() => {}
    }
}
