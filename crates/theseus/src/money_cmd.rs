//! `theseus budgets reopen|settle|forgive` (issue 6; theseus-x3m9, theseus-cv3v): the owner's
//! two words on money the kernel holds or has stopped. Each is the owner's, from their own shell: inside a
//! job the CLI refuses them (`OPERATORS`), and the daemon refuses anyone else. Neither changes a default.

use anyhow::Result;
use clap::Subcommand;
use theseus_client::Conn;
use theseus_protocol::method;
use theseus_protocol::money::{
    ActionSettleParams, ActionSettleResult, ExecutionReopenParams, ExecutionReopenResult,
};

use crate::cmd::output;

/// What `theseus budgets` does besides listing.
#[derive(Debug, Subcommand)]
pub enum BudgetsCmd {
    /// Reopen an execution that ended at its dollar limit: it gets the new limit, which must be above
    /// what it has spent, and waits for your next message. The limit is its own; no default changes.
    Reopen {
        /// The execution's id.
        execution: String,
        /// Its new limit, in dollars.
        #[arg(long, value_name = "DOLLARS")]
        limit: f64,
    },
    /// Settle a call whose spend is held as unknown, at what it cost (from the provider's bill): the held
    /// amount is released into that cost. Its outcome stays unknown; only the money is settled.
    Settle {
        /// The call's id (`corr_…`), as `theseus budgets` and the ledger name it.
        call: String,
        /// What it cost, in dollars.
        #[arg(long, value_name = "DOLLARS")]
        usd: f64,
    },
    /// Forgive a call whose spend is held as unknown: it counts as nothing, and its row says so, so the
    /// books are never silent.
    Forgive {
        /// The call's id (`corr_…`).
        call: String,
    },
}

/// `theseus budgets`, and its subcommands.
pub async fn run(conn: &mut Conn, json: bool, cmd: Option<BudgetsCmd>) -> Result<()> {
    match cmd {
        None => crate::budgets::budgets(conn, json).await,
        Some(BudgetsCmd::Reopen { execution, limit }) => reopen(conn, json, execution, limit).await,
        Some(other) => settle(conn, json, other).await,
    }
}

/// `theseus budgets reopen EXECUTION --limit DOLLARS`.
async fn reopen(conn: &mut Conn, json: bool, execution: String, limit: f64) -> Result<()> {
    let p = ExecutionReopenParams {
        execution_id: execution,
        limit_usd: limit,
    };
    let v = conn.request(method::EXECUTION_REOPEN, p).await?;
    output(json, v, |r: ExecutionReopenResult| {
        println!("{}", reopened_words(&r));
        Ok(())
    })
}

/// What a reopen says: what it had spent against its old limit, the new limit, and that it waits for you.
pub(crate) fn reopened_words(r: &ExecutionReopenResult) -> String {
    format!(
        "Reopened {}: it had spent ${:.2} against its ${:.2} limit, its limit is now ${:.2} (its own: \
         the default is unchanged), and it waits for your next message.",
        r.execution_id, r.spent_usd, r.was_limit_usd, r.limit_usd
    )
}

/// `theseus budgets settle|forgive`.
async fn settle(conn: &mut Conn, json: bool, cmd: BudgetsCmd) -> Result<()> {
    let p = match cmd {
        BudgetsCmd::Settle { call, usd } => ActionSettleParams {
            correlation_id: call,
            usd: Some(usd),
        },
        BudgetsCmd::Forgive { call } => ActionSettleParams {
            correlation_id: call,
            usd: None,
        },
        BudgetsCmd::Reopen { .. } => unreachable!("a reopen is run by `reopen`"),
    };
    let v = conn.request(method::ACTION_SETTLE, p).await?;
    output(json, v, |r: ActionSettleResult| {
        println!("{}", settled_words(&r));
        Ok(())
    })
}

/// What a settle or a forgiveness says.
pub(crate) fn settled_words(r: &ActionSettleResult) -> String {
    match r.forgiven {
        true => format!(
            "Forgave {}: its ${:.2}, held as unknown spend, is released and counts as nothing.",
            r.correlation_id, r.held_usd
        ),
        false => format!(
            "Settled {}: it was held as unknown spend at ${:.2} and now counts as ${:.2}.",
            r.correlation_id, r.held_usd, r.usd
        ),
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;
    use crate::Cmd;

    #[test]
    fn the_verbs_parse() {
        let c =
            crate::Cli::try_parse_from(["theseus", "budgets", "reopen", "exe_1", "--limit", "25"])
                .unwrap();
        assert!(matches!(
            c.cmd,
            Cmd::Budgets { cmd: Some(BudgetsCmd::Reopen { limit, .. }) } if (limit - 25.0).abs() < 1e-9
        ));
        let c =
            crate::Cli::try_parse_from(["theseus", "budgets", "settle", "corr_1", "--usd", "0.40"])
                .unwrap();
        assert!(matches!(
            c.cmd,
            Cmd::Budgets { cmd: Some(BudgetsCmd::Settle { usd, .. }) } if (usd - 0.4).abs() < 1e-9
        ));
        let c = crate::Cli::try_parse_from(["theseus", "budgets", "forgive", "corr_1"]).unwrap();
        assert!(matches!(
            c.cmd,
            Cmd::Budgets {
                cmd: Some(BudgetsCmd::Forgive { .. })
            }
        ));
        // The listing is still the bare command.
        let c = crate::Cli::try_parse_from(["theseus", "budgets"]).unwrap();
        assert!(matches!(c.cmd, Cmd::Budgets { cmd: None }));
        // A reopen needs its limit, and a settle its amount: no guessing at money.
        assert!(crate::Cli::try_parse_from(["theseus", "budgets", "reopen", "exe_1"]).is_err());
        assert!(crate::Cli::try_parse_from(["theseus", "budgets", "settle", "corr_1"]).is_err());
    }

    #[test]
    fn the_answers_say_what_changed() {
        let r = ExecutionReopenResult {
            execution_id: "exe_1".into(),
            spent_usd: 5.0,
            was_limit_usd: 5.0,
            limit_usd: 10.0,
        };
        let said = reopened_words(&r);
        assert!(
            said.contains("spent $5.00 against its $5.00 limit"),
            "{said}"
        );
        assert!(said.contains("now $10.00"), "{said}");
        let settle = ActionSettleResult {
            correlation_id: "corr_1".into(),
            execution_id: "exe_1".into(),
            usd: 0.4,
            held_usd: 1.0,
            forgiven: false,
        };
        assert!(settled_words(&settle)
            .contains("held as unknown spend at $1.00 and now counts as $0.40"));
        let forgave = ActionSettleResult {
            forgiven: true,
            usd: 0.0,
            ..settle
        };
        assert!(settled_words(&forgave).starts_with("Forgave corr_1: its $1.00"));
    }
}
