//! The fake Drive (contract 6): files that live, so a write can be read back.
//! `Drive::install` puts its routes on a [`Fake`]; the Docs export, the Sheets
//! values and the Slides read it stands in for are answered from the same
//! files. A markdown upload as a Doc is kept as markdown and exported as it
//! was; a CSV as a Sheet becomes a one-tab table.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use super::{Fake, Reply};
use crate::http1::Req;

pub const DOC: &str = "application/vnd.google-apps.document";
pub const SHEET: &str = "application/vnd.google-apps.spreadsheet";
pub const SLIDES: &str = "application/vnd.google-apps.presentation";

#[derive(Debug, Clone, PartialEq)]
pub enum Content {
    Bytes(Vec<u8>),
    /// Tabs: a title and its rows.
    Sheet(Vec<(String, Vec<Vec<String>>)>),
    /// Slides: each slide's text elements, `(object id, text)`, and its layout's name.
    Slides(Vec<(String, Vec<(String, String)>)>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Permission {
    pub id: String,
    pub kind: String,
    pub email: Option<String>,
    pub role: String,
}

#[derive(Debug, Clone)]
pub struct File {
    pub id: String,
    pub name: String,
    pub mime: String,
    pub parents: Vec<String>,
    /// The account owns it; else someone else does.
    pub mine: bool,
    pub shared: bool,
    pub trashed: bool,
    pub content: Content,
    pub permissions: Vec<Permission>,
}

impl File {
    fn new(id: &str, name: &str, mime: &str, content: Content) -> File {
        File {
            id: id.into(),
            name: name.into(),
            mime: mime.into(),
            parents: vec!["root".into()],
            mine: true,
            shared: false,
            trashed: false,
            content,
            permissions: Vec::new(),
        }
    }

    pub fn doc(id: &str, name: &str, markdown: &str) -> File {
        File::new(id, name, DOC, Content::Bytes(markdown.as_bytes().to_vec()))
    }

    pub fn binary(id: &str, name: &str, mime: &str, bytes: Vec<u8>) -> File {
        File::new(id, name, mime, Content::Bytes(bytes))
    }

    pub fn sheet(id: &str, name: &str, tabs: Vec<(&str, Vec<Vec<&str>>)>) -> File {
        let tabs = tabs
            .into_iter()
            .map(|(t, rows)| {
                let rows = rows
                    .into_iter()
                    .map(|r| r.into_iter().map(String::from).collect())
                    .collect();
                (t.to_string(), rows)
            })
            .collect();
        File::new(id, name, SHEET, Content::Sheet(tabs))
    }

    pub fn slides(id: &str, name: &str, slides: Vec<(&str, Vec<(&str, &str)>)>) -> File {
        let slides = slides
            .into_iter()
            .map(|(s, els)| {
                (
                    s.to_string(),
                    els.into_iter()
                        .map(|(a, b)| (a.to_string(), b.to_string()))
                        .collect(),
                )
            })
            .collect();
        File::new(id, name, SLIDES, Content::Slides(slides))
    }

    /// Someone else's, shared with the account.
    pub fn theirs(mut self) -> File {
        self.mine = false;
        self.shared = true;
        self
    }

    pub fn shared(mut self) -> File {
        self.shared = true;
        self
    }

    fn meta(&self) -> Value {
        let owner = if self.mine {
            json!({"emailAddress": "owner@example.test", "me": true})
        } else {
            json!({"emailAddress": "collaborator@example.test", "me": false})
        };
        let size = match &self.content {
            Content::Bytes(b) => Some(b.len().to_string()),
            _ => None,
        };
        let mut m = json!({"id": self.id, "name": self.name, "mimeType": self.mime, "owners": [owner],
            "shared": self.shared, "trashed": self.trashed, "parents": self.parents,
            "modifiedTime": "2026-10-20T09:00:00Z", "webViewLink": format!("https://drive.example.test/{}", self.id)});
        if let Some(s) = size {
            m["size"] = s.into();
        }
        m
    }
}

#[derive(Default)]
struct State {
    files: Vec<File>,
    made: u32,
}

/// A handle on the fake's files.
#[derive(Clone)]
pub struct Drive {
    state: Arc<Mutex<State>>,
}

fn not_found() -> Reply {
    Reply::json(
        404,
        json!({"error": {"code": 404, "message": "File not found", "errors": [{"reason": "notFound"}]}}),
    )
}

fn bad(why: &str) -> Reply {
    Reply::json(
        400,
        json!({"error": {"code": 400, "message": why, "errors": [{"reason": "badRequest"}]}}),
    )
}

/// The segment of `req`'s path at `n` (0 is the empty one before the first `/`).
fn seg(req: &Req, n: usize) -> String {
    req.path().split('/').nth(n).unwrap_or("").to_string()
}

fn query(req: &Req, key: &str) -> Option<String> {
    req.query()
        .into_iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v)
}

/// A multipart/related body's parts: each its content type and bytes.
fn parts(req: &Req) -> Vec<(String, Vec<u8>)> {
    let Some(b) = req
        .headers
        .get("content-type")
        .and_then(|c| c.split("boundary=").nth(1))
        .map(|b| b.trim_matches('"').to_string())
    else {
        return Vec::new();
    };
    let delim = format!("--{b}").into_bytes();
    let at = |from: usize| {
        req.body[from..]
            .windows(delim.len())
            .position(|w| w == delim)
            .map(|p| p + from)
    };
    let mut out = Vec::new();
    let mut start = at(0);
    while let Some(s) = start {
        let from = s + delim.len();
        let next = at(from);
        let Some(end) = next else { break };
        let part = &req.body[from..end];
        let part = part.strip_prefix(b"\r\n").unwrap_or(part);
        if let Some(h) = part.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&part[..h]).to_ascii_lowercase();
            let kind = head
                .split("content-type:")
                .nth(1)
                .unwrap_or("")
                .trim()
                .to_string();
            let body = &part[h + 4..];
            let body = body.strip_suffix(b"\r\n").unwrap_or(body);
            out.push((kind, body.to_vec()));
        }
        start = next;
    }
    out
}

fn csv(bytes: &[u8]) -> Content {
    let text = String::from_utf8_lossy(bytes);
    let rows = text
        .lines()
        .map(|l| l.split(',').map(String::from).collect())
        .collect();
    Content::Sheet(vec![("Sheet1".into(), rows)])
}

impl Drive {
    /// The routes of Drive, the Sheets values and the Slides read, on `fake`.
    pub fn install(fake: &Fake) -> Drive {
        let d = Drive {
            state: Arc::default(),
        };
        let on = |verb: &str, path: &str, f: fn(&Drive, &Req) -> Reply| {
            let me = d.clone();
            fake.route(verb, path, move |r| f(&me, r));
        };
        on("GET", "/drive/v3/files", Drive::list);
        on("GET", "/drive/v3/files/*", Drive::get);
        on("GET", "/drive/v3/files/*/export", Drive::export);
        on("POST", "/drive/v3/files", Drive::create);
        on("POST", "/upload/drive/v3/files", Drive::upload_new);
        on("PATCH", "/upload/drive/v3/files/*", Drive::upload_over);
        on("PATCH", "/drive/v3/files/*", Drive::update);
        on("GET", "/drive/v3/files/*/permissions", Drive::permissions);
        on("POST", "/drive/v3/files/*/permissions", Drive::grant);
        on("DELETE", "/drive/v3/files/*/permissions/*", Drive::revoke);
        on("GET", "/v4/spreadsheets/*", Drive::tabs);
        on("GET", "/v4/spreadsheets/*/values:batchGet", Drive::values);
        on("GET", "/v1/presentations/*", Drive::presentation);
        d
    }

    pub fn add(&self, f: File) {
        self.state.lock().unwrap().files.push(f);
    }

    pub fn file(&self, id: &str) -> Option<File> {
        self.state
            .lock()
            .unwrap()
            .files
            .iter()
            .find(|f| f.id == id)
            .cloned()
    }

    pub fn files(&self) -> Vec<File> {
        self.state.lock().unwrap().files.clone()
    }

    fn with<R>(&self, id: &str, f: impl FnOnce(&mut File) -> R) -> Option<R> {
        self.state
            .lock()
            .unwrap()
            .files
            .iter_mut()
            .find(|x| x.id == id)
            .map(f)
    }

    fn list(&self, _: &Req) -> Reply {
        let files: Vec<Value> = self
            .files()
            .iter()
            .filter(|f| !f.trashed)
            .map(File::meta)
            .collect();
        Reply::json(200, json!({"files": files}))
    }

    fn get(&self, r: &Req) -> Reply {
        let id = seg(r, 4);
        let media = query(r, "alt").as_deref() == Some("media");
        match self.with(&id, |f| (f.meta(), f.content.clone(), f.mime.clone())) {
            Some((_, Content::Bytes(b), mime)) if media => Reply::raw(&mime, b),
            Some((_, _, _)) if media => bad("only a blob file downloads"),
            Some((meta, ..)) => Reply::json(200, meta),
            None => not_found(),
        }
    }

    fn export(&self, r: &Req) -> Reply {
        if query(r, "mimeType").as_deref() != Some("text/markdown") {
            return bad("this fake exports markdown only");
        }
        match self.with(&seg(r, 4), |f| (f.mime == DOC).then(|| f.content.clone())) {
            Some(Some(Content::Bytes(b))) => Reply::raw("text/markdown", b),
            Some(_) => bad("only a Doc exports"),
            None => not_found(),
        }
    }

    fn make(&self, name: &str, mime: &str, parents: Vec<String>, content: Content) -> File {
        let mut st = self.state.lock().unwrap();
        st.made += 1;
        let mut f = File::new(&format!("new{}", st.made), name, mime, content);
        if !parents.is_empty() {
            f.parents = parents;
        }
        st.files.push(f.clone());
        f
    }

    fn parents(meta: &Value) -> Vec<String> {
        meta["parents"]
            .as_array()
            .map(|p| {
                p.iter()
                    .filter_map(|p| p.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn create(&self, r: &Req) -> Reply {
        let m = r.json();
        let mime = m["mimeType"].as_str().unwrap_or("application/octet-stream");
        let content = match mime {
            SHEET => Content::Sheet(vec![("Sheet1".into(), Vec::new())]),
            SLIDES => Content::Slides(Vec::new()),
            _ => Content::Bytes(Vec::new()),
        };
        let f = self.make(
            m["name"].as_str().unwrap_or(""),
            mime,
            Self::parents(&m),
            content,
        );
        Reply::json(200, f.meta())
    }

    fn upload_new(&self, r: &Req) -> Reply {
        let p = parts(r);
        let [(_, meta), (media, bytes)] = p.as_slice() else {
            return bad("a multipart upload has a metadata part and a media part");
        };
        let m: Value = serde_json::from_slice(meta).unwrap_or(Value::Null);
        let mime = m["mimeType"].as_str().map_or(media.as_str(), |s| s);
        let content = if mime == SHEET {
            csv(bytes)
        } else {
            Content::Bytes(bytes.clone())
        };
        let f = self.make(
            m["name"].as_str().unwrap_or(""),
            mime,
            Self::parents(&m),
            content,
        );
        Reply::json(200, f.meta())
    }

    fn upload_over(&self, r: &Req) -> Reply {
        let bytes = r.body.clone();
        // As real Drive converts: a native Doc takes markdown and a Sheet CSV;
        // any other type would land as literal text, so it is refused.
        let sent = r
            .headers
            .get("content-type")
            .map_or("", |t| t.split(';').next().unwrap_or("").trim());
        let want = self
            .with(&seg(r, 5), |f| match f.mime.as_str() {
                DOC => Some("text/markdown"),
                SHEET => Some("text/csv"),
                _ => None,
            })
            .flatten();
        if let Some(want) = want.filter(|w| *w != sent) {
            return bad(&format!(
                "a replace of this file takes {want}, not {sent:?}"
            ));
        }
        let reply = self.with(&seg(r, 5), |f| {
            f.content = if f.mime == SHEET {
                csv(&bytes)
            } else {
                Content::Bytes(bytes)
            };
            f.meta()
        });
        reply.map_or_else(not_found, |m| Reply::json(200, m))
    }

    fn update(&self, r: &Req) -> Reply {
        let body = r.json();
        let (add, remove) = (query(r, "addParents"), query(r, "removeParents"));
        let reply = self.with(&seg(r, 4), |f| {
            if let Some(n) = body["name"].as_str() {
                f.name = n.into();
            }
            if let Some(t) = body["trashed"].as_bool() {
                f.trashed = t;
            }
            if let Some(rm) = &remove {
                f.parents.retain(|p| !rm.split(',').any(|x| x == p));
            }
            if let Some(a) = add {
                f.parents.push(a);
            }
            f.meta()
        });
        reply.map_or_else(not_found, |m| Reply::json(200, m))
    }

    fn permissions(&self, r: &Req) -> Reply {
        let list = self.with(&seg(r, 4), |f| f.permissions.clone());
        list.map_or_else(not_found, |p| {
            let p: Vec<Value> = p
                .iter()
                .map(|p| json!({"id": p.id, "type": p.kind, "emailAddress": p.email, "role": p.role}))
                .collect();
            Reply::json(200, json!({"permissions": p}))
        })
    }

    fn grant(&self, r: &Req) -> Reply {
        let b = r.json();
        let made = self.with(&seg(r, 4), |f| {
            let p = Permission {
                id: format!("perm{}", f.permissions.len() + 1),
                kind: b["type"].as_str().unwrap_or("user").into(),
                email: b["emailAddress"].as_str().map(String::from),
                role: b["role"].as_str().unwrap_or("reader").into(),
            };
            f.shared = true;
            f.permissions.push(p.clone());
            p
        });
        made.map_or_else(not_found, |p| {
            Reply::json(200, json!({"id": p.id, "type": p.kind, "role": p.role}))
        })
    }

    fn revoke(&self, r: &Req) -> Reply {
        let pid = seg(r, 6);
        let gone = self.with(&seg(r, 4), |f| {
            let n = f.permissions.len();
            f.permissions.retain(|p| p.id != pid);
            f.shared = !f.permissions.is_empty();
            n != f.permissions.len()
        });
        match gone {
            Some(true) => Reply::raw("application/json", Vec::new()),
            _ => not_found(),
        }
    }

    fn tabs(&self, r: &Req) -> Reply {
        match self.with(&seg(r, 3), |f| f.content.clone()) {
            Some(Content::Sheet(tabs)) => {
                let sheets: Vec<Value> = tabs
                    .iter()
                    .map(|(t, rows)| {
                        let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
                        json!({"properties": {"title": t, "gridProperties": {"rowCount": rows.len(), "columnCount": cols}}})
                    })
                    .collect();
                Reply::json(200, json!({"sheets": sheets}))
            }
            _ => not_found(),
        }
    }

    /// The first tab's rows up to the row the range ends at, or the tab named
    /// in a `Tab!A1:B2` range with every row (the fake keeps no cell ranges).
    fn values(&self, r: &Req) -> Reply {
        let range = query(r, "ranges").unwrap_or_default();
        let tab = range
            .split('!')
            .next()
            .unwrap_or("")
            .trim_matches('\'')
            .replace("''", "'");
        let upto: usize = range
            .rsplit([':', '!'])
            .next()
            .and_then(|e| {
                e.trim_start_matches(|c: char| c.is_ascii_alphabetic())
                    .parse()
                    .ok()
            })
            .unwrap_or(usize::MAX);
        match self.with(&seg(r, 3), |f| f.content.clone()) {
            Some(Content::Sheet(tabs)) => {
                let rows = tabs
                    .iter()
                    .find(|(t, _)| *t == tab)
                    .or(tabs.first())
                    .map(|(_, rows)| rows.clone());
                let rows: Vec<&Vec<String>> = rows.iter().flatten().take(upto).collect();
                Reply::json(
                    200,
                    json!({"valueRanges": [{"range": range, "values": rows}]}),
                )
            }
            _ => not_found(),
        }
    }

    fn presentation(&self, r: &Req) -> Reply {
        match self.with(&seg(r, 3), |f| f.content.clone()) {
            Some(Content::Slides(slides)) => {
                let slides: Vec<Value> = slides
                    .iter()
                    .map(|(id, els)| {
                        let els: Vec<Value> = els
                            .iter()
                            .map(|(eid, t)| {
                                json!({"objectId": eid,
                                    "shape": {"text": {"textElements": [{"textRun": {"content": t}}]}}})
                            })
                            .collect();
                        json!({"objectId": id, "slideProperties": {"layoutObjectId": "layout1"}, "pageElements": els})
                    })
                    .collect();
                Reply::json(
                    200,
                    json!({"slides": slides, "layouts": [{"objectId": "layout1", "layoutProperties": {"displayName": "TITLE_AND_BODY"}}]}),
                )
            }
            _ => not_found(),
        }
    }
}
