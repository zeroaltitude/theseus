//! The fake's Sheets (gw-sheets-slides): spreadsheets that keep their cells,
//! so a write can be read back. Values only: a tab is a grid from `A1`, a range
//! is `Tab!A1:B2` or `A1`, and `USER_ENTERED` is recorded in the request log
//! (the fake does not evaluate formulas).

use std::collections::BTreeMap;

use percent_encoding::percent_decode_str;
use serde_json::{json, Value};

use super::Reply;
use crate::http1::Req;

#[derive(Default)]
pub struct State {
    sheets: BTreeMap<String, Sheet>,
    made: u32,
}

struct Sheet {
    title: String,
    /// The one tab, `Sheet1`: its cells by (row, column), from zero.
    cells: BTreeMap<(usize, usize), Value>,
}

const TAB: &str = "Sheet1";

fn column(letters: &str) -> usize {
    letters.bytes().fold(0, |n, b| {
        n * 26 + (b.to_ascii_uppercase() - b'A') as usize + 1
    }) - 1
}

fn letters(mut col: usize) -> String {
    let mut s = String::new();
    loop {
        s.insert(0, (b'A' + (col % 26) as u8) as char);
        if col < 26 {
            return s;
        }
        col = col / 26 - 1;
    }
}

/// `B2` as (row, column), from zero.
fn cell(a1: &str) -> Option<(usize, usize)> {
    let split = a1.find(|c: char| c.is_ascii_digit())?;
    let (col, row) = a1.split_at(split);
    let row: usize = row.parse().ok()?;
    (!col.is_empty() && row > 0).then(|| (row - 1, column(col)))
}

/// A range's first cell and its last, when it names one.
type Cell = (usize, usize);

fn span(range: &str) -> Option<(Cell, Option<Cell>)> {
    if range == TAB {
        return Some(((0, 0), None));
    }
    let a1 = range.rsplit('!').next()?;
    match a1.split_once(':') {
        Some((a, b)) => Some((cell(a)?, Some(cell(b)?))),
        None => Some((cell(a1)?, None)),
    }
}

impl Sheet {
    fn extent(&self) -> (usize, usize) {
        self.cells
            .keys()
            .fold((0, 0), |(r, c), (kr, kc)| (r.max(kr + 1), c.max(kc + 1)))
    }

    fn read(&self, range: &str) -> Value {
        let Some((from, to)) = span(range) else {
            return json!({"range": range, "majorDimension": "ROWS"});
        };
        let (rows, cols) = self.extent();
        let to = to.unwrap_or((rows.max(1) - 1, cols.max(1) - 1));
        let mut values: Vec<Vec<Value>> = (from.0..=to.0)
            .map(|r| {
                let mut row: Vec<Value> = (from.1..=to.1)
                    .map(|c| self.cells.get(&(r, c)).cloned().unwrap_or(Value::Null))
                    .collect();
                while row.last().is_some_and(Value::is_null) {
                    row.pop();
                }
                row.into_iter()
                    .map(|v| if v.is_null() { json!("") } else { v })
                    .collect()
            })
            .collect();
        while values.last().is_some_and(Vec::is_empty) {
            values.pop();
        }
        let mut out = json!({"range": range, "majorDimension": "ROWS"});
        if !values.is_empty() {
            out["values"] = json!(values);
        }
        out
    }
}

fn not_found(id: &str) -> Reply {
    Reply::json(
        404,
        json!({"error": {"code": 404, "message": format!("Requested entity was not found: {id}"),
            "errors": [{"reason": "notFound"}]}}),
    )
}

fn properties(id: &str, s: &Sheet) -> Value {
    let (rows, cols) = s.extent();
    json!({"spreadsheetId": id, "properties": {"title": s.title},
        "spreadsheetUrl": format!("https://docs.google.com/spreadsheets/d/{id}/edit"),
        "sheets": [{"properties": {"sheetId": 0, "title": TAB,
            "gridProperties": {"rowCount": rows.max(1000), "columnCount": cols.max(26)}}}]})
}

pub fn answer(st: &mut State, req: &Req) -> Option<Reply> {
    let path = percent_decode_str(req.path())
        .decode_utf8_lossy()
        .into_owned();
    let rest = path.strip_prefix("/v4/spreadsheets")?;
    if rest.is_empty() {
        if req.method != "POST" {
            return None;
        }
        st.made += 1;
        let id = format!("sheet-{}", st.made);
        let title = req.json()["properties"]["title"]
            .as_str()
            .unwrap_or("Untitled spreadsheet")
            .to_string();
        let s = Sheet {
            title,
            cells: BTreeMap::new(),
        };
        let reply = properties(&id, &s);
        st.sheets.insert(id, s);
        return Some(Reply::json(200, reply));
    }
    let rest = rest.strip_prefix('/')?;
    let (id, tail) = rest.split_once('/').unwrap_or((rest, ""));
    let Some(sheet) = st.sheets.get_mut(id) else {
        return Some(not_found(id));
    };
    let query = req.query();
    Some(match (req.method.as_str(), tail) {
        ("GET", "") => Reply::json(200, properties(id, sheet)),
        ("GET", "values:batchGet") => {
            let ranges: Vec<Value> = query
                .iter()
                .filter(|(k, _)| k == "ranges")
                .map(|(_, r)| sheet.read(r))
                .collect();
            Reply::json(200, json!({"spreadsheetId": id, "valueRanges": ranges}))
        }
        (verb, t) if t.starts_with("values/") => {
            let target = &t["values/".len()..];
            let (range, action) = match target.rsplit_once(':') {
                Some((r, a)) if matches!(a, "append" | "clear") => (r, a),
                _ => (target, ""),
            };
            let Some((from, to)) = span(range) else {
                return Some(Reply::json(
                    400,
                    json!({"error": {"code": 400, "message": format!("Unable to parse range: {range}"),
                        "errors": [{"reason": "badRequest"}]}}),
                ));
            };
            match (verb, action) {
                ("PUT", "") => {
                    let at = from;
                    Reply::json(200, put(sheet, id, at, req, false))
                }
                ("POST", "append") => {
                    let (rows, _) = sheet.extent();
                    Reply::json(200, put(sheet, id, (rows, from.1), req, true))
                }
                ("POST", "clear") => {
                    let to = to.unwrap_or_else(|| {
                        let (r, c) = sheet.extent();
                        (r.max(1) - 1, c.max(1) - 1)
                    });
                    sheet.cells.retain(|(r, c), _| {
                        !((from.0..=to.0).contains(r) && (from.1..=to.1).contains(c))
                    });
                    Reply::json(200, json!({"spreadsheetId": id, "clearedRange": range}))
                }
                _ => return None,
            }
        }
        _ => return None,
    })
}

/// Rows from the body at `at`: an update's answer, or an append's.
fn put(sheet: &mut Sheet, id: &str, at: (usize, usize), req: &Req, append: bool) -> Value {
    let body = req.json();
    let values = body["values"].as_array().cloned().unwrap_or_default();
    let mut width = 0;
    for (i, row) in values.iter().enumerate() {
        let row = row.as_array().cloned().unwrap_or_default();
        width = width.max(row.len());
        for (j, v) in row.into_iter().enumerate() {
            sheet.cells.insert((at.0 + i, at.1 + j), v);
        }
    }
    let last = (at.0 + values.len().max(1), at.1 + width.max(1));
    let updated = format!(
        "{TAB}!{}{}:{}{}",
        letters(at.1),
        at.0 + 1,
        letters(last.1 - 1),
        last.0
    );
    let done = json!({"spreadsheetId": id, "updatedRange": updated, "updatedRows": values.len(),
        "updatedColumns": width, "updatedCells": values.len() * width});
    if append {
        json!({"spreadsheetId": id, "tableRange": format!("{TAB}!A1"), "updates": done})
    } else {
        done
    }
}
