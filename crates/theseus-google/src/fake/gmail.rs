//! The fake's Gmail (the mail row's stubs): a mailbox of threads and drafts
//! that answers the REST calls the typed mail tools make, and keeps what was
//! sent, so a test sets up a thread, drives a tool, and reads what Gmail
//! holds after. It reads the MIME a draft is made of, so threading headers
//! and encoded subjects come back as a client would see them.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use serde_json::{json, Value};

use super::Reply;
use crate::http1::Req;

/// A message a test puts in a thread.
#[derive(Debug, Clone, Default)]
pub struct MsgSpec {
    pub id: String,
    pub headers: Vec<(String, String)>,
    pub text: Option<String>,
    pub html: Option<String>,
    pub labels: Vec<String>,
    /// Name, type and bytes.
    pub files: Vec<(String, String, Vec<u8>)>,
}

impl MsgSpec {
    pub fn new(id: &str, from: &str, subject: &str, text: &str) -> Self {
        let mut m = Self {
            id: id.into(),
            text: Some(text.into()),
            labels: vec!["INBOX".into()],
            ..Default::default()
        };
        m.headers = vec![
            ("From".into(), from.into()),
            ("Subject".into(), subject.into()),
            ("Date".into(), "Mon, 19 Oct 2026 09:00:00 -0700".into()),
            ("Message-ID".into(), format!("<{id}@mail.example.test>")),
        ];
        m
    }

    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.retain(|(n, _)| !n.eq_ignore_ascii_case(name));
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn html(mut self, html: &str) -> Self {
        self.html = Some(html.into());
        self.text = None;
        self
    }

    pub fn label(mut self, l: &str) -> Self {
        self.labels.push(l.into());
        self
    }

    pub fn file(mut self, name: &str, mime: &str, bytes: &[u8]) -> Self {
        self.files.push((name.into(), mime.into(), bytes.to_vec()));
        self
    }
}

#[derive(Debug, Clone, Default)]
struct Msg {
    spec: MsgSpec,
    thread: String,
}

#[derive(Default)]
struct State {
    threads: BTreeMap<String, Vec<Msg>>,
    drafts: BTreeMap<String, (Msg, String)>,
    sent: Vec<Msg>,
    labels: Vec<(String, String)>,
    next: u64,
    /// After a send is recorded, answer this instead (a connection that
    /// dropped: the client cannot know it went).
    after_send: Option<Reply>,
}

/// The mailbox, shared between the fake's handler and the test.
#[derive(Clone, Default)]
pub struct Mailbox(Arc<Mutex<State>>);

fn b64(s: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(s)
}

impl Mailbox {
    fn with<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        f(&mut self.0.lock().unwrap())
    }

    /// A thread of messages, in order.
    pub fn thread(&self, id: &str, msgs: Vec<MsgSpec>) {
        self.with(|s| {
            let v = msgs
                .into_iter()
                .map(|spec| Msg {
                    spec,
                    thread: id.into(),
                })
                .collect();
            s.threads.insert(id.into(), v);
        });
    }

    /// A user label.
    pub fn label(&self, id: &str, name: &str) {
        self.with(|s| s.labels.push((id.into(), name.into())));
    }

    /// A draft the owner made in Gmail itself.
    pub fn put_draft(&self, id: &str, msg: MsgSpec) {
        self.with(|s| {
            s.drafts.insert(
                id.into(),
                (
                    Msg {
                        spec: msg,
                        thread: String::new(),
                    },
                    String::new(),
                ),
            );
        });
    }

    /// Change a draft's body behind the tool's back.
    pub fn edit_draft(&self, id: &str, text: &str) {
        self.with(|s| {
            if let Some((m, _)) = s.drafts.get_mut(id) {
                m.spec.text = Some(text.into());
            }
        });
    }

    /// The draft's headers as Gmail stored them.
    pub fn draft_headers(&self, id: &str) -> Vec<(String, String)> {
        self.with(|s| {
            s.drafts
                .get(id)
                .map(|(m, _)| m.spec.headers.clone())
                .unwrap_or_default()
        })
    }

    /// The draft's thread id, and its text.
    pub fn draft_text(&self, id: &str) -> Option<(String, String)> {
        self.with(|s| {
            s.drafts
                .get(id)
                .map(|(m, _)| (m.thread.clone(), m.spec.text.clone().unwrap_or_default()))
        })
    }

    pub fn draft_ids(&self) -> Vec<String> {
        self.with(|s| s.drafts.keys().cloned().collect())
    }

    /// The messages sent: each one's headers and text.
    pub fn sent(&self) -> Vec<(Vec<(String, String)>, String)> {
        self.with(|s| {
            s.sent
                .iter()
                .map(|m| {
                    (
                        m.spec.headers.clone(),
                        m.spec.text.clone().unwrap_or_default(),
                    )
                })
                .collect()
        })
    }

    /// The labels on a message of a thread.
    pub fn labels_of(&self, message: &str) -> Vec<String> {
        self.with(|s| {
            s.threads
                .values()
                .flatten()
                .find(|m| m.spec.id == message)
                .map(|m| m.spec.labels.clone())
                .unwrap_or_default()
        })
    }

    /// A message already sent to `addr`, for "never mailed before".
    pub fn sent_to(&self, addr: &str, thread: &str) {
        let spec = MsgSpec::new(
            &format!("old-{addr}"),
            "owner@example.test",
            "Earlier",
            "Hello.",
        )
        .header("To", addr)
        .label("SENT");
        self.with(|s| {
            s.sent.push(Msg {
                spec,
                thread: thread.into(),
            })
        });
    }

    /// After a send is recorded, answer `reply` (a dropped connection, as the
    /// client sees it).
    pub fn after_send(&self, reply: Reply) {
        self.with(|s| s.after_send = Some(reply));
    }
}

// ------------------------------------------------------------------ JSON

fn payload(m: &Msg, headers: Option<&[String]>) -> Value {
    let hs: Vec<Value> = m
        .spec
        .headers
        .iter()
        .filter(|(n, _)| headers.is_none_or(|w| w.iter().any(|h| h.eq_ignore_ascii_case(n))))
        .map(|(n, v)| json!({"name": n, "value": v}))
        .collect();
    if headers.is_some() {
        return json!({"headers": hs});
    }
    let mut parts = Vec::new();
    if let Some(t) = &m.spec.text {
        parts.push(json!({"mimeType": "text/plain", "filename": "", "body": {"size": t.len(), "data": b64(t.as_bytes())}}));
    }
    if let Some(h) = &m.spec.html {
        parts.push(json!({"mimeType": "text/html", "filename": "", "body": {"size": h.len(), "data": b64(h.as_bytes())}}));
    }
    for (i, (name, mime, bytes)) in m.spec.files.iter().enumerate() {
        parts.push(json!({"mimeType": mime, "filename": name,
            "body": {"size": bytes.len(), "attachmentId": format!("att-{}-{i}", m.spec.id)}}));
    }
    json!({"headers": hs, "mimeType": "multipart/mixed", "parts": parts})
}

fn message(m: &Msg, headers: Option<&[String]>) -> Value {
    json!({"id": m.spec.id, "threadId": m.thread, "labelIds": m.spec.labels,
        "snippet": m.spec.text.clone().or(m.spec.html.clone()).unwrap_or_default().chars().take(60).collect::<String>(),
        "payload": payload(m, headers)})
}

fn wanted(req: &Req) -> Option<Vec<String>> {
    let q = req.query();
    (q.iter().any(|(k, v)| k == "format" && v == "metadata")).then(|| {
        q.into_iter()
            .filter(|(k, _)| k == "metadataHeaders")
            .map(|(_, v)| v)
            .collect()
    })
}

// ------------------------------------------------------------------ MIME in

/// A draft's `raw` read as a client's own MIME: headers (unfolded, words
/// left encoded) and the text.
fn parse_raw(raw: &str) -> (Vec<(String, String)>, String) {
    let (head, body) = raw.split_once("\r\n\r\n").unwrap_or((raw, ""));
    let mut headers: Vec<(String, String)> = Vec::new();
    for line in head.split("\r\n") {
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(last) = headers.last_mut() {
                last.1.push_str(line);
            }
        } else if let Some((n, v)) = line.split_once(':') {
            headers.push((n.trim().into(), v.trim().into()));
        }
    }
    let b64 = headers
        .iter()
        .any(|(n, v)| n.eq_ignore_ascii_case("content-transfer-encoding") && v == "base64");
    let text = if b64 {
        let joined: String = body.split("\r\n").collect();
        String::from_utf8_lossy(&STANDARD.decode(joined).unwrap_or_default()).into_owned()
    } else {
        body.to_string()
    };
    (headers, text)
}

// ------------------------------------------------------------------ routes

fn not_found() -> Reply {
    Reply::json(
        404,
        json!({"error": {"code": 404, "message": "Requested entity was not found.", "errors": [{"reason": "notFound"}]}}),
    )
}

/// Answer a request under `/gmail/v1/users/me/`, or None for a path this
/// fake does not stub.
pub fn answer(mb: &Mailbox, req: &Req) -> Option<Reply> {
    let path = req.path().strip_prefix("/gmail/v1/users/me/")?;
    let segs: Vec<&str> = path.split('/').collect();
    let method = req.method.as_str();
    mb.with(|s| {
        Some(match (method, segs.as_slice()) {
            ("GET", ["threads"]) => list_threads(s, req),
            ("GET", ["threads", id]) => match s.threads.get(*id) {
                Some(msgs) => {
                    let h = wanted(req);
                    Reply::json(
                        200,
                        json!({"id": id, "messages": msgs.iter().map(|m| message(m, h.as_deref())).collect::<Vec<_>>()}),
                    )
                }
                None => not_found(),
            },
            ("GET", ["messages"]) => list_sent(s, req),
            ("GET", ["messages", id]) => match find(s, id) {
                Some(m) => Reply::json(200, message(&m, wanted(req).as_deref())),
                None => not_found(),
            },
            ("GET", ["messages", id, "attachments", aid]) => {
                let hit = find(s, id).and_then(|m| {
                    m.spec.files.iter().enumerate().find_map(|(i, (_, _, bytes))| {
                        (*aid == format!("att-{id}-{i}")).then(|| bytes.clone())
                    })
                });
                match hit {
                    Some(b) => Reply::json(200, json!({"size": b.len(), "data": b64(&b)})),
                    None => not_found(),
                }
            }
            ("GET", ["labels"]) => {
                let mut l: Vec<Value> = ["INBOX", "UNREAD", "STARRED"]
                    .iter()
                    .map(|n| json!({"id": n, "name": n, "type": "system"}))
                    .collect();
                l.extend(s.labels.iter().map(|(i, n)| json!({"id": i, "name": n, "type": "user"})));
                Reply::json(200, json!({"labels": l}))
            }
            ("POST", ["drafts"]) => put_draft(s, None, req),
            ("PUT", ["drafts", id]) => put_draft(s, Some(id), req),
            ("GET", ["drafts", id]) => match s.drafts.get(*id) {
                Some((m, _)) => Reply::json(200, json!({"id": id, "message": message(m, None)})),
                None => not_found(),
            },
            ("POST", ["drafts", "send"]) => send(s, req),
            ("POST", ["threads", id, op @ ("modify" | "trash")]) => {
                let (add, remove) = labels_of_op(op, req);
                match s.threads.get_mut(*id) {
                    Some(msgs) => {
                        msgs.iter_mut().for_each(|m| relabel(&mut m.spec.labels, &add, &remove));
                        Reply::json(200, json!({"id": id}))
                    }
                    None => not_found(),
                }
            }
            ("POST", ["messages", id, op @ ("modify" | "trash")]) => {
                let (add, remove) = labels_of_op(op, req);
                let hit = s
                    .threads
                    .values_mut()
                    .flatten()
                    .find(|m| m.spec.id == *id);
                match hit {
                    Some(m) => {
                        relabel(&mut m.spec.labels, &add, &remove);
                        Reply::json(200, json!({"id": id}))
                    }
                    None => not_found(),
                }
            }
            _ => return None,
        })
    })
}

fn find(s: &State, id: &str) -> Option<Msg> {
    s.threads
        .values()
        .flatten()
        .chain(s.sent.iter())
        .chain(s.drafts.values().map(|(m, _)| m))
        .find(|m| m.spec.id == id)
        .cloned()
}

fn labels_of_op(op: &str, req: &Req) -> (Vec<String>, Vec<String>) {
    if op == "trash" {
        return (vec!["TRASH".into()], vec!["INBOX".into()]);
    }
    let b = req.json();
    let list = |k: &str| -> Vec<String> {
        b[k].as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(String::from))
            .collect()
    };
    (list("addLabelIds"), list("removeLabelIds"))
}

fn relabel(labels: &mut Vec<String>, add: &[String], remove: &[String]) {
    labels.retain(|l| !remove.contains(l));
    for a in add {
        if !labels.contains(a) {
            labels.push(a.clone());
        }
    }
}

fn list_threads(s: &State, req: &Req) -> Reply {
    let q = req.query();
    let get = |k: &str| q.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    let needle = get("q").unwrap_or_default().to_lowercase();
    let max: usize = get("maxResults").and_then(|v| v.parse().ok()).unwrap_or(20);
    let from: usize = get("pageToken").and_then(|v| v.parse().ok()).unwrap_or(0);
    let hits: Vec<(&String, &Vec<Msg>)> = s
        .threads
        .iter()
        .filter(|(_, msgs)| {
            needle.is_empty()
                || msgs.iter().any(|m| {
                    let hay = format!(
                        "{} {}",
                        m.spec.text.clone().unwrap_or_default(),
                        m.spec
                            .headers
                            .iter()
                            .map(|(_, v)| v.as_str())
                            .collect::<Vec<_>>()
                            .join(" ")
                    );
                    hay.to_lowercase().contains(&needle)
                })
        })
        .collect();
    let page: Vec<Value> = hits
        .iter()
        .skip(from)
        .take(max)
        .map(|(id, msgs)| {
            json!({"id": id, "snippet": msgs[0].spec.text.clone().unwrap_or_default().chars().take(60).collect::<String>()})
        })
        .collect();
    let mut body = json!({"resultSizeEstimate": hits.len()});
    if !page.is_empty() {
        body["threads"] = page.into();
    }
    if from + max < hits.len() {
        body["nextPageToken"] = (from + max).to_string().into();
    }
    Reply::json(200, body)
}

/// `in:sent (to:"a" OR …)`: a message sent that names the address.
fn list_sent(s: &State, req: &Req) -> Reply {
    let q = req
        .query()
        .into_iter()
        .find(|(k, _)| k == "q")
        .map(|(_, v)| v)
        .unwrap_or_default();
    let addr = q.split('"').nth(1).unwrap_or("").to_lowercase();
    let hit = s.sent.iter().find(|m| {
        m.spec.headers.iter().any(|(n, v)| {
            ["to", "cc", "bcc"].contains(&n.to_lowercase().as_str())
                && v.to_lowercase().contains(&addr)
        })
    });
    match hit {
        Some(m) => Reply::json(
            200,
            json!({"messages": [{"id": m.spec.id, "threadId": m.thread}], "resultSizeEstimate": 1}),
        ),
        None => Reply::json(200, json!({"resultSizeEstimate": 0})),
    }
}

fn put_draft(s: &mut State, id: Option<&str>, req: &Req) -> Reply {
    let body = req.json();
    let raw = body["message"]["raw"].as_str().unwrap_or("");
    let bytes = URL_SAFE_NO_PAD.decode(raw).unwrap_or_default();
    let (headers, text) = parse_raw(&String::from_utf8_lossy(&bytes));
    s.next += 1;
    let draft_id = id.map_or_else(|| format!("r{}", s.next), String::from);
    let thread = body["message"]["threadId"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let spec = MsgSpec {
        id: format!("dm{}", s.next),
        headers,
        text: Some(text),
        labels: vec!["DRAFT".into()],
        ..Default::default()
    };
    let m = Msg {
        spec,
        thread: thread.clone(),
    };
    let out = json!({"id": draft_id, "message": {"id": m.spec.id, "threadId": thread}});
    s.drafts.insert(draft_id, (m, raw.to_string()));
    Reply::json(200, out)
}

fn send(s: &mut State, req: &Req) -> Reply {
    let id = req.json()["id"].as_str().unwrap_or("").to_string();
    let Some((mut m, _)) = s.drafts.remove(&id) else {
        return not_found();
    };
    m.spec.labels = vec!["SENT".into()];
    let out = json!({"id": m.spec.id, "threadId": m.thread, "labelIds": ["SENT"]});
    s.sent.push(m);
    s.after_send
        .clone()
        .unwrap_or_else(|| Reply::json(200, out))
}
