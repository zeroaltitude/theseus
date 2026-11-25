//! `google.call` (the Google design §3.1, row 15): any REST method of the
//! granted services, the long tail the typed tools do not cover. The method is
//! an id the method table knows (`drive.files.list`) or `"VERB /path"`; it is
//! classified by its verb, its path, its query and its body
//! (`theseus_google::classify`): a GET reads, and every other call is
//! **authority** unless it is one of the few told acts, exactly (a Gmail
//! message moved to or from the Trash, a Drive file trashed or restored, a
//! calendar made). `Plan.authority` makes the gate ask at every posture, so no
//! `[policy.tools]` line, grant, trust or judgment loosens it (the owner:
//! every send asks, deterministically). A told act runs at the tool's posture,
//! its target read first so the owner's Undo puts it back exactly
//! ([`super::take_back`]), and is told ([`super::told`]).
//! Its results are always private and outside text.

use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_google::methods::{resolve, Resolved};
use theseus_google::{Class, Tidy};
use theseus_tools::{parse, AsyncRun, Backend, Plan, Retry, Tool, ToolClass, ToolCtx, ToolFailure};

use super::external::{finish, Source};
use super::{Google, Meta};

pub const NAME: &str = "google.call";

pub fn tool(g: &Arc<Google>) -> Arc<dyn Tool> {
    Arc::new(CallTool(g.clone()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    method: String,
    #[serde(default)]
    params: Option<Value>,
    #[serde(default)]
    body: Option<Value>,
}

/// A call as `plan` checked it: resolved, and in a service the config turns on.
struct Planned {
    resolved: Resolved,
    label: String,
}

impl Planned {
    fn class(&self) -> Class {
        self.resolved.class.class
    }

    fn class_word(&self) -> &'static str {
        match self.class() {
            Class::Read => "read",
            Class::Write => "write",
            Class::Authority => "authority",
        }
    }

    /// The call in words for the card: its verb and path, and a body's own `id`
    /// (a draft to send), never content.
    fn summary(&self) -> String {
        let r = &self.resolved.request;
        let mut s = format!(
            "{} {}: {} {}",
            self.class_word(),
            self.label,
            r.verb,
            r.path
        );
        if let theseus_google::Body::Json(b) = &r.body {
            if let Some(id) = b.get("id").and_then(Value::as_str).filter(|i| i.len() < 80) {
                s.push_str(&format!(" (id {id})"));
            }
        }
        s
    }
}

/// A write that makes, sends or deletes something returns Theseus's own
/// action echoed back (a created draft's id, an inserted event, a sent
/// message's id), not text anyone else wrote, so it holds no session
/// (theseus-e31an: a `drafts.create` echo held the owner's DM for three
/// days). An update's echo (`patch`, `modify`, `update`) is the whole of an
/// object someone else may have written, and stays outside text, as every
/// read does unless its own source clears it.
pub fn own_echo(class: Class, method: &str) -> bool {
    let verb = method.rsplit('.').next().unwrap_or_default();
    class != Class::Read && matches!(verb, "create" | "insert" | "send" | "delete")
}

pub struct CallTool(Arc<Google>);

/// What a told act's target was before it: a Gmail message's labels, or
/// whether a Drive file was in the Trash. One Google no longer has, or a read
/// that fails, stops the act before it is made.
async fn read_before(g: &Google, rows: &mut Vec<Value>, t: &Tidy) -> Result<Option<Value>, ToolFailure> {
    use super::take_back;
    let refuse = |message: String, rows: &[Value]| {
        let mut meta = json!({"private": true});
        if let Some((last, also)) = rows.split_last() {
            meta["google_called"] = last.clone();
            meta["google_called_also"] = also.to_vec().into();
        }
        ToolFailure { message, meta }
    };
    let read = match t {
        Tidy::GmailTrash(id) | Tidy::GmailUntrash(id) => {
            match take_back::gmail_labels(g, rows, NAME, id).await {
                Ok(Some(l)) => Ok(Some(take_back::gmail_before(id, &l))),
                Ok(None) => Err(format!("no Gmail message is {id}: nothing was changed")),
                Err(f) => Err(format!("reading the message first: {}", f.message)),
            }
        }
        Tidy::DriveTrash { id, .. } => match take_back::drive_trashed(g, rows, NAME, id).await {
            Ok(Some(w)) => Ok(Some(take_back::drive_before(id, w))),
            Ok(None) => Err(format!("no Drive file is {id}: nothing was changed")),
            Err(f) => Err(format!("reading the file first: {}", f.message)),
        },
        Tidy::CalendarMade => Ok(None),
    };
    read.map_err(|m| refuse(m, rows))
}

/// The note of a told act, from what its target was before it.
fn note_of(t: &Tidy, before: Option<Value>) -> Value {
    use super::told;
    let undo = before.into_iter().collect::<Vec<_>>();
    match t {
        Tidy::GmailTrash(_) => told::mail_trashed(undo, 1),
        Tidy::GmailUntrash(_) => told::mail_untrashed(undo),
        Tidy::DriveTrash { id, trashed } => {
            let was = undo
                .first()
                .and_then(|b| b["trashed"].as_bool())
                .unwrap_or(*trashed);
            told::drive_trashed(id, *trashed, was)
        }
        Tidy::CalendarMade => told::calendar_made(),
    }
}

impl CallTool {
    fn planned(&self, input: &Value) -> Result<Planned, String> {
        let a: Args = parse(input)?;
        let resolved = resolve(&a.method, a.params.as_ref(), a.body.as_ref())?;
        let service = resolved.request.service;
        if !self.0.services().contains(&service) {
            return Err(format!(
                "{} is not in `[google] services`, so nothing of it is called",
                service.name()
            ));
        }
        if resolved.class.send {
            self.0.sends.allows()?;
        }
        let label = resolved.id.map_or_else(|| a.method.clone(), String::from);
        Ok(Planned { resolved, label })
    }
}

impl Tool for CallTool {
    fn name(&self) -> &str {
        NAME
    }
    fn description(&self) -> &str {
        "Call any REST method of the granted Google services (Gmail, Drive, Sheets, Slides, Docs, \
         Calendar) with Theseus's own credential. `method` is an id (drive.files.list, \
         gmail.users.messages.get, calendar.events.list) or \"VERB /path\" \
         (\"GET /gmail/v1/users/me/labels\"); `params` holds the path and query parameters \
         together, as Google's reference names them (userId defaults to me, calendarId to \
         primary); `body` is the JSON body. A GET reads. Every other call asks the owner, \
         except moving one Gmail message to or from the Trash, trashing or restoring a Drive \
         file (a body of `trashed` alone), and making a calendar: those are told. Results are \
         private and outside text."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "method": {"type": "string", "description": "A method id, or \"VERB /path\"."},
                "params": {"type": "object", "description": "Path and query parameters, by Google's names."},
                "body": {"description": "The JSON request body."}
            },
            "required": ["method"],
            "additionalProperties": false
        })
    }
    fn class(&self) -> ToolClass {
        ToolClass::Read
    }
    fn backend(&self) -> Backend {
        Backend::Async
    }
    fn retry(&self) -> Retry {
        // A write may have run when a crash cut it: unknown, never run again.
        Retry::NonRepeatable
    }
    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let p = self.planned(input)?;
        let class = match p.class() {
            Class::Read => ToolClass::Read,
            Class::Write | Class::Authority => ToolClass::Write,
        };
        let authority = p.resolved.class.why.map(|why| {
            format!(
                "{why}, so the owner decides it, every time, at every posture: {}",
                p.label
            )
        });
        Ok(Plan {
            summary: p.summary(),
            class: Some(class),
            authority,
            ..Default::default()
        })
    }
    fn run_async(&self, input: &Value, ctx: &ToolCtx) -> AsyncRun {
        let planned = self.planned(input);
        let g = self.0.clone();
        let approved = ctx.approved;
        Box::pin(async move {
            let p = planned.map_err(ToolFailure::new)?;
            let send = p.resolved.class.send;
            if send {
                g.sends.reserve().map_err(ToolFailure::new)?;
            }
            let meta = Meta {
                tool: NAME,
                method: p.label.clone(),
                resource: p.resolved.resource.clone(),
                class: p.class_word(),
                recipients: None,
            };
            // A told act's target as it was, read before it (issue 57).
            let tidy = p.resolved.class.tidy.clone().filter(|_| !approved);
            let mut rows = Vec::new();
            let before = match &tidy {
                Some(t) => Some(read_before(&g, &mut rows, t).await?),
                None => None,
            };
            match g.call(meta, p.resolved.request.clone()).await {
                Ok(done) => {
                    let r = &done.response;
                    let body = match serde_json::from_slice::<Value>(&r.body) {
                        Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_default(),
                        Err(_) => r.text(),
                    };
                    let text = format!(
                        "{}: HTTP {} · {} attempt(s)\n{body}\n",
                        p.label, r.status, r.attempts
                    );
                    let mut meta = json!({"method": p.label, "http_status": r.status});
                    if let (Some(t), Some(b)) = (&tidy, before) {
                        meta[super::told::KEY] = note_of(t, b);
                    }
                    // The long tail cannot say which calendar it read, so any
                    // Calendar read counts as the sensitive one's (fail closed).
                    let calendar = p.resolved.request.service == theseus_google::Service::Calendar;
                    if calendar && g.calendars.any_sensitive() {
                        meta[super::private::SENSITIVE] = true.into();
                    }
                    let label = format!("google:{}", p.label);
                    let source = match own_echo(p.class(), &p.label) {
                        true => Source::None,
                        false => Source::Outside { label: &label },
                    };
                    rows.push(done.row);
                    let (meta, row) = super::sheets::with_rows(meta, rows);
                    Ok(finish(g.account(), text, meta, row, source))
                }
                Err(f) => {
                    // A send that was refused before it could go out is not a send.
                    if send && !f.unknown {
                        g.sends.release();
                    }
                    let mut meta = json!({"private": true, "google_called": f.row, "outcome_unknown": f.unknown});
                    if !rows.is_empty() {
                        meta["google_called_also"] = rows.into();
                    }
                    Err(ToolFailure {
                        message: f.message,
                        meta,
                    })
                }
            }
        })
    }
    fn rest(&self, _left_out: &str) -> String {
        "a narrower call returns them: params.fields, or a smaller pageSize".into()
    }
}
