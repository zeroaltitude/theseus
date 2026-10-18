//! `theseus remind` and `theseus wake pause|resume|snooze` (theseus-8ux6x): the
//! owner's verbs on wakes, with no model turn (a fired reminder's done and snooze are
//! `theseus answer ID done|snooze`). Each is one call
//! (`wake.create`, `wake.snooze`, `wake.pause`, `wake.resume`, `wake.done`) that
//! the daemon answers in one frame, whatever the conversation is doing.
//!
//! A reminder set with no `--session` lives in the owner's Home: the
//! conversation their Discord DM runs on, or a "Reminders" conversation when no
//! DM is bound. When it is due its note is posted as it is, with Done, Snooze 10
//! min, Snooze 1 h and Tomorrow 9:00, and every surface that watches hears it.

use anyhow::Result;
use theseus_client::Conn;
use theseus_protocol::{
    method, WakeCreateParams, WakeEditResult, WakeNameParams, WakePauseParams, WakeSnoozeParams,
};

use crate::cmd::output;

/// `theseus wake …`: the pending wakes, and the verbs on one.
#[derive(clap::Subcommand, Debug)]
pub enum WakeCmd {
    /// Pending wakes, soonest first, with session, due time, and note (default).
    List {
        /// Only this session's wakes.
        #[arg(long, short)]
        session: Option<String>,
    },
    /// Hold a wake: a series keeps its place and passes its occurrences over (`--until WHEN`
    /// moves it past that time instead). `wake resume` lets it go on.
    Pause {
        #[arg(value_name = "ID")]
        wake: String,
        #[arg(long)]
        until: Option<String>,
    },
    /// Let a paused wake go on: a series at its next occurrence, numbered by its place.
    Resume {
        #[arg(value_name = "ID")]
        wake: String,
    },
    /// Move a wake or a reminder that fired to WHEN (10 minutes by default): `theseus wake snooze
    /// 3f9a1c 1h`, `theseus wake snooze wak_… tomorrow 9:00` (a reminder that fired is named by its
    /// whole id, as its notice carries it).
    Snooze {
        #[arg(value_name = "ID")]
        wake: String,
        /// `10m`, `1h`, `tomorrow 9:00`, `fri 9:00`.
        when: Option<String>,
    },
    /// Cancel a wake, so nothing fires; the same handler as `theseus cancel ID`, which also stops a
    /// task.
    Cancel {
        #[arg(value_name = "ID")]
        name: String,
    },
}

impl Default for WakeCmd {
    fn default() -> Self {
        Self::List { session: None }
    }
}

/// `theseus wake`: one arm of `main`'s `run`, which is at clippy's line limit.
pub async fn wake(conn: &mut Conn, json: bool, cmd: WakeCmd) -> Result<()> {
    match cmd {
        WakeCmd::List { session } => crate::cmd::wakes(conn, json, session).await,
        WakeCmd::Pause { wake, until } => pause(conn, json, wake, until).await,
        WakeCmd::Resume { wake } => resume(conn, json, wake).await,
        WakeCmd::Snooze { wake, when } => snooze(conn, json, wake, when).await,
        WakeCmd::Cancel { name } => crate::cmd::cancel(conn, json, name).await,
    }
}

/// `theseus remind`'s arguments.
#[derive(clap::Args, Debug)]
pub struct RemindArgs {
    /// When: `in 10m`, `10m`, `tomorrow 9:00`, `fri 9:00`, `17:30`, `2026-10-14 09:00`, or an
    /// RFC 3339 time, in the daemon's zone.
    pub when: String,
    /// What it says: the whole message, posted as it is.
    #[arg(required = true)]
    pub note: Vec<String>,
    /// Repeat it at this span: 30m, 1d, 1w.
    #[arg(long)]
    pub every: Option<String>,
    /// With `--every 1d`: the weekdays it runs on (`--days mon,fri`).
    #[arg(long, value_delimiter = ',')]
    pub days: Vec<String>,
    /// With `--every`: no occurrence after this time.
    #[arg(long)]
    pub until: Option<String>,
    /// Keep it in this conversation instead of your Home.
    #[arg(long, short)]
    pub session: Option<String>,
}

/// `theseus remind WHEN NOTE`: WHEN is `in 10m`, `10m`, `tomorrow 9:00`, `fri
/// 9:00`, `17:30`, `2026-10-14 09:00`, or an RFC 3339 time.
pub async fn remind(conn: &mut Conn, json: bool, p: RemindArgs) -> Result<()> {
    let v = conn
        .request(
            method::WAKE_CREATE,
            WakeCreateParams {
                when: p.when,
                note: p.note.join(" "),
                every: p.every,
                days: p.days,
                until: p.until,
                session_id: p.session,
                author: None,
            },
        )
        .await?;
    output(json, v, |r: WakeEditResult| {
        let w = &r.wake;
        let series = w
            .every
            .as_deref()
            .map(|e| format!(", every {e}"))
            .unwrap_or_default();
        let home = if r.home {
            " (in your Home conversation)"
        } else {
            ""
        };
        println!(
            "reminder {} set for {}{series}{home}: it is posted as it is, with no model turn; \
             `theseus wake snooze {}`, `theseus cancel {}`",
            w.short, w.due_local, w.short, w.short
        );
        Ok(())
    })
}

/// `theseus wake snooze ID [WHEN]` and `theseus answer ID snooze [WHEN]`: move a pending wake, or set a
/// reminder that fired again, for WHEN (10 minutes by default).
pub async fn snooze(conn: &mut Conn, json: bool, wake: String, when: Option<String>) -> Result<()> {
    let fired = wake.starts_with("wak_");
    let v = conn
        .request(
            method::WAKE_SNOOZE,
            WakeSnoozeParams {
                wake,
                when,
                fired,
                author: None,
            },
        )
        .await?;
    output(json, v, |r: WakeEditResult| {
        println!(
            "wake {} will fire {}: {}",
            r.wake.short, r.wake.due_local, r.wake.note
        );
        Ok(())
    })
}

/// `theseus wake pause ID [--until WHEN]`.
pub async fn pause(conn: &mut Conn, json: bool, wake: String, until: Option<String>) -> Result<()> {
    let v = conn
        .request(
            method::WAKE_PAUSE,
            WakePauseParams {
                wake,
                until,
                author: None,
            },
        )
        .await?;
    output(json, v, |r: WakeEditResult| {
        if r.wake.paused {
            println!(
                "wake {} paused: it keeps its place, and `theseus wake resume {}` lets it go on",
                r.wake.short, r.wake.short
            );
        } else {
            println!("wake {} moved to {}", r.wake.short, r.wake.due_local);
        }
        Ok(())
    })
}

/// `theseus wake resume ID`.
pub async fn resume(conn: &mut Conn, json: bool, wake: String) -> Result<()> {
    let v = conn
        .request(method::WAKE_RESUME, WakeNameParams { wake, author: None })
        .await?;
    output(json, v, |r: WakeEditResult| {
        println!("wake {} goes on: next {}", r.wake.short, r.wake.due_local);
        Ok(())
    })
}
