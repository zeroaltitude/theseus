//! `aws.bootstrap` (AWS design §5, C2 = 14b): the operator's plan of an
//! account's first stacks, and its apply on their yes. The CLI's alone:
//! refused from Discord and from the web UI. The CLI refuses it inside a
//! Theseus job (theseus-zmgb), a speed bump; L1 is the boundary.

use theseus_protocol::{
    error_code, AwsBootstrapParams, AwsBootstrapResult, AwsConfirmAlertsParams,
    AwsConfirmAlertsResult,
};

use super::server::{Conn, RpcFailure};
use crate::approval::Surface;
use crate::aws::{alerts, bootstrap};

impl crate::Core {
    pub(super) async fn aws_bootstrap(
        &self,
        p: AwsBootstrapParams,
        conn: Conn<'_>,
    ) -> Result<AwsBootstrapResult, RpcFailure> {
        if conn.surface != Surface::Cli {
            return Err(RpcFailure::new(
                error_code::REFUSED,
                "aws.bootstrap is the operator's, from the CLI on this machine: \
                 theseus aws bootstrap",
            ));
        }
        let aws = self.tools.aws.clone().ok_or_else(|| {
            RpcFailure::new(
                error_code::INVALID_PARAMS,
                "no AWS account is bound: the config has no [aws.accounts.<id>] table",
            )
        })?;
        let account = aws
            .account(p.account.as_deref())
            .map_err(|e| RpcFailure::new(error_code::INVALID_PARAMS, e))?
            .clone();
        let failed = |e: String| RpcFailure::new(error_code::INTERNAL, e);
        match p.apply.clone() {
            None => {
                let plan = bootstrap::plan(&account, &p).await.map_err(failed)?;
                Ok(bootstrap::result(&account, &plan, false))
            }
            Some(digest) => {
                let plan = bootstrap::apply(&account, &p, &digest)
                    .await
                    .map_err(failed)?;
                tracing::info!(account = %account.id, digest = %digest, "aws: the bootstrap applied");
                Ok(bootstrap::result(&account, &plan, true))
            }
        }
    }

    /// `aws.confirm_alerts` (theseus-9p40): the alerts subscription
    /// confirmed with the token from SNS's email, authenticated on
    /// unsubscribe. The CLI's alone, as the bootstrap is. The token is never
    /// logged or kept.
    pub(super) async fn aws_confirm_alerts(
        &self,
        p: AwsConfirmAlertsParams,
        conn: Conn<'_>,
    ) -> Result<AwsConfirmAlertsResult, RpcFailure> {
        if conn.surface != Surface::Cli {
            return Err(RpcFailure::new(
                error_code::REFUSED,
                "aws.confirm_alerts is the operator's, from the CLI on this machine: \
                 theseus aws confirm-alerts",
            ));
        }
        let aws = self.tools.aws.clone().ok_or_else(|| {
            RpcFailure::new(
                error_code::INVALID_PARAMS,
                "no AWS account is bound: the config has no [aws.accounts.<id>] table",
            )
        })?;
        let account = aws
            .account(p.account.as_deref())
            .map_err(|e| RpcFailure::new(error_code::INVALID_PARAMS, e))?
            .clone();
        let token = alerts::token_of(&p.token)
            .map_err(|e| RpcFailure::new(error_code::INVALID_PARAMS, e))?;
        alerts::confirm(&account, &token)
            .await
            .map_err(|e| RpcFailure::new(error_code::INTERNAL, e))
    }
}
