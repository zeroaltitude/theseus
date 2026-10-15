//! `theseus self narrow …` (theseus-pw1q.15): the narrower policies Theseus
//! proposes for its hand roles from what CloudTrail showed them calling.
//! `list` and `show` read; `run`, `apply`, `decline` and `revert` are the
//! owner's (inside a job the CLI refuses them, `OPERATORS`, and the daemon
//! refuses a non-owner and ledgers the refusal). An apply changes IAM through
//! a change set of the hands stack, and only on the owner's word.

use anyhow::Result;
use clap::Subcommand;
use theseus_client::Conn;
use theseus_protocol::method;
use theseus_protocol::rsi::{SelfNarrowParams, SelfNarrowResult};

use crate::cmd::output;

#[derive(Subcommand, Debug)]
pub enum NarrowCmd {
    /// The proposals, newest first, with their state.
    List,
    /// One proposal whole: the evidence, what goes, what stays, the checks, and the change as JSON.
    Show {
        /// The proposal's id, or the end of it (four characters or more).
        proposal: String,
    },
    /// Look at a role now instead of at the weekly pass: the owner's. A proposal goes to your
    /// card; `--dry` only prints it.
    Run {
        /// `owner` or `read`; both when left out.
        #[arg(long)]
        profile: Option<String>,
        /// Only look: make the reads and print what would be proposed; keep and post nothing.
        /// Works with `[self] mode = "off"`.
        #[arg(long)]
        dry: bool,
    },
    /// Apply a proposal: the owner's. Plans a change set of the hands stack, checks it touches
    /// only the role and its new policy, and runs it.
    Apply { proposal: String },
    /// Not now: the role is looked at again in a week.
    Decline { proposal: String },
    /// Bring back the policy from before an applied narrowing.
    Revert { proposal: String },
}

pub async fn run(conn: &mut Conn, json: bool, cmd: Option<NarrowCmd>) -> Result<()> {
    let from_job = theseus_client::client::job_session();
    let p = match cmd.unwrap_or(NarrowCmd::List) {
        NarrowCmd::List => SelfNarrowParams {
            action: "list".into(),
            ..Default::default()
        },
        NarrowCmd::Show { proposal } => SelfNarrowParams {
            action: "show".into(),
            proposal: Some(proposal),
            ..Default::default()
        },
        NarrowCmd::Run { profile, dry } => SelfNarrowParams {
            action: "run".into(),
            profile,
            dry,
            from_job,
            ..Default::default()
        },
        NarrowCmd::Apply { proposal } => answer("apply", proposal, from_job),
        NarrowCmd::Decline { proposal } => answer("decline", proposal, from_job),
        NarrowCmd::Revert { proposal } => answer("revert", proposal, from_job),
    };
    let v = conn
        .request(method::SELF_NARROW, serde_json::to_value(&p)?)
        .await?;
    output(json, v, |r: SelfNarrowResult| {
        print!("{}", r.text);
        Ok(())
    })
}

fn answer(action: &str, proposal: String, from_job: Option<String>) -> SelfNarrowParams {
    SelfNarrowParams {
        action: action.into(),
        proposal: Some(proposal),
        from_job,
        ..Default::default()
    }
}
