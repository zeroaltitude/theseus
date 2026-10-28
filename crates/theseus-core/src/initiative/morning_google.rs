//! The morning note's day (theseus-batvg.6): today's meetings, the unread mail
//! that matters and the drafts waiting, read from the owner's Google account
//! when `[google]` is configured, and nothing otherwise.
//!
//! - **Three reads, each through `Google::call`** with its `google.called` row
//!   (one frame for the lot): today's events from local midnight to midnight
//!   (`calendar.rs`'s own request and reading, with a `conflict` mark on those
//!   that overlap), the unread mail that is no promotion or social
//!   (`is:unread -category:promotions -category:social newer_than:12h`: a
//!   count and the top three senders and subjects), and the drafts waiting (a
//!   count). A read that fails leaves its line out and says `calendar
//!   unreachable` or `mail unreachable`; it never blocks the note.
//! - **The reads are the note's own task's**, never the tick's: `morning_note`
//!   runs on the runtime after the day is claimed.
//! - **The brief is private text.** The note's model call is made for no
//!   session, which the provider floor refuses to a model of restricted origin,
//!   and the note then goes out as its template (`initiative_speak` never
//!   retries on another profile).

use std::sync::Arc;

use serde_json::Value;
use theseus_google::Request;

use crate::google::calendar::{self, Brief};
use crate::google::{mail, mime, Google, Meta};
use crate::rpc::Core;
use crate::wake::local;

/// The unread mail that matters, as Gmail searches it.
const UNREAD: &str = "is:unread -category:promotions -category:social newer_than:12h";
/// Messages one count reads, and the senders and subjects shown.
const LISTED: &str = "20";
const SHOWN: usize = 3;

/// What the reads found: the note's lines and the three counts.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Day {
    pub lines: Vec<String>,
    pub events: u64,
    pub unread: u64,
    pub drafts: u64,
    /// A sensitive calendar's events are in the lines.
    pub sensitive: bool,
}

impl Day {
    /// What the day holds that counts as something happening: its meetings
    /// and the mail that matters.
    pub fn happened(&self) -> u64 {
        self.events + self.unread
    }
}

/// One event as a line: its time, title, attendees and call, and a mark when
/// another of the day's events overlaps it.
fn event_line(b: &Brief, conflict: bool) -> String {
    let when = match (b.start_ms, b.end_ms) {
        (Some(s), Some(e)) => format!("{}–{}", local(s).hm(), local(e).hm()),
        _ => b.when.clone(),
    };
    let tag = match b.label.as_str() {
        "" => String::new(),
        l => format!("[{l}] "),
    };
    let mut line = format!("- {tag}{when} · {}", b.title);
    if b.attendees > 0 {
        line.push_str(&format!(" · {} attendees", b.attendees));
    }
    if let Some(c) = &b.call {
        line.push_str(&format!(" · call: {c}"));
    }
    if conflict {
        line.push_str(" · conflict");
    }
    line
}

/// The events' lines, in order, each marked `conflict` that another overlaps.
fn event_lines(events: &[Brief]) -> Vec<String> {
    let span = |b: &Brief| b.start_ms.zip(b.end_ms);
    events
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let conflict = span(b).is_some_and(|(s, e)| {
                events
                    .iter()
                    .enumerate()
                    .any(|(j, o)| j != i && span(o).is_some_and(|(os, oe)| s < oe && os < e))
            });
            event_line(b, conflict)
        })
        .collect()
}

async fn call(
    g: &Google,
    rows: &mut Vec<Value>,
    meta: (&'static str, &str, Option<String>),
    req: Request,
) -> Option<Value> {
    let meta = Meta {
        tool: meta.0,
        method: meta.1.into(),
        resource: meta.2,
        class: "read",
        recipients: None,
    };
    match g.call(meta, req).await {
        Ok(done) => {
            rows.push(done.row);
            Some(done.response.json())
        }
        Err(f) => {
            rows.push(f.row);
            None
        }
    }
}

impl Core {
    /// Today's events, the unread mail and the drafts waiting, or none when
    /// `[google]` binds no account. Never the tick's: the note's own task.
    pub(crate) async fn morning_google(self: &Arc<Self>, now: u64) -> Option<Day> {
        let g = self.tools.google.clone()?;
        let mut rows = Vec::new();
        let mut day = Day::default();
        let scrub = |t: &str| self.tools.scrubber.scrub(t).0;
        let (events, down, sensitive) = self.morning_events(&g, now, &mut rows).await;
        day.events = events.len() as u64;
        day.sensitive = sensitive;
        day.lines.extend(
            event_lines(&events)
                .iter()
                .map(|l| scrub(l))
                .collect::<Vec<_>>(),
        );
        day.lines
            .extend(down.into_iter().map(|l| format!("{l} unreachable")));
        match self.morning_mail(&g, &mut rows).await {
            Some((unread, shown, drafts)) => {
                day.unread = unread;
                day.drafts = drafts;
                if unread > 0 {
                    day.lines.push(format!(
                        "Unread mail that matters (last 12 hours): {unread}"
                    ));
                    day.lines.extend(shown.iter().map(|l| scrub(l)));
                }
                if drafts > 0 {
                    day.lines
                        .push(format!("Drafts waiting for a send: {drafts}"));
                }
            }
            None => day.lines.push("mail unreachable".into()),
        }
        // One frame for the calls' rows, however many there were.
        let rec = self.rec(None);
        let records: Vec<_> = rows
            .iter()
            .filter_map(|row| rec.row(&crate::fact::google::GoogleCalled { row }).ok())
            .collect();
        if !records.is_empty() {
            if let Err(e) = self.store.append(&records) {
                tracing::warn!(error = %format!("{e:#}"), "the morning note's Google rows were not written");
            }
        }
        Some(day)
    }

    /// Today's events of every configured calendar, in time order, each
    /// labelled when there is more than one; the labels of those that could not
    /// be read (`calendar` when it is the only one); and whether a sensitive
    /// calendar was read.
    async fn morning_events(
        &self,
        g: &Google,
        now: u64,
        rows: &mut Vec<Value>,
    ) -> (Vec<Brief>, Vec<String>, bool) {
        let (targets, summary_rows) = g.calendar_all().await;
        rows.extend(summary_rows);
        let many = targets.len() > 1;
        let (mut events, mut down, mut sensitive) = (Vec::new(), Vec::new(), false);
        for t in &targets {
            let read = match calendar::today(now, Some(&t.id)) {
                Ok(req) => {
                    let meta = (
                        calendar::EVENTS,
                        "calendar.events.list",
                        Some(t.label.clone()),
                    );
                    call(g, rows, meta, req).await
                }
                Err(_) => None,
            };
            let Some(reply) = read else {
                down.push(if many {
                    t.label.clone()
                } else {
                    "calendar".into()
                });
                continue;
            };
            sensitive |= t.sensitive;
            events.extend(calendar::briefs(&reply).into_iter().map(|mut b| {
                if many {
                    b.label = t.label.clone();
                }
                b
            }));
        }
        events.sort_by_key(|b| b.start_ms.unwrap_or(0));
        (events, down, sensitive)
    }

    /// The unread count, the top senders and subjects, and the drafts waiting;
    /// none when any of the mail reads failed.
    async fn morning_mail(
        &self,
        g: &Google,
        rows: &mut Vec<Value>,
    ) -> Option<(u64, Vec<String>, u64)> {
        let list = mail::get(
            "/gmail/v1/users/me/messages".into(),
            &[("q", UNREAD), ("maxResults", LISTED)],
        );
        let listed = call(
            g,
            rows,
            (mail::SEARCH, "gmail.users.messages.list", None),
            list,
        )
        .await?;
        let ids: Vec<&str> = listed["messages"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| m["id"].as_str())
            .collect();
        let more = listed.get("nextPageToken").is_some();
        let mut shown = Vec::new();
        for id in ids.iter().take(SHOWN) {
            let query = [
                ("format", "metadata"),
                ("metadataHeaders", "From"),
                ("metadataHeaders", "Subject"),
            ];
            let req = mail::get(format!("/gmail/v1/users/me/messages/{id}"), &query);
            let meta = (
                mail::SEARCH,
                "gmail.users.messages.get",
                Some(id.to_string()),
            );
            let msg = call(g, rows, meta, req).await?;
            let head = |n: &str| mime::header(&msg["payload"], n).unwrap_or_default();
            shown.push(format!(
                "- {} · {}",
                one_line(&head("From")),
                one_line(&head("Subject"))
            ));
        }
        let drafts = mail::get("/gmail/v1/users/me/drafts".into(), &[("maxResults", "100")]);
        let waiting = call(
            g,
            rows,
            (mail::SEARCH, "gmail.users.drafts.list", None),
            drafts,
        )
        .await?;
        let drafts = waiting["drafts"].as_array().map_or(0, Vec::len) as u64;
        let unread = ids.len() as u64 + u64::from(more);
        Some((unread, shown, drafts))
    }
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}
