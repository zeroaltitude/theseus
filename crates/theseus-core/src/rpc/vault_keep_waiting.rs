//! `vault.keep.waiting` (issue 29): the one question `theseus vault keep` asks
//! first, before it prompts for anything. Which keep cards wait for the owner's
//! terminal and, when there is none, why; and whether anything stops a prompt
//! from being useful (no authenticator app enrolled, a vault the account may
//! not write to). The daemon answers in words and the terminal filters
//! nothing, so what a person is told about a miss is the daemon's, and the
//! daemon's own record says what every run asked and was told.
//!
//! Every call writes a `vault.keep` row (`listed`, with the class of the
//! answer, the client's process and the number of cards; or `client_ended`, when
//! the terminal reports a run that ended before a value was sent). A row holds
//! names and classes, never a value, its length or its digest: this request
//! carries none.

use std::sync::Arc;

use serde_json::Value;
use theseus_protocol::vault_keep::{KeepCard, VaultKeepWaiting, VaultKeepWaitingParams};
use theseus_protocol::{error_code, LedgerKind};

use super::server::{Conn, RpcFailure};
use super::Core;
use crate::approval::Answerer;
use crate::config_change::history::of_kind;
use crate::config_change::vault_keep::done::attempt_row;
use crate::config_change::vault_keep::target::{self, Target};
use crate::config_change::vault_keep::{is_card, ARG};
use crate::ledger::LedgerRow;

/// What the terminal is told when it has no terminal to prompt on.
const NO_TERMINAL: &str = "the value is typed at a hidden prompt of your own terminal, and \
     standard input and standard error must both be that terminal: a pipe, a capture or a \
     redirect is refused, so the value cannot be read by anything else and the prompts cannot \
     be missed. Nothing was kept";

/// What is said when no authenticator app is enrolled: before any prompt.
const NOT_ENROLLED: &str = "keeping a secret needs a code from your authenticator app, and none \
     is enrolled yet: run `theseus owner proof enroll` in your own terminal first. Nothing was kept";

fn refused(why: impl Into<String>) -> RpcFailure {
    RpcFailure::new(error_code::REFUSED, why)
}

/// What a keep is, in one heading line: what is kept where, and that the value
/// is typed. Nothing in it reads as a result.
fn heading(t: &Target) -> String {
    use target::State;
    match t.state {
        State::New => format!(
            "To keep a new item \"{}\" (field \"{}\") in the vault \"{}\": type its value.",
            t.title, t.field, t.vault
        ),
        State::Adds => format!(
            "To add a field \"{}\" to the item \"{}\" in the vault \"{}\": type its value.",
            t.field, t.title, t.vault
        ),
        State::Replaces => format!(
            "To REPLACE the value of the field \"{}\" of the item \"{}\" in the vault \"{}\": \
             type the new value.",
            t.field, t.title, t.vault
        ),
    }
}

impl Core {
    fn keep_row(&self, data: Value) {
        let _ = self
            .store
            .append_ledger(&LedgerRow::new(LedgerKind::VaultKeep, None, None, data));
    }

    /// `vault.keep.waiting`.
    pub(super) async fn keep_waiting(
        self: &Arc<Self>,
        p: VaultKeepWaitingParams,
        conn: &Conn<'_>,
    ) -> Result<VaultKeepWaiting, RpcFailure> {
        let pid = conn.peer.pid();
        let who: Answerer = match self.owners_shell(
            conn,
            p.from_job.as_deref(),
            "keep a secret in the vault",
            "`theseus vault keep`",
        ) {
            Ok(who) => who,
            Err(f) => {
                self.keep_stopped("not_owners_shell", &conn.answerer(None, None), pid, None);
                return Err(f);
            }
        };
        let place = who.via();
        let row = |outcome: &str, why: &str, n: Option<usize>| {
            self.keep_row(attempt_row(
                outcome,
                why,
                (&who.label, &place, pid),
                p.question.as_deref(),
                n,
            ));
        };
        if let Some(end) = p.ended {
            row("client_ended", end.name(), None);
            return Ok(VaultKeepWaiting::default());
        }
        if !p.terminal {
            row("stopped", "no_terminal", None);
            return Err(refused(NO_TERMINAL));
        }
        let mut found: Vec<(Option<String>, Target, bool)> = Vec::new();
        if let (Some(vault), Some(title), Some(field)) = (&p.vault, &p.title, &p.field) {
            // A keep with no card: always a fresh code.
            let reader = self
                .keep_reader()
                .inspect_err(|_| row("stopped", "no_vault", None))?;
            match target::resolve(&reader, &target::readers(self), (vault, title, field)).await {
                Ok(t) => found.push((None, t, true)),
                Err(why) => {
                    let class = match why.starts_with(target::CANNOT_WRITE) {
                        true => "not_writable",
                        false => "vault",
                    };
                    row("stopped", class, None);
                    return Err(refused(why));
                }
            }
        } else {
            let want = p
                .question
                .as_deref()
                .map(str::trim)
                .filter(|q| !q.is_empty());
            for a in self
                .kernel
                .pending_confirms()
                .map_err(RpcFailure::invalid)?
            {
                if !crate::config_change::vault_keep::is_keep(&a)
                    || want.is_some_and(|q| !a.correlation_id.ends_with(q))
                {
                    continue;
                }
                let args = a.proposal.as_ref().map(|p| &p.args);
                let Some(t) = args
                    .filter(|args| is_card(args))
                    .and_then(|args| Target::of_args(&args[ARG]))
                else {
                    continue;
                };
                found.push((Some(a.correlation_id.clone()), t.clone(), t.replaces()));
            }
        }
        let mut cards = Vec::new();
        let mut blocked = None;
        for (question_id, t, fresh) in found {
            let standing = self.proofs.standing(&place, fresh);
            if standing.not_enrolled && blocked.is_none() {
                blocked = Some(NOT_ENROLLED.to_string());
            }
            cards.push(KeepCard {
                question_id,
                vault: t.vault.clone(),
                title: t.title.clone(),
                field: t.field.clone(),
                state: t.state.as_str().into(),
                code_needed: standing.code_needed,
                heading: heading(&t),
            });
            // The account's permission is asked now, for the one card a prompt would answer.
            if blocked.is_none() && p.vault.is_none() {
                if let Ok(reader) = self.keep_reader() {
                    if let Err(why) = target::still_writable(&reader, &t).await {
                        blocked = Some(why);
                    }
                }
            }
        }
        let (none_class, none) = match cards.len() {
            0 => self.why_none(p.question.as_deref()),
            _ => ("", String::new()),
        };
        let class = match (cards.len(), &blocked) {
            (0, _) => none_class,
            (1, Some(_)) => "blocked",
            (1, None) => "one",
            _ => "several",
        };
        row("listed", class, Some(cards.len()));
        if cards.len() != 1 {
            blocked = None;
        }
        Ok(VaultKeepWaiting {
            cards,
            none,
            blocked,
        })
    }

    /// Why no keep card waits: what became of the latest one asked, from the
    /// daemon's own rows. The class is the row's; the words are the owner's.
    fn why_none(&self, question: Option<&str>) -> (&'static str, String) {
        let kept = of_kind(&self.store, LedgerKind::VaultKeep);
        let declined = of_kind(&self.store, LedgerKind::ConfigDeclined);
        let names = |d: &Value| {
            format!(
                "the vault \"{}\", item \"{}\", field \"{}\"",
                d["vault"].as_str().unwrap_or("?"),
                d["title"].as_str().unwrap_or("?"),
                d["field"].as_str().unwrap_or("?")
            )
        };
        let asked = kept.iter().rev().find(|r| {
            r.data["outcome"] == "asked"
                && question
                    .is_none_or(|q| r.data["question"].as_str().is_some_and(|c| c.ends_with(q)))
        });
        let Some(asked) = asked else {
            return match question {
                Some(q) => (
                    "none_named",
                    format!(
                        "No keep card ends in \"{q}\", waiting or not: run `theseus vault keep` \
                         with no argument to see what waits, or ask Theseus to keep the secret \
                         again and it puts a card"
                    ),
                ),
                None => (
                    "none_asked",
                    "No secret is waiting to be kept, and Theseus has no record of asking to \
                     keep one: ask Theseus to keep a secret in the vault and it puts a card"
                        .to_string(),
                ),
            };
        };
        let q = asked.data["question"].as_str().unwrap_or_default();
        let card = names(&asked.data);
        let ago = super::question_waits::waited_words(
            theseus_protocol::now_unix_ms().saturating_sub(asked.at_unix_ms),
        );
        if let Some(d) = declined.iter().rev().find(|r| r.data["question"] == q) {
            return match d.data["outcome"].as_str() {
                Some("expired") => (
                    "expired",
                    format!(
                        "The card for {card}, asked {ago} ago, expired: nobody answered in time. \
                         Ask Theseus to keep it again and it puts a new card"
                    ),
                ),
                _ => (
                    "declined",
                    format!(
                        "The card for {card}, asked {ago} ago, was declined{}. Nothing was kept. \
                         Ask Theseus to keep it again and it puts a new card",
                        match d.data["answered_by"].as_str() {
                            Some(by) => format!(" by {by}"),
                            None => String::new(),
                        }
                    ),
                ),
            };
        }
        let settled = kept.iter().rev().find(|r| {
            r.data["question"] == q
                && matches!(
                    r.data["outcome"].as_str(),
                    Some("kept" | "not_written" | "unverified" | "differs" | "changed")
                )
        });
        match settled.and_then(|r| r.data["outcome"].as_str()) {
            Some("kept") => (
                "answered",
                format!("The card for {card}, asked {ago} ago, was already answered: it was kept."),
            ),
            Some("not_written" | "changed") => (
                "answered",
                format!(
                    "The card for {card}, asked {ago} ago, was answered, and nothing was kept: \
                     the vault was not changed. Ask Theseus to keep it again"
                ),
            ),
            Some(_) => (
                "answered",
                format!(
                    "The card for {card}, asked {ago} ago, was answered, but the vault's reply \
                     could not be confirmed: check the item in the vault before relying on it"
                ),
            ),
            None => (
                "ended",
                format!(
                    "The card for {card}, asked {ago} ago, is not waiting any more, and no answer, \
                     decline or expiry is recorded for it (it may have been withdrawn). Ask \
                     Theseus to keep it again"
                ),
            ),
        }
    }
}
