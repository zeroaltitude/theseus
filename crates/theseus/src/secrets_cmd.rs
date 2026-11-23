//! `theseus secrets refresh [NAME]` (issue 44): the daemon fetches one
//! `[secrets]` entry again now, or every entry the vault holds. For a value
//! that changed in the vault, an entry you fixed, or the retries that stopped
//! at the day's ceiling of vault reads. Yours alone: `secrets.refresh` is in
//! `client::OPERATORS`, so a job's shell is refused before anything is sent,
//! and the daemon refuses it from a shared place. Once a minute.

use anyhow::Result;
use clap::Subcommand;
use theseus_client::Conn;
use theseus_protocol::{method, SecretsRefreshParams, SecretsRefreshResult};

use crate::cmd::output;

#[derive(Subcommand, Debug)]
pub enum SecretsCmd {
    /// Fetch NAME (a `[secrets]` entry) from the vault again now, or every vault entry when
    /// none is named. `theseus health` then says how it went.
    Refresh {
        /// The entry's name in `[secrets]`.
        name: Option<String>,
    },
}

pub async fn run(conn: &mut Conn, json: bool, cmd: SecretsCmd) -> Result<()> {
    match cmd {
        SecretsCmd::Refresh { name } => {
            let v = conn
                .request(method::SECRETS_REFRESH, SecretsRefreshParams { name })
                .await?;
            output(json, v, |r: SecretsRefreshResult| {
                println!("{}", refresh_line(&r));
                Ok(())
            })
        }
    }
}

/// What a refresh says: what it asked the vault for, and where to look next.
fn refresh_line(r: &SecretsRefreshResult) -> String {
    match r.asked.as_slice() {
        [] => "nothing to fetch: no [secrets] entry is read from the vault".to_string(),
        asked => format!(
            "asked the vault again for {}; `theseus health` says how it went",
            asked.join(", ")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refresh_says_what_it_asked_for() {
        let r = SecretsRefreshResult {
            asked: vec!["anthropic_api_key".into(), "courier_token".into()],
        };
        assert_eq!(
            refresh_line(&r),
            "asked the vault again for anthropic_api_key, courier_token; `theseus health` says \
             how it went"
        );
        assert_eq!(
            refresh_line(&SecretsRefreshResult::default()),
            "nothing to fetch: no [secrets] entry is read from the vault"
        );
    }
}
