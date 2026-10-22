//! `sheets.read` and `sheets.write` (the Google design §3.1, rows 12 and 13).
//!
//! Reads return the tabs and the values asked for, formatted as the sheet
//! shows them or raw. A write updates, appends rows to, or clears a range; no
//! `id` makes a new spreadsheet. `input: user` is `USER_ENTERED`, so a formula
//! evaluates; the default `raw` stores what was written, as text and numbers.
//! The quota (about 60 reads and 60 writes a minute) is met by the client's
//! backoff, which honours `Retry-After`: there is no limiter of its own.
//! Results are private and, as a spreadsheet's author is not known without a
//! Drive call, outside text.

use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_google::{Body, Repeat, Request, Service};
use theseus_tools::{parse, AsyncRun, Backend, Plan, Retry, Tool, ToolClass, ToolCtx, ToolFailure};

use super::external::{finish, Source};
use super::{CallFailed, Google, Meta};

pub const READ: &str = "sheets.read";
pub const WRITE: &str = "sheets.write";

pub fn tools(g: &Arc<Google>) -> Vec<Arc<dyn Tool>> {
    vec![Arc::new(Read(g.clone())), Arc::new(Write(g.clone()))]
}

/// A path segment: everything but the unreserved and the few a range uses.
pub(super) fn segment(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'.'
            | b'_'
            | b'~'
            | b'!'
            | b':'
            | b'$' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub(super) fn request(
    service: Service,
    verb: &str,
    path: String,
    query: Vec<(&str, String)>,
    body: Option<Value>,
    repeat: Repeat,
) -> Request {
    Request {
        service,
        verb: verb.into(),
        path,
        query: query.into_iter().map(|(k, v)| (k.into(), v)).collect(),
        body: body.map_or(Body::None, Body::Json),
        repeat,
    }
}

/// A call that did not go through, with the rows of the calls before it.
pub(super) fn failed(f: CallFailed, earlier: Vec<Value>, note: &str) -> ToolFailure {
    ToolFailure {
        message: format!("{note}{}", f.message),
        meta: json!({"private": true, "google_called": f.row, "google_called_also": earlier,
            "outcome_unknown": f.unknown}),
    }
}

/// An error after the calls in `rows` went through: no call of its own failed.
pub(super) fn refused(message: impl Into<String>, rows: Vec<Value>) -> ToolFailure {
    let (mut meta, last) = with_rows(json!({"private": true}), rows);
    meta["google_called"] = last;
    ToolFailure {
        message: message.into(),
        meta,
    }
}

/// A result's meta with every call's row: the last is the result's own.
pub(super) fn with_rows(mut meta: Value, mut rows: Vec<Value>) -> (Value, Value) {
    let last = rows.pop().unwrap_or(Value::Null);
    if !rows.is_empty() {
        meta["google_called_also"] = rows.into();
    }
    (meta, last)
}

fn cell_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.replace(['\t', '\n'], " "),
        Value::Null => String::new(),
        v => v.to_string(),
    }
}

/// The spreadsheet's title and its tabs, in words.
fn tabs_text(id: &str, info: &Value) -> String {
    let title = info["properties"]["title"].as_str().unwrap_or("");
    let mut text = format!("Spreadsheet \"{title}\" ({id})\n");
    for t in info["sheets"].as_array().into_iter().flatten() {
        let p = &t["properties"];
        text.push_str(&format!(
            "Tab {} (sheetId {}, {} rows x {} columns)\n",
            p["title"].as_str().unwrap_or(""),
            p["sheetId"],
            p["gridProperties"]["rowCount"],
            p["gridProperties"]["columnCount"],
        ));
    }
    text
}

/// Each range of a `batchGet` answer as a header and tab-separated rows.
fn values_text(answer: &Value) -> String {
    let mut text = String::new();
    for vr in answer["valueRanges"].as_array().into_iter().flatten() {
        let grid = vr["values"].as_array().cloned().unwrap_or_default();
        let range = vr["range"].as_str().unwrap_or("");
        text.push_str(&format!("\n{range} ({} rows)\n", grid.len()));
        for row in grid {
            let cells: Vec<String> = row
                .as_array()
                .into_iter()
                .flatten()
                .map(cell_text)
                .collect();
            text.push_str(&cells.join("\t"));
            text.push('\n');
        }
    }
    text
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadArgs {
    id: String,
    #[serde(default)]
    ranges: Vec<String>,
    #[serde(default)]
    raw: bool,
}

pub struct Read(Arc<Google>);

impl Tool for Read {
    fn name(&self) -> &str {
        READ
    }
    fn description(&self) -> &str {
        "Read a Google spreadsheet: its tabs (title, size) and the values of each A1 range \
         (\"Sheet1!A1:C20\"). Values are formatted as the sheet shows them; raw: true gives the \
         underlying numbers. No ranges gives the tabs only. Results are private and outside text."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {"type": "string", "description": "The spreadsheet's id."},
                "ranges": {"type": "array", "items": {"type": "string"}, "description": "A1 ranges."},
                "raw": {"type": "boolean", "description": "Unformatted values."}
            },
            "required": ["id"],
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
        Retry::SafeToRepeat
    }
    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let a: ReadArgs = parse(input)?;
        Ok(Plan {
            summary: format!("read {} range(s) of spreadsheet {}", a.ranges.len(), a.id),
            class: Some(ToolClass::Read),
            ..Default::default()
        })
    }
    fn run_async(&self, input: &Value, _ctx: &ToolCtx) -> AsyncRun {
        let args: Result<ReadArgs, String> = parse(input);
        let g = self.0.clone();
        Box::pin(async move {
            let a = args.map_err(ToolFailure::new)?;
            let base = format!("/v4/spreadsheets/{}", segment(&a.id));
            let meta = |method: &str| Meta {
                tool: READ,
                method: method.into(),
                resource: Some(a.id.clone()),
                class: "read",
                recipients: None,
            };
            let fields = "properties.title,sheets.properties(sheetId,title,gridProperties)";
            let req = request(
                Service::Sheets,
                "GET",
                base.clone(),
                vec![("fields", fields.into())],
                None,
                Repeat::Safe,
            );
            let tabs = g
                .call(meta("sheets.spreadsheets.get"), req)
                .await
                .map_err(|f| failed(f, vec![], ""))?;
            let mut rows = vec![tabs.row];
            let info = tabs.response.json();
            let mut text = tabs_text(&a.id, &info);
            if !a.ranges.is_empty() {
                let mut query: Vec<(&str, String)> =
                    a.ranges.iter().map(|r| ("ranges", r.clone())).collect();
                let render = if a.raw {
                    "UNFORMATTED_VALUE"
                } else {
                    "FORMATTED_VALUE"
                };
                query.push(("valueRenderOption", render.into()));
                let req = request(
                    Service::Sheets,
                    "GET",
                    format!("{base}/values:batchGet"),
                    query,
                    None,
                    Repeat::Safe,
                );
                let done = match g
                    .call(meta("sheets.spreadsheets.values.batchGet"), req)
                    .await
                {
                    Ok(d) => d,
                    Err(f) => return Err(failed(f, rows, "")),
                };
                text.push_str(&values_text(&done.response.json()));
                rows.push(done.row);
            }
            let (meta, row) = with_rows(json!({"id": a.id, "ranges": a.ranges.len()}), rows);
            Ok(finish(
                g.account(),
                text,
                meta,
                row,
                Source::Outside {
                    label: &format!("sheets:{}", a.id),
                },
            ))
        })
    }
    fn rest(&self, _left_out: &str) -> String {
        "a narrower range returns them".into()
    }
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
enum Mode {
    #[default]
    Update,
    Append,
    Clear,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
enum Input {
    #[default]
    Raw,
    User,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteArgs {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    range: Option<String>,
    #[serde(default)]
    values: Vec<Vec<Value>>,
    #[serde(default)]
    mode: Mode,
    #[serde(default)]
    input: Input,
}

impl WriteArgs {
    /// What the call asks for, or why it cannot: the same words at the plan
    /// and at the run.
    fn checked(input: &Value) -> Result<WriteArgs, String> {
        let a: WriteArgs = parse(input)?;
        if a.values
            .iter()
            .flatten()
            .any(|v| v.is_array() || v.is_object())
        {
            return Err("a cell is text, a number, a boolean or null".into());
        }
        let has_values = a.values.iter().any(|r| !r.is_empty());
        match (&a.id, a.mode) {
            (None, Mode::Append | Mode::Clear) => {
                return Err("append and clear need the spreadsheet's `id`".into())
            }
            (Some(_), Mode::Update | Mode::Append) if !has_values => {
                return Err("`values` holds nothing to write".into())
            }
            (Some(_), _) if a.range.is_none() => return Err("`range` names where to write".into()),
            _ => {}
        }
        Ok(a)
    }

    fn range(&self) -> &str {
        self.range.as_deref().unwrap_or("A1")
    }

    /// The method's name and the request that writes `values` to `id`.
    fn values_request(&self, id: &str) -> (&'static str, Request) {
        let a = self;
        let path = format!(
            "/v4/spreadsheets/{}/values/{}",
            segment(id),
            segment(a.range())
        );
        let entered = match a.input {
            Input::User => "USER_ENTERED",
            Input::Raw => "RAW",
        };
        let body = json!({"range": a.range(), "majorDimension": "ROWS", "values": a.values});
        let (method, req) = match a.mode {
            Mode::Update => (
                "sheets.spreadsheets.values.update",
                request(
                    Service::Sheets,
                    "PUT",
                    path,
                    vec![("valueInputOption", entered.into())],
                    Some(body),
                    Repeat::Safe,
                ),
            ),
            Mode::Append => (
                "sheets.spreadsheets.values.append",
                request(
                    Service::Sheets,
                    "POST",
                    format!("{path}:append"),
                    vec![
                        ("valueInputOption", entered.into()),
                        ("insertDataOption", "INSERT_ROWS".into()),
                    ],
                    Some(body),
                    Repeat::NonRepeatable,
                ),
            ),
            Mode::Clear => (
                "sheets.spreadsheets.values.clear",
                request(
                    Service::Sheets,
                    "POST",
                    format!("{path}:clear"),
                    vec![],
                    Some(json!({})),
                    Repeat::Safe,
                ),
            ),
        };
        (method, req)
    }

    /// What Sheets answered, in words.
    fn wrote_text(&self, j: &Value) -> String {
        let u = if self.mode == Mode::Append {
            &j["updates"]
        } else {
            j
        };
        match self.mode {
            Mode::Clear => format!(
                "Cleared {}\n",
                j["clearedRange"].as_str().unwrap_or(self.range())
            ),
            _ => format!(
                "Wrote {} cell(s) in {} row(s) at {}\n",
                u["updatedCells"],
                u["updatedRows"],
                u["updatedRange"].as_str().unwrap_or(self.range())
            ),
        }
    }
}

pub struct Write(Arc<Google>);

impl Tool for Write {
    fn name(&self) -> &str {
        WRITE
    }
    fn description(&self) -> &str {
        "Write to a Google spreadsheet. mode update (default) sets the cells from `range`'s first \
         cell; append adds the rows after the table; clear empties the range. No `id` creates a \
         spreadsheet titled `title` and writes `values` at `range` (default A1). input raw \
         (default) stores text and numbers as given; user evaluates formulas (\"=SUM(A1:A3)\"). \
         Drive keeps the revisions."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {"type": "string", "description": "The spreadsheet; none creates one."},
                "title": {"type": "string", "description": "The new spreadsheet's title."},
                "range": {"type": "string", "description": "A1 range, \"Sheet1!A1\"."},
                "values": {"type": "array", "items": {"type": "array"}, "description": "Rows of cells."},
                "mode": {"enum": ["update", "append", "clear"]},
                "input": {"enum": ["raw", "user"]}
            },
            "additionalProperties": false
        })
    }
    fn class(&self) -> ToolClass {
        ToolClass::Write
    }
    fn backend(&self) -> Backend {
        Backend::Async
    }
    fn retry(&self) -> Retry {
        // An append or a create repeated would write twice.
        Retry::NonRepeatable
    }
    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let a = WriteArgs::checked(input)?;
        let what = match (&a.id, a.mode) {
            (None, _) => "create a spreadsheet".to_string(),
            (Some(id), Mode::Update) => format!("update {} of {id}", a.range()),
            (Some(id), Mode::Append) => {
                format!("append {} row(s) to {} of {id}", a.values.len(), a.range())
            }
            (Some(id), Mode::Clear) => format!("clear {} of {id}", a.range()),
        };
        Ok(Plan {
            summary: what,
            class: Some(ToolClass::Write),
            ..Default::default()
        })
    }
    fn run_async(&self, input: &Value, _ctx: &ToolCtx) -> AsyncRun {
        let args = WriteArgs::checked(input);
        let g = self.0.clone();
        Box::pin(async move {
            let a = args.map_err(ToolFailure::new)?;
            let mut rows = Vec::new();
            let meta = |method: &str, id: &str| Meta {
                tool: WRITE,
                method: method.into(),
                resource: (!id.is_empty()).then(|| id.to_string()),
                class: "write",
                recipients: None,
            };
            let mut note = String::new();
            let (id, url) = match &a.id {
                Some(id) => (id.clone(), None),
                None => {
                    let body = json!({"properties": {"title": a.title.as_deref().unwrap_or("Untitled spreadsheet")}});
                    let req = request(
                        Service::Sheets,
                        "POST",
                        "/v4/spreadsheets".into(),
                        vec![],
                        Some(body),
                        Repeat::NonRepeatable,
                    );
                    let made = g
                        .call(meta("sheets.spreadsheets.create", ""), req)
                        .await
                        .map_err(|f| failed(f, vec![], ""))?;
                    rows.push(made.row);
                    let j = made.response.json();
                    let Some(id) = j["spreadsheetId"].as_str().map(String::from) else {
                        return Err(refused("the create answer named no spreadsheet id", rows));
                    };
                    note = format!("The spreadsheet {id} was created, but writing to it failed: ");
                    (id, j["spreadsheetUrl"].as_str().map(String::from))
                }
            };
            let mut text = format!("Spreadsheet {id}");
            if let Some(u) = &url {
                text.push_str(&format!(" ({u}), created"));
            }
            text.push('\n');
            if a.id.is_some() || a.values.iter().any(|r| !r.is_empty()) {
                let (method, req) = a.values_request(&id);
                let done = match g.call(meta(method, &id), req).await {
                    Ok(d) => d,
                    Err(f) => return Err(failed(f, rows, &note)),
                };
                text.push_str(&a.wrote_text(&done.response.json()));
                rows.push(done.row);
            }
            let (meta, row) = with_rows(json!({"id": id, "url": url}), rows);
            Ok(finish(g.account(), text, meta, row, Source::None))
        })
    }
}
