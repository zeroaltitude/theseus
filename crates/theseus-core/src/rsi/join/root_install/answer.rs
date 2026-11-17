//! The owner's answer to "Install the change Theseus joined": `action.confirm` on its `self.install.ack` question,
//! judged by the place rule before it gets here, and refused to an agent's process (in `rpc/confirms.rs`'s dispatch).
//!
//! A yes is the second proof's to give (`config_change::proof_seam`); until it is built in, a yes settles as declined,
//! with the seam's words. Else the install runs, as a task, off the answer's path: the answer settles at once (one
//! frame: the bind or the decline, the answer's row, the ledger row), and the run's steps are ledger rows of their own.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use serde_json::{json, Value};
use theseus_kernel::Action;
use theseus_store::{kinds, NewRecord};

use super::offer::release_of;
use super::{
    exec_program, is_commit, program_of, repoint, rooted, short, Pending, Stage, Unit,
    CONFIRM_WITHIN_SECS,
};
use crate::config_change::proof_seam;
use crate::fact::answer::CallAnswered;
use crate::ledger::LedgerRow;
use crate::outbox::Closed;
use crate::rpc::Core;
use crate::rsi::join::{blocking, git, remote_of, rows_of};
use crate::turn::OPERATOR;
use theseus_protocol::LedgerKind;

/// What an install is for: the commit, why, where it was built, and who said yes.
#[derive(Debug, Clone)]
pub(crate) struct Go {
    /// The question that approved it, or `auto`.
    pub question: String,
    pub merge: String,
    pub branch: String,
    pub what: String,
    /// The build tree's target directory: its `release/` holds the programs.
    pub target: PathBuf,
    /// The program the service ran when the card was made.
    pub old_program: String,
    pub approved_by: String,
    pub via: String,
}

/// The shell's single quotes around `s`.
fn q(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The restart's script: restart onto the new unit, wait for the new daemon's confirmation, and if it does not come
/// (or the restart itself fails) put the unit as it was back and restart onto it. At most two restarts, in all
/// cases. Everything it needs is in its text: it reads no file but the confirmation.
pub(super) fn restart_script(
    signal: &Path,
    unit: &Path,
    before: &str,
    service: &str,
    merge: &str,
    wait_secs: u64,
) -> String {
    format!(
        "# Install of {merge} (issue 22): restart onto it, wait for the new daemon to confirm itself, else go back.\n\
         set -u\n\
         signal={signal}\nunit={unit}\nservice={service}\n\
         back() {{\n  printf '%s' {before} > \"$unit.back\" && mv -f \"$unit.back\" \"$unit\"\n  \
         systemctl --user daemon-reload\n  systemctl --user restart \"$service\"\n}}\n\
         systemctl --user daemon-reload\n\
         if ! systemctl --user restart \"$service\"; then back; exit 1; fi\n\
         end=$((SECONDS + {wait}))\n\
         while [ \"$SECONDS\" -lt \"$end\" ]; do\n  [ -e \"$signal\" ] && exit 0\n  sleep 2\ndone\n\
         back\nexit 1\n",
        signal = q(&signal.to_string_lossy()),
        unit = q(&unit.to_string_lossy()),
        service = q(service),
        before = q(before),
        wait = wait_secs,
    )
}

impl Core {
    /// Whether a yes is let stand: the second proof is given, and the card names a commit that is not already being
    /// installed. Else it settles as a decline, with the reason.
    fn root_yes_stands(&self, a: &Action, merge: &str, via: &str) -> Result<(), String> {
        if !self.root_proof_given(Some(a), via) {
            // The card is not made while the proof is not built in; a yes that arrives anyway is refused.
            return Err(proof_seam::NOT_INSTALLED.to_string());
        }
        if !is_commit(merge) || self.root_install_pending_for(merge) {
            return Err(
                "an install of that commit is already under way, or the card names no commit"
                    .into(),
            );
        }
        Ok(())
    }

    /// The answer's one frame: the bind or the decline, the answer's row and the ledger row; then the card closes.
    fn root_settle(
        &self,
        a: &Action,
        args: &Value,
        approve: bool,
        note: Option<&str>,
        (by, via): (&str, &str),
    ) -> Result<()> {
        let merge = args["merge"].as_str().unwrap_or_default();
        let corr = a.correlation_id.as_str();
        let (step, what) = match approve {
            true => (
                "approved",
                format!("the owner said yes to installing {}", short(merge)),
            ),
            false => (
                "declined",
                format!("not installed: {}", note.unwrap_or("the owner declined")),
            ),
        };
        let row = LedgerRow::new(
            LedgerKind::SelfRootInstall,
            Some(&a.session_id),
            None,
            json!({"merge": merge, "step": step, "what": what, "why": args["what"], "numbers": {"question": corr},
                   "undo": null, "answered_by": by, "via": via}),
        );
        let answered = CallAnswered {
            action: a,
            approve,
            note,
            by,
            via,
            option: (by != crate::rpc::EXPIRY).then_some(if approve {
                "approve"
            } else {
                "decline"
            }),
        };
        let session = Some(a.session_id.as_str());
        let rows = vec![
            crate::fact::row(&answered, session, None)?,
            NewRecord::json(kinds::LEDGER, None, &row)?,
        ];
        let proposal = a.proposal.clone().unwrap_or_default();
        self.kernel.frame(&[&a.execution_id], |k| {
            match approve {
                true => k.bind_confirm(corr, OPERATOR, &proposal).map(drop)?,
                false => {
                    k.decline_action(corr, by, note.unwrap_or("the owner declined"))
                        .map(drop)?;
                }
            }
            k.stage(&rows)
        })?;
        self.session_rec(&a.session_id).announce(&answered);
        let closed = match (approve, by) {
            (true, _) => Closed::new("approved", Some(by)),
            (false, crate::rpc::EXPIRY) => Closed {
                note: note.map(str::to_string),
                ..Closed::new("expired", None)
            },
            (false, _) => Closed {
                note: note.map(str::to_string),
                ..Closed::new("declined", Some(by))
            },
        };
        self.card_closed(corr, closed);
        Ok(())
    }

    /// The answer to `a`, a `self.install.ack` question still waiting, by `by` through `via`; an expiry is an answer of
    /// `by == EXPIRY` with `approve` false.
    pub(crate) fn answer_root_install(
        &self,
        a: &Action,
        approve: bool,
        note: Option<&str>,
        by: &str,
        via: &str,
    ) -> Result<theseus_protocol::ActionConfirmResult> {
        let args = a
            .proposal
            .as_ref()
            .map(|p| p.args.clone())
            .unwrap_or_default();
        let text = |k: &str| args[k].as_str().unwrap_or_default().to_string();
        let merge = text("merge");
        let (approve, note) = match (approve, self.root_yes_stands(a, &merge, via)) {
            (true, Err(why)) => (false, Some(why)),
            (_, _) => (approve, note.map(str::to_string)),
        };
        self.root_settle(a, &args, approve, note.as_deref(), (by, via))?;
        let go = Go {
            question: a.correlation_id.clone(),
            merge,
            branch: text("branch"),
            what: text("what"),
            target: PathBuf::from(text("target")),
            old_program: text("old_program"),
            approved_by: by.into(),
            via: via.into(),
        };
        match (approve, self.tools.root_install.core()) {
            (true, Some(core)) => {
                tokio::spawn(core.run_root_install(go));
            }
            (true, None) => {
                tracing::warn!("self-install: no handle on the daemon, so the install did not run")
            }
            (false, core) => self.reap_declined(core, &go),
        }
        Ok(theseus_protocol::ActionConfirmResult {
            correlation_id: a.correlation_id.clone(),
            approved: approve,
            session_id: a.session_id.clone(),
            execution_id: a.execution_id.clone(),
            resumes: false,
        })
    }

    /// A declined or expired install's build tree is regenerable and about 2 GB: reaped, off this path.
    fn reap_declined(&self, core: Option<Arc<Core>>, go: &Go) {
        let (Some(core), Some(repo)) = (core, self.cfg.self_improve.repo.clone()) else {
            return;
        };
        let merge = go.merge.clone();
        tokio::spawn(async move {
            let place = core.tools.self_pr.place(&repo);
            let _ =
                blocking(move || Ok(crate::rsi::join::install::reap_target(&place, &merge))).await;
        });
    }

    /// The install, after the yes (or with no card, for `auto`): every step a ledger row, and a failure says so and
    /// changes nothing the service runs.
    pub(crate) async fn run_root_install(self: Arc<Self>, go: Go) {
        let c8 = short(&go.merge).to_string();
        self.root_row(
            "started",
            &go.merge,
            &format!("installing {c8}"),
            &go.what,
            json!({"by": go.approved_by}),
        );
        match self.root_install_steps(&go).await {
            Ok(unit) => self.root_row(
                "restarting",
                &go.merge,
                "the service is restarting onto the new build",
                &format!("by {unit}; it goes back by itself if the new build does not confirm in {CONFIRM_WITHIN_SECS} s"),
                json!({"new_program": program_of(&go.merge)}),
            ),
            Err(why) => {
                self.root_row("failed", &go.merge, "the install did not happen", &why, Value::Null);
                self.root_tell(format!(
                    "Theseus did not install the change it joined ({what}, {c8}): {why}. Nothing it runs has changed. \
                     The change is joined to main; the manual step is in docs/setup.md, \"A root-owned install\".",
                    what = go.what
                ));
            }
        }
    }

    /// A commit the loop's own join row names, and main's head on the remote now.
    async fn root_check_head(&self, go: &Go) -> Result<(), String> {
        let repo = self
            .cfg
            .self_improve
            .repo
            .clone()
            .ok_or("this daemon has no [self] repo")?;
        let place = self.tools.self_pr.place(&repo);
        let rows = rows_of(&self.store, &["self.joined"])
            .map_err(|e| format!("{e:#}"))?
            .ok_or("the ledger's index is still being built")?;
        if !rows
            .iter()
            .any(|r| r.kind == "self.joined" && r.data["merge"] == go.merge.as_str())
        {
            return Err("no join of this loop names that commit".into());
        }
        let remote = remote_of(&self.cfg);
        let head = blocking(move || {
            git::fetch(&place, &remote)?;
            git::main_of(&place, &remote)
        })
        .await
        .map_err(|e| format!("{e:#}"))?;
        if head != go.merge {
            return Err(format!(
                "main has moved on to {} since this was joined, so this is no longer main's head",
                short(&head)
            ));
        }
        Ok(())
    }

    /// The unit as it is now, which must be what the card named, and what it becomes: (the unit, its program, the new
    /// program, the new text).
    async fn root_plan(&self, go: &Go) -> Result<(Unit, String, String, String), String> {
        let units = self.tools.root_install.units();
        let unit = blocking(move || units.read())
            .await
            .map_err(|e| format!("{e:#}"))?;
        let old = exec_program(&unit.text)
            .ok_or("the service's unit has no single ExecStart naming a program")?;
        if old != go.old_program || rooted(Path::new(&old)).is_none() {
            return Err("the service's program is not the one the card named".into());
        }
        let new = program_of(&go.merge).display().to_string();
        if new == old {
            return Err("the service already runs that build".into());
        }
        let text = repoint(&unit.text, &old, &new)?;
        Ok((unit, old, new, text))
    }

    async fn root_install_steps(&self, go: &Go) -> Result<String, String> {
        let board = self.tools.root_install.clone();
        let say = |e: anyhow::Error| format!("{e:#}");
        self.root_check_head(go).await?;
        let (unit, old, new, new_text) = self.root_plan(go).await?;
        // The programs, into the staging directory, and the helper.
        let (helper, release) = (board.helper(), release_of(&go.target));
        blocking(move || helper.stage(&release))
            .await
            .map_err(say)?;
        self.root_row(
            "staged",
            &go.merge,
            "the four programs are in the staging directory",
            "",
            Value::Null,
        );
        let (helper, commit) = (board.helper(), go.merge.clone());
        let said = blocking(move || helper.install(&commit))
            .await
            .map_err(|e| format!("the helper refused: {e:#}"))?;
        self.root_row(
            "helper",
            &go.merge,
            "the helper installed the build",
            &said,
            json!({"dir": program_of(&go.merge).parent()}),
        );
        // What a start must find, in the store, before anything the service runs changes.
        let mut pending = Pending {
            question: go.question.clone(),
            merge: go.merge.clone(),
            branch: go.branch.clone(),
            what: go.what.clone(),
            old_program: old.clone(),
            new_program: new.clone(),
            unit: unit.path.clone(),
            unit_before: unit.text.clone(),
            approved_by: go.approved_by.clone(),
            via: go.via.clone(),
            requested_ms: theseus_protocol::now_unix_ms(),
            stage: Stage::Restarting,
            took_ms: None,
            why: None,
        };
        self.root_save(&pending).map_err(say)?;
        let signal = board.signal();
        let _ = std::fs::remove_file(&signal);
        let (units, at) = (board.units(), unit.path.clone());
        blocking(move || units.write(&at, &new_text))
            .await
            .map_err(|e| format!("the unit could not be pointed at the new build: {e:#}"))?;
        self.root_row(
            "unit",
            &go.merge,
            "the service's unit now names the new build",
            &new,
            json!({"was": old}),
        );
        // The restart, which waits for the new daemon and goes back by itself.
        let service = crate::rsi::join::install::SERVICE;
        let script = restart_script(
            &signal,
            &unit.path,
            &unit.text,
            service,
            &go.merge,
            CONFIRM_WITHIN_SECS,
        );
        let (units, c8) = (board.units(), short(&go.merge).to_string());
        match blocking(move || units.restart(&script, &c8)).await {
            Ok(name) => Ok(name),
            Err(e) => {
                // Nothing restarted: put the unit back, and end the install.
                let (units, at, before) = (board.units(), unit.path.clone(), unit.text.clone());
                let _ = blocking(move || units.write(&at, &before)).await;
                pending.stage = Stage::RolledBack;
                pending.why = Some(format!("{e:#}"));
                let _ = self.root_save(&pending);
                Err(format!(
                    "the restart could not be started, and the unit is as it was: {e:#}"
                ))
            }
        }
    }
}
