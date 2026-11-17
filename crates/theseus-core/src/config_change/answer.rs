//! The owner's answer to a proposed change (issue 22, line 1 step 4):
//! `action.confirm` on its `config.ack` question, judged by the place rule
//! before it gets here (`Core::confirm_action_with`'s `judge_act`).
//!
//! A yes writes the config, and only if everything is as the card showed it:
//! the file is byte for byte the one the candidate was built on, the kept
//! candidate is the one the card names and still loads, and no sensitive
//! setting is in it without the second proof. Else nothing is written, the
//! question settles declined with the reason, and `config.declined` says so.
//! A decline, or no answer in the question's time, writes nothing and is
//! recorded the same way. Each is one frame: the answer's row, the ledger row
//! and the question's settlement. None wakes the proposing execution.

use anyhow::Result;
use serde_json::{json, Value};
use theseus_kernel::Action;
use theseus_protocol::LedgerKind;
use theseus_store::{kinds, NewRecord};

use super::restart::{self, Pending, Stage};
use super::{diff, proof_seam, sensitive, write};
use crate::config_copy::sha256;
use crate::fact::answer::CallAnswered;
use crate::ledger::LedgerRow;
use crate::outbox::Closed;
use crate::rpc::Core;
use crate::turn::OPERATOR;

/// How a question ended.
pub(crate) enum Answer<'a> {
    Yes,
    No(Option<&'a str>),
    /// Nobody answered by its time.
    Expired(&'a str),
}

/// What a yes did.
struct Applied {
    previous: u32,
    before: String,
    after: String,
    restart: bool,
    /// The marker the restart follows, kept before the file was written.
    pending: Option<Pending>,
}

/// The `config.undone` record a yes to an undo writes beside `config.changed`:
/// none for any other question, or for an answer that wrote nothing.
fn undone_row(
    a: &Action,
    approve: bool,
    args: &Value,
    who: &Value,
    by: &str,
    via: &str,
) -> Result<Option<NewRecord>> {
    let Some(u) = args.get("undoes").filter(|u| approve && u.is_object()) else {
        return Ok(None);
    };
    let row = super::undo::undone_row(&a.session_id, &a.correlation_id, u, args, who, by, via);
    Ok(Some(NewRecord::json(kinds::LEDGER, None, &row)?))
}

impl Core {
    /// The yes to a restart the owner asked for in words: nothing is written,
    /// but the marker is, so the start that follows says it is back, and goes
    /// back to the copy kept (the same text) if it is not healthy.
    fn plan_plain_restart(
        &self,
        a: &Action,
        file: &std::path::Path,
        by: &str,
        via: &str,
    ) -> Result<Applied, String> {
        let w = &self.tools.config_change;
        let now = std::fs::read_to_string(file)
            .map_err(|e| format!("the settings file cannot be read: {e}"))?;
        let (previous, copy) = write::keep_previous(&w.state, &now)
            .map_err(|e| format!("the settings could not be kept, so nothing restarts: {e}"))?;
        let sha = sha256(&now);
        let args = a
            .proposal
            .as_ref()
            .map(|p| p.args.clone())
            .unwrap_or_default();
        let p = Pending {
            id: a.correlation_id.clone(),
            file: file.to_path_buf(),
            previous: copy,
            previous_sha256: sha.clone(),
            digest_before: sha.clone(),
            digest_after: sha.clone(),
            paths: vec![],
            session_id: a.session_id.clone(),
            place: args["asked_by"]["place"].as_str().map(str::to_string),
            approved_by: by.to_string(),
            via: via.to_string(),
            requested_ms: theseus_protocol::now_unix_ms(),
            stage: Stage::Restarting,
            restart_only: true,
            took_ms: None,
            why: None,
        };
        restart::save(&restart::marker_path(&w.state), &p).map_err(|e| {
            format!("the restart's marker could not be kept, so nothing restarts: {e}")
        })?;
        Ok(Applied {
            previous,
            before: sha.clone(),
            after: sha,
            restart: true,
            pending: Some(p),
        })
    }

    /// The yes: write the candidate the card showed, or say why not.
    fn write_change(
        &self,
        a: &Action,
        args: &Value,
        by: &str,
        via: &str,
    ) -> Result<Applied, String> {
        let w = &self.tools.config_change;
        // One yes at a time, and none while a change is still being applied.
        let _one = w
            .writes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(p) = restart::load(&restart::marker_path(&w.state)) {
            if matches!(p.stage, Stage::Restarting | Stage::Started) {
                return Err(
                    "another change is still being applied (Theseus restarts first): \
                            ask again once it is back"
                        .into(),
                );
            }
        }
        let file = w
            .file
            .clone()
            .ok_or("the settings are not a file this daemon can change")?;
        if args["restart_only"].as_bool() == Some(true) {
            return self.plan_plain_restart(a, &file, by, via);
        }
        let want_base = args["base_sha256"].as_str().unwrap_or_default();
        let sha = args["candidate_sha256"].as_str().unwrap_or_default();
        let now = std::fs::read_to_string(&file)
            .map_err(|e| format!("the settings file cannot be read: {e}"))?;
        if sha256(&now) != want_base {
            return Err(
                "the settings file changed after this card was made, so nothing was written: \
                 ask for the change again"
                    .into(),
            );
        }
        let candidate = write::read_candidate(&w.state, sha)?;
        crate::Config::parse(&candidate)
            .map(drop)
            .map_err(|e| format!("the candidate no longer loads: {e:#}"))?;
        let settings = diff::between(&now, &candidate)?;
        let sens = sensitive(&[], &settings);
        if !sens.is_empty() {
            // The card is never made for these, so this is a second wall.
            match proof_seam::required_and_given(a, "") {
                proof_seam::Proof::NotInstalled => return Err(super::needs_proof(&sens)),
            }
        }
        let (previous, copy) = write::keep_previous(&w.state, &now).map_err(|e| {
            format!("the previous settings could not be kept, so nothing was written: {e}")
        })?;
        let restart = args["restart"].as_bool().unwrap_or(true);
        // The restart's memory is kept before the file changes, so a start
        // that cannot load the new settings can find its way back.
        let pending = restart.then(|| Pending {
            id: a.correlation_id.clone(),
            file: file.clone(),
            previous: copy,
            previous_sha256: sha256(&now),
            digest_before: sha256(&now),
            digest_after: sha256(&candidate),
            paths: args["paths"]
                .as_array()
                .map(|v| {
                    v.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
            session_id: a.session_id.clone(),
            place: args["asked_by"]["place"].as_str().map(str::to_string),
            approved_by: by.to_string(),
            via: via.to_string(),
            requested_ms: theseus_protocol::now_unix_ms(),
            stage: Stage::Restarting,
            restart_only: false,
            took_ms: None,
            why: None,
        });
        let marker = restart::marker_path(&w.state);
        if let Some(p) = &pending {
            restart::save(&marker, p).map_err(|e| {
                format!("the restart's marker could not be kept, so nothing was written: {e}")
            })?;
        }
        if let Err(e) = write::write_config(&file, &candidate) {
            let _ = std::fs::remove_file(&marker);
            return Err(format!("the settings could not be written: {e}"));
        }
        Ok(Applied {
            previous,
            before: sha256(&now),
            after: sha256(&candidate),
            restart,
            pending,
        })
    }

    /// The answer to `a`, a `config.ack` question still waiting, by `by`
    /// through `via`.
    pub(crate) fn answer_config_change(
        &self,
        a: &Action,
        answer: Answer<'_>,
        by: &str,
        via: &str,
    ) -> Result<theseus_protocol::ActionConfirmResult> {
        let args = a
            .proposal
            .as_ref()
            .map(|p| p.args.clone())
            .unwrap_or_default();
        let state = self.tools.config_change.state.clone();
        let (applied, note): (Option<Applied>, Option<String>) = match answer {
            Answer::Yes => match self.write_change(a, &args, by, via) {
                Ok(done) => (Some(done), None),
                Err(why) => (None, Some(why)),
            },
            Answer::No(n) => (None, n.map(str::to_string)),
            Answer::Expired(why) => (None, Some(why.to_string())),
        };
        let approve = applied.is_some();
        let session = Some(a.session_id.as_str());
        let corr = a.correlation_id.as_str();
        let who = json!({"session_id": a.session_id, "execution_id": a.execution_id,
                         "turn_id": args["asked_by"]["turn_id"], "place": args["asked_by"]["place"]});
        let row = match &applied {
            Some(d) => LedgerRow::new(
                LedgerKind::ConfigChanged,
                session,
                None,
                json!({
                    "source": "owner", "question": corr, "paths": args["paths"],
                    "asked_by": who, "approved_by": by, "via": via,
                    "digest_before": d.before, "digest_after": d.after,
                    "previous_copy": d.previous, "restart": d.restart,
                    "restart_only": args["restart_only"].as_bool().unwrap_or(false),
                }),
            ),
            None => LedgerRow::new(
                LedgerKind::ConfigDeclined,
                session,
                None,
                json!({
                    "question": corr, "paths": args["paths"], "asked_by": who,
                    "answered_by": by, "via": via,
                    "outcome": match answer { Answer::Yes => "refused", Answer::No(_) => "declined", Answer::Expired(_) => "expired" },
                    "why": note,
                }),
            ),
        };
        let answered = CallAnswered {
            action: a,
            approve,
            note: note.as_deref(),
            by,
            via,
            option: (by != crate::rpc::EXPIRY).then_some(if approve {
                "approve"
            } else {
                "decline"
            }),
        };
        let mut rows = vec![
            crate::fact::row(&answered, session, None)?,
            NewRecord::json(kinds::LEDGER, None, &row)?,
        ];
        // An undo that was written names the change it undid.
        rows.extend(undone_row(a, approve, &args, &who, by, via)?);
        let proposal = a.proposal.clone().unwrap_or_default();
        self.kernel.frame(&[&a.execution_id], |k| {
            match approve {
                true => k.bind_confirm(corr, OPERATOR, &proposal).map(drop)?,
                false => {
                    let n = note.as_deref().unwrap_or("the owner declined");
                    k.decline_action(corr, by, n).map(drop)?;
                }
            }
            k.stage(&rows)
        })?;
        if let Some(sha) = args["candidate_sha256"].as_str() {
            write::drop_candidate(&state, sha);
        }
        self.session_rec(&a.session_id).announce(&answered);
        let closed = match (&applied, &answer) {
            (Some(_), _) => Closed::new("approved", Some(by)),
            (None, Answer::Expired(why)) => Closed {
                note: Some((*why).to_string()),
                ..Closed::new("expired", None)
            },
            (None, _) => Closed {
                note: note.clone(),
                ..Closed::new("declined", Some(by))
            },
        };
        self.card_closed(corr, closed);
        // Written and answered: now the restart, which waits for a running
        // turn to end for a stated time and then cuts it.
        if let Some(p) = applied.as_ref().and_then(|d| d.pending.as_ref()) {
            self.restart_for_change(p, std::time::Duration::from_secs(super::RESTART_WAIT_SECS));
        }
        Ok(theseus_protocol::ActionConfirmResult {
            correlation_id: corr.into(),
            approved: approve,
            session_id: a.session_id.clone(),
            execution_id: a.execution_id.clone(),
            resumes: false,
        })
    }
}
