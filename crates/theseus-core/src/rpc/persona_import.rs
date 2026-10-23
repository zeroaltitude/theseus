//! `persona.import` and `persona.card_confirm` (theseus-a8n7): the owner's
//! agents come over as personas. The files and the pure steps are
//! `persona_import/`; this is the owner's act around them: judged as the
//! import's writes are (`Act::Import`: the owner, from a private place, and
//! refused by the CLI inside a job), run on the blocking pool because it
//! reads and writes files. It writes no frame: the files are the persona's,
//! beside the store, and the next compile that sees their digests change
//! writes the `persona.changed` row (`persona::Board::observe`).

use std::sync::Arc;

use serde_json::Value;
use theseus_protocol::{
    method, PersonaCardConfirmParams, PersonaCardConfirmResult, PersonaImportParams,
    PersonaImportResult,
};

use super::confirms::Act;
use super::server::{parse, reply, Conn, RpcFailure};
use super::Core;
use crate::persona_import::{run, Manifest};

pub(super) const OWN: [&str; 2] = [method::PERSONA_IMPORT, method::PERSONA_CARD_CONFIRM];

impl Core {
    pub(super) async fn rpc_persona_import(
        self: Arc<Self>,
        name: &str,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        let who = conn.answerer(None, None);
        if name == method::PERSONA_IMPORT {
            let p: PersonaImportParams = parse(params)?;
            let what = format!("the import of {} as persona {}", p.from, p.name);
            self.judge_act(
                &who,
                Act::Import {
                    method: method::PERSONA_IMPORT,
                    what: &what,
                },
            )
            .map_err(refused)?;
            let core = Arc::clone(&self);
            let out = tokio::task::spawn_blocking(move || core.persona_import(&p))
                .await
                .map_err(|e| RpcFailure::invalid(anyhow::anyhow!("{e}")))?
                .map_err(RpcFailure::invalid)?;
            return reply(out);
        }
        let p: PersonaCardConfirmParams = parse(params)?;
        let what = format!("keeping the drafted card of persona {}", p.name);
        self.judge_act(
            &who,
            Act::Import {
                method: method::PERSONA_CARD_CONFIRM,
                what: &what,
            },
        )
        .map_err(refused)?;
        reply(self.persona_card_confirm(&p).map_err(RpcFailure::invalid)?)
    }

    /// The import, run now.
    pub(crate) fn persona_import(
        &self,
        p: &PersonaImportParams,
    ) -> anyhow::Result<PersonaImportResult> {
        let now = theseus_protocol::now_unix_ms();
        let (mut result, found) = run::import(
            &self.runner.state_root(),
            p,
            &self.cfg.persona_import.import_files,
            crate::context_files::MAX_BYTES,
            now,
        )?;
        result.dry_run = p.dry_run;
        // The feedback it found is proposed, never live: each proposal is a
        // META record of its own, in one frame with the import's row. A dry
        // run reports them and writes none.
        let (mut frame, fresh, known) =
            crate::standing_proposals::new_records(&self.store, &p.name, &found, now)?;
        result.proposals = fresh
            .iter()
            .map(|x| theseus_protocol::ProposedStanding {
                id: x.id.clone(),
                kind: x.kind,
                text: x.text.clone(),
                from: x.from.clone(),
            })
            .collect();
        result.proposals_known = known;
        if p.dry_run || (result.unchanged && fresh.is_empty()) {
            return Ok(result);
        }
        let fact = crate::fact::persona_import::PersonaImported {
            name: &p.name,
            agent: &result.agent,
            version: result.version,
            files: result.files.len(),
            unchanged: result.unchanged,
            proposed: &fresh,
            known,
        };
        frame.push(crate::fact::row(&fact, None, None)?);
        theseus_store::blocking(|| self.store.append(&frame))?;
        self.rec(None).announce(&fact);
        Ok(result)
    }

    /// The owner has read the drafted card: it is offered to Jev from now on.
    pub(crate) fn persona_card_confirm(
        &self,
        p: &PersonaCardConfirmParams,
    ) -> anyhow::Result<PersonaCardConfirmResult> {
        let book = self.personas();
        let Some(cfg) = book.get(&p.name) else {
            anyhow::bail!("{:?} is not a persona: {}", p.name, book.names());
        };
        let Some(card) = cfg.card.clone() else {
            anyhow::bail!(
                "persona {} has no card: set one with `theseus persona edit {} --card \"…\"`",
                p.name,
                p.name
            );
        };
        let dir = run::dir_of(&self.runner.state_root(), &p.name);
        if let Some(mut m) = Manifest::read(&dir) {
            if !m.card_confirmed {
                m.card_confirmed = true;
                m.write(&dir)?;
            }
        }
        Ok(PersonaCardConfirmResult {
            name: p.name.clone(),
            card,
        })
    }
}

fn refused(e: anyhow::Error) -> RpcFailure {
    match e.downcast::<crate::approval::Refusal>() {
        Ok(r) => RpcFailure {
            code: theseus_protocol::error_code::REFUSED,
            message: format!(
                "a persona import from {} does not count: {}. Nothing was written.",
                r.who, r.why
            ),
            data: serde_json::json!({"who": r.who, "via": r.via, "why": r.why}),
        },
        Err(e) => RpcFailure::invalid(e),
    }
}
