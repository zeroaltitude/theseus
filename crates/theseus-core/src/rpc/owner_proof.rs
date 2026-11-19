//! `owner.proof.*` (issue 22): pairing the owner's authenticator app, its
//! status, its reset, and ending every open window. **From the owner's own
//! shell and nowhere else**: only the CLI on the daemon's own socket (whose
//! mode is the owner's alone), never from a Theseus job's shell
//! (`THESEUS_SESSION`), a process under an agent, the web UI, Discord or MCP.
//! A refusal is a ledger row (`owner.proof.changed`, what `refused`) and says
//! where to run it.
//!
//! The secret leaves the daemon once, in `owner.proof.enroll`'s answer, to the
//! terminal that asked, and in no other answer, row, log or card after it.
//! `lock` is the one of the five a private place may also make: it only ends
//! windows, which tightens.

use serde_json::{json, Value};
use theseus_protocol::owner_proof::{
    OwnerProofCodeParams, OwnerProofDone, OwnerProofEnrollParams, OwnerProofEnrollResult,
    OwnerProofStatus, ProofWindow,
};
use theseus_protocol::{error_code, method};

use super::server::{parse, reply, Conn, RpcFailure};
use super::Core;
use crate::approval::{Answerer, Surface};

/// Where the owner runs these.
const OWN_CMD: &str = "`theseus owner proof <enroll|status|reset>`";

impl Core {
    /// The five methods, from one dispatch arm.
    pub(super) fn rpc_owner_proof(
        &self,
        m: &str,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        match m {
            method::OWNER_PROOF_ENROLL => {
                let p: OwnerProofEnrollParams = parse(params)?;
                let who = self.owners_shell(
                    &conn,
                    p.from_job.as_deref(),
                    "pair an authenticator app",
                    OWN_CMD,
                )?;
                let offer = self
                    .proofs
                    .begin_enrol(&who.via())
                    .map_err(|e| refused(e.to_string()))?;
                reply(OwnerProofEnrollResult {
                    uri: offer.uri,
                    secret: offer.base32,
                })
            }
            method::OWNER_PROOF_CONFIRM => {
                let p: OwnerProofCodeParams = parse(params)?;
                let who = self.owners_shell(
                    &conn,
                    p.from_job.as_deref(),
                    "pair an authenticator app",
                    OWN_CMD,
                )?;
                let code = typed(&p)?;
                self.proofs
                    .confirm_enrol(code, &who.via())
                    .map_err(|e| refused(e.to_string()))?;
                reply(OwnerProofDone::default())
            }
            method::OWNER_PROOF_STATUS => {
                let p: OwnerProofEnrollParams = parse(params)?;
                self.owners_shell(
                    &conn,
                    p.from_job.as_deref(),
                    "read the authenticator's status",
                    OWN_CMD,
                )?;
                reply(self.owner_proof_status())
            }
            method::OWNER_PROOF_RESET => {
                let p: OwnerProofCodeParams = parse(params)?;
                let who = self.owners_shell(
                    &conn,
                    p.from_job.as_deref(),
                    "reset the authenticator",
                    OWN_CMD,
                )?;
                let code = typed(&p)?;
                self.proofs.abandon_enrol();
                self.proofs
                    .reset(code, &who.via())
                    .map_err(|e| refused(e.to_string()))?;
                reply(OwnerProofDone::default())
            }
            _ => {
                let p: OwnerProofCodeParams = parse(params)?;
                let who = self.proof_lockers(&conn, p.from_job.as_deref())?;
                let closed = self
                    .proofs
                    .lock(&who.label, &who.via())
                    .map_err(|e| refused(e.to_string()))?;
                reply(OwnerProofDone {
                    closed: u32::try_from(closed).unwrap_or(u32::MAX),
                })
            }
        }
    }

    fn owner_proof_status(&self) -> OwnerProofStatus {
        let s = self.proofs.status();
        OwnerProofStatus {
            enrolled: s.enrolled,
            enrolled_at_ms: s.enrolled_at_ms,
            locked_until_ms: s.locked_until_ms,
            unavailable: s.unavailable,
            window_hours: self
                .cfg
                .owner_proof
                .window_hours
                .min(crate::owner_proof::MAX_WINDOW_HOURS),
            windows: s
                .windows
                .into_iter()
                .map(|w| ProofWindow {
                    place: w.place,
                    opened_at_ms: w.opened_at_ms,
                    until_ms: w.until_ms,
                })
                .collect(),
        }
    }

    /// The owner's own shell: the CLI on this machine's socket, with no
    /// Theseus job's marker and no agent's process behind it. Anything else is
    /// refused, ledgered, and told where to run it.
    pub(super) fn owners_shell(
        &self,
        conn: &Conn<'_>,
        from_job: Option<&str>,
        what: &str,
        run: &str,
    ) -> Result<Answerer, RpcFailure> {
        let who = conn.answerer(None, None);
        let why = match (who.surface, from_job, &who.agent) {
            (Surface::Cli, None, None) => return Ok(who),
            (Surface::Cli, Some(job), _) => format!(
                "this shell is a Theseus job's (THESEUS_SESSION={job} is set), so it would {what} \
                 for the owner"
            ),
            (Surface::Cli, None, Some(agent)) => {
                format!("it came from an agent's process ({agent}), and only the owner can {what}")
            }
            (other, _, _) => format!(
                "it came through {}, and only the owner's own terminal can {what}",
                other.name()
            ),
        };
        let words = format!("{why}. Run it from your own terminal: {run}");
        self.proofs.changed("refused", &who.via(), Some(&words));
        Err(RpcFailure {
            code: error_code::REFUSED,
            message: words,
            data: json!({"who": who.who(), "via": who.via(), "why": why}),
        })
    }

    /// Who may end the windows: the owner's own terminal, or the owner in a
    /// private place; never a job, an agent's process, or MCP. It only
    /// tightens.
    fn proof_lockers(
        &self,
        conn: &Conn<'_>,
        from_job: Option<&str>,
    ) -> Result<Answerer, RpcFailure> {
        let who = conn.answerer(None, None);
        let why = match (&who.agent, from_job) {
            (None, None) => who
                .unknown()
                .map_or_else(|| self.may_answer(&who).err(), Some),
            (Some(agent), _) => Some(format!("it came from an agent's process ({agent})")),
            (None, Some(job)) => Some(format!(
                "this shell is a Theseus job's (THESEUS_SESSION={job} is set)"
            )),
        };
        match why {
            None => Ok(who),
            Some(why) => {
                self.proofs.changed("refused", &who.via(), Some(&why));
                Err(RpcFailure {
                    code: error_code::REFUSED,
                    message: format!("only the owner ends the proof's windows: {why}"),
                    data: json!({"who": who.who(), "via": who.via(), "why": why}),
                })
            }
        }
    }
}

fn refused(why: String) -> RpcFailure {
    RpcFailure::new(error_code::REFUSED, why)
}

/// The code a request carries, or why there is none.
fn typed(p: &OwnerProofCodeParams) -> Result<&str, RpcFailure> {
    match &p.code {
        Some(c) if !c.is_blank() => Ok(c.expose()),
        _ => Err(RpcFailure::new(
            error_code::INVALID_PARAMS,
            "that takes the six digits your authenticator app shows: `--code <digits>`".to_string(),
        )),
    }
}
