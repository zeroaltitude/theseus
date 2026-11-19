//! `vault.keep.answer` and `vault.keep` (issue 26, line 2): the owner gives the
//! value of a secret to keep in the vault, in a surface's own form, and the
//! daemon writes it with its own token. `docs/design/vault-keep.md` has the
//! whole design.
//!
//! **The value is in this file for one request.** It rides the request in a
//! [`SecretText`] (which prints as nothing), is trimmed and checked, handed to
//! the vault's program on stdin, and dropped. The answers name the vault, the
//! item and the field, never the value; a request that does not parse is
//! refused without echoing what was in it (serde's own message can quote it);
//! no row, record, card, log or error holds it.
//!
//! - `vault.keep.answer`: the card a turn's `vault.keep` put to the owner. The
//!   question's own record says the target; the request names only the
//!   question. It is judged as every answer is (the place rule, no agent's
//!   process, the second proof: a code for a new item, a fresh code for a
//!   replace), and re-checks the target is still what the card showed.
//! - `vault.keep`: the owner's own terminal, with no card and no turn, the one
//!   way to replace an item Theseus's own settings read. Always a fresh code.

use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use theseus_kernel::{Action, ActionState};
use theseus_protocol::vault_keep::{VaultKeepAnswerParams, VaultKeepParams, VaultKept};
use theseus_protocol::{error_code, method, LedgerKind};

use super::confirms::Act;
use super::server::{reply, Conn, RpcFailure};
use super::Core;
use crate::approval::{Answerer, Refusal};
use crate::config_change::vault_keep::done::{self, Outcome};
use crate::config_change::vault_keep::target::{self, Target};
use crate::config_change::vault_keep::{is_card, ARG};
use crate::ledger::LedgerRow;
use crate::owner_proof::{Answer as ProofAnswer, Proof};

/// Params, or a refusal that says nothing of what was in them: serde's message
/// for a wrong type can quote the value it found.
fn parse_quiet<T: DeserializeOwned>(v: Value) -> Result<T, RpcFailure> {
    serde_json::from_value(v).map_err(|_| {
        RpcFailure::new(
            error_code::INVALID_PARAMS,
            "that request is not in the form the daemon takes: a value is text",
        )
    })
}

fn refused(why: impl Into<String>) -> RpcFailure {
    RpcFailure::new(error_code::REFUSED, why)
}

/// A `Refusal` (the place rule, the proof) as the failure a client reads.
fn failure(e: anyhow::Error) -> RpcFailure {
    match e.downcast::<Refusal>() {
        Ok(r) => RpcFailure {
            code: error_code::REFUSED,
            message: r.to_string(),
            data: json!({"who": r.who, "via": r.via, "why": r.why}),
        },
        Err(e) => RpcFailure::invalid(e),
    }
}

/// The two methods, for `rpc_prefixed`'s one arm.
pub(super) const OWN: [&str; 2] = [method::VAULT_KEEP_ANSWER, method::VAULT_KEEP];

impl Core {
    /// The two methods.
    pub(super) async fn rpc_vault_keep(
        self: &Arc<Self>,
        m: &str,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        match m {
            method::VAULT_KEEP_ANSWER => reply(self.keep_answer(parse_quiet(params)?, conn).await?),
            _ => reply(self.keep_direct(parse_quiet(params)?, conn).await?),
        }
    }

    /// The vault's reader, or why this daemon has none.
    fn keep_reader(&self) -> Result<Arc<crate::secrets::OpReader>, RpcFailure> {
        self.tools
            .look
            .reader()
            .filter(|r| !r.has_no_vault())
            .ok_or_else(|| {
                refused(
                    "this Theseus has no vault session (its secrets come from the environment \
                     or from files), so it cannot keep a secret in the vault",
                )
            })
    }

    /// The owner gives the value of a keep card.
    async fn keep_answer(
        self: &Arc<Self>,
        p: VaultKeepAnswerParams,
        conn: Conn<'_>,
    ) -> Result<VaultKept, RpcFailure> {
        let who = conn.answerer(None, None);
        if let Some(job) = p.from_job.as_deref() {
            return Err(refused(format!(
                "this shell is a Theseus job's (THESEUS_SESSION={job} is set), so it would give \
                 a secret in the owner's name: run it from your own terminal"
            )));
        }
        let a = self
            .kernel
            .action(&p.question_id)
            .map_err(RpcFailure::invalid)?
            .filter(|a| a.tool == crate::config_change::ACK)
            .ok_or_else(|| {
                RpcFailure::new(
                    error_code::NOT_FOUND,
                    "no such question waiting for a value",
                )
            })?;
        let args = a
            .proposal
            .as_ref()
            .map(|p| p.args.clone())
            .unwrap_or_default();
        let Some(recorded) = is_card(&args)
            .then(|| Target::of_args(&args[ARG]))
            .flatten()
        else {
            return Err(RpcFailure::invalid(anyhow::anyhow!(
                "that question is not a secret to keep: it takes no value"
            )));
        };
        if a.state != ActionState::Planned {
            return Err(RpcFailure::invalid(anyhow::anyhow!(
                "that question is {}, not waiting for a value",
                a.state.as_str()
            )));
        }
        self.judge_act(
            &who,
            Act::Answer {
                action: &a,
                approve: true,
            },
        )
        .map_err(failure)?;
        self.keep_wall(&a, &who)?;
        let value = done::checked(p.value.expose()).map_err(RpcFailure::invalid_text)?;
        let footing = self
            .proof_gate(&a, &who, p.code.as_ref(), false)
            .map_err(|r| failure(r.into()))?;
        let _ = footing;
        let reader = self.keep_reader()?;
        // The target is re-found: the card is the owner's yes to THIS item and field.
        let refs = target::readers(self);
        let names = (
            recorded.vault_id.as_str(),
            recorded.item_id.as_deref().unwrap_or(&recorded.title),
            recorded.field.as_str(),
        );
        let outcome = match target::resolve(&reader, &refs, names).await {
            Ok(now) if same_place(&now, &recorded) => {
                let o = done::perform(&reader, &recorded, &value).await;
                (o, recorded.clone())
            }
            _ => (Outcome::Changed, recorded.clone()),
        };
        drop(value);
        self.keep_settle(&a, &outcome.1, outcome.0, &who)
            .map_err(failure)?;
        Self::keep_told(&outcome.1, outcome.0)
    }

    /// The owner keeps a secret from their own terminal.
    async fn keep_direct(
        self: &Arc<Self>,
        p: VaultKeepParams,
        conn: Conn<'_>,
    ) -> Result<VaultKept, RpcFailure> {
        let who = self.owners_shell(
            &conn,
            p.from_job.as_deref(),
            "keep a secret in the vault",
            "`theseus vault keep --vault V --item T --field F`",
        )?;
        let value = done::checked(p.value.expose()).map_err(RpcFailure::invalid_text)?;
        let reader = self.keep_reader()?;
        let refs = target::readers(self);
        let t = target::resolve(&reader, &refs, (&p.vault, &p.title, &p.field))
            .await
            .map_err(refused)?;
        // Always a fresh code: from the terminal the owner may replace what a
        // turn may not, and nothing here is a card a window could cover.
        let id = format!("vault.keep:{}/{}", t.vault_id, t.title);
        let place = who.via();
        let answer = ProofAnswer {
            code: p.code.as_ref().map(|c| c.expose()),
            place: &place,
        };
        match self.proofs.judge(&id, true, true, &answer) {
            Proof::Given | Proof::GivenEarlier { .. } => {}
            Proof::Missing | Proof::NotRequired => return Err(RpcFailure {
                code: error_code::REFUSED,
                message:
                    "it needs a fresh code from your authenticator app: `--code <the six digits>`"
                        .into(),
                data: json!({"proof": "needed"}),
            }),
            Proof::Refused(why) => return Err(refused(why.to_string())),
        }
        let outcome = done::perform(&reader, &t, &value).await;
        drop(value);
        let row = LedgerRow::new(
            LedgerKind::VaultKeep,
            None,
            None,
            done::row(&t, outcome, &who.label, &place, None),
        );
        let _ = self.store.append_ledger(&row);
        Self::keep_told(&t, outcome)
    }

    /// What no keep card may be answered from: an agent's process.
    fn keep_wall(&self, a: &Action, who: &Answerer) -> Result<(), RpcFailure> {
        let Some(agent) = &who.agent else {
            return Ok(());
        };
        let r = Refusal {
            who: who.who(),
            via: who.via(),
            why: format!(
                "an agent's process ({agent}) cannot give a secret in the owner's name: the \
                 owner gives it from their own terminal or on its card"
            ),
        };
        let _ = self.refused(
            Act::Answer {
                action: a,
                approve: true,
            },
            who,
            &r,
        );
        Err(failure(r.into()))
    }

    /// The question settles, and the row says how.
    fn keep_settle(
        &self,
        a: &Action,
        t: &Target,
        o: Outcome,
        who: &Answerer,
    ) -> anyhow::Result<()> {
        let row = LedgerRow::new(
            LedgerKind::VaultKeep,
            Some(a.session_id.as_str()),
            None,
            done::row(t, o, &who.label, &who.via(), Some(&a.correlation_id)),
        );
        self.store.append_ledger(&row)?;
        self.settle_keep(a, t, o, &who.label, &who.via())
    }

    /// The answer a client reads: by name, never a value.
    fn keep_told(t: &Target, o: Outcome) -> Result<VaultKept, RpcFailure> {
        let words = done::words(t, o);
        match o {
            Outcome::Kept | Outcome::NotChecked(_) | Outcome::Differs => Ok(VaultKept {
                vault: t.vault.clone(),
                title: t.title.clone(),
                field: t.field.clone(),
                replaced: t.replaces(),
                words,
            }),
            Outcome::NotWritten(_) | Outcome::Changed => Err(refused(words)),
        }
    }
}

/// Whether the vault still shows the place the card named.
fn same_place(now: &Target, then: &Target) -> bool {
    now.vault_id == then.vault_id
        && now.item_id == then.item_id
        && now.field_id == then.field_id
        && now.state == then.state
}

impl RpcFailure {
    fn invalid_text(why: String) -> Self {
        Self::new(error_code::INVALID_PARAMS, why)
    }
}
