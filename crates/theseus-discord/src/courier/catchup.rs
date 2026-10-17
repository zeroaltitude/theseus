//! One catch-up message after Discord was unreachable (theseus-ss36w): when a
//! lane comes back from `away()` and three or more of its waiting posts would
//! be silent and were written during the outage, they go as one silent
//! message, "While Discord was unreachable (14:02–14:19): 2 tasks reported,
//! 1 failed: …", each folded post settled with that message's id so none goes
//! twice. A child of `courier`.
//!
//! - **What folds** is news that was not a buzz: a task's report, a turn's
//!   failure and a plain notice, each when its category is silent in this
//!   place (`policy::pings` says no). **Never folded**: a card or a needs-you
//!   question (it always posts on its own), a settle, a reply (so the answer
//!   to the owner's own message, owed or not, is never folded), a reminder, a
//!   Jev notice, a post that may have landed already (`unsure`: its nonce
//!   dedupes the resend), and a pointer's quiet note.
//! - **Exactly once.** The fold's message key is its oldest post's, and the
//!   posts are dispatched before it is sent and settled after it, the newest
//!   first, so the oldest is the last to settle. A send that landed and a
//!   settle that then failed leave the fold in `Lane::catchup.folding`: the
//!   retry settles the rest against the same message and sends nothing. A
//!   crash in that window sends each post that was left on its own, as any
//!   post that may have landed is (the nonce of its own key dedupes it only
//!   within Discord's window).

use serde_json::{json, Value};
use theseus_core::outbox::{body_of, kind_of};
use theseus_kernel::{Action, Outcome};

use super::{lone, Lane, SendErr};
use crate::policy::{self, Event};
use crate::render::clip;

/// The fewest posts worth a message of their own.
const MIN: usize = 3;
/// Discord's limit on a message's text.
const LIMIT: usize = 2000;

/// What the lane remembers of an outage.
#[derive(Default)]
pub(super) struct State {
    /// The outage: when the first post that met it was written, and when the
    /// lane first noticed (wall clock, ms). None while Discord answers.
    outage: Option<(u64, u64)>,
    /// A fold whose message is sent and whose posts are not all settled.
    folding: Option<Folding>,
}

struct Folding {
    /// Its posts, oldest first.
    posts: Vec<String>,
    key: String,
    channel: u64,
    message: u64,
}

impl State {
    /// A post met an outage: `planned_ms` is when it was written.
    pub(super) fn begin(&mut self, planned_ms: u64) {
        let now = theseus_protocol::now_unix_ms();
        self.outage.get_or_insert((planned_ms, now));
    }

    /// Whether a lane that comes back has an outage to look at.
    pub(super) fn pending(&self) -> bool {
        self.outage.is_some() || self.folding.is_some()
    }
}

/// What kind of news a folded post is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Reported,
    Failed,
    Notice,
}

/// A folded post's line.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Line {
    kind: Kind,
    words: String,
}

/// The line `a` would add to a fold, when it may fold: a silent report,
/// failure or notice, written at or after `since_ms`.
fn line_of(
    a: &Action,
    since_ms: u64,
    silent: &[theseus_core::config::discord::Category],
) -> Option<Line> {
    fold_line(kind_of(a), body_of(a), a.planned_at_ms, since_ms, silent)
}

/// `line_of`'s rule over a post's parts.
fn fold_line(
    post_kind: &str,
    body: &Value,
    planned_ms: u64,
    since_ms: u64,
    silent: &[theseus_core::config::discord::Category],
) -> Option<Line> {
    if planned_ms < since_ms {
        return None;
    }
    let (kind, event, words) = match post_kind {
        "report" => {
            let event = policy::of_report(body);
            let title = body["title"]
                .as_str()
                .filter(|t| !t.is_empty())
                .or_else(|| body["short"].as_str())
                .unwrap_or("a task");
            let reason = body["reason"].as_str().unwrap_or("");
            let (kind, words) = match body["outcome"].as_str() {
                Some("complete") => (Kind::Reported, format!("📋 {title} finished")),
                Some("cancelled") => (Kind::Reported, format!("⏹️ {title} stopped")),
                _ if reason.is_empty() => (Kind::Failed, format!("⚠️ {title} failed")),
                _ => (Kind::Failed, format!("⚠️ {title} failed: {reason}")),
            };
            (kind, event, words)
        }
        "failed" => (
            Kind::Failed,
            Event::TurnFailed,
            format!("⚠️ a turn failed: {}", body["error"].as_str().unwrap_or("")),
        ),
        "notice" if body["purpose"].as_str() != Some("watch.met") && body["quiet"] != true => (
            Kind::Notice,
            Event::Note,
            body["text"]
                .as_str()
                .unwrap_or("")
                .lines()
                .next()
                .unwrap_or("")
                .to_string(),
        ),
        _ => return None,
    };
    (!policy::pings(event, silent)).then(|| Line {
        kind,
        words: clip(&words, 120),
    })
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The message: when, how many of each, then a line each that fits.
fn text(lines: &[Line], (from, to): (String, String)) -> String {
    let count = |k| lines.iter().filter(|l| l.kind == k).count();
    let mut parts = Vec::new();
    for (k, one, many) in [
        (Kind::Reported, "task reported", "tasks reported"),
        (Kind::Failed, "failed", "failed"),
        (Kind::Notice, "notice", "notices"),
    ] {
        if count(k) > 0 {
            parts.push(plural(count(k), one, many));
        }
    }
    let head = format!(
        "While Discord was unreachable ({from}–{to}): {}:",
        parts.join(", ")
    );
    let mut out = head;
    for (i, l) in lines.iter().enumerate() {
        let more = format!("\n… and {} more", lines.len() - i);
        if out.len() + l.words.len() + 3 + more.len() > LIMIT {
            out.push_str(&more);
            break;
        }
        out.push_str(&format!("\n• {}", l.words));
    }
    out
}

impl Lane {
    /// Called as the lane comes back from an outage, before it delivers
    /// anything: fold what should be folded. An error is Discord still away.
    pub(super) async fn catch_up(&mut self) -> Result<(), SendErr> {
        if let Some(f) = self.catchup.folding.take() {
            return self.finish_fold(f);
        }
        let Some((since_ms, began_ms)) = self.catchup.outage else {
            return Ok(());
        };
        let silent = self.silent();
        let posts: Vec<(Action, Line)> = self
            .shared
            .core
            .outbox
            .open_for(&self.target)
            .into_iter()
            .filter(|a| !self.unsure.contains(&a.correlation_id))
            .filter_map(|a| line_of(&a, since_ms, &silent).map(|l| (a, l)))
            .collect();
        if posts.len() < MIN {
            self.catchup.outage = None;
            return Ok(());
        }
        let Some(_sending) = self.shared.core.outbox.sending() else {
            return Err(SendErr {
                away: true,
                unsure: false,
                gone: false,
                message: "the daemon is stopping".into(),
            });
        };
        let channel = self.place_channel().await?;
        // In flight from before the send, so a crash reads them as maybe sent.
        for (a, _) in &posts {
            if let Err(e) = self.shared.core.outbox.dispatch(&a.correlation_id) {
                tracing::warn!(error = %format!("{e:#}"), post = %a.correlation_id, "outbox dispatch failed");
                return Err(SendErr {
                    away: true,
                    unsure: false,
                    gone: false,
                    message: format!("dispatch: {e:#}"),
                });
            }
        }
        let hm = |ms: u64| theseus_core::push::hm(ms);
        let lines: Vec<Line> = posts.iter().map(|(_, l)| l.clone()).collect();
        let key = format!("catchup:{}", posts[0].0.correlation_id);
        let w = lone(
            key.clone(),
            channel,
            text(&lines, (hm(began_ms), hm(theseus_protocol::now_unix_ms()))),
            false,
        );
        let sent = self.write(&w).await;
        let (c, message) = match sent {
            Ok(Some(m)) => m,
            // A message with nothing to say was not sent.
            Ok(None) => return self.give_up(),
            Err(e) if e.away => {
                // A send that may have landed leaves each post to be sent
                // again on its own, under its own nonce.
                if e.unsure {
                    self.unsure
                        .extend(posts.iter().map(|(a, _)| a.correlation_id.clone()));
                }
                return Err(e);
            }
            Err(e) => {
                tracing::warn!(error = %e.message, "the catch-up message was refused; the posts go on their own");
                return self.give_up();
            }
        };
        self.catchup.outage = None;
        let f = Folding {
            posts: posts
                .iter()
                .map(|(a, _)| a.correlation_id.clone())
                .collect(),
            key,
            channel: c,
            message,
        };
        self.finish_fold(f)
    }

    /// The fold's message is sent: settle its posts, the newest first. A
    /// settle the store refuses leaves the rest for the retry.
    fn finish_fold(&mut self, mut f: Folding) -> Result<(), SendErr> {
        while let Some(id) = f.posts.last().cloned() {
            let open = self
                .shared
                .core
                .outbox
                .open_for(&self.target)
                .into_iter()
                .find(|a| a.correlation_id == id);
            if let Some(a) = open {
                let detail = json!({"folded_into": f.message.to_string(), "catchup": f.key});
                let settled = self.settle(
                    &a,
                    Outcome::Succeeded,
                    vec![(f.key.clone(), f.channel, f.message)],
                    detail,
                );
                if let Err(e) = settled {
                    self.catchup.folding = Some(f);
                    return Err(e);
                }
            }
            f.posts.pop();
        }
        Ok(())
    }

    /// No fold after all: the posts go as they would have.
    fn give_up(&mut self) -> Result<(), SendErr> {
        self.catchup.outage = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_core::config::discord::Category;

    fn lines(kinds: &[Kind]) -> Vec<Line> {
        kinds
            .iter()
            .enumerate()
            .map(|(i, k)| Line {
                kind: *k,
                words: format!("line {i}"),
            })
            .collect()
    }

    #[test]
    fn the_message_counts_each_kind_and_names_the_hours() {
        let t = text(
            &lines(&[Kind::Reported, Kind::Reported, Kind::Failed]),
            ("14:02".into(), "14:19".into()),
        );
        assert!(
            t.starts_with("While Discord was unreachable (14:02–14:19): 2 tasks reported, 1 failed:\n• line 0"),
            "{t}"
        );
    }

    #[test]
    fn a_long_fold_says_how_many_lines_it_left_out_and_fits() {
        let many: Vec<Line> = (0..200)
            .map(|i| Line {
                kind: Kind::Notice,
                words: format!("notice number {i} {}", "x".repeat(60)),
            })
            .collect();
        let t = text(&many, ("1:00".into(), "2:00".into()));
        assert!(t.len() <= LIMIT, "{}", t.len());
        assert!(t.contains("… and ") && t.ends_with(" more"), "{t}");
    }

    fn line_of(kind: &str, body: Value, at: u64, since: u64, silent: &[Category]) -> Option<Line> {
        fold_line(kind, &body, at, since, silent)
    }

    #[test]
    fn only_silent_news_written_in_the_outage_folds() {
        let silent = [Category::Reports, Category::Failures, Category::Notices];
        let report = json!({"title": "Tide chart", "outcome": "complete"});
        assert_eq!(
            line_of("report", report.clone(), 10, 10, &silent)
                .unwrap()
                .words,
            "📋 Tide chart finished"
        );
        assert!(
            line_of("report", report.clone(), 10, 11, &silent).is_none(),
            "before the outage"
        );
        assert!(
            line_of("report", report, 10, 10, &[]).is_none(),
            "it pings: it posts on its own"
        );
        for kind in [
            "card",
            "needs_you",
            "settle",
            "reply",
            "reminder",
            "jev_notice",
        ] {
            assert!(line_of(kind, json!({}), 10, 0, &silent).is_none(), "{kind}");
        }
        let met = json!({"text": "met", "purpose": "watch.met"});
        assert!(line_of("notice", met, 10, 0, &silent).is_none());
        let pointer = json!({"text": "from the DM", "quiet": true});
        assert!(line_of("notice", pointer, 10, 0, &silent).is_none());
    }
}
