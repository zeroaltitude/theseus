//! A task's model from its brief (theseus-ypnxt.7). A task the owner starts
//! with no profile has no message of its own, so its first turn would run on
//! its parent's model whatever the brief says. Instead its first turn asks
//! the route pack about the brief (that one request and no other pack) and waits
//! for the verdict as a conversation's first message does: the class's first
//! usable profile, under the place's cap, becomes the task's base
//! ([`routing::brief::brief_base`]). No verdict in time, a verdict under its
//! bar, or a class with no usable profile leaves the parent's model, as before.
//!
//! The mark that a task wants this is the META record `task.route.<session>`,
//! written in the open's frame when no profile was asked for; a task started
//! with a profile is pinned and has no mark. Every other turn pays a check of
//! `tc.task` and the turn's number.

use theseus_store::{kinds, NewRecord};

use super::*;
use crate::config::PackMode;
use crate::judge::inbound::{author_of, place_kind, Inbound};
use crate::routing::{self, Decision, Verdict};

/// The META key of a task whose base is chosen from its brief.
fn key(task_session: &str) -> String {
    format!("task.route.{task_session}")
}

/// Whether the task was started with no profile: its base came from its
/// brief or its parent, so it is not pinned (a checkpoint may move it).
pub(crate) fn marked(store: &crate::store::Store, task_session: &str) -> bool {
    matches!(store.get_meta::<String>(&key(task_session)), Ok(Some(_)))
}

/// The record a task opened with no profile carries.
pub(crate) fn mark(task_session: &str) -> anyhow::Result<NewRecord> {
    NewRecord::json(kinds::META, Some(&key(task_session)), &"brief")
}

impl TurnRunner {
    /// A task's first turn, marked for it, asks the route pack about
    /// the brief once its node is on the record; the verdict's channel is kept
    /// for the first compile, as for a message.
    pub(super) fn brief_point(&self, t: &mut Turn<'_>) {
        let (sid, class) = (t.tc.session_id, t.tc.class);
        if t.tc.task.is_none() || t.tc.guard.turn != 1 || !self.judge.config().enabled {
            return;
        }
        if !marked(t.tc.store, sid) {
            return;
        }
        // Live only: a shadow route (or none) leaves the parent's model.
        let route = self.route_mode(t.target, sid);
        if route < PackMode::Canary {
            return;
        }
        let Ok(Some(node)) = crate::task::brief(t.tc.store, sid) else {
            return;
        };
        let Body::UserMessage { text, .. } = &node.body else {
            return;
        };
        let place = self.outbox.try_target(sid).ok().flatten();
        let restricted = theseus_judge::builders::RestrictedModels::allowed_if(
            self.cn_read(t) == super::data_step::Cn::Allowed,
        );
        let wait = self.judge.at_brief(
            &mut t.trace,
            Inbound {
                session_id: sid.into(),
                execution_id: t.tc.execution_id.into(),
                turn_id: t.tc.turn_id.into(),
                node_id: node.id.clone(),
                text: text.clone(),
                place_kind: place_kind(place.as_deref(), "harness", class),
                author: author_of(class).into(),
                kernel: self.kernel.clone(),
                route,
                chosen: None,
                task: true,
                threads: Vec::new(),
                in_thread: false,
                persona: self.persona_ctx(sid, class),
                capture: false,
                data_names: self.cfg.privacy.data_names(),
                attachments: Vec::new(),
                restricted,
            },
        );
        t.route.brief = wait.is_some();
        Self::route_asked(t, wait, route, false);
    }

    /// The brief's verdict as the task's base: Jev's pick, at once
    /// (`routing::brief::brief_base`).
    pub(super) fn decide_brief(&self, t: &Turn<'_>, v: &Verdict) -> Decision {
        let profiles = self.route_profiles(t.route.floor());
        routing::brief::brief_base(
            &self.cfg.routing,
            &profiles,
            &routing::Ask {
                verdict: v,
                base: &t.target.profile,
                est_tokens: 0,
                hold: None,
                images: t.route.images,
                cleared: t.route.floor().cn_ok,
            },
        )
    }
}
