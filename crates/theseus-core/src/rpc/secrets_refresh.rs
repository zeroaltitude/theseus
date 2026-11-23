//! `secrets.refresh` (issue 44): the owner fetches a `[secrets]` entry
//! again now, or every entry the vault holds. For a value that changed in
//! the vault, an entry the owner fixed, or the retries that stopped at the
//! day's ceiling of vault reads (`secrets::pace`). **The owner's, in a
//! private place** (`places::owner_in_private`, as `calendar.window` judges
//! the asker), refused inside a job by the CLI (`OPERATORS`), and at most
//! once a minute (`SecretBoard::refresh`). It reads the vault and changes
//! nothing else: no act, so no card and no `judge_act` arm. The answer names
//! what was asked; the round runs behind it, and health says how it went.

use theseus_protocol::{error_code, SecretsRefreshParams, SecretsRefreshResult};

use super::server::{Conn, RpcFailure};
use super::Core;

impl Core {
    /// `secrets.refresh`, from its dispatch arm.
    pub(super) fn secrets_refresh(
        &self,
        p: SecretsRefreshParams,
        conn: Conn<'_>,
    ) -> Result<SecretsRefreshResult, RpcFailure> {
        let who = conn.answerer(None, None);
        crate::places::owner_in_private(&who, &self.runner.place_rule, &self.cfg).map_err(
            |why| {
                RpcFailure::new(
                    error_code::REFUSED,
                    format!("secrets.refresh is the owner's, in a private place: {why}"),
                )
            },
        )?;
        let asked = self
            .secrets
            .refresh(p.name.as_deref())
            .map_err(|why| RpcFailure::new(error_code::INVALID_PARAMS, why))?;
        tracing::info!(
            entries = asked.len(),
            "the owner asked for the vault's entries again"
        );
        Ok(SecretsRefreshResult { asked })
    }
}
