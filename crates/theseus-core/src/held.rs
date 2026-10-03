//! The held post (M4 19c; design m4-boundaries §2.7; decision 14: it waits,
//! and is never refused).
//!
//! A compile admits only what its audience may read (19a), but a guild
//! channel's audience can grow between the compile and the post: a member
//! joins, or a role or an overwrite changes, and the binding hears of a join
//! only at its next read (theseus-4qiz). So before a post that carries the
//! model's words leaves for a guild channel, the binding reads who can view
//! the channel then, never the copy the turn compiled for, and the outbox
//! asks `labels::may_leave(readers, audience)`:
//! - it fits: the post goes, with no new frame and no new write;
//! - it does not: the post is held, and a question goes to the owner, routed
//!   as approvals are (the approvals DM, `theseus confirm`, the web UI). The
//!   question, its card, and the `label.held_post` row are one frame.
//!   Approve posts it; decline leaves "a reply was held back" in its place.
//!
//! Who views a channel that cannot be read (no Server Members intent, a read
//! refused) counts as public (§5, question 6). A DM's audience is fixed, so a
//! DM's posts are never checked, and neither is a post whose readers fit any
//! audience its place can have (public, or the place's own words).

use anyhow::Result;
use serde_json::{json, Value};
use theseus_kernel::{Action, ActionState};
use theseus_protocol::{
    ActionConfirmResult, Audience, ConfirmRequest, HeldPosts, Readers, HELD_POST_TOOL,
};

use crate::fact;
use crate::labels::{self, Judge};
use crate::outbox::{body_of, kind_of, target_of, Closed, OPERATOR_TARGET};
use crate::peer::Traced;
use crate::session::SessionRecord;
use crate::Core;

/// Where a held post stands, by its question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Held {
    /// The owner has not answered.
    Waiting,
    /// Approved: it goes.
    Released,
    /// Declined, or its question closed otherwise (its session's execution
    /// was cancelled or stopped): its place gets the note instead.
    HeldBack,
}

/// What the check at post time decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PostCheck {
    /// Its readers cover who can view its place now: it goes.
    Go,
    /// Held, with its question's correlation id.
    Held(String),
}

/// The longest a held post's text goes on its question, in characters.
const PREVIEW_CHARS: usize = 1500;

/// The held post's question in words: what the post draws on, and what
/// changed.
pub fn held_reason(readers: &Readers, audience: &Audience) -> String {
    let (place, now) = match audience {
        Audience::Place {
            place,
            name,
            viewers,
            ..
        } => (
            name.as_ref()
                .map_or_else(|| format!("channel {place}"), |n| format!("#{n}")),
            match viewers {
                Some(1) => "1 person".to_string(),
                Some(n) => format!("{n} people"),
                None => "public: who can view it cannot be read".to_string(),
            },
        ),
        a => (a.describe(), a.describe()),
    };
    format!(
        "This reply draws on material labeled {}, and {place}'s audience changed (now {now}). \
         Post it?",
        readers.describe()
    )
}

impl Core {
    /// The readers of what a post carries of the model's words: a reply's
    /// loops' answers, met, and a task's report's node. A node from before
    /// labels is read as its place's own words. None for every other post:
    /// the harness's own words, which its place may always read.
    pub fn post_readers(&self, post: &Action) -> Option<Readers> {
        let body = body_of(post);
        let ids: Vec<&str> = match kind_of(post) {
            "reply" => body["loops"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|l| l[1].as_str())
                .collect(),
            "report" => body["node"].as_str().into_iter().collect(),
            _ => return None,
        };
        let target = target_of(post);
        let own = labels::readers_of(Some(target));
        let judge = self.runner.judge(&post.session_id, Some(target));
        let mut readers = Readers::Public;
        for id in ids {
            // A node that is gone posts nothing.
            let Ok(Some((_, n))) = self.store.get_node(id) else {
                continue;
            };
            let r = n.label.map_or_else(|| own.clone(), |l| l.readers);
            if r != readers {
                readers = judge.meet(&readers, &r);
            }
        }
        Some(readers)
    }

    /// A post the outbox held for the owner, and where it stands; None when
    /// it was never held.
    pub fn held_state(&self, post_id: &str) -> Option<Held> {
        let q = self.outbox.held_question(post_id)?;
        Some(match self.kernel.action(&q) {
            Ok(Some(a)) => match a.state {
                ActionState::Planned => Held::Waiting,
                ActionState::Succeeded => Held::Released,
                _ => Held::HeldBack,
            },
            // A question no longer in the store cannot be approved.
            Ok(None) => Held::HeldBack,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), post = post_id, "a held post's question is unreadable: it keeps waiting");
                Held::Waiting
            }
        })
    }

    /// The check at post time (§2.7): may `post`, whose words `readers` may
    /// read, leave for its place's audience now? The binding has just told
    /// the core who can view the place (`place_viewers`); `readable` is false
    /// when that read failed, and the place then counts as public (§5,
    /// question 6). When it may not, the post is held (`hold_post`).
    pub fn check_post(
        &self,
        post: &Action,
        readers: &Readers,
        readable: bool,
        read_ms: f64,
    ) -> Result<PostCheck> {
        let mut judge = self.runner.judge(&post.session_id, Some(target_of(post)));
        if !readable {
            judge = judge.counted_public();
        }
        if labels::may_leave(readers, &judge) {
            return Ok(PostCheck::Go);
        }
        let q = self.hold_post(post, readers, &judge, read_ms)?;
        Ok(PostCheck::Held(q.correlation_id))
    }

    /// Hold a post for the owner: its question (a planned action of the
    /// post's execution, `HELD_POST_TOOL`), its card where approvals go
    /// (never its own place, whose audience is the question), and its
    /// `label.held_post` row, in one frame.
    fn hold_post(
        &self,
        post: &Action,
        readers: &Readers,
        judge: &Judge,
        read_ms: f64,
    ) -> Result<Action> {
        let target = target_of(post);
        let text: String = self
            .outbox
            .reply_texts(body_of(post))
            .into_iter()
            .map(|(_, t)| t)
            .collect::<Vec<_>>()
            .join("\n\n");
        let text = match text.char_indices().nth(PREVIEW_CHARS) {
            Some((at, _)) => format!("{}…", &text[..at]),
            None => text,
        };
        let place_name = match &judge.audience {
            Audience::Place { name: Some(n), .. } => Some(format!("#{n}")),
            _ => None,
        };
        let args = json!({
            "post": post.correlation_id,
            "place": target,
            "place_name": place_name,
            "readers": readers,
            "audience": judge.audience,
            "text": text,
        });
        let ids: Vec<&str> = [post.execution_id.as_str()]
            .into_iter()
            .filter(|e| !e.is_empty())
            .collect();
        let (q, card) = self.kernel.frame(&ids, |k| {
            let (q, records) =
                k.held_post_stage(&post.session_id, &post.execution_id, target, args.clone())?;
            k.stage(&records)?;
            let body =
                json!({"kind": "card", "question": q.correlation_id, "held": post.correlation_id});
            let (card, records) =
                self.outbox
                    .stage(&post.session_id, &post.execution_id, OPERATOR_TARGET, body)?;
            k.stage(&records)?;
            let held = fact::label::PostHeld {
                post,
                question: &q.correlation_id,
                readers,
                audience: &judge.audience,
                read_ms,
            };
            let row = fact::row(&held, Some(&post.session_id), None)?;
            k.stage(std::slice::from_ref(&row))?;
            Ok((q, card))
        })?;
        self.outbox.held(&post.correlation_id, &q.correlation_id);
        self.outbox.posted(&card);
        self.session_rec(&post.session_id)
            .announce(&fact::label::PostHeld {
                post,
                question: &q.correlation_id,
                readers,
                audience: &judge.audience,
                read_ms,
            });
        Ok(q)
    }

    /// Answer a held post's question (through `confirm_action`, which judged
    /// the answer): approve, and it goes; decline, and its place gets the
    /// note. The question and the answer's row are one frame; then the
    /// question's card settles, and the lanes look again.
    pub(crate) fn answer_held_post(
        &self,
        q: &Action,
        approve: bool,
        note: Option<&str>,
        by: &str,
        via: &str,
        asker: &Traced,
    ) -> Result<ActionConfirmResult> {
        let post = q
            .proposal
            .as_ref()
            .and_then(|p| p.args["post"].as_str())
            .unwrap_or("")
            .to_string();
        let f = fact::label::HeldPostAnswered {
            question: q,
            post: &post,
            approve,
            by,
            via,
            asker: asker.json(),
        };
        let row = fact::row(&f, Some(&q.session_id), None)?;
        self.kernel.frame(&[&q.execution_id], |k| {
            if approve {
                k.release_held(&q.correlation_id, by)?;
            } else {
                k.decline_action(
                    &q.correlation_id,
                    by,
                    note.unwrap_or("the owner held the reply back"),
                )?;
            }
            k.stage(std::slice::from_ref(&row))?;
            Ok(())
        })?;
        self.session_rec(&q.session_id).announce(&f);
        self.card_closed(
            &q.correlation_id,
            Closed::new(if approve { "approved" } else { "declined" }, Some(by)),
        );
        self.outbox.wake();
        self.admission.notify_waiters();
        Ok(ActionConfirmResult {
            correlation_id: q.correlation_id.clone(),
            approved: approve,
            session_id: q.session_id.clone(),
            execution_id: q.execution_id.clone(),
            resumes: false,
        })
    }

    /// Health's held posts (M4 19c): held now, and since the start; None
    /// while none has been.
    pub fn held_health(&self) -> Option<HeldPosts> {
        let (now, since_start) = (self.outbox.held_now(), self.outbox.held_since_start());
        (now > 0 || since_start > 0).then_some(HeldPosts { now, since_start })
    }
}

/// A held post's question, as the confirm it is: what the post draws on,
/// who can view its place now, and the start of what it says. It holds
/// until it is answered.
pub(crate) fn held_confirm(q: &Action, session: &SessionRecord) -> Option<ConfirmRequest> {
    let args: &Value = &q.proposal.as_ref()?.args;
    let readers: Readers = serde_json::from_value(args["readers"].clone()).ok()?;
    let audience: Audience = serde_json::from_value(args["audience"].clone()).ok()?;
    Some(ConfirmRequest {
        correlation_id: q.correlation_id.clone(),
        session_id: session.session_id.clone(),
        execution_id: q.execution_id.clone(),
        tool: HELD_POST_TOOL.into(),
        input: args.clone(),
        resource: q.resource.clone(),
        reason: held_reason(&readers, &audience),
        by: crate::turn::OPERATOR.into(),
        requested_at_ms: q.planned_at_ms,
        expires_at_ms: 0,
        floor: false,
        budget: None,
        task: crate::task::task_ref(session),
        external_text: None,
    })
}
