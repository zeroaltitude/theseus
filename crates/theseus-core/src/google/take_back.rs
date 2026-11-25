//! A told Google act's exact undo (issue 57): what each thing it changed was
//! before it, read from Google before the act, and the owner's Undo putting
//! each back to exactly that. A Gmail message gets back its own labels and
//! whether it was in the Trash; a Drive file whether it was in the Trash.
//! Only the messages and files the act changed are recorded, so a message
//! that joined the thread afterwards is never touched, and an act that
//! changed nothing has an Undo that changes nothing.
//!
//! The record is built here, by Theseus's own code, from Google's answers:
//! never from the model's words or a result's text.

use serde_json::{json, Value};
use theseus_google::{Body, Repeat, Request, Service};

use super::{CallFailed, Google, Meta};

/// The Trash's own label.
const TRASH: &str = "TRASH";

/// A Gmail message as it was: its id and its labels.
pub fn gmail_before(id: &str, labels: &[String]) -> Value {
    json!({"gmail": id, "labels": labels})
}

/// A Drive file as it was: its id and whether it was in the Trash.
pub fn drive_before(id: &str, trashed: bool) -> Value {
    json!({"drive": id, "trashed": trashed})
}

fn request(
    service: Service,
    verb: &str,
    path: String,
    query: &[(&str, &str)],
    body: Body,
) -> Request {
    Request {
        service,
        verb: verb.into(),
        path,
        query: query
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect(),
        body,
        repeat: Repeat::Safe,
    }
}

/// One call, its row kept in `rows` whatever came of it; a 404 is `None`.
async fn one(
    g: &Google,
    rows: &mut Vec<Value>,
    (tool, method, id, class): (&'static str, &str, &str, &'static str),
    req: Request,
) -> Result<Option<Value>, CallFailed> {
    let meta = Meta {
        tool,
        method: method.to_string(),
        resource: Some(id.to_string()),
        class,
        recipients: None,
    };
    match g.call(meta, req).await {
        Ok(done) => {
            rows.push(done.row);
            Ok(Some(done.response.json()))
        }
        Err(f) => {
            rows.push(f.row.clone());
            match f.row["status"] == 404 {
                true => Ok(None),
                false => Err(f),
            }
        }
    }
}

fn labels_of(v: &Value) -> Vec<String> {
    v["labelIds"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|l| l.as_str().map(String::from))
        .collect()
}

/// A Gmail message's labels now; `None` when Gmail has no such message.
pub(crate) async fn gmail_labels(
    g: &Google,
    rows: &mut Vec<Value>,
    tool: &'static str,
    id: &str,
) -> Result<Option<Vec<String>>, CallFailed> {
    let req = request(
        Service::Gmail,
        "GET",
        format!("/gmail/v1/users/me/messages/{id}"),
        &[("format", "minimal")],
        Body::None,
    );
    let got = one(g, rows, (tool, "gmail.users.messages.get", id, "read"), req).await?;
    Ok(got.map(|m| labels_of(&m)))
}

/// A Gmail thread's messages and each one's labels now; `None` when Gmail
/// has no such thread.
pub(crate) async fn gmail_thread(
    g: &Google,
    rows: &mut Vec<Value>,
    tool: &'static str,
    id: &str,
) -> Result<Option<Vec<(String, Vec<String>)>>, CallFailed> {
    let req = request(
        Service::Gmail,
        "GET",
        format!("/gmail/v1/users/me/threads/{id}"),
        &[("format", "minimal")],
        Body::None,
    );
    let got = one(g, rows, (tool, "gmail.users.threads.get", id, "read"), req).await?;
    Ok(got.map(|t| {
        t["messages"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| Some((m["id"].as_str()?.to_string(), labels_of(m))))
            .collect()
    }))
}

/// `POST …/messages/{id}/{op}` (`trash`, `untrash` or `modify`).
pub(crate) async fn gmail_post(
    g: &Google,
    rows: &mut Vec<Value>,
    tool: &'static str,
    id: &str,
    op: &str,
    body: Value,
) -> Result<Option<Value>, CallFailed> {
    let req = request(
        Service::Gmail,
        "POST",
        format!("/gmail/v1/users/me/messages/{id}/{op}"),
        &[],
        Body::Json(body),
    );
    let method = format!("gmail.users.messages.{op}");
    one(g, rows, (tool, &method, id, "write"), req).await
}

/// Whether a Drive file is in the Trash now; `None` when Drive has no such
/// file.
pub(crate) async fn drive_trashed(
    g: &Google,
    rows: &mut Vec<Value>,
    tool: &'static str,
    id: &str,
) -> Result<Option<bool>, CallFailed> {
    let req = request(
        Service::Drive,
        "GET",
        format!("/drive/v3/files/{id}"),
        &[("fields", "id,trashed"), ("supportsAllDrives", "true")],
        Body::None,
    );
    let got = one(g, rows, (tool, "drive.files.get", id, "read"), req).await?;
    Ok(got.map(|f| f["trashed"].as_bool().unwrap_or(false)))
}

/// What an Undo did: how many things it put back of how many, how many
/// Google has removed for good, and the first failure, if any.
#[derive(Debug, Default)]
pub struct Progress {
    pub restored: usize,
    pub of: usize,
    pub gone: usize,
    pub failed: Option<String>,
}

impl Progress {
    /// Everything put back.
    pub fn whole(&self) -> bool {
        self.restored == self.of
    }

    /// The words for an Undo that did not put everything back.
    pub fn words(&self) -> String {
        let mut s = match self.restored {
            0 => format!("Nothing was restored (0 of {}).", self.of),
            n => format!("Partly restored: {n} of {}.", self.of),
        };
        if self.gone > 0 {
            s.push_str(&format!(
                " {} of them no longer exist in Google (deleted for good, or emptied from the \
                 Trash), so they cannot come back.",
                self.gone
            ));
        }
        if let Some(f) = &self.failed {
            s.push_str(&format!(
                " Google refused the rest: {f}. Pressing Undo again tries the rest."
            ));
        }
        s
    }
}

/// Each recorded thing put back exactly as it was. Every one is tried, so a
/// failure leaves the rest restored and says which part.
pub(crate) async fn restore(
    g: &Google,
    (gmail_tool, drive_tool): (&'static str, &'static str),
    before: &[Value],
    rows: &mut Vec<Value>,
) -> Progress {
    let mut p = Progress {
        of: before.len(),
        ..Default::default()
    };
    for b in before {
        let done = if let Some(id) = b["gmail"].as_str() {
            let labels = labels_of(&json!({"labelIds": b["labels"].clone()}));
            gmail(g, rows, gmail_tool, id, &labels).await
        } else if let (Some(id), Some(t)) = (b["drive"].as_str(), b["trashed"].as_bool()) {
            drive(g, rows, drive_tool, id, t).await
        } else {
            Err(CallFailed {
                message: "a record this build cannot read".into(),
                row: Value::Null,
                unknown: false,
            })
        };
        match done {
            Ok(true) => p.restored += 1,
            Ok(false) => p.gone += 1,
            Err(f) => {
                p.failed.get_or_insert(f.message);
            }
        }
    }
    p
}

/// A Gmail message back to `before`: into or out of the Trash as it was,
/// then its other labels as they were. `false`: Gmail no longer has it.
async fn gmail(
    g: &Google,
    rows: &mut Vec<Value>,
    tool: &'static str,
    id: &str,
    before: &[String],
) -> Result<bool, CallFailed> {
    let Some(mut now) = gmail_labels(g, rows, tool, id).await? else {
        return Ok(false);
    };
    let was = before.iter().any(|l| l == TRASH);
    if was != now.iter().any(|l| l == TRASH) {
        let op = if was { "trash" } else { "untrash" };
        if gmail_post(g, rows, tool, id, op, json!({}))
            .await?
            .is_none()
        {
            return Ok(false);
        }
        match gmail_labels(g, rows, tool, id).await? {
            Some(l) => now = l,
            None => return Ok(false),
        }
    }
    let less = |a: &[String], b: &[String]| -> Vec<String> {
        a.iter()
            .filter(|l| *l != TRASH && !b.contains(l))
            .cloned()
            .collect()
    };
    let (add, remove) = (less(before, &now), less(&now, before));
    if !add.is_empty() || !remove.is_empty() {
        let body = json!({"addLabelIds": add, "removeLabelIds": remove});
        if gmail_post(g, rows, tool, id, "modify", body)
            .await?
            .is_none()
        {
            return Ok(false);
        }
    }
    Ok(true)
}

/// A Drive file back into or out of the Trash as it was. `false`: Drive no
/// longer has it.
async fn drive(
    g: &Google,
    rows: &mut Vec<Value>,
    tool: &'static str,
    id: &str,
    before: bool,
) -> Result<bool, CallFailed> {
    let Some(now) = drive_trashed(g, rows, tool, id).await? else {
        return Ok(false);
    };
    if now != before {
        let req = request(
            Service::Drive,
            "PATCH",
            format!("/drive/v3/files/{id}"),
            &[("supportsAllDrives", "true")],
            Body::Json(json!({"trashed": before})),
        );
        let put = one(g, rows, (tool, "drive.files.update", id, "write"), req).await?;
        if put.is_none() {
            return Ok(false);
        }
    }
    Ok(true)
}
