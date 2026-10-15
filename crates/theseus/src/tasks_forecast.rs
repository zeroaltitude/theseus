//! `theseus tasks forecast` (theseus-zz752): how often a task's forecast
//! held. One read, `forecast.calibration`: of the tasks that finished in the
//! last 7 and 30 days, how many came in inside their p90 of dollars and of
//! minutes, by class. Each task is judged against the forecast it would have
//! had when it opened (the tasks that had finished before it), so the number
//! is a backtest, not a promise. The words are the protocol's
//! (`ForecastCalibrationResult::lines`).

use anyhow::Result;
use clap::Subcommand;
use theseus_client::Conn;
use theseus_protocol::forecast::{ForecastCalibrationParams, ForecastCalibrationResult};
use theseus_protocol::method;

#[derive(Subcommand, Debug)]
pub enum TasksCmd {
    /// How many tasks of the last 7 and 30 days finished inside their forecast's p90 of dollars
    /// and of minutes, by class.
    Forecast,
}

/// `theseus tasks`: the list, or a subcommand.
pub async fn dispatch(
    conn: &mut Conn,
    json: bool,
    session: Option<String>,
    cmd: Option<TasksCmd>,
) -> Result<()> {
    match cmd {
        Some(k) => run(conn, json, k).await,
        None => crate::cmd::tasks(conn, json, session).await,
    }
}

async fn run(conn: &mut Conn, json: bool, cmd: TasksCmd) -> Result<()> {
    match cmd {
        TasksCmd::Forecast => {
            let v = conn
                .request(method::FORECAST_CALIBRATION, ForecastCalibrationParams {})
                .await?;
            crate::cmd::output(json, v, |r: ForecastCalibrationResult| {
                for l in r.lines() {
                    println!("{l}");
                }
                Ok(())
            })
        }
    }
}
