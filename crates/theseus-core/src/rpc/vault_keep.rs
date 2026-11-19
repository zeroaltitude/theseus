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
use theseus_kernel::Action;
use theseus_protocol::vault_keep::{VaultKeepAnswerParams, VaultKeepParams, VaultKept};
use theseus_protocol::{error_code, method, LedgerKind};

use super::confirms::Act;
use super::server::{reply, Conn, RpcFailure};
use super::Core;
use crate::approval::{Answerer, Refusal, Surface};
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

/// The three methods, for `rpc_prefixed`'s one arm.
pub(super) const OWN: [&str; 3] = [
    method::VAULT_KEEP_ANSWER,
    method::VAULT_KEEP,
    method::VAULT_KEEP_WAITING,
];

/// An attempt that stopped: what the client reads, and the class its row is
/// written with. `None` when the settlement has written its own row.
pub(super) struct Stop {
    pub failure: RpcFailure,
    pub class: Option<&'static str>,
}

/// `map_err` for a failure of `class`.
fn stop(class: &'static str) -> impl Fn(RpcFailure) -> Stop {
    move |failure| Stop {
        failure,
        class: Some(class),
    }
}

impl Core {
    /// The three methods. An attempt to give a value that stops writes a row
    /// saying what stopped it (a class, never a value) before the client is told.
    pub(super) async fn rpc_vault_keep(
        self: &Arc<Self>,
        m: &str,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        let pid = conn.pid;
        // The card, and who the binding says pressed: the row of an attempt that stopped names both, as the
        // rows of its outcomes do, so a form's refusal says where it was typed.
        let (question, presser, done) = match m {
            method::VAULT_KEEP_WAITING => {
                return reply(self.keep_waiting(parse_quiet(params)?, &conn).await?)
            }
            method::VAULT_KEEP_ANSWER => match parse_quiet::<VaultKeepAnswerParams>(params) {
                Ok(p) => {
                    let q = Some(p.question_id.clone());
                    let by = (conn.vouched_author(p.author.as_deref()), p.discord.clone());
                    (q, by, self.keep_answer(p, &conn).await.map(reply))
                }
                Err(f) => (None, (None, None), Err(stop("bad_request")(f))),
            },
            _ => match parse_quiet::<VaultKeepParams>(params) {
                Ok(p) => (
                    None,
                    (None, None),
                    self.keep_direct(p, &conn).await.map(reply),
                ),
                Err(f) => (None, (None, None), Err(stop("bad_request")(f))),
            },
        };
        match done {
            Ok(r) => r,
            Err(Stop { failure, class }) => {
                if let Some(class) = class {
                    let who = conn.answerer(presser.0, presser.1);
                    self.keep_stopped(class, &who, pid, question.as_deref());
                }
                Err(failure)
            }
        }
    }

    /// The row of an attempt that stopped: its class, who, where, which process,
    /// and the card when there was one.
    pub(super) fn keep_stopped(
        &self,
        class: &str,
        who: &Answerer,
        pid: Option<u32>,
        question: Option<&str>,
    ) {
        let data = done::attempt_row(
            "stopped",
            class,
            (&who.label, &who.via(), pid),
            question,
            None,
        );
        let _ = self
            .store
            .append_ledger(&LedgerRow::new(LedgerKind::VaultKeep, None, None, data));
    }

    /// The vault's reader, or why this daemon has none.
    pub(super) fn keep_reader(&self) -> Result<Arc<crate::secrets::OpReader>, RpcFailure> {
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
        conn: &Conn<'_>,
    ) -> Result<VaultKept, Stop> {
        // The presser a form's submit names, as the Discord binding read it off the press: judged
        // below as every press is. Any other surface that names one is refused by `judge_act`.
        let who = conn.answerer(conn.vouched_author(p.author.as_deref()), p.discord.clone());
        if let Some(job) = p.from_job.as_deref() {
            return Err(stop("job")(refused(format!(
                "this shell is a Theseus job's (THESEUS_SESSION={job} is set), so it would give \
                 a secret in the owner's name: run it from your own terminal"
            ))));
        }
        let (a, recorded) = self.keep_card(&p.question_id)?;
        self.judge_act(
            &who,
            Act::Answer {
                action: &a,
                approve: true,
            },
        )
        .map_err(|e| stop("place")(failure(e)))?;
        self.keep_wall(&a, &who).map_err(stop("agent"))?;
        self.keep_place(&a, &who)?;
        // The binding's form found the value one this daemon would refuse and sent none: the attempt is
        // recorded by its class and told in the daemon's words. Nothing in `value` is read.
        if let Some(refusal) = p.refused {
            return Err(stop(refusal.class())(RpcFailure::invalid_text(
                done::refusal_words(refusal),
            )));
        }
        let value = done::checked(p.value.expose()).map_err(|why| {
            let class = if p.value.is_blank() {
                "blank"
            } else {
                "not_a_value"
            };
            stop(class)(RpcFailure::invalid_text(why))
        })?;
        // One answer at a time for a question: two surfaces answering at once must not both write.
        let _one = InFlight::take(&a.correlation_id).ok_or_else(|| {
            stop("in_flight")(refused(
                "another answer to this question is being kept right now: wait for it",
            ))
        })?;
        let reader = self.keep_reader().map_err(stop("no_vault"))?;
        // The target is re-found BEFORE a code is spent: the card is the owner's yes to THIS
        // item and field, and a vault that did not answer is no reason to use a code up. The
        // account's permission to write there is asked again, here: it may have been taken away
        // since the card was put.
        let refs = target::readers(self);
        let names = (
            recorded.vault_id.as_str(),
            recorded.item_id.as_deref().unwrap_or(&recorded.title),
            recorded.field_id.as_deref().unwrap_or(&recorded.field),
        );
        let now = target::resolve(&reader, &refs, names)
            .await
            .map_err(|why| {
                let class = if why.starts_with(target::CANNOT_WRITE) {
                    "not_writable"
                } else {
                    "vault"
                };
                stop(class)(refused(format!(
                    "the vault could not be checked just now, so nothing was written and no code \
                 was spent: {why}. It keeps waiting for a value"
                )))
            })?;
        let outcome = match same_place(&now, &recorded) {
            true => (
                {
                    // Only now the proof: a code is spent on a write that will be tried.
                    self.proof_gate(&a, &who, p.code.as_ref(), false)
                        .map_err(|r| {
                            let enrolled = !self.proofs.standing(&who.via(), false).not_enrolled;
                            let class = if enrolled {
                                "proof"
                            } else {
                                "no_authenticator"
                            };
                            stop(class)(failure(r.into()))
                        })?;
                    done::perform(&reader, &recorded, &value).await
                },
                recorded.clone(),
            ),
            false => (Outcome::Changed, recorded.clone()),
        };
        drop(value);
        self.keep_settle(&a, &outcome.1, outcome.0, &who, conn.pid)
            .map_err(|e| Stop {
                failure: failure(e),
                class: None,
            })?;
        Self::keep_told(&outcome.1, outcome.0).map_err(|failure| Stop {
            failure,
            class: None,
        })
    }

    /// The question a value is given to, and the target its card recorded: or why it is
    /// not a keep card that waits.
    fn keep_card(&self, question_id: &str) -> Result<(Action, Target), Stop> {
        let a = self
            .kernel
            .action(question_id)
            .map_err(|e| stop("no_card")(RpcFailure::invalid(e)))?
            .filter(|a| a.tool == crate::config_change::ACK)
            .ok_or_else(|| {
                stop("no_card")(RpcFailure::new(
                    error_code::NOT_FOUND,
                    "no such question waiting for a value",
                ))
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
            return Err(stop("not_a_keep_card")(RpcFailure::invalid(
                anyhow::anyhow!("that question is not a secret to keep: it takes no value"),
            )));
        };
        // The one test of a pending confirm: a card that was answered stays planned, with its
        // confirmation bound, until the turn goes on, and it takes no second value.
        if !a.awaits_confirm() {
            let state = match a.confirm.is_some() {
                true => "already answered",
                false => a.state.as_str(),
            };
            return Err(stop("not_waiting")(RpcFailure::invalid(anyhow::anyhow!(
                "that question is {state}, not waiting for a value"
            ))));
        }
        Ok((a, recorded))
    }

    /// The owner keeps a secret from their own terminal.
    async fn keep_direct(
        self: &Arc<Self>,
        p: VaultKeepParams,
        conn: &Conn<'_>,
    ) -> Result<VaultKept, Stop> {
        let who = self
            .owners_shell(
                conn,
                p.from_job.as_deref(),
                "keep a secret in the vault",
                "`theseus vault keep --vault V --item T --field F`",
            )
            .map_err(stop("not_owners_shell"))?;
        let value = done::checked(p.value.expose()).map_err(|why| {
            let class = if p.value.is_blank() {
                "blank"
            } else {
                "not_a_value"
            };
            stop(class)(RpcFailure::invalid_text(why))
        })?;
        let reader = self.keep_reader().map_err(stop("no_vault"))?;
        let refs = target::readers(self);
        let t = target::resolve(&reader, &refs, (&p.vault, &p.title, &p.field))
            .await
            .map_err(|why| {
                let class = if why.starts_with(target::CANNOT_WRITE) {
                    "not_writable"
                } else {
                    "vault"
                };
                stop(class)(refused(why))
            })?;
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
            Proof::Missing | Proof::NotRequired => return Err(stop("proof")(RpcFailure {
                code: error_code::REFUSED,
                message:
                    "it needs a fresh code from your authenticator app: `--code <the six digits>`"
                        .into(),
                data: json!({"proof": "needed"}),
            })),
            Proof::Refused(why) => return Err(stop("proof")(refused(why.to_string()))),
        }
        let outcome = done::perform(&reader, &t, &value).await;
        drop(value);
        let row = LedgerRow::new(
            LedgerKind::VaultKeep,
            None,
            None,
            done::row(&t, outcome, (&who.label, &place, conn.pid), None),
        );
        let _ = self.store.append_ledger(&row);
        Self::keep_told(&t, outcome).map_err(|failure| Stop {
            failure,
            class: None,
        })
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

    /// Where a value may come from on a transport (issue 29): the owner's direct messages alone, and
    /// only while `[vault] keep_by_discord` is on. The terminal and the cockpit are not asked. Judged
    /// after `judge_act` (an owner, a private place), so a channel bound private is refused here.
    fn keep_place(&self, a: &Action, who: &Answerer) -> Result<(), Stop> {
        if who.surface != Surface::Discord {
            return Ok(());
        }
        let (class, verdict) = match self.cfg.vault.keep_by_discord {
            false => (
                "setting_off",
                Err(
                    "typing a secret's value in Discord is turned off ([vault] keep_by_discord): \
                     the owner gives it from their own terminal or on its card in the cockpit"
                        .to_string(),
                ),
            ),
            true => (
                "place",
                crate::places::owner_in_person_of(
                    who,
                    &self.runner.place_rule,
                    &self.cfg,
                    "a secret's value is given only where the owner is alone with Theseus: a \
                     value typed in a channel passes through the chat service and the people \
                     who can read it",
                    "a secret's value is given",
                ),
            ),
        };
        let Err(why) = verdict else {
            return Ok(());
        };
        let r = Refusal {
            who: who.who(),
            via: who.via(),
            why,
        };
        let _ = self.refused(
            Act::Answer {
                action: a,
                approve: true,
            },
            who,
            &r,
        );
        Err(stop(class)(failure(r.into())))
    }

    /// The question settles, and the row says how.
    fn keep_settle(
        &self,
        a: &Action,
        t: &Target,
        o: Outcome,
        who: &Answerer,
        pid: Option<u32>,
    ) -> anyhow::Result<()> {
        let row = LedgerRow::new(
            LedgerKind::VaultKeep,
            Some(a.session_id.as_str()),
            None,
            done::row(t, o, (&who.label, &who.via(), pid), Some(&a.correlation_id)),
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

/// The questions whose value is being kept now, so a second answer to one waits.
static KEEPING: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// A question's turn at the vault, given back on drop.
struct InFlight(String);

impl InFlight {
    fn take(question: &str) -> Option<Self> {
        let mut held = KEEPING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match held.iter().any(|q| q == question) {
            true => None,
            false => {
                held.push(question.to_string());
                Some(Self(question.to_string()))
            }
        }
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        let mut held = KEEPING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        held.retain(|q| *q != self.0);
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

/// The turn at the vault, for the tests of `config_change::vault_keep`.
#[cfg(test)]
pub(crate) mod vault_keep_test {
    pub(crate) fn in_flight(question: &str) -> Option<impl Drop> {
        super::InFlight::take(question)
    }
}
