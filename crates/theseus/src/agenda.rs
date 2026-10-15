//! `theseus agenda` (theseus-nuhsz; horizon HZ8): one view of the future.
//! Now, Today, This week, Later, Recurring, Waiting on the world and
//! Projects, from the work board's future views (`work.watch`) and the
//! occurrences of the next 30 days (`work.agenda`), with the verbs that act
//! on what is listed. `--watch` keeps it current from the push; a redraw at
//! the local midnight moves a reminder from This week to Today with no frame
//! from the daemon (the day is this client's).
//!
//! The words are `theseus_client::render::horizon`, which the TUI's Horizon
//! section shares. `--json` is the raw answers.

use std::collections::BTreeMap;
use std::io::IsTerminal;
use std::time::Duration;

use anyhow::Result;
use clap::Args;
use serde_json::Value;
use theseus_client::render::horizon::{build, lines, local_day, window};
use theseus_client::Conn;
use theseus_protocol::notify;
use theseus_protocol::work::horizon::{AgendaParams, AgendaResult};
use theseus_protocol::work::WorkView;
use theseus_protocol::{method, now_unix_ms, Event, Message, WorkWatchParams, WorkWatchResult};

#[derive(Args, Debug)]
pub struct AgendaArgs {
    /// Keep the connection and draw it again at each change, and at the local midnight, until
    /// Ctrl-C.
    #[arg(long)]
    watch: bool,
    /// The time to draw it at, in Unix ms: a test's clock.
    #[arg(long, hide = true)]
    now: Option<u64>,
}

/// The views the board holds, by id, applied by the position rule.
#[derive(Default)]
struct Held {
    views: BTreeMap<String, WorkView>,
}

impl Held {
    fn load(&mut self, snap: WorkWatchResult) {
        self.views = snap.views.into_iter().map(|v| (v.id.clone(), v)).collect();
    }

    fn apply(&mut self, v: WorkView) {
        if self
            .views
            .get(&v.id)
            .is_none_or(|o| v.position > o.position)
        {
            self.views.insert(v.id.clone(), v);
        }
    }
}

async fn snapshot(
    conn: &mut Conn,
    interactive: bool,
    early: &mut Vec<(String, Value)>,
) -> Result<(Value, Held)> {
    let params = WorkWatchParams {
        limit: Some(0),
        roots: None,
        interactive: Some(interactive),
    };
    let v = conn
        .call(method::WORK_WATCH, serde_json::to_value(params)?, |m, p| {
            early.push((m.to_string(), p.clone()))
        })
        .await?;
    let mut held = Held::default();
    held.load(serde_json::from_value(v.clone())?);
    Ok((v, held))
}

async fn occurrences(conn: &mut Conn, now: u64) -> Result<(Value, AgendaResult)> {
    let (from_ms, to_ms) = window(now);
    let p = AgendaParams {
        from_ms,
        to_ms,
        roots: None,
    };
    let v = conn
        .call(method::WORK_AGENDA, serde_json::to_value(p)?, |_, _| {})
        .await?;
    Ok((v.clone(), serde_json::from_value(v)?))
}

fn draw(held: &Held, agenda: &AgendaResult, now: u64) -> Vec<String> {
    let views: Vec<WorkView> = held.views.values().cloned().collect();
    let a = build(&views, &agenda.lines, now, &local_day(now));
    lines(&a).into_iter().map(|l| l.text).collect()
}

/// Run `theseus agenda`.
pub async fn run(conn: &mut Conn, json: bool, a: AgendaArgs) -> Result<()> {
    let mut early: Vec<(String, Value)> = Vec::new();
    let (raw, mut held) = snapshot(conn, a.watch, &mut early).await?;
    let now = a.now.unwrap_or_else(now_unix_ms);
    let (raw_agenda, mut agenda) = occurrences(conn, now).await?;
    if json {
        println!(
            "{}",
            serde_json::json!({"board": raw, "agenda": raw_agenda})
        );
    } else {
        for l in draw(&held, &agenda, now) {
            println!("{l}");
        }
    }
    if !a.watch {
        let _ = conn
            .call(method::WORK_UNWATCH, serde_json::json!({}), |_, _| {})
            .await;
        return Ok(());
    }
    let tty = std::io::stdout().is_terminal();
    for (m, p) in early {
        if let Some(Event::WorkChanged(c)) = Event::from_notification(&m, &p)? {
            c.views.into_iter().for_each(|v| held.apply(v));
        }
    }
    loop {
        // Until the local midnight, when the day moves with no frame.
        let tomorrow = local_day(now_unix_ms()).tomorrow_ms;
        let until = Duration::from_millis(tomorrow.saturating_sub(now_unix_ms()).max(1_000));
        let mut redraw = false;
        tokio::select! {
            msg = conn.next() => {
                let Some(Message::Notification(n)) = msg? else { return Ok(()); };
                if n.method == notify::EVENTS_LOST {
                    let mut lost = Vec::new();
                    held = snapshot(conn, true, &mut lost).await?.1;
                    redraw = true;
                } else if let Some(Event::WorkChanged(c)) = Event::from_notification(&n.method, &n.params)? {
                    for id in &c.gone {
                        held.views.remove(id);
                    }
                    c.views.into_iter().for_each(|v| held.apply(v));
                    redraw = true;
                }
            }
            () = tokio::time::sleep(until) => redraw = true,
        }
        if redraw {
            let now = now_unix_ms();
            agenda = occurrences(conn, now).await?.1;
            if json {
                continue;
            }
            if tty {
                print!("\x1b[2J\x1b[H");
            } else {
                println!("---");
            }
            for l in draw(&held, &agenda, now) {
                println!("{l}");
            }
        }
    }
}
