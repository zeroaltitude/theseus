//! The judgment's filing, in the judgment's frame (theseus-xzxqv): the
//! sink's frame already holds each `onto.v1` judgment's row, and the records
//! Jev's answers came to, their rows, and the owner's cards ride it too, so a
//! write and the answer it rests on are written together or not at all.
//!
//! The decision is made again here, under the ontology's writer lock, against
//! the snapshot the records are checked against, so no write is built on a
//! list another has changed since the reply's footer was said. A plan with
//! nothing to write, ask or refuse stages nothing: a plain turn adds no byte.

use serde_json::json;
use theseus_judge::{Judgment, Mode};
use theseus_kernel::Action;
use theseus_ontology::Origin;
use theseus_protocol::now_unix_ms;
use theseus_store::NewRecord;

use super::decide::{decide, Bands, Plan, Step};
use super::ONTO_PACK;
use crate::fact::{self, onto::OntoFiled};
use crate::ontology::Staged;
use crate::rpc::ontology_machine::{scope, Machine};
use crate::rpc::Core;

/// `[ontology]`'s bands.
pub(super) fn bands(core: &Core) -> Bands {
    Bands {
        act: core.cfg.ontology.act,
        confirm: core.cfg.ontology.confirm,
    }
}

/// A frame's worth of filing, staged: appended with the sink's records, then
/// [`Pending::written`].
pub struct Pending<'a> {
    core: &'a Core,
    staged: Staged<'a>,
    machines: Vec<Machine>,
    plans: Vec<Plan>,
    posts: Vec<Action>,
}

impl Pending<'_> {
    /// The records and rows to append to the frame.
    pub fn frame(&self) -> &[NewRecord] {
        self.staged.frame()
    }

    /// The frame was written: hold the snapshot, say each write, and send
    /// the cards.
    pub fn written(self) {
        self.staged.commit();
        for m in &self.machines {
            m.announce(self.core);
        }
        for p in &self.plans {
            self.core
                .rec(Some(&p.session))
                .announce(&OntoFiled { plan: p });
            if let Some(project) = &p.attach {
                attach(self.core, &p.session, project);
            }
        }
        for post in &self.posts {
            self.core.outbox.posted(post);
        }
    }
}

/// Attach `session` to `project`, only while it has none (theseus-jrf4c): what
/// the owner or the model attached first stands, and Jev never moves it. The
/// slice reaches the session's next compile, as `project.attach`'s does.
pub fn attach(core: &Core, session: &str, project: &str) {
    let done = core.store.update_session(session, |r| {
        if r.project.is_none() {
            r.project = Some(project.to_string());
        }
        Ok(vec![])
    });
    if let Err(e) = done {
        tracing::warn!(session, error = %format!("{e:#}"), "judge: the project was not attached");
    }
}

/// The card a step sends, if it sends one: a mint says so with an undo; a
/// question asks, and writes nothing; a plain filing sends none.
fn card(plan: &Plan, step: &Step) -> Option<serde_json::Value> {
    match step {
        Step::Filed {
            kind,
            path,
            minted: true,
            confidence,
            ..
        } => Some(json!({
            "kind": "onto_card", "write": plan.judgment, "undo": true,
            "text": minted_text(kind, &path.join(" › "), *confidence, &plan.judgment),
        })),
        Step::Ask {
            text,
            category,
            confidence,
            ..
        } => {
            let how = category.as_ref().map_or_else(String::new, |c| {
                format!(
                    "\n`theseus ontology member {} +{c}` files it.",
                    plan.session
                )
            });
            Some(json!({
                "kind": "onto_card", "write": plan.judgment, "undo": false,
                "text": ask_text(text, *confidence, &how),
            }))
        }
        _ => None,
    }
}

/// A mint's card (theseus-x8b9k).
pub(crate) fn minted_text(kind: &str, path: &str, confidence: f64, write: &str) -> String {
    crate::owner_note::note(
        crate::owner_note::Kind::Fyi,
        &format!("Jev started a new {kind}: {path}"),
        &format!(
            "Jev filed this session under it ({:.0}% sure)",
            confidence * 100.0
        ),
        &format!("`theseus ontology undo {write}` takes it back."),
    )
}

/// A question's card: Jev's own question, and the command that files it.
pub(crate) fn ask_text(question: &str, confidence: f64, how: &str) -> String {
    crate::owner_note::note(
        crate::owner_note::Kind::Decision,
        "Where does this session belong?",
        &format!(
            "Jev was not sure enough to file it ({:.0}% sure)",
            confidence * 100.0
        ),
        &format!("{question}{how}"),
    )
}

/// Whether a judgment of the batch is one the ontology may act on: `onto.v1`'s,
/// answered by the model its pack pins, in a mode that acts.
fn acts(j: &Judgment) -> bool {
    j.pack == ONTO_PACK && j.actionable() && j.mode != Mode::Shadow
}

/// The filing the batch's `onto.v1` judgments come to, staged, or none: no
/// such judgment, or nothing to write or say. A failure is warned of and the
/// frame goes without it, so the judgments' own rows are always written.
pub fn stage<'a>(core: &'a Core, batch: &[Judgment]) -> Option<Pending<'a>> {
    let wanted: Vec<&Judgment> = batch.iter().filter(|j| acts(j)).collect();
    if wanted.is_empty() {
        return None;
    }
    let (bands, now_ms) = (bands(core), now_unix_ms());
    let (mut machines, mut plans, mut posts) = (Vec::new(), Vec::new(), Vec::new());
    let staged = core.runner.ontology.stage(&core.store, Origin::Jev, |o| {
        // One working copy, so a second judgment of the batch for the same
        // session is decided on the first's writes.
        let (mut now, mut records, mut rows) = (o.clone(), Vec::new(), Vec::new());
        for j in &wanted {
            let plan = decide(j, &now, bands, now_ms);
            if plan.is_empty() {
                continue;
            }
            if !plan.records.is_empty() {
                let m = Machine::new(&now, Origin::Jev, &j.id, plan.records.clone())?;
                rows.extend(m.rows()?);
                for r in &plan.records {
                    now.put(r.clone(), Origin::Jev)?;
                }
                records.extend(plan.records.iter().cloned());
                machines.push(m);
            }
            rows.push(
                fact::row(&OntoFiled { plan: &plan }, Some(&plan.session), None)?
                    .scoped(&scope(&j.id)),
            );
            for step in &plan.steps {
                if let Some(body) = card(&plan, step) {
                    let (post, more) = core.outbox.stage_to_operator(None, body)?;
                    rows.extend(more);
                    posts.push(post);
                }
            }
            plans.push(plan);
        }
        Ok((records, rows))
    });
    match staged {
        Ok(Some(staged)) => Some(Pending {
            core,
            staged,
            machines,
            plans,
            posts,
        }),
        Ok(None) => None,
        Err(e) => {
            tracing::warn!(error = %format!("{e:#}"), "judge: onto.v1's filing was not staged");
            None
        }
    }
}

/// [`stage`], with its frame added to the sink's `records`.
pub fn stage_into<'a>(
    core: &'a Core,
    batch: &[Judgment],
    records: &mut Vec<NewRecord>,
) -> Option<Pending<'a>> {
    let pending = stage(core, batch)?;
    records.extend(pending.frame().iter().cloned());
    Some(pending)
}
