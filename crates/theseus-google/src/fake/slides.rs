//! The fake's Slides (gw-sheets-slides): decks that keep their slides and
//! their text. `batchUpdate` applies `createSlide` (placeholders named in
//! `placeholderIdMappings` become shapes), `deleteObject` of a slide,
//! `insertText`, `deleteText` of all, and `replaceAllText`; any other request
//! is accepted and answered with `{}`, so raw requests pass through.

use std::collections::BTreeMap;

use percent_encoding::percent_decode_str;
use serde_json::{json, Value};

use super::Reply;
use crate::http1::Req;

#[derive(Default)]
pub struct State {
    decks: BTreeMap<String, Deck>,
    made: u32,
}

#[derive(Clone)]
struct Deck {
    title: String,
    slides: Vec<String>,
    /// Each shape's text, by object id.
    text: BTreeMap<String, String>,
}

fn url(id: &str) -> String {
    format!("https://docs.google.com/presentation/d/{id}/edit")
}

fn describe(id: &str, d: &Deck) -> Value {
    let slides: Vec<Value> = d
        .slides
        .iter()
        .map(|s| {
            json!({"objectId": s, "slideProperties": {"notesPage": {"notesProperties":
                {"speakerNotesObjectId": format!("{s}_notes")}}}})
        })
        .collect();
    json!({"presentationId": id, "title": d.title, "slides": slides,
        "revisionId": "r1", "presentationUrl": url(id)})
}

fn bad(message: String) -> Reply {
    Reply::json(
        400,
        json!({"error": {"code": 400, "message": message, "errors": [{"reason": "badRequest"}]}}),
    )
}

/// One request of a batch; the reply, or why it is refused.
fn apply(d: &mut Deck, r: &Value) -> Result<Value, String> {
    let Some((kind, a)) = r.as_object().and_then(|o| o.iter().next()) else {
        return Err("a request names one operation".into());
    };
    let object = a["objectId"].as_str().unwrap_or("").to_string();
    match kind.as_str() {
        "createSlide" => {
            if object.is_empty() || d.slides.contains(&object) {
                return Err(format!("the object id `{object}` is empty or taken"));
            }
            for m in a["placeholderIdMappings"].as_array().into_iter().flatten() {
                if let Some(shape) = m["objectId"].as_str() {
                    d.text.insert(shape.to_string(), String::new());
                }
            }
            d.slides.push(object.clone());
            Ok(json!({"createSlide": {"objectId": object}}))
        }
        "deleteObject" => {
            let before = d.slides.len() + d.text.len();
            d.slides.retain(|s| *s != object);
            d.text.remove(&object);
            if d.slides.len() + d.text.len() == before {
                return Err(format!("no object `{object}`"));
            }
            Ok(json!({}))
        }
        "insertText" => {
            let Some(t) = d.text.get_mut(&object) else {
                return Err(format!("no shape `{object}` to insert text into"));
            };
            t.push_str(a["text"].as_str().unwrap_or(""));
            Ok(json!({}))
        }
        "deleteText" => {
            let Some(t) = d.text.get_mut(&object) else {
                return Err(format!("no shape `{object}` to delete text from"));
            };
            t.clear();
            Ok(json!({}))
        }
        "replaceAllText" => {
            let find = a["containsText"]["text"].as_str().unwrap_or("");
            let with = a["replaceText"].as_str().unwrap_or("");
            let mut n = 0;
            for t in d.text.values_mut().filter(|_| !find.is_empty()) {
                n += t.matches(find).count();
                *t = t.replace(find, with);
            }
            Ok(json!({"replaceAllText": {"occurrencesChanged": n}}))
        }
        _ => Ok(json!({})),
    }
}

pub fn answer(st: &mut State, req: &Req) -> Option<Reply> {
    let path = percent_decode_str(req.path())
        .decode_utf8_lossy()
        .into_owned();
    let rest = path.strip_prefix("/v1/presentations")?;
    if rest.is_empty() {
        if req.method != "POST" {
            return None;
        }
        st.made += 1;
        let id = format!("deck-{}", st.made);
        let title = req.json()["title"]
            .as_str()
            .unwrap_or("Untitled presentation")
            .to_string();
        // A new deck starts with one empty title slide.
        let first = format!("p{}", st.made);
        let mut d = Deck {
            title,
            slides: vec![first.clone()],
            text: BTreeMap::new(),
        };
        d.text.insert(format!("{first}_notes"), String::new());
        let reply = describe(&id, &d);
        st.decks.insert(id, d);
        return Some(Reply::json(200, reply));
    }
    let rest = rest.strip_prefix('/')?;
    let (id, action) = match rest.split_once(':') {
        Some((id, a)) => (id, a),
        None => (rest, ""),
    };
    let Some(deck) = st.decks.get_mut(id) else {
        return Some(Reply::json(
            404,
            json!({"error": {"code": 404, "message": format!("Requested entity was not found: {id}"),
                "errors": [{"reason": "notFound"}]}}),
        ));
    };
    Some(match (req.method.as_str(), action) {
        ("GET", "") => Reply::json(200, describe(id, deck)),
        ("POST", "batchUpdate") => {
            let mut replies = Vec::new();
            // A batch is atomic: it lands whole or not at all.
            let mut work = deck.clone();
            for r in req.json()["requests"].as_array().into_iter().flatten() {
                match apply(&mut work, r) {
                    Ok(v) => replies.push(v),
                    Err(m) => {
                        return Some(bad(format!("Invalid requests[{}]: {m}", replies.len())))
                    }
                }
            }
            // Every slide a batch creates has its notes shape.
            for s in &work.slides {
                work.text.entry(format!("{s}_notes")).or_default();
            }
            *deck = work;
            Reply::json(200, json!({"presentationId": id, "replies": replies}))
        }
        _ => return None,
    })
}
