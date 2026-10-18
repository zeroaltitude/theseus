//! `theseus persona rate` (theseus-slvl.9): what Theseus said first. `persona
//! rate up|down [ID]` is the owner's verdict on a message it sent unprompted
//! (a wake's or a task's reply, or a note it wrote itself), one
//! `initiative.rate` call; with no ID, the latest one. The week's totals are
//! in `theseus persona`. The verdict is in `client::OPERATORS`: refused
//! inside a job.

use anyhow::Result;
use theseus_client::Conn;
use theseus_protocol::initiative::{InitiativeRateParams, InitiativeRateResult};
use theseus_protocol::method;

use crate::cmd::output;

/// `theseus persona rate up|down [ID]`: one `initiative.rate`.
pub async fn rate(
    conn: &mut Conn,
    json: bool,
    verdict: String,
    id: Option<String>,
    session: Option<String>,
) -> Result<()> {
    let p = InitiativeRateParams {
        session_id: session,
        turn_id: id,
        rating: verdict,
        via: Some("cli".into()),
        discord: None,
    };
    let v = conn.request(method::INITIATIVE_RATE, p).await?;
    output(json, v, |r: InitiativeRateResult| {
        println!("{}", rated_line(&r));
        Ok(())
    })
}

/// What a recorded verdict says.
pub fn rated_line(r: &InitiativeRateResult) -> String {
    let thumb = if r.rating == "up" { "👍" } else { "👎" };
    format!("{thumb} recorded on {}", r.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_verdict_says_which_message_it_is_on() {
        let r = InitiativeRateResult {
            id: "ini_0a1b".into(),
            turn_id: "ini_0a1b".into(),
            rating: "down".into(),
        };
        assert_eq!(rated_line(&r), "👎 recorded on ini_0a1b");
    }
}
