//! `theseus route report [--week]` and `theseus turn replay …` (own-benchmark,
//! theseus-pw1q.12): which model your own turns need, and one past turn's
//! first request sent to another profile. The report reads rows; the replay
//! spends money and is the owner's (refused inside a job, `client::OPERATORS`).

use anyhow::Result;
use clap::Subcommand;
use theseus_client::render::own_bench::{replay_lines, report_lines};
use theseus_client::Conn;
use theseus_protocol::method;
use theseus_protocol::own_bench::{
    RouteReportParams, RouteReportResult, TurnReplayParams, TurnReplayResult,
};

use crate::cmd::output;

#[derive(Subcommand, Debug)]
pub enum RouteCmd {
    /// Per (model, effort) cell of your own turns: the turns and their spend, the turns replayed
    /// one rung down the routing table and what `compare.v1` made of them, the turns you checked,
    /// and the dollars moving the cell down would have saved at the stated confidence; and the
    /// replays' cost against `[route] replay_day_usd`.
    Report {
        /// The last seven days (the default is today).
        #[arg(long)]
        week: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum TurnCmd {
    /// Rebuild a past turn's first request byte for byte, send it to another profile with no tool
    /// run, and judge its reply against the original's (`compare.v1`). Writes one `turn.replayed`
    /// row and nothing else. Refused when the request cannot be rebuilt, naming the part.
    Replay {
        /// The turn's session.
        session: String,
        /// The turn.
        turn: String,
        /// The profile to send it to.
        #[arg(long, short = 'P')]
        profile: String,
        /// The effort to run it at (low, medium, high, xhigh, max).
        #[arg(long)]
        effort: Option<String>,
        /// Do not judge the replies.
        #[arg(long)]
        no_compare: bool,
    },
}

/// The two commands, flattened into the CLI's own.
#[derive(Subcommand, Debug)]
pub enum OwnBench {
    /// Your own benchmark (theseus-pw1q.12): which model your own turns need, measured by
    /// replaying them one rung down the routing table.
    Route {
        #[command(subcommand)]
        cmd: RouteCmd,
    },
    /// A past turn: `turn replay SESSION TURN -P PROFILE`, its first request rebuilt byte for byte
    /// and sent to another profile (yours alone; refused inside a job).
    Turn {
        #[command(subcommand)]
        cmd: TurnCmd,
    },
}

pub async fn run(conn: &mut Conn, json: bool, cmd: OwnBench) -> Result<()> {
    match cmd {
        OwnBench::Route { cmd } => route(conn, json, cmd).await,
        OwnBench::Turn { cmd } => turn(conn, json, cmd).await,
    }
}

async fn route(conn: &mut Conn, json: bool, cmd: RouteCmd) -> Result<()> {
    match cmd {
        RouteCmd::Report { week } => {
            let p = RouteReportParams {
                week: week.then_some(true),
            };
            let v = conn
                .request(method::ROUTE_REPORT, serde_json::to_value(&p)?)
                .await?;
            output(json, v, |r: RouteReportResult| {
                for l in report_lines(&r) {
                    println!("{l}");
                }
                Ok(())
            })
        }
    }
}

async fn turn(conn: &mut Conn, json: bool, cmd: TurnCmd) -> Result<()> {
    match cmd {
        TurnCmd::Replay {
            session,
            turn,
            profile,
            effort,
            no_compare,
        } => {
            let p = TurnReplayParams {
                session,
                turn,
                profile,
                effort,
                scope: None,
                compare: no_compare.then_some(false),
            };
            let v = conn
                .request(method::TURN_REPLAY, serde_json::to_value(&p)?)
                .await?;
            output(json, v, |r: TurnReplayResult| {
                for l in replay_lines(&r) {
                    println!("{l}");
                }
                Ok(())
            })
        }
    }
}
