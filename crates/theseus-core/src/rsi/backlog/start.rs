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

/// The evidence entry that names a started item's execution.
const EXECUTION: &str = "execution:";

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
                format!(
                    "{EXECUTION}{}",
                    rec.execution_id.clone().unwrap_or_default()
                ),
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
            });
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
