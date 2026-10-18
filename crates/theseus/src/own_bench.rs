//! `theseus model bench [--week]` and `theseus model replay …` (own-benchmark,
//! theseus-pw1q.12), with `model prefix` and `model catalog`: which model your own turns need, and
//! one past turn's first request (or, with `--whole`, the whole turn) sent to another profile. The
//! report reads rows; the replay spends money and is the owner's (refused inside a job,
//! `client::OPERATORS`).

use anyhow::Result;
use clap::Subcommand;
use theseus_client::render::own_bench::{replay_lines, report_lines};
use theseus_client::Conn;
use theseus_protocol::method;
use theseus_protocol::own_bench::{
    RouteReportParams, RouteReportResult, TurnReplayParams, TurnReplayResult,
};

use crate::cmd::output;

/// `theseus model …`: the commands about models and their cost.
#[derive(Subcommand, Debug)]
pub enum ModelCmd {
    /// Per (model, effort) cell of your own turns: the turns and their spend, the turns replayed
    /// one rung down the routing table and what `compare.v1` made of them, the turns you checked,
    /// and the dollars moving the cell down would have saved at the stated confidence; and the
    /// replays' cost against `[route_bench] replay_day_usd`.
    Bench {
        /// The last seven days (the default is today).
        #[arg(long)]
        week: bool,
    },
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
        /// Run the whole turn, tools and all, on a scratch copy of its directory: programs in L1
        /// with their writes discarded, tools that would reach you or the world answered with a
        /// recorded stub (the row names each), under `[route_bench] whole_loops|whole_seconds|
        /// whole_usd`. The replies are judged and the tests, the paths changed and the exit codes
        /// compared; a confident match becomes a machine label for routing's learning (train
        /// split only). Refused when the turn's directory cannot be put back as it was.
        #[arg(long)]
        whole: bool,
    },
    /// The prefix library: its picks, the cache writes they saved, and the false fits with their
    /// scores.
    Prefix {
        /// The newest picks read, at most (2,000).
        #[arg(long)]
        limit: Option<u32>,
    },
    /// The model catalog: context windows, output limits, prices per million tokens.
    Catalog,
}

pub async fn run(conn: &mut Conn, json: bool, cmd: ModelCmd) -> Result<()> {
    match cmd {
        ModelCmd::Bench { week } => bench(conn, json, week).await,
        ModelCmd::Replay {
            session,
            turn,
            profile,
            effort,
            no_compare,
            whole,
        } => {
            let p = TurnReplayParams {
                session,
                turn,
                profile,
                effort,
                scope: whole.then(|| "whole_turn".to_string()),
                compare: no_compare.then_some(false),
            };
            replay(conn, json, p).await
        }
        ModelCmd::Prefix { limit } => {
            crate::prefix_cmd::run(conn, json, crate::prefix_cmd::PrefixCmd::Report { limit }).await
        }
        ModelCmd::Catalog => crate::cmd::catalog(conn, json).await,
    }
}

async fn bench(conn: &mut Conn, json: bool, week: bool) -> Result<()> {
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

async fn replay(conn: &mut Conn, json: bool, p: TurnReplayParams) -> Result<()> {
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
