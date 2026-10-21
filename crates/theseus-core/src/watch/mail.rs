//! The mail watch's look (theseus-batvg.6): new mail matching a Gmail query,
//! read from the account's history with no model.
//!
//! - **First look**: the profile's `historyId` is the cursor, and nothing
//!   matches yet. Every later look is `users.history.list` from the cursor
//!   (`messageAdded` only): with no new message that is the whole look, one
//!   call of about 2 quota units. With some, one `messages.list` for the
//!   watch's query keeps the added ids it returns, and each is read for its
//!   `From`, `Subject` and snippet (metadata only: never a body).
//! - **A 404** on the history (the cursor is older than Gmail keeps) reads the
//!   profile again and starts from there; the look says so (`Seen.rebaselined`),
//!   and the watch's row does too: never silently.
//! - **Every call** goes through `Google::call`, so each has its
//!   `google.called` row (`rows`, recorded by the caller); a mail watch makes
//!   no model call, ever.
//! - **What a match carries** is a quote: sender, subject and the first 200
//!   characters of the snippet, scrubbed, one line each. It is outside text.

use std::collections::BTreeSet;

use serde_json::Value;
use theseus_google::{Request, Response};

use super::cond::Observed;
use super::reply::quote_of;
use crate::google::{mail, mime, CallFailed, Google, Meta};
use crate::scrub::Scrubber;

/// The META key a watch's cursor is kept under (`<prefix><wake id>`).
pub const CURSOR_PREFIX: &str = "watch.mail.";

/// Pages of history one look reads at most.
const MAX_PAGES: usize = 5;
/// Matching messages one look reads, at most.
const MAX_MATCHES: usize = 5;

/// What a look found beside its value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Seen {
    /// Where the next look starts.
    pub cursor: String,
    /// The old cursor was gone: this one is a fresh start, and mail that
    /// arrived between may have been missed.
    pub rebaselined: bool,
    /// The addresses (lower case) the matching messages came from.
    pub senders: Vec<String>,
    /// Every one of them is in `[google] trusted_senders`: set by the core,
    /// which holds the config, before the look is applied.
    pub trusted: bool,
}

async fn call(
    g: &Google,
    rows: &mut Vec<Value>,
    method: &str,
    resource: Option<String>,
    req: Request,
) -> Result<Response, CallFailed> {
    let meta = Meta {
        tool: mail::SEARCH,
        method: method.into(),
        resource,
        class: "read",
        recipients: None,
    };
    match g.call(meta, req).await {
        Ok(done) => {
            rows.push(done.row);
            Ok(done.response)
        }
        Err(f) => {
            rows.push(f.row.clone());
            Err(f)
        }
    }
}

/// The mailbox's current `historyId`.
async fn baseline(g: &Google, rows: &mut Vec<Value>) -> Result<String, String> {
    let req = mail::get("/gmail/v1/users/me/profile".into(), &[]);
    let r = call(g, rows, "gmail.users.getProfile", None, req)
        .await
        .map_err(|f| f.message)?;
    match r.json()["historyId"].as_str() {
        Some(h) => Ok(h.to_string()),
        None => Err("Gmail's profile named no historyId".into()),
    }
}

fn nothing(cursor: String, rebaselined: bool) -> Observed {
    Observed {
        mail: Some(Seen {
            cursor,
            rebaselined,
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// One look at the mailbox for `q`, from `cursor` (none: the first look).
pub async fn look(
    g: &Google,
    q: &str,
    cursor: Option<&str>,
    scrubber: &Scrubber,
    rows: &mut Vec<Value>,
) -> Result<Observed, String> {
    let Some(start) = cursor else {
        return Ok(nothing(baseline(g, rows).await?, false));
    };
    let (mut added, mut page, mut newest) = (BTreeSet::new(), None::<String>, None::<String>);
    for _ in 0..MAX_PAGES {
        let mut query = vec![
            ("startHistoryId", start),
            ("historyTypes", "messageAdded"),
            ("maxResults", "500"),
        ];
        if let Some(p) = &page {
            query.push(("pageToken", p));
        }
        let req = mail::get("/gmail/v1/users/me/history".into(), &query);
        let v = match call(g, rows, "gmail.users.history.list", None, req).await {
            Ok(r) => r.json(),
            // The cursor is older than Gmail keeps.
            Err(_) if rows.last().is_some_and(|r| r["status"] == 404) => {
                return Ok(nothing(baseline(g, rows).await?, true));
            }
            Err(f) => return Err(f.message),
        };
        for h in v["history"].as_array().into_iter().flatten() {
            for m in h["messagesAdded"].as_array().into_iter().flatten() {
                added.extend(m["message"]["id"].as_str().map(str::to_string));
            }
        }
        newest = v["historyId"].as_str().map(str::to_string).or(newest);
        page = v["nextPageToken"].as_str().map(str::to_string);
        if page.is_none() {
            break;
        }
    }
    let cursor = newest.unwrap_or_else(|| start.to_string());
    if added.is_empty() {
        return Ok(nothing(cursor, false));
    }
    let req = mail::get(
        "/gmail/v1/users/me/messages".into(),
        &[("q", q), ("maxResults", "50")],
    );
    let listed = call(g, rows, "gmail.users.messages.list", None, req)
        .await
        .map_err(|f| f.message)?
        .json();
    let matched: Vec<String> = listed["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["id"].as_str())
        .filter(|id| added.contains(*id))
        .take(MAX_MATCHES)
        .map(str::to_string)
        .collect();
    if matched.is_empty() {
        return Ok(nothing(cursor, false));
    }
    let (mut lines, mut senders) = (Vec::new(), Vec::new());
    for id in matched {
        let query = [
            ("format", "metadata"),
            ("metadataHeaders", "From"),
            ("metadataHeaders", "Subject"),
        ];
        let path = format!("/gmail/v1/users/me/messages/{id}");
        let msg = call(
            g,
            rows,
            "gmail.users.messages.get",
            Some(id),
            mail::get(path, &query),
        )
        .await
        .map_err(|f| f.message)?
        .json();
        let head = |n: &str| mime::header(&msg["payload"], n).unwrap_or_default();
        let (from, subject) = (head("From"), head("Subject"));
        // A From with no address is no one trusted: an empty sender, never a
        // skipped one, so the look cannot pass as all trusted.
        senders.push(
            mime::address_of(&from)
                .map(|a| a.to_ascii_lowercase())
                .unwrap_or_default(),
        );
        let said = |t: &str| quote_of(&scrubber.scrub(t).0);
        lines.push(format!(
            "From: {} · Subject: {} · {}",
            said(&from),
            said(&subject),
            said(msg["snippet"].as_str().unwrap_or(""))
        ));
    }
    Ok(Observed {
        value: lines.join("\n"),
        exists: true,
        mail: Some(Seen {
            cursor,
            senders,
            ..Default::default()
        }),
        ..Default::default()
    })
}
