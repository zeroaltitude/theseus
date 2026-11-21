//! `theseus judge correct` (theseus-q31l): the owner's correction of a
//! turn's routing, judged as a label is (the owner, from a private place;
//! refused inside a job, `client::refuse_in_a_job`).

use anyhow::Result;
use clap::Args;
use theseus_client::Conn;
use theseus_protocol::method;
use theseus_protocol::route::{RouteCorrectParams, RouteCorrectResult};

use crate::cmd::output;

#[derive(Args, Debug)]
pub struct CorrectArgs {
    /// Where it should have run: a profile (`fable`), a task class of the route pack (`deep_coding`),
    /// `stronger`, or `cheaper`; or who should have answered: `persona:<name>`.
    to: String,
    /// The session whose turn it was.
    #[arg(long, short, value_name = "SESSION")]
    session: String,
    /// The turn (`turn_…`); none: the session's last turn routing decided.
    #[arg(long, value_name = "TURN")]
    turn: Option<String>,
}

/// `theseus judge correct`: the owner's label on the turn's route judgment,
/// and the session's next turn where the owner says.
pub async fn correct(conn: &mut Conn, json: bool, a: CorrectArgs) -> Result<()> {
    let p = RouteCorrectParams {
        session_id: a.session,
        turn_id: a.turn,
        to: a.to,
        via: Some("cli".into()),
        provenance: None,
        discord: None,
    };
    let v = conn.request(method::ROUTE_CORRECT, p).await?;
    output(json, v, |r: RouteCorrectResult| {
        println!("{}", r.line);
        Ok(())
    })
}
