//! After a join: whether to offer the install, the build, and the card.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering::SeqCst;
use std::sync::Arc;

use anyhow::{Context, Result};
use serde_json::{json, Value};
use theseus_kernel::{Action, RetryClass, TurnEnd, Wake};
use theseus_protocol::{ConfirmRequest, LedgerKind, Proposal, SessionOpenParams};

use super::answer::Go;
use super::hand::hand_words;
use super::{exec_program, is_commit, rooted, short, Presence, ACK, CONFIRM_WITHIN_SECS, LIB_DIR};
use crate::config::self_improve::SelfInstall;
use crate::ledger::LedgerRow;
use crate::rpc::Core;
use crate::rsi::join::{blocking, install};
use crate::rsi::tree::Place;
use crate::session::SessionRecord;
use crate::turn::OPERATOR;

/// The meta key of the session the install's questions are planned on.
const SESSION: &str = "self.install.session";

impl Core {
    /// One step of an install, as a ledger row: `what` and `why` in words, `numbers` as the step has them.
    pub(crate) fn root_row(&self, step: &str, merge: &str, what: &str, why: &str, numbers: Value) {
        let row = LedgerRow::new(
            LedgerKind::SelfRootInstall,
            None,
            None,
            json!({"merge": merge, "step": step, "what": what, "why": why, "numbers": numbers, "undo": null}),
        );
        if let Err(e) = self.store.append_ledger(&row) {
            tracing::warn!(error = %e, "self-install: a ledger row was not written");
        }
    }

    /// A line to the owner, where approvals go.
    pub(crate) fn root_tell(&self, text: String) {
        if let Err(e) = self
            .outbox
            .to_operator(None, json!({"kind": "notice", "text": text}))
        {
            tracing::warn!(error = %format!("{e:#}"), "self-install: the owner's notice was not written");
        }
    }

    /// Whether the service runs a root-owned build and `[self] install` is not `"off"`: whether a join is this
    /// module's to take. Cheap: it reads the unit.
    pub(crate) async fn root_install_applies(&self) -> bool {
        if self.cfg.self_improve.install == SelfInstall::Off {
            return false;
        }
        let units = self.tools.root_install.units();
        let Ok(unit) = blocking(move || units.read()).await else {
            return false;
        };
        exec_program(&unit.text).is_some_and(|p| rooted(Path::new(&p)).is_some())
    }

    /// After a join of `merge`: on a root-owned layout, offer to install it. True when this took the change (it asked,
    /// refused, or installed), false when it is not this module's: the layout is the user's own, the setting is off,
    /// or the helper is not there (the notice of the loop's own install says the one step).
    pub(crate) async fn root_install_after_join(
        self: &Arc<Self>,
        place: &Place,
        merge: &str,
        branch: &str,
        what: &str,
    ) -> bool {
        let mode = self.cfg.self_improve.install;
        let board = self.tools.root_install.clone();
        if mode == SelfInstall::Off || !is_commit(merge) {
            return false;
        }
        let units = board.units();
        let Ok(unit) = blocking(move || units.read()).await else {
            return false;
        };
        let Some(program) = exec_program(&unit.text).filter(|p| rooted(Path::new(p)).is_some())
        else {
            return false;
        };
        let helper = board.helper();
        if let Ok(Presence::NotInstalled(why)) = blocking(move || Ok(helper.presence())).await {
            self.root_no_helper(mode, merge, what, &why);
            return false;
        }
        if let Some(why) = &unit.refusal {
            // The start command wins from a file Theseus will not write: no card, no build, and the owner is told.
            let step = self.root_hand_step(merge, &program);
            self.root_row(
                "refused",
                merge,
                "the service's start command is not Theseus's to change",
                why,
                json!({"step": step}),
            );
            self.root_tell(format!(
                "The change Theseus joined ({what}, {c8}) is not what the service runs: Theseus did not install it, \
                 because {why}. {by_hand}",
                c8 = short(merge),
                by_hand = hand_words(step.as_deref()),
            ));
            return true;
        }
        // The loop is installing from here (issue 41): its build, then the helper's run, until a terminal answer or the
        // record's own stages.
        self.root_install_busy(merge, branch, what);
        let (p, m, build) = (place.clone(), merge.to_string(), board.build());
        let target = match blocking(move || install::built_tree(&p, &m, &*build)).await {
            Ok(t) => t,
            Err(e) => {
                self.root_install_idle();
                let why = format!("{e:#}");
                self.root_row(
                    "refused",
                    merge,
                    "the build for the install failed",
                    &why,
                    Value::Null,
                );
                self.root_tell(format!(
                    "Theseus joined a change to main ({what}, {c8}) but could not build it for the install: {why}",
                    c8 = short(merge)
                ));
                return true;
            }
        };
        let go = Go {
            question: "auto".into(),
            merge: merge.into(),
            branch: branch.into(),
            what: what.into(),
            target,
            old_program: program,
            approved_by: "config".into(),
            via: "[self] install = auto".into(),
        };
        if mode == SelfInstall::Auto {
            self.root_row(
                "auto",
                merge,
                "installing with no card: [self] install is auto",
                "",
                Value::Null,
            );
            tokio::spawn(self.clone().run_root_install(go));
            return true;
        }
        // The card waits on the owner: nothing is under way until the answer.
        self.root_install_idle();
        if let Err(e) = self.ask_root_install(&go) {
            self.root_row(
                "refused",
                merge,
                "the card could not be made",
                &format!("{e:#}"),
                Value::Null,
            );
        }
        true
    }

    /// The helper is not there: a row, and for `"ask"` (which has no other word to the owner) the plain step.
    fn root_no_helper(&self, mode: SelfInstall, merge: &str, what: &str, why: &str) {
        self.root_row(
            "no_helper",
            merge,
            "the helper is not installed",
            why,
            Value::Null,
        );
        if mode.asks() {
            self.root_tell(format!(
                "Theseus joined a change to main ({what}, {c8}). This machine runs a root-owned build, and the \
                 helper that would let Theseus install it itself is not installed ({why}), so it is not \
                 installed. To install it by hand: docs/setup.md, \"A root-owned install\". To let Theseus do \
                 it, the administrator runs scripts/root-install.sh once (docs/root-install.md).",
                c8 = short(merge)
            ));
        }
    }

    /// The install's own session, opened at its first card: the question is planned on it, as a pack promotion's is on
    /// the ladder's (`rpc::packs`), since the owner's CLI and cockpit open no session to name.
    fn install_session(&self) -> Result<SessionRecord> {
        if let Some(id) = self.store.get_meta::<String>(SESSION)? {
            if let Some(rec) = self.store.get_session::<SessionRecord>(&id)? {
                let open = rec
                    .execution_id
                    .as_deref()
                    .and_then(|x| self.kernel.execution(x).ok().flatten())
                    .is_some_and(|e| !e.state.is_terminal());
                if open {
                    return Ok(rec);
                }
            }
        }
        let rec = self.open_session(SessionOpenParams {
            dir: None,
            kind: None,
            label: Some("self-install: changes waiting on the owner".into()),
            opened_from: None,
            persona: None,
        })?;
        self.store.put_meta(SESSION, &rec.session_id)?;
        Ok(rec)
    }

    /// The card: "Install the change Theseus joined: <title>, <commit8>?", planned on the install's session in one short
    /// turn that parks it again. Returns the question's id.
    fn ask_root_install(&self, go: &Go) -> Result<String> {
        let rec = self.install_session()?;
        let exec = rec
            .execution_id
            .clone()
            .context("the install's session has no execution")?;
        let old = Path::new(&go.old_program)
            .parent()
            .map_or_else(String::new, |d| d.display().to_string());
        let question = question_text(go, &old);
        let proposal = Proposal {
            tool: ACK.into(),
            args: json!({
                "question": question, "settled": crate::config_change::words::install(&go.merge), "merge": go.merge, "branch": go.branch, "what": go.what,
                "target": go.target, "old_program": go.old_program,
            }),
            resource: Some(format!("self-install:{}", short(&go.merge))),
            policy_context: json!({}),
        };
        let guard = self.kernel.admit_input(&exec)?;
        let planned = self.kernel.plan_confirm_with(
            &guard,
            &proposal,
            RetryClass::NonRepeatable,
            None,
            // The card alone, with no code (issue 57).
            |_| Ok(Vec::new()),
        );
        let ended = self
            .kernel
            .end_turn(guard, TurnEnd::Wait { wake: Wake::Input });
        let a = planned?;
        ended?;
        let ttl = self.kernel.config().confirm_ttl_ms;
        self.tools
            .question_due
            .fetch_min(a.planned_at_ms + ttl, SeqCst);
        self.session_rec(&rec.session_id)
            .record(&crate::fact::tool::CallAsked {
                request: &confirm_request(&a, &rec, ttl),
            });
        self.root_row(
            "asked",
            &go.merge,
            &format!("asked the owner to install {}", short(&go.merge)),
            &go.what,
            json!({"question": a.correlation_id}),
        );
        Ok(a.correlation_id)
    }
}

/// The card's words, for someone who is not an expert: what is installed, where, and the way back.
pub(super) fn question_text(go: &Go, old_dir: &str) -> String {
    let c8 = short(&go.merge);
    format!(
        "Install the change Theseus joined: {what}, {c8}?\n\
         It is already joined to main. Theseus will put its programs in {LIB_DIR}/{c8} with the administrator's \
         helper, point the service at them, and restart. A turn that is running when it restarts is cut. If the new \
         build is not answering within {mins} minutes, Theseus goes back to the build before ({old_dir}) and tells \
         you.",
        what = go.what,
        mins = CONFIRM_WITHIN_SECS / 60,
    )
}

/// The question as the owner sees it: its card, `confirm.list`.
pub fn confirm_request(a: &Action, session: &SessionRecord, ttl_ms: u64) -> ConfirmRequest {
    let args = a
        .proposal
        .as_ref()
        .map(|p| p.args.clone())
        .unwrap_or_default();
    let plain = crate::config_change::words::plain_of(&args);
    crate::rpc::offer(ConfirmRequest {
        correlation_id: a.correlation_id.clone(),
        session_id: session.session_id.clone(),
        execution_id: a.execution_id.clone(),
        turn_id: None,
        tool: ACK.into(),
        reason: args["question"].as_str().unwrap_or_default().to_string(),
        input: args,
        resource: a.resource.clone(),
        by: OPERATOR.into(),
        requested_at_ms: a.planned_at_ms,
        expires_at_ms: a.planned_at_ms + ttl_ms,
        floor: false,
        budget: None,
        task: crate::task::task_ref(session),
        change: None,
        options: Vec::new(),
        may_answer: None,
        preview: None,
        grant_offer: None,
        plain,
        // `confirm.list` reads the proof's marks onto it; the card asked says it in `ask_root_install`.
        needs_proof: false,
        needs_fresh_proof: false,
        proof: None,
    })
}

/// The build tree of the install of `merge`, where its programs are.
pub(super) fn release_of(target: &Path) -> PathBuf {
    target.join("release")
}
