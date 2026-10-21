//! Gmail as five typed tools (the Google design §3.1, rows 1 to 5): search,
//! read, draft, send and label. Every call goes through [`Google::call`] and
//! every result through [`finish`]; this module adds the mail: the MIME a
//! draft is made of (`mime.rs`), the owner-authored rule's `From` headers, and
//! the send.
//!
//! **A send sends a draft the owner saw, and nothing else** (the owner: every
//! send asks, deterministically). `gmail.send {draft_id}` plans with
//! `Plan.authority`, so the gate asks at every posture and no line, grant,
//! trust or judgment loosens it. A plan reads no network, so the card's facts
//! are made when the draft is made or read (`gmail.draft`, `gmail.read
//! draft:<id>`): the draft as Gmail holds it, its content digest, and the card
//! (from, to, cc, bcc with each stranger flagged new, the subject, the body's
//! first lines, the attachments' names). The send's plan pins that digest,
//! and its run reads the draft again and refuses, sending nothing, unless it
//! still has the digest the card showed. A draft written again (by
//! `gmail.draft`, or read again) unpins, so a card is never for a draft other
//! than the one that goes. The send is non-repeatable (`outcome_unknown`,
//! never retried) and counts on the daily cap.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use theseus_google::{Body as Payload, Repeat, Request, Response, Service};
use theseus_tools::{
    parse, Access, AsyncRun, Backend, Plan, Resource, Retry, Tool, ToolClass, ToolCtx, ToolFailure,
};

use super::mime::{self, Draft};
use super::{finish, CallFailed, Finished, Google, Meta, Source};

pub const SEARCH: &str = "gmail.search";
pub const READ: &str = "gmail.read";
pub const DRAFT: &str = "gmail.draft";
pub const SEND: &str = "gmail.send";
pub const LABEL: &str = "gmail.label";

/// The mail tools' names, for the config's check of `[policy.tools]`.
pub const NAMES: [&str; 5] = [SEARCH, READ, DRAFT, SEND, LABEL];

/// How many body lines a card shows.
pub const CARD_LINES: usize = 15;
/// Recipients in one draft, and threads in one search.
const MAX_RECIPIENTS: usize = 50;
const MAX_SEARCH: u32 = 50;
/// The largest attachment saved, and ids one label call takes.
const MAX_ATTACHMENT: u64 = 25 * 1024 * 1024;
const MAX_IDS: usize = 25;

/// A draft the card showed: its digest and its facts, and the digest a send's
/// plan pinned.
#[derive(Clone, Debug)]
struct Shown {
    digest: String,
    card: Value,
    pinned: Option<String>,
}

/// The mail tools' shared state: the account, and the drafts shown in this
/// daemon's life (a restart shows them again).
pub struct Mail {
    g: Arc<Google>,
    shown: Mutex<HashMap<String, Shown>>,
}

pub fn tools(g: &Arc<Google>) -> Vec<Arc<dyn Tool>> {
    let m = Arc::new(Mail {
        g: g.clone(),
        shown: Mutex::new(HashMap::new()),
    });
    vec![
        Arc::new(SearchTool(m.clone())),
        Arc::new(ReadTool(m.clone())),
        Arc::new(DraftTool(m.clone())),
        Arc::new(SendTool(m.clone())),
        Arc::new(LabelTool(m)),
    ]
}

// ------------------------------------------------------------------ calls

/// The rows of one tool call's REST calls, as the one `google.called` row its
/// result carries: attempts and quota units add, the leading call's service,
/// tool and method stay, a failure makes the outcome the failure's.
fn merged(rows: &[Value]) -> Value {
    // A send or a trash is the call that matters: its row leads.
    let lead = rows
        .iter()
        .find(|r| r["class"] == "authority")
        .or(rows.first());
    let mut row = lead.cloned().unwrap_or(Value::Null);
    if rows.len() > 1 {
        let sum = |k: &str| rows.iter().filter_map(|r| r[k].as_u64()).sum::<u64>();
        row["attempts"] = sum("attempts").into();
        row["quota_units"] = sum("quota_units").into();
        row["ms"] = sum("ms").into();
        row["calls"] = rows.len().into();
        if let Some(bad) = rows.iter().find(|r| r["outcome"] != "ok") {
            for k in ["status", "outcome", "error"] {
                row[k] = bad[k].clone();
            }
        } else if let Some(last) = rows.last() {
            row["status"] = last["status"].clone();
        }
    }
    row
}

fn failure(f: CallFailed, rows: &[Value]) -> ToolFailure {
    ToolFailure {
        message: f.message,
        meta: json!({"private": true, "google_called": merged(rows), "outcome_unknown": f.unknown}),
    }
}

fn refusal(message: impl Into<String>, rows: &[Value]) -> ToolFailure {
    ToolFailure {
        message: message.into(),
        meta: json!({"private": true, "google_called": merged(rows)}),
    }
}

impl Mail {
    /// One REST call of `tool`: its row is kept in `rows` whatever came of it.
    async fn rest(
        &self,
        rows: &mut Vec<Value>,
        (tool, method, class): (&'static str, &str, &'static str),
        resource: Option<String>,
        req: Request,
    ) -> Result<Response, CallFailed> {
        let meta = Meta {
            tool,
            method: method.to_string(),
            resource,
            class,
            recipients: None,
        };
        match self.g.call(meta, req).await {
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

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Shown>> {
        self.shown.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn get(path: String, query: &[(&str, &str)]) -> Request {
    let mut r = Request::get(Service::Gmail, path);
    r.query = query
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();
    r
}

fn post(path: String, body: Value, repeat: Repeat) -> Request {
    Request {
        verb: "POST".into(),
        body: Payload::Json(body),
        repeat,
        ..get(path, &[])
    }
}

fn metadata(headers: &[&str]) -> Vec<(&'static str, String)> {
    let mut q = vec![("format", "metadata".to_string())];
    q.extend(
        headers
            .iter()
            .map(|h| ("metadataHeaders", (*h).to_string())),
    );
    q
}

fn with_query(path: String, q: Vec<(&'static str, String)>) -> Request {
    let q: Vec<(&str, &str)> = q.iter().map(|(k, v)| (*k, v.as_str())).collect();
    get(path, &q)
}

fn id_ok(id: &str) -> Result<(), String> {
    let ok = !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    ok.then_some(())
        .ok_or_else(|| format!("`{id}` is not a Gmail id"))
}

fn senders(messages: &[Value]) -> Vec<String> {
    messages
        .iter()
        .map(|m| mime::header(&m["payload"], "From").unwrap_or_default())
        .collect()
}

// ------------------------------------------------------------------ draft facts

/// A draft as Gmail holds it, read into the fields a card and a digest use.
struct Parsed {
    thread: Option<String>,
    from: String,
    to: Vec<String>,
    cc: Vec<String>,
    bcc: Vec<String>,
    subject: String,
    body: String,
    files: Vec<String>,
}

fn parse_draft(draft: &Value) -> Parsed {
    let msg = &draft["message"];
    let p = &msg["payload"];
    let list = |h: &str| {
        mime::header(p, h)
            .map(|v| mime::addresses(&v))
            .unwrap_or_default()
    };
    let read = mime::read(p);
    Parsed {
        thread: msg["threadId"].as_str().map(str::to_string),
        from: mime::header(p, "From")
            .and_then(|f| mime::address_of(&f))
            .unwrap_or_default(),
        to: list("To"),
        cc: list("Cc"),
        bcc: list("Bcc"),
        subject: mime::header(p, "Subject").unwrap_or_default(),
        body: read.raw.replace("\r\n", "\n").trim_end().to_string(),
        files: read.files.into_iter().map(|f| f.name).collect(),
    }
}

impl Parsed {
    /// The digest a send is bound to: every field the card shows and the
    /// thread the message joins.
    fn digest(&self) -> String {
        let canon = json!([
            self.thread,
            self.from,
            self.to,
            self.cc,
            self.bcc,
            self.subject,
            self.body,
            self.files
        ]);
        Sha256::digest(canon.to_string().as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    fn recipients(&self) -> impl Iterator<Item = &String> {
        self.to.iter().chain(&self.cc).chain(&self.bcc)
    }

    /// The card: what the owner is shown before a send (`new` marks a
    /// recipient not on the thread and never mailed before).
    fn card(&self, id: &str, known: &BTreeSet<String>) -> Value {
        let who = |l: &[String]| -> Vec<Value> {
            l.iter()
                .map(|a| json!({"address": a, "new": !known.contains(a)}))
                .collect()
        };
        let lines: Vec<&str> = self.body.lines().collect();
        let head: Vec<String> = lines
            .iter()
            .take(CARD_LINES)
            .map(|l| l.chars().take(200).collect())
            .collect();
        json!({
            "draft_id": id, "from": self.from, "to": who(&self.to), "cc": who(&self.cc),
            "bcc": who(&self.bcc), "subject": self.subject, "body": head,
            "body_lines": lines.len(), "attachments": self.files, "digest": self.digest(),
        })
    }
}

/// The card as lines of text, for a plan's summary and a result.
pub fn card_text(card: &Value) -> String {
    let list = |k: &str| -> Option<String> {
        let all: Vec<String> = card[k]
            .as_array()?
            .iter()
            .map(|r| {
                let a = r["address"].as_str().unwrap_or("");
                if r["new"] == true {
                    format!("{a} (new)")
                } else {
                    a.to_string()
                }
            })
            .collect();
        (!all.is_empty()).then(|| all.join(", "))
    };
    let mut out = vec![format!(
        "draft {} from {}",
        card["draft_id"].as_str().unwrap_or(""),
        card["from"].as_str().unwrap_or("")
    )];
    for (label, k) in [("To", "to"), ("Cc", "cc"), ("Bcc", "bcc")] {
        out.extend(list(k).map(|l| format!("{label}: {l}")));
    }
    out.push(format!(
        "Subject: {}",
        card["subject"].as_str().unwrap_or("")
    ));
    let total = card["body_lines"].as_u64().unwrap_or(0);
    let body: Vec<&str> = card["body"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    out.push(format!("Body (first {} of {total} lines):", body.len()));
    out.extend(body.iter().map(|l| format!("  {l}")));
    if let Some(files) = card["attachments"].as_array().filter(|a| !a.is_empty()) {
        let names: Vec<&str> = files.iter().filter_map(Value::as_str).collect();
        out.push(format!("Attachments: {}", names.join(", ")));
    }
    out.join("\n")
}

impl Mail {
    /// The draft as Gmail holds it now.
    async fn fetch_draft(
        &self,
        rows: &mut Vec<Value>,
        tool: &'static str,
        id: &str,
    ) -> Result<(Value, Parsed), CallFailed> {
        let path = format!("/gmail/v1/users/me/drafts/{id}");
        let r = self
            .rest(
                rows,
                (tool, "gmail.users.drafts.get", "read"),
                Some(id.to_string()),
                get(path, &[("format", "full")]),
            )
            .await?;
        let v = r.json();
        let parsed = parse_draft(&v);
        Ok((v, parsed))
    }

    /// The addresses a card does not flag: the account, the thread's own, and
    /// anyone the account has mailed before (one search of Sent each, the
    /// answer an address's alone; a search that fails leaves it flagged).
    async fn known(
        &self,
        rows: &mut Vec<Value>,
        tool: &'static str,
        p: &Parsed,
    ) -> BTreeSet<String> {
        let mut known = BTreeSet::from([self.g.account().to_ascii_lowercase()]);
        if let Some(t) = &p.thread {
            let path = format!("/gmail/v1/users/me/threads/{t}");
            let q = metadata(&["From", "To", "Cc"]);
            let m = ("gmail.users.threads.get", "read");
            if let Ok(r) = self
                .rest(rows, (tool, m.0, m.1), Some(t.clone()), with_query(path, q))
                .await
            {
                for msg in r.json()["messages"].as_array().into_iter().flatten() {
                    for h in ["From", "To", "Cc"] {
                        known.extend(mime::addresses(
                            &mime::header(&msg["payload"], h).unwrap_or_default(),
                        ));
                    }
                }
            }
        }
        for a in p
            .recipients()
            .filter(|a| !known.contains(*a))
            .cloned()
            .collect::<Vec<_>>()
        {
            let q = format!("in:sent (to:\"{a}\" OR cc:\"{a}\" OR bcc:\"{a}\")");
            let req = get(
                "/gmail/v1/users/me/messages".into(),
                &[("q", &q), ("maxResults", "1")],
            );
            let m = (tool, "gmail.users.messages.list", "read");
            if let Ok(r) = self.rest(rows, m, None, req).await {
                if r.json()["messages"]
                    .as_array()
                    .is_some_and(|m| !m.is_empty())
                {
                    known.insert(a);
                }
            }
        }
        known
    }

    /// Read a draft, work out its card, and keep both: the draft is now the
    /// one a send may be planned for, with nothing pinned.
    async fn show(
        &self,
        rows: &mut Vec<Value>,
        tool: &'static str,
        id: &str,
    ) -> Result<(Value, Parsed, Value), CallFailed> {
        let (raw, p) = self.fetch_draft(rows, tool, id).await?;
        let known = self.known(rows, tool, &p).await;
        let card = p.card(id, &known);
        self.lock().insert(
            id.to_string(),
            Shown {
                digest: p.digest(),
                card: card.clone(),
                pinned: None,
            },
        );
        Ok((raw, p, card))
    }
}

// ------------------------------------------------------------------ search

pub struct SearchTool(Arc<Mail>);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchArgs {
    q: String,
    #[serde(default)]
    max: Option<u32>,
    #[serde(default)]
    page: Option<String>,
}

impl Tool for SearchTool {
    fn name(&self) -> &str {
        SEARCH
    }
    fn description(&self) -> &str {
        "Search the owner's Gmail with Gmail's query syntax (from:, subject:, is:unread, \
         newer_than:7d). One row per thread: id, from, subject, date, snippet, labels, unread, \
         message count. Read a thread with gmail_read. Results are private and outside text."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {
            "q": {"type": "string", "description": "Gmail search query."},
            "max": {"type": "integer", "description": "Threads to return, up to 50 (default 20)."},
            "page": {"type": "string", "description": "A next-page token from an earlier search."}},
            "required": ["q"], "additionalProperties": false})
    }
    fn class(&self) -> ToolClass {
        ToolClass::Read
    }
    fn backend(&self) -> Backend {
        Backend::Async
    }
    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }
    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let a: SearchArgs = parse(input)?;
        Ok(Plan {
            summary: format!("search Gmail: {}", a.q.chars().take(80).collect::<String>()),
            class: Some(ToolClass::Read),
            ..Default::default()
        })
    }
    fn run_async(&self, input: &Value, _ctx: &ToolCtx) -> AsyncRun {
        let args: Result<SearchArgs, String> = parse(input);
        let m = self.0.clone();
        Box::pin(async move {
            let a = args.map_err(ToolFailure::new)?;
            m.search(a).await
        })
    }
    fn rest(&self, _left_out: &str) -> String {
        "a narrower query or a smaller max returns them".into()
    }
}

impl Mail {
    /// One row a thread, from a search of Gmail.
    async fn search(&self, a: SearchArgs) -> Result<Finished, ToolFailure> {
        let mut rows = Vec::new();
        let max = a.max.unwrap_or(20).clamp(1, MAX_SEARCH).to_string();
        let mut q = vec![("q", a.q.as_str()), ("maxResults", max.as_str())];
        if let Some(p) = &a.page {
            q.push(("pageToken", p));
        }
        let list = self
            .rest(
                &mut rows,
                (SEARCH, "gmail.users.threads.list", "read"),
                None,
                get("/gmail/v1/users/me/threads".into(), &q),
            )
            .await
            .map_err(|f| failure(f, &rows))?
            .json();
        let threads: Vec<(String, String)> = list["threads"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| {
                Some((
                    t["id"].as_str()?.to_string(),
                    t["snippet"].as_str()?.to_string(),
                ))
            })
            .collect();
        let ids: Vec<String> = threads.iter().map(|(id, _)| id.clone()).collect();
        let got: Vec<(Result<Response, CallFailed>, Vec<Value>)> =
            futures_util::stream::iter(ids.into_iter().map(|id| async move {
                let mut rows = Vec::new();
                let path = format!("/gmail/v1/users/me/threads/{id}");
                let q = metadata(&["From", "Subject", "Date"]);
                let r = self
                    .rest(
                        &mut rows,
                        (SEARCH, "gmail.users.threads.get", "read"),
                        Some(id),
                        with_query(path, q),
                    )
                    .await;
                (r, rows)
            }))
            .buffered(5)
            .collect()
            .await;
        let (mut lines, mut from, mut first) = (Vec::new(), Vec::new(), None);
        for ((id, snippet), (r, mut more)) in threads.iter().zip(got) {
            rows.append(&mut more);
            let t = r.map_err(|f| failure(f, &rows))?.json();
            let msgs = t["messages"].as_array().cloned().unwrap_or_default();
            from.extend(senders(&msgs));
            first.get_or_insert(id.clone());
            let (head, last) = (msgs.first(), msgs.last());
            let h = |m: Option<&Value>, n| {
                m.and_then(|m| mime::header(&m["payload"], n))
                    .unwrap_or_default()
            };
            let mut labels = BTreeSet::new();
            for msg in &msgs {
                labels.extend(
                    msg["labelIds"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str),
                );
            }
            let unread = labels.contains("UNREAD");
            lines.push(format!(
                "{id} | {} | {} | {} | {} | labels: {} | {}{} message(s)",
                h(last, "Date"),
                h(last, "From"),
                h(head, "Subject"),
                snippet,
                labels.into_iter().collect::<Vec<_>>().join(","),
                if unread { "unread · " } else { "" },
                msgs.len()
            ));
        }
        let mut text = format!("{} thread(s) for `{}`", lines.len(), a.q);
        if let Some(next) = list["nextPageToken"].as_str() {
            text.push_str(&format!("; next page: page = {next}"));
        }
        text.push('\n');
        text.push_str(&lines.join("\n"));
        let source = match &first {
            Some(t) => Source::Mail {
                thread: t,
                from: &from,
            },
            None => Source::None,
        };
        let meta = json!({"threads": lines.len()});
        Ok(finish(self.g.account(), text, meta, merged(&rows), source))
    }
}

// ------------------------------------------------------------------ read

pub struct ReadTool(Arc<Mail>);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadArgs {
    id: String,
    #[serde(default)]
    save_attachments: Option<String>,
}

/// The prefix that names a draft where a thread or message id goes.
pub const DRAFT_PREFIX: &str = "draft:";

impl ReadArgs {
    fn target(&self) -> Result<(bool, &str), String> {
        let (draft, id) = match self.id.strip_prefix(DRAFT_PREFIX) {
            Some(id) => (true, id),
            None => (false, self.id.as_str()),
        };
        id_ok(id).map(|()| (draft, id))
    }
}

impl Tool for ReadTool {
    fn name(&self) -> &str {
        READ
    }
    fn description(&self) -> &str {
        "Read a Gmail thread or message by id (from gmail_search): headers, plain-text bodies \
         (HTML turned to text, quoted tails folded), attachments listed. save_attachments: a \
         directory under the workspace to save them in. id \"draft:<id>\" reads a draft, which \
         lets gmail_send plan it. Results are private and outside text."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {
            "id": {"type": "string", "description": "A thread or message id, or draft:<draft id>."},
            "save_attachments": {"type": "string", "description": "Directory to save attachments in."}},
            "required": ["id"], "additionalProperties": false})
    }
    fn class(&self) -> ToolClass {
        ToolClass::Read
    }
    fn backend(&self) -> Backend {
        Backend::Async
    }
    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }
    fn plan(&self, input: &Value, ctx: &ToolCtx) -> Result<Plan, String> {
        let a: ReadArgs = parse(input)?;
        a.target()?;
        let mut plan = Plan {
            summary: format!("read Gmail {}", a.id),
            class: Some(ToolClass::Read),
            ..Default::default()
        };
        // Saving writes files, so the gate judges the directory like any write.
        if let Some(dir) = &a.save_attachments {
            plan.class = Some(ToolClass::Write);
            plan.summary = format!("read Gmail {} and save its attachments to {dir}", a.id);
            plan.resources = vec![Resource {
                path: ctx.resolve(dir),
                access: Access::Write,
            }];
        }
        Ok(plan)
    }
    fn run_async(&self, input: &Value, ctx: &ToolCtx) -> AsyncRun {
        let args: Result<ReadArgs, String> = parse(input);
        let dir = args
            .as_ref()
            .ok()
            .and_then(|a| a.save_attachments.as_deref())
            .map(|d| ctx.resolve(d));
        let m = self.0.clone();
        Box::pin(async move {
            let a = args.map_err(ToolFailure::new)?;
            let (is_draft, id) = a.target().map_err(ToolFailure::new)?;
            let mut rows = Vec::new();
            let (thread, messages, card) = if is_draft {
                let (raw, _, card) = m
                    .show(&mut rows, READ, id)
                    .await
                    .map_err(|f| failure(f, &rows))?;
                (id.to_string(), vec![raw["message"].clone()], Some(card))
            } else {
                let (t, msgs) = m
                    .thread_or_message(&mut rows, id)
                    .await
                    .map_err(|f| failure(f, &rows))?;
                (t, msgs, None)
            };
            let from = senders(&messages);
            let mut text = format!(
                "{} · {} message(s)\n",
                if is_draft { "Draft" } else { "Thread" },
                messages.len()
            );
            let mut saved = Vec::new();
            for msg in &messages {
                let p = &msg["payload"];
                let body = mime::read(p);
                let head = ["From", "To", "Cc", "Date", "Subject"]
                    .iter()
                    .filter_map(|h| mime::header(p, h).map(|v| format!("{h}: {v}")))
                    .collect::<Vec<_>>()
                    .join("\n");
                let mid = msg["id"].as_str().unwrap_or("");
                text.push_str(&format!("\n--- message {mid}\n{head}\n\n{}\n", body.text));
                for f in &body.files {
                    text.push_str(&format!(
                        "[attachment: {} ({}, {} bytes)]\n",
                        f.name, f.mime, f.size
                    ));
                }
                if let Some(dir) = &dir {
                    for f in &body.files {
                        match m.save(&mut rows, dir, mid, f).await {
                            Ok(path) => saved.push(path),
                            Err(Err(f)) => return Err(failure(f, &rows)),
                            Err(Ok(why)) => {
                                text.push_str(&format!("[not saved: {} — {why}]\n", f.name))
                            }
                        }
                    }
                }
            }
            for path in &saved {
                text.push_str(&format!("saved {}\n", path.display()));
            }
            if let Some(card) = &card {
                text.push_str(&format!("\nGmail holds it as:\n{}\n", card_text(card)));
            }
            let meta = json!({"id": id, "messages": messages.len(), "saved": saved.len()});
            Ok(finish(
                m.g.account(),
                text,
                meta,
                merged(&rows),
                Source::Mail {
                    thread: &thread,
                    from: &from,
                },
            ))
        })
    }
    fn rest(&self, _left_out: &str) -> String {
        "read one message by its id for the rest".into()
    }
}

impl Mail {
    /// A thread, else (an id of a message that is no thread's first) the one
    /// message: its id and its messages.
    async fn thread_or_message(
        &self,
        rows: &mut Vec<Value>,
        id: &str,
    ) -> Result<(String, Vec<Value>), CallFailed> {
        let path = format!("/gmail/v1/users/me/threads/{id}");
        let m = (READ, "gmail.users.threads.get", "read");
        match self
            .rest(rows, m, Some(id.into()), get(path, &[("format", "full")]))
            .await
        {
            Ok(r) => Ok((
                id.to_string(),
                r.json()["messages"].as_array().cloned().unwrap_or_default(),
            )),
            Err(f) if rows.last().is_some_and(|r| r["status"] == 404) => {
                let path = format!("/gmail/v1/users/me/messages/{id}");
                let m = (READ, "gmail.users.messages.get", "read");
                match self
                    .rest(rows, m, Some(id.into()), get(path, &[("format", "full")]))
                    .await
                {
                    Ok(r) => {
                        let msg = r.json();
                        let thread = msg["threadId"].as_str().unwrap_or(id).to_string();
                        Ok((thread, vec![msg]))
                    }
                    Err(_) => Err(f),
                }
            }
            Err(f) => Err(f),
        }
    }

    /// One attachment, under `dir`, by a name that is only a name and never
    /// over a file there. `Err(Ok(why))` is a file not saved, `Err(Err(_))` a
    /// call that failed.
    async fn save(
        &self,
        rows: &mut Vec<Value>,
        dir: &Path,
        mid: &str,
        f: &mime::Attachment,
    ) -> Result<PathBuf, Result<String, CallFailed>> {
        if f.size > MAX_ATTACHMENT {
            return Err(Ok(format!(
                "{} bytes is over the {} MiB cap",
                f.size,
                MAX_ATTACHMENT >> 20
            )));
        }
        let path = format!("/gmail/v1/users/me/messages/{mid}/attachments/{}", f.id);
        let m = (READ, "gmail.users.messages.attachments.get", "read");
        let r = self
            .rest(rows, m, Some(mid.into()), get(path, &[]))
            .await
            .map_err(Err)?;
        let bytes = mime::unbase64(r.json()["data"].as_str().unwrap_or(""));
        tokio::fs::create_dir_all(dir)
            .await
            .map_err(|e| Ok(e.to_string()))?;
        let name = mime::safe_name(&f.name);
        for n in 0..100 {
            let candidate = if n == 0 {
                dir.join(&name)
            } else {
                dir.join(format!("{n}-{name}"))
            };
            let open = tokio::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
                .await;
            match open {
                Ok(mut file) => {
                    let wrote = tokio::io::AsyncWriteExt::write_all(&mut file, &bytes).await;
                    wrote.map_err(|e| Ok(e.to_string()))?;
                    tokio::io::AsyncWriteExt::flush(&mut file)
                        .await
                        .map_err(|e| Ok(e.to_string()))?;
                    return Ok(candidate);
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(Ok(e.to_string())),
            }
        }
        Err(Ok("too many files of that name".into()))
    }
}

// ------------------------------------------------------------------ draft

pub struct DraftTool(Arc<Mail>);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DraftArgs {
    #[serde(default)]
    to: Vec<String>,
    #[serde(default)]
    cc: Vec<String>,
    #[serde(default)]
    bcc: Vec<String>,
    #[serde(default)]
    subject: String,
    body: String,
    #[serde(default)]
    reply_to: Option<String>,
    #[serde(default)]
    draft_id: Option<String>,
}

impl DraftArgs {
    fn checked(&self) -> Result<(), String> {
        let all = self.to.iter().chain(&self.cc).chain(&self.bcc);
        if all.clone().count() > MAX_RECIPIENTS {
            return Err(format!("a draft has at most {MAX_RECIPIENTS} recipients"));
        }
        for a in all {
            mime::one_line("an address", a)?;
            mime::address_of(a).ok_or_else(|| format!("`{a}` is not an email address"))?;
        }
        mime::one_line("a subject", &self.subject)?;
        for id in [&self.reply_to, &self.draft_id].into_iter().flatten() {
            id_ok(id)?;
        }
        if self.to.is_empty() && self.reply_to.is_none() {
            return Err("a draft needs `to`, or `reply_to` (the message it answers)".into());
        }
        if self.subject.trim().is_empty() && self.reply_to.is_none() {
            return Err("a draft needs a `subject`".into());
        }
        Ok(())
    }
}

impl Tool for DraftTool {
    fn name(&self) -> &str {
        DRAFT
    }
    fn description(&self) -> &str {
        "Create or replace a Gmail draft (plain text). Nobody sees it until gmail_send, which \
         asks the owner every time and shows this draft. reply_to: the id of the message \
         answered; it sets the thread, In-Reply-To, References and a Re: subject, and `to` \
         defaults to its sender. draft_id replaces that draft. Needs no approval to write."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {
            "to": {"type": "array", "items": {"type": "string"}},
            "cc": {"type": "array", "items": {"type": "string"}},
            "bcc": {"type": "array", "items": {"type": "string"}},
            "subject": {"type": "string"},
            "body": {"type": "string", "description": "Plain text."},
            "reply_to": {"type": "string", "description": "A message id this answers."},
            "draft_id": {"type": "string", "description": "An existing draft to replace."}},
            "required": ["body"], "additionalProperties": false})
    }
    fn class(&self) -> ToolClass {
        ToolClass::Write
    }
    fn backend(&self) -> Backend {
        Backend::Async
    }
    fn retry(&self) -> Retry {
        // A create that may have run may have made a draft: never made twice.
        Retry::NonRepeatable
    }
    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let a: DraftArgs = parse(input)?;
        a.checked()?;
        let verb = if a.draft_id.is_some() {
            "replace"
        } else {
            "create"
        };
        Ok(Plan {
            summary: format!(
                "{verb} a Gmail draft to {} recipient(s)",
                a.to.len() + a.cc.len() + a.bcc.len()
            ),
            class: Some(ToolClass::Write),
            ..Default::default()
        })
    }
    fn run_async(&self, input: &Value, _ctx: &ToolCtx) -> AsyncRun {
        let args: Result<DraftArgs, String> = parse(input);
        let m = self.0.clone();
        Box::pin(async move {
            let a = args.map_err(ToolFailure::new)?;
            a.checked().map_err(ToolFailure::new)?;
            let mut rows = Vec::new();
            let mut d = Draft {
                from: m.g.account().to_string(),
                to: a.to.clone(),
                cc: a.cc.clone(),
                bcc: a.bcc.clone(),
                subject: a.subject.trim().to_string(),
                body: a.body.clone(),
                ..Default::default()
            };
            let mut thread = None;
            if let Some(mid) = &a.reply_to {
                let path = format!("/gmail/v1/users/me/messages/{mid}");
                let q = metadata(&["Message-ID", "References", "Subject", "From", "Reply-To"]);
                let orig = m
                    .rest(
                        &mut rows,
                        (DRAFT, "gmail.users.messages.get", "read"),
                        Some(mid.clone()),
                        with_query(path, q),
                    )
                    .await
                    .map_err(|f| failure(f, &rows))?
                    .json();
                let p = &orig["payload"];
                thread = orig["threadId"].as_str().map(str::to_string);
                d.in_reply_to =
                    mime::header(p, "Message-ID").or_else(|| mime::header(p, "Message-Id"));
                d.references = mime::header(p, "References")
                    .map(|r| r.split_whitespace().map(str::to_string).collect())
                    .unwrap_or_default();
                let original = mime::header(p, "Subject").unwrap_or_default();
                d.subject = mime::reply_subject(if d.subject.is_empty() {
                    &original
                } else {
                    &d.subject
                });
                if d.to.is_empty() {
                    let back = mime::header(p, "Reply-To").or_else(|| mime::header(p, "From"));
                    d.to = back.into_iter().collect();
                }
            }
            let message = mime::build(&d).map_err(|e| refusal(e, &rows))?;
            let mut body = json!({"message": {"raw": mime::raw(&message)}});
            if let Some(t) = &thread {
                body["message"]["threadId"] = t.clone().into();
            }
            let (verb, path, repeat) = match &a.draft_id {
                Some(id) => (
                    "PUT",
                    format!("/gmail/v1/users/me/drafts/{id}"),
                    Repeat::Safe,
                ),
                None => (
                    "POST",
                    "/gmail/v1/users/me/drafts".to_string(),
                    Repeat::NonRepeatable,
                ),
            };
            let method = if verb == "PUT" {
                "gmail.users.drafts.update"
            } else {
                "gmail.users.drafts.create"
            };
            let req = Request {
                verb: verb.into(),
                ..post(path, body, repeat)
            };
            let made = m
                .rest(&mut rows, (DRAFT, method, "write"), a.draft_id.clone(), req)
                .await
                .map_err(|f| failure(f, &rows))?
                .json();
            let id = made["id"].as_str().unwrap_or_default().to_string();
            if id_ok(&id).is_err() {
                return Err(refusal("Gmail did not name the draft it made", &rows));
            }
            let (_, _, card) = m
                .show(&mut rows, DRAFT, &id)
                .await
                .map_err(|f| failure(f, &rows))?;
            let text = format!(
                "Draft {id} saved; nothing is sent. gmail_send {{draft_id: \"{id}\"}} asks the owner \
                 to send exactly this:\n{}\n",
                card_text(&card)
            );
            let meta = json!({"draft_id": id, "digest": card["digest"]});
            Ok(finish(
                m.g.account(),
                text,
                meta,
                merged(&rows),
                Source::None,
            ))
        })
    }
}

// ------------------------------------------------------------------ send

pub struct SendTool(Arc<Mail>);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SendArgs {
    draft_id: String,
}

impl SendTool {
    /// The draft's card, and the digest this plan pins for the run.
    fn planned(&self, input: &Value) -> Result<(SendArgs, Value), String> {
        let a: SendArgs = parse(input)?;
        id_ok(&a.draft_id)?;
        self.0.g.sends.allows()?;
        let mut shown = self.0.lock();
        let s = shown.get_mut(&a.draft_id).ok_or_else(|| {
            format!(
                "this draft has not been shown yet, so there is no card to ask the owner with: save \
                 it with gmail_draft, or read it with gmail_read id \"draft:{}\", and send again",
                a.draft_id
            )
        })?;
        s.pinned = Some(s.digest.clone());
        Ok((a, s.card.clone()))
    }
}

fn recipient_count(card: &Value) -> u32 {
    ["to", "cc", "bcc"]
        .iter()
        .map(|k| card[k].as_array().map_or(0, Vec::len) as u32)
        .sum()
}

impl Tool for SendTool {
    fn name(&self) -> &str {
        SEND
    }
    fn description(&self) -> &str {
        "Send an existing Gmail draft, only the one the owner is shown: it asks the owner every \
         time, whatever any setting says, and refuses (sending nothing) if the draft changed \
         after it was shown. The draft must have been made or read in this session \
         (gmail_draft, or gmail_read id draft:<id>). Cannot be undone, never retried, and counts \
         against the daily send cap."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {
            "draft_id": {"type": "string", "description": "The draft's id, from gmail_draft."}},
            "required": ["draft_id"], "additionalProperties": false})
    }
    fn class(&self) -> ToolClass {
        ToolClass::Write
    }
    fn backend(&self) -> Backend {
        Backend::Async
    }
    fn retry(&self) -> Retry {
        Retry::NonRepeatable
    }
    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let (_, card) = self.planned(input)?;
        let n = recipient_count(&card);
        Ok(Plan {
            summary: format!("send {}", card_text(&card)),
            class: Some(ToolClass::Write),
            authority: Some(format!(
                "sends mail to {n} recipient(s) and cannot be taken back, so the owner decides it, \
                 every time, at every posture"
            )),
            card: Some(card),
            ..Default::default()
        })
    }
    fn run_async(&self, input: &Value, _ctx: &ToolCtx) -> AsyncRun {
        let args: Result<SendArgs, String> = parse(input);
        let m = self.0.clone();
        Box::pin(async move {
            let a = args.map_err(ToolFailure::new)?;
            let id = a.draft_id;
            id_ok(&id).map_err(ToolFailure::new)?;
            let mut rows = Vec::new();
            // What the card showed: the digest the approved plan pinned.
            let shown = m.lock().get(&id).cloned();
            let Some((shown, pinned)) = shown.and_then(|s| s.pinned.clone().map(|p| (s, p))) else {
                return Err(refusal(
                    "nothing was sent: the draft was written or read again since the owner's card, or \
                     this daemon restarted. Send again, so the card shows what goes out",
                    &rows,
                ));
            };
            m.g.sends.reserve().map_err(ToolFailure::new)?;
            let live = match m.fetch_draft(&mut rows, SEND, &id).await {
                Ok((_, p)) => p.digest(),
                Err(f) => {
                    m.g.sends.release();
                    return Err(failure(f, &rows));
                }
            };
            if live != pinned || live != shown.digest {
                m.g.sends.release();
                m.lock().remove(&id);
                return Err(refusal(
                    "nothing was sent: the draft changed after the owner's card showed it. Read it \
                     again (gmail_read id \"draft:<id>\") and send again, so the card shows what goes out",
                    &rows,
                ));
            }
            let meta = Meta {
                tool: SEND,
                method: "gmail.users.drafts.send".into(),
                resource: Some(id.clone()),
                class: "authority",
                recipients: Some(recipient_count(&shown.card)),
            };
            let req = Request {
                repeat: Repeat::NonRepeatable,
                ..post(
                    "/gmail/v1/users/me/drafts/send".into(),
                    json!({"id": id}),
                    Repeat::NonRepeatable,
                )
            };
            match m.g.call(meta, req).await {
                Ok(done) => {
                    rows.push(done.row);
                    m.lock().remove(&id);
                    let sent = done.response.json();
                    let text = format!(
                        "Sent draft {id} (message {}).\n",
                        sent["id"].as_str().unwrap_or("?")
                    );
                    let meta = json!({"draft_id": id, "message_id": sent["id"]});
                    Ok(finish(
                        m.g.account(),
                        text,
                        meta,
                        merged(&rows),
                        Source::None,
                    ))
                }
                Err(f) => {
                    // A send refused before it could go out is not a send; one that may have gone is counted.
                    if !f.unknown {
                        m.g.sends.release();
                    }
                    rows.push(f.row.clone());
                    Err(failure(f, &rows))
                }
            }
        })
    }
}

// ------------------------------------------------------------------ label

pub struct LabelTool(Arc<Mail>);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LabelArgs {
    ids: Vec<String>,
    #[serde(default)]
    add: Vec<String>,
    #[serde(default)]
    remove: Vec<String>,
    #[serde(default)]
    trash: bool,
}

impl LabelArgs {
    fn checked(&self) -> Result<(), String> {
        if self.ids.is_empty() || self.ids.len() > MAX_IDS {
            return Err(format!("`ids` holds 1 to {MAX_IDS} ids"));
        }
        self.ids.iter().try_for_each(|i| id_ok(i))?;
        if self.add.is_empty() && self.remove.is_empty() && !self.trash {
            return Err("nothing to do: give `add`, `remove` or `trash`".into());
        }
        Ok(())
    }
}

/// Gmail's own label ids, by the name a person gives them.
fn system_label(name: &str) -> Option<&'static str> {
    const ALL: [&str; 9] = [
        "INBOX",
        "UNREAD",
        "STARRED",
        "IMPORTANT",
        "SPAM",
        "TRASH",
        "SENT",
        "DRAFT",
        "CHAT",
    ];
    ALL.iter().copied().find(|l| l.eq_ignore_ascii_case(name))
}

impl Tool for LabelTool {
    fn name(&self) -> &str {
        LABEL
    }
    fn description(&self) -> &str {
        "Change labels on Gmail threads (ids from gmail_search): add or remove INBOX (archive), \
         UNREAD (mark read), STARRED, or a user label by name. trash moves them to Trash \
         (recoverable) and asks the owner every time. In a session that read outside text, \
         this asks."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {
            "ids": {"type": "array", "items": {"type": "string"}, "description": "Thread ids, up to 25."},
            "add": {"type": "array", "items": {"type": "string"}},
            "remove": {"type": "array", "items": {"type": "string"}},
            "trash": {"type": "boolean"}},
            "required": ["ids"], "additionalProperties": false})
    }
    fn class(&self) -> ToolClass {
        ToolClass::Write
    }
    fn backend(&self) -> Backend {
        Backend::Async
    }
    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }
    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let a: LabelArgs = parse(input)?;
        a.checked()?;
        Ok(Plan {
            summary: format!(
                "{} {} Gmail thread(s)",
                if a.trash { "trash" } else { "relabel" },
                a.ids.len()
            ),
            class: Some(ToolClass::Write),
            authority: a.trash.then(|| {
                "moves mail to Trash, so the owner decides it, every time, at every posture".into()
            }),
            ..Default::default()
        })
    }
    fn run_async(&self, input: &Value, _ctx: &ToolCtx) -> AsyncRun {
        let args: Result<LabelArgs, String> = parse(input);
        let m = self.0.clone();
        Box::pin(async move {
            let a = args.map_err(ToolFailure::new)?;
            a.checked().map_err(ToolFailure::new)?;
            let mut rows = Vec::new();
            let class = if a.trash { "authority" } else { "write" };
            let add = m.label_ids(&mut rows, &a.add).await?;
            let remove = m.label_ids(&mut rows, &a.remove).await?;
            let (what, body) = if a.trash {
                ("trash", json!({}))
            } else {
                (
                    "modify",
                    json!({"addLabelIds": add, "removeLabelIds": remove}),
                )
            };
            let mut done = Vec::new();
            for id in &a.ids {
                m.act(&mut rows, class, id, what, &body)
                    .await
                    .map_err(|f| failure(f, &rows))?;
                done.push(id.as_str());
            }
            let text = format!(
                "{} {} thread(s): {}\n",
                if a.trash { "Trashed" } else { "Relabelled" },
                done.len(),
                done.join(", ")
            );
            let meta = json!({"ids": done, "trash": a.trash});
            Ok(finish(
                m.g.account(),
                text,
                meta,
                merged(&rows),
                Source::None,
            ))
        })
    }
}

impl Mail {
    /// A thread's `modify` or `trash`, else (an id that is no thread's first
    /// message) the message's own.
    async fn act(
        &self,
        rows: &mut Vec<Value>,
        class: &'static str,
        id: &str,
        what: &str,
        body: &Value,
    ) -> Result<(), CallFailed> {
        let mut first = None;
        for kind in ["threads", "messages"] {
            let method = format!("gmail.users.{kind}.{what}");
            let req = post(
                format!("/gmail/v1/users/me/{kind}/{id}/{what}"),
                body.clone(),
                Repeat::Safe,
            );
            match self
                .rest(rows, (LABEL, &method, class), Some(id.into()), req)
                .await
            {
                Ok(_) => return Ok(()),
                Err(f) if first.is_none() && rows.last().is_some_and(|r| r["status"] == 404) => {
                    first = Some(f)
                }
                Err(f) => return Err(f),
            }
        }
        Err(first.expect("the thread's refusal was kept"))
    }

    /// The label ids for names: Gmail's own by name, the rest through the
    /// account's label list (read once, and only when a user label is named).
    async fn label_ids(
        &self,
        rows: &mut Vec<Value>,
        names: &[String],
    ) -> Result<Vec<String>, ToolFailure> {
        let mut out = Vec::new();
        let mut listed: Option<Value> = None;
        for n in names {
            if let Some(s) = system_label(n) {
                out.push(s.to_string());
                continue;
            }
            if listed.is_none() {
                let req = get("/gmail/v1/users/me/labels".into(), &[]);
                let m = (LABEL, "gmail.users.labels.list", "read");
                match self.rest(rows, m, None, req).await {
                    Ok(r) => listed = Some(r.json()),
                    Err(f) => return Err(failure(f, rows)),
                }
            }
            let found = listed.as_ref().and_then(|l| {
                l["labels"].as_array()?.iter().find(|l| {
                    l["name"]
                        .as_str()
                        .is_some_and(|x| x.eq_ignore_ascii_case(n))
                })
            });
            match found.and_then(|l| l["id"].as_str()) {
                Some(id) => out.push(id.to_string()),
                None => return Err(refusal(format!("no Gmail label is named `{n}`"), rows)),
            }
        }
        Ok(out)
    }
}
