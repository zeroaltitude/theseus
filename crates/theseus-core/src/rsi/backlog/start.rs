//! A night's last two steps: the top unmarked items start, each in a session
//! of its own under the reserved `"self"` principal, and the owner is told.
//! A started item whose execution has ended is closed first, so its cause can
//! be filed again if it still shows.

use anyhow::Result;
use serde_json::json;
use theseus_kernel::ExecState;
use theseus_protocol::tasks::{TaskEvidence, TaskState};
use theseus_protocol::SessionOpenParams;

use super::{gate, Gate, Night, Started, Work, PRINCIPAL};
use crate::fact::rsi_backlog::{BacklogStarted, Spoken};
use crate::rpc::Core;
use crate::rsi::mint::kind::Choice;
use theseus_protocol::rsi::SelfMode;

/// The evidence entry that names a started item's execution.
const EXECUTION: &str = "execution:";

/// How a minted item ended (`backlog_mint_settle`).
pub(crate) enum Settled {
    /// Every tier passed, for the spec of this digest.
    Passed(String),
    /// A tier failed, or the mint could not run: why.
    Failed(String),
    /// The model never called the tool: the motif is real, the tool's shape
    /// or description is wrong. The evidence goes back with the item.
    Back(String),
}

impl Core {
    /// Close the started items whose executions ended: `done` when it
    /// completed, else `failed`.
    pub(super) fn backlog_sweep(&self, w: &mut Work) {
        let ended: Vec<(String, TaskState)> = w
            .tasks
            .values()
            .filter(|t| t.state == TaskState::InProgress)
            .filter_map(|t| {
                let id = t
                    .evidence
                    .iter()
                    .find_map(|e| e.identity.strip_prefix(EXECUTION))?;
                let e = self.kernel.execution(id).ok().flatten()?;
                e.state.is_terminal().then(|| {
                    let state = match e.state {
                        ExecState::Complete => TaskState::Done,
                        _ => TaskState::Failed,
                    };
                    (t.id.clone(), state)
                })
            })
            .collect();
        for (id, state) in ended {
            if let Some(t) = w.tasks.get_mut(&id) {
                t.state = state;
                t.version += 1;
                t.updated_at_ms = w.now;
                t.evidence.push(TaskEvidence {
                    node: None,
                    identity: format!("ended:{}", state.as_str()),
                    note: Some("its session ended".into()),
                    by: "self-backlog".into(),
                    at_ms: w.now,
                });
                w.touch(&id);
            }
        }
    }

    /// Start one proposed item now, whatever its rank: the self pipeline's
    /// start (theseus-pw1q.16), under the backlog's lock as a night's is.
    /// None when it is not proposed (dropped, started, or held meanwhile).
    pub(crate) fn backlog_start_item(&self, id: &str) -> Result<Option<Started>> {
        let _one = self
            .rsi
            .backlog
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut w = self.backlog_work()?;
        let mut night = Night::default();
        self.backlog_start(&mut w, 1, &mut night, Some(id))?;
        Ok(night.started.pop())
    }

    /// Start up to `n` of the top unmarked items (or `only` that one), one
    /// frame each.
    pub(super) fn backlog_start(
        &self,
        w: &mut Work,
        n: usize,
        night: &mut Night,
        only: Option<&str>,
    ) -> Result<()> {
        let mut top: Vec<(u32, String)> = w
            .tasks
            .values()
            .filter(|t| t.state == TaskState::Proposed)
            .filter(|t| only.is_none_or(|o| o == t.id))
            // A pattern is minted by a later step, never started as a fix.
            .filter(|t| w.st.items.get(&t.id).is_some_and(|m| m.pattern.is_none()))
            .filter_map(|t| w.st.items.get(&t.id).map(|m| (m.rank, t.id.clone())))
            .filter(|(rank, _)| *rank > 0 || only.is_some())
            .collect();
        top.sort();
        for (rank, id) in top.into_iter().take(n) {
            if gate(&self.self_ctx()) != Gate::Allowed {
                break;
            }
            let Some(title) = w.tasks.get(&id).map(|t| t.title.clone()) else {
                continue;
            };
            let rec = self.open_session_as(
                SessionOpenParams {
                    label: Some(format!("self: {title}")),
                    ..SessionOpenParams::default()
                },
                theseus_kernel::Authority {
                    principal: PRINCIPAL.into(),
                    ..Default::default()
                },
                None,
            )?;
            let session = rec.session_id.clone();
            let Some(t) = w.tasks.get_mut(&id) else {
                continue;
            };
            t.session = Some(session.clone());
            t.state = TaskState::InProgress;
            t.version += 1;
            t.updated_at_ms = w.now;
            for identity in [
                format!("session:{session}"),
                format!("{EXECUTION}{}", rec.execution_id.unwrap_or_default()),
            ] {
                t.evidence.push(TaskEvidence {
                    node: None,
                    identity,
                    note: None,
                    by: "self-backlog".into(),
                    at_ms: w.now,
                });
            }
            let objective = t.objective.clone();
            if let Some(m) = w.st.items.get_mut(&id) {
                m.rank = 0;
            }
            w.said.push(Spoken::Started(BacklogStarted {
                item: id.clone(),
                title: title.clone(),
                session: session.clone(),
                rank,
            }));
            w.touch(&id);
            self.backlog_write(w)?;
            night.started.push(Started {
                item: id,
                title,
                session_id: session,
                objective,
                mint: None,
            });
        }
        Ok(())
    }

    /// The mint start, beside the fix start with a cap of its own
    /// (`[self.mint] per_night`, so neither starves the other): the top pattern
    /// items that the kind rule lets be minted start in a `"self"` session, whose
    /// first turn is the writer's brief, made from the build half of the
    /// motif's occurrences alone. Nothing starts with `[self.mint] mine`
    /// off, the mode not `act`, or `rsi::gate` not allowing. A motif is minted
    /// at most once: one whose pending record exists is not started again.
    /// A skeleton that would need the network or a secret is not minted: its
    /// item waits for the owner's word (`waiting_human`, as a keel class does).
    pub(super) fn backlog_mint_start(&self, w: &mut Work, night: &mut Night) -> Result<()> {
        let mint = &self.cfg.self_improve.mint;
        if !mint.mine
            || mint.per_night == 0
            || self.cfg.self_improve.mode != SelfMode::Act
            || gate(&self.self_ctx()) != Gate::Allowed
        {
            return Ok(());
        }
        let mut top: Vec<(u32, String, String)> = w
            .tasks
            .values()
            .filter(|t| t.state == TaskState::Proposed)
            .filter_map(|t| {
                let m = w.st.items.get(&t.id)?;
                (m.pattern.is_some() && m.rank > 0).then(|| (m.rank, t.id.clone(), m.cause.clone()))
            })
            .collect();
        if top.is_empty() {
            return Ok(());
        }
        top.sort();
        let mined = match self.mine_run(w.now, false) {
            Ok(n) => n.mined,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "self mint: the motifs were not read");
                return Ok(());
            }
        };
        let mut started = 0u32;
        for (rank, id, cause) in top {
            if started >= mint.per_night || gate(&self.self_ctx()) != Gate::Allowed {
                break;
            }
            let Some(motif) = mined.motifs.iter().find(|m| m.cause_key() == cause) else {
                continue;
            };
            if self.mint_pending_exists(motif) {
                continue;
            }
            match self.mint_choice(motif) {
                Choice::NotAllowed(_) => {}
                Choice::OwnerCard { why } => Self::mint_owner_card(w, &id, why),
                Choice::Mint(kind) => {
                    // The data rule: a private motif is written only on a model
                    // of US origin, failing closed (theseus-xs5ah).
                    if let Err(why) = self.mint_profile_ok(motif) {
                        tracing::warn!(item = %id, %why, "self mint: not started");
                        continue;
                    }
                    match self.mint_open(w, (&id, rank), motif, kind) {
                        Ok(Some(s)) => {
                            night.started.push(s);
                            started += 1;
                        }
                        Ok(None) => {}
                        // A motif that cannot be prepared (its skeleton is no
                        // valid flow, its check half could not be written)
                        // fails its own item, saying why, and nothing else:
                        // the night's fix starts still run.
                        Err(e) => Self::mint_unprepared(w, &id, &format!("{e:#}")),
                    }
                }
            }
        }
        Ok(())
    }

    /// An item the owner decides: the tool would need the network or a secret.
    fn mint_owner_card(w: &mut Work, id: &str, why: &str) {
        let now = w.now;
        let Some(t) = w.tasks.get_mut(id) else {
            return;
        };
        t.state = TaskState::WaitingHuman;
        t.version += 1;
        t.updated_at_ms = now;
        t.evidence.push(TaskEvidence {
            node: None,
            identity: format!("owner-card:{why}"),
            note: Some(format!(
                "{why}: a minted tool never asks for either on its own"
            )),
            by: "self-mint".into(),
            at_ms: now,
        });
        w.touch(id);
    }

    /// An item whose mint could not be prepared: `failed`, with the reason.
    /// A later night does not try it again (the owner can reopen it).
    fn mint_unprepared(w: &mut Work, id: &str, why: &str) {
        tracing::warn!(item = %id, %why, "self mint: not started");
        let now = w.now;
        let Some(t) = w.tasks.get_mut(id) else {
            return;
        };
        t.state = TaskState::Failed;
        t.version += 1;
        t.updated_at_ms = now;
        t.evidence.push(TaskEvidence {
            node: None,
            identity: "mint:failed".into(),
            note: Some(format!("the mint could not be prepared: {why}")),
            by: "self-mint".into(),
            at_ms: now,
        });
        w.touch(id);
    }

    /// Start one minted item: its brief, its check half, and a `"self"`
    /// session of its own for the writer. None when the item is gone.
    fn mint_open(
        &self,
        w: &mut Work,
        (id, rank): (&str, u32),
        motif: &crate::rsi::mine::Motif,
        kind: crate::rsi::mint::kind::Kind,
    ) -> Result<Option<Started>> {
        let (pending, brief) = self.mint_prepare(id, motif, kind)?;
        let title = w.tasks.get(id).map(|t| t.title.clone()).unwrap_or_default();
        let rec = self.open_session_as(
            SessionOpenParams {
                label: Some(format!("self mint: {title}")),
                ..SessionOpenParams::default()
            },
            theseus_kernel::Authority {
                principal: PRINCIPAL.into(),
                ..Default::default()
            },
            None,
        )?;
        let session = rec.session_id.clone();
        let now = w.now;
        let Some(t) = w.tasks.get_mut(id) else {
            return Ok(None);
        };
        t.session = Some(session.clone());
        t.state = TaskState::InProgress;
        t.version += 1;
        t.updated_at_ms = now;
        t.objective.clone_from(&brief);
        for identity in [
            format!("session:{session}"),
            format!("{EXECUTION}{}", rec.execution_id.unwrap_or_default()),
            format!("mint:{}", pending.placeholder),
        ] {
            t.evidence.push(TaskEvidence {
                node: None,
                identity,
                note: None,
                by: "self-mint".into(),
                at_ms: now,
            });
        }
        if let Some(m) = w.st.items.get_mut(id) {
            m.rank = 0;
        }
        w.said.push(Spoken::Started(BacklogStarted {
            item: id.into(),
            title: title.clone(),
            session: session.clone(),
            rank,
        }));
        w.touch(id);
        self.backlog_write(w)?;
        Ok(Some(Started {
            item: id.into(),
            title,
            session_id: session,
            objective: brief,
            mint: Some(pending.placeholder),
        }))
    }

    /// A minted item's end: a pass closes it `done`, a failure `failed`, and a
    /// tool the model never called goes back to `proposed` with that evidence.
    /// Under the backlog's lock, as every change to it is.
    pub(crate) fn backlog_mint_settle(&self, item: &str, outcome: Settled) -> Result<()> {
        let _one = self
            .rsi
            .backlog
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut w = self.backlog_work()?;
        let (state, identity, note) = match outcome {
            Settled::Passed(digest) => (
                TaskState::Done,
                format!("validated:{digest}"),
                "its open replays passed".to_string(),
            ),
            Settled::Failed(why) => (TaskState::Failed, "mint:failed".into(), why),
            Settled::Back(evidence) => (TaskState::Proposed, "mint:back".into(), evidence),
        };
        if let Some(t) = w.tasks.get_mut(item) {
            t.state = state;
            t.version += 1;
            t.updated_at_ms = w.now;
            t.evidence.push(TaskEvidence {
                node: None,
                identity,
                note: Some(note),
                by: "self-mint".into(),
                at_ms: w.now,
            });
            w.touch(item);
            self.backlog_write(&mut w)?;
        }
        Ok(())
    }

    /// One line to the owner's DM (the digest's path) when the night filed,
    /// started, trimmed or holds something for the owner.
    pub(super) fn backlog_tell(&self, w: &Work, night: &Night) {
        let quiet = night.filed.is_empty() && night.started.is_empty() && night.trimmed.is_empty();
        if quiet || !self.runner.place_rule.owner_dm_bound() {
            return;
        }
        let title = |id: &String| -> String {
            w.tasks
                .get(id)
                .map_or_else(|| id.clone(), |t| t.title.clone())
        };
        let mut text = String::from("Self backlog, last night:\n");
        for s in &night.started {
            text.push_str(&format!("- started: {} ({})\n", s.title, s.item));
        }
        for id in night.filed.iter().take(10) {
            let held = night.waiting.contains(id);
            text.push_str(&format!(
                "- filed{}: {} ({id})\n",
                if held {
                    ", waits for your word (keel)"
                } else {
                    ""
                },
                title(id)
            ));
        }
        if night.filed.len() > 10 {
            text.push_str(&format!("- and {} more filed\n", night.filed.len() - 10));
        }
        for id in &night.trimmed {
            text.push_str(&format!("- dropped for size: {} ({id})\n", title(id)));
        }
        text.push_str("Reorder or drop with `theseus self move <item> <rank>` / `theseus self drop <item>`; `theseus self backlog` lists it.");
        let written = (|| -> Result<()> {
            let (a, frame) = self
                .outbox
                .stage_to_operator(None, json!({"kind": "self_digest", "text": text}))?;
            self.store.append(&frame)?;
            self.outbox.posted(&a);
            Ok(())
        })();
        if let Err(e) = written {
            tracing::warn!(error = %format!("{e:#}"), "self backlog: the night's notice was not written");
        }
    }
}
