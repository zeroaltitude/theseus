//! The binding's courier, as the sim plays it: each place's posts in order,
//! with the binding's own check at post time (theseus-discord's `courier.rs`,
//! `leave`): a reply or a report for a guild channel asks the core for its
//! readers, needs no read when they fit any audience the channel can have,
//! and otherwise reads who views the channel now, tells the core, and asks
//! `check_post`. A held post waits, and its place's later posts wait behind
//! it, until the owner answers.
//!
//! The invariant, at each post that goes: every atom it carries may be read by
//! whoever views its place at that moment (the truth, not the core's copy),
//! unless it was held and the owner released it. A held post's card goes
//! where approvals go, never to its place.

use std::collections::BTreeSet;

use anyhow::Result;
use serde_json::json;
use theseus_core::held::{Held, PostCheck};
use theseus_core::outbox::{body_of, kind_of, target_of, OPERATOR_TARGET};
use theseus_kernel::{Action, Outcome};

use super::atoms::{scan, Aud};
use super::world::OWNER;
use super::{push_all, HeldPost, Sim};

/// What the courier does with a place's next post.
enum Leave {
    /// It goes; `released` when the owner approved it after a hold.
    Go { released: bool },
    /// It is held, or its question waits: the place's posts wait.
    Waits,
    /// The owner declined it: the place gets the note instead.
    Note,
}

/// The channel a target names, if it is a guild channel.
fn channel_of(target: &str) -> Option<u64> {
    target
        .strip_prefix("discord:channel:")
        .and_then(|c| c.parse().ok())
}

impl Sim {
    /// Deliver every place's posts, in order, until each place waits on a
    /// held post or has none left.
    pub(crate) fn deliver(&mut self) -> Result<()> {
        for target in self.core.outbox.open_targets() {
            for _ in 0..256 {
                let Some(a) = self.core.outbox.next_for(&target) else {
                    break;
                };
                match self.leave(&a)? {
                    Leave::Waits => break,
                    Leave::Go { released } => {
                        self.check_post(&a, released)?;
                        self.settle(&a, json!({"messages": []}))?;
                    }
                    Leave::Note => {
                        self.rep.held_back += 1;
                        self.settle(&a, json!({"messages": [], "held_back": true}))?;
                    }
                }
            }
        }
        Ok(())
    }

    fn settle(&mut self, a: &Action, detail: serde_json::Value) -> Result<()> {
        self.core.outbox.dispatch(&a.correlation_id)?;
        self.core
            .outbox
            .settle(&a.correlation_id, Outcome::Succeeded, None, detail, "sim")?;
        self.rep.posts += 1;
        if kind_of(a) == "report" {
            self.rep.reports += 1;
        }
        let what = format!("posted a {} to {}", kind_of(a), target_of(a));
        self.note(&what);
        Ok(())
    }

    /// The binding's `leave`: may this post go now?
    fn leave(&mut self, a: &Action) -> Result<Leave> {
        let Some(channel) = channel_of(target_of(a)) else {
            return Ok(Leave::Go { released: false });
        };
        if !matches!(kind_of(a), "reply" | "report") {
            return Ok(Leave::Go { released: false });
        }
        match self.core.held_state(&a.correlation_id) {
            Some(Held::Waiting) => return Ok(Leave::Waits),
            Some(Held::Released) => {
                // Only the owner's approval releases a held post.
                let q = self.core.outbox.held_question(&a.correlation_id);
                if !q.as_ref().is_some_and(|q| self.approved.contains(q)) {
                    return Err(self.violation(
                        "posted",
                        format!(
                            "a held post {} reads as released, and the owner did not approve its question {q:?}",
                            a.correlation_id
                        ),
                    ));
                }
                self.rep.released += 1;
                return Ok(Leave::Go { released: true });
            }
            Some(Held::HeldBack) => return Ok(Leave::Note),
            None => {}
        }
        let Some(readers) = self.core.post_readers(a) else {
            return Ok(Leave::Go { released: false });
        };
        let place = format!("discord:{channel}");
        if theseus_core::labels::fits_any_audience(&readers, Some(&place)) {
            return Ok(Leave::Go { released: false });
        }
        // A fresh read of who views the channel: told to the core when the
        // bot can read it, and a refused read counts as public.
        self.rep.read_at_post += 1;
        let (readable, push) = {
            let mut s = self.lock();
            let readable = s.channel(channel).is_some_and(|c| c.readable);
            (readable, if readable { s.read_one(channel) } else { None })
        };
        if let Some(p) = push {
            push_all(&self.core, &[p]);
        }
        match self.core.check_post(a, &readers, readable, 0.0)? {
            PostCheck::Go => Ok(Leave::Go { released: false }),
            PostCheck::Held(question) => {
                self.rep.held += 1;
                self.note(&format!("held a post in channel {channel}"));
                self.held.push(HeldPost { question });
                Ok(Leave::Waits)
            }
        }
    }

    /// A post that goes: every atom it carries may be read by whoever views
    /// its place now, unless the owner released it after a hold. A held
    /// post's card goes only where approvals go.
    fn check_post(&mut self, a: &Action, released: bool) -> Result<()> {
        let target = target_of(a).to_string();
        let body = body_of(a);
        if body.get("held").is_some() && target != OPERATOR_TARGET {
            return Err(self.violation(
                "posted",
                format!("a held post's card went to {target}, not where approvals go"),
            ));
        }
        // What the binding renders: a reply's loops; a report's title and its
        // task's last message (theseus-discord's `render::report`); any other
        // post, everything it carries.
        let mut titled = BTreeSet::new();
        let text = match kind_of(a) {
            "reply" => self
                .core
                .outbox
                .reply_texts(body)
                .into_iter()
                .map(|(_, t)| t)
                .collect::<Vec<_>>()
                .join("\n"),
            "report" => {
                let title = body["title"].as_str().unwrap_or("");
                titled.extend(scan(title));
                let said = body["node"]
                    .as_str()
                    .and_then(|n| self.core.outbox.said(n))
                    .unwrap_or_default();
                format!("{title}\n{said}")
            }
            _ => body.to_string(),
        };
        // Who views the place now is the truth, whether or not the bot can
        // read it; another place's readers are judged as the core last knew
        // them (only the post's own place is read at post time).
        let (truth, views) = {
            let s = self.lock();
            (s.truth(), s.pushed_views())
        };
        let aud = if target == OPERATOR_TARGET {
            Aud::Owner
        } else {
            super::check::target_aud(Some(&target), &truth)
        };
        if released {
            // The owner let it go to whoever views the place now.
            let mut s = self.lock();
            for id in scan(&text) {
                s.atoms.saw(id, &aud);
            }
            return Ok(());
        }
        self.judge_post(a, &text, &aud, (&views, &titled))
    }

    fn judge_post(
        &mut self,
        a: &Action,
        text: &str,
        aud: &Aud,
        (views, titled): (&super::atoms::Views, &BTreeSet<u32>),
    ) -> Result<()> {
        let bad: Vec<u32> = {
            let s = self.lock();
            scan(text)
                .into_iter()
                .filter(|id| !s.atoms.allowed(*id, aud, OWNER, views))
                .collect()
        };
        let bad = self.excuse(bad, titled);
        self.rep.invariant_checks += 1;
        if bad.is_empty() {
            // Who views the place was shown what the post carried.
            let mut s = self.lock();
            for id in scan(text) {
                s.atoms.saw(id, aud);
            }
            return Ok(());
        }
        let what = {
            let s = self.lock();
            bad.iter()
                .map(|id| s.atoms.describe(*id))
                .collect::<Vec<_>>()
                .join(", ")
        };
        Err(self.violation(
            "posted",
            format!(
                "a {} post {} went to {} carrying {what}",
                kind_of(a),
                a.correlation_id,
                aud.describe()
            ),
        ))
    }
}
