//! The application's emojis for the stand-in (theseus-ss36w): listed, added
//! (a name once, an image of at most Discord's 256 KB) and deleted, at
//! `/applications/<id>/emojis`, the way the binding keeps the bot's own task
//! glyphs. `refuse_app_emojis` fails every call (a Discord that says no), and
//! a test deletes one to see it uploaded again.

use std::net::TcpStream;

use serde_json::{json, Value};

use super::{reply, reply_empty, seen, FakeDiscord};

/// Discord's limit on an emoji's image.
pub const MAX_IMAGE_BYTES: usize = 256 * 1024;

/// An emoji the application holds.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct FakeEmoji {
    pub id: u64,
    pub name: String,
    pub animated: bool,
    /// The image's size, decoded.
    pub bytes: usize,
}

/// What the stand-in keeps of them.
#[derive(Default)]
pub(super) struct Emojis {
    list: Vec<FakeEmoji>,
    refuse: bool,
}

fn json_of(e: &FakeEmoji) -> Value {
    json!({"id": e.id.to_string(), "name": e.name, "animated": e.animated, "available": true,
           "managed": false, "require_colons": true, "roles": []})
}

fn bad(code: u32, message: &str) -> Value {
    json!({"message": message, "code": code})
}

/// The decoded size and kind of a `data:<mime>;base64,<data>` URI, or none
/// when it is not one.
fn image_of(uri: &str) -> Option<(usize, bool)> {
    let rest = uri.strip_prefix("data:")?;
    let (mime, data) = rest.split_once(";base64,")?;
    let pad = data.bytes().rev().take_while(|b| *b == b'=').count();
    Some((
        (data.len() / 4 * 3).saturating_sub(pad),
        mime == "image/gif",
    ))
}

impl FakeDiscord {
    /// The emojis the application holds, in the order they were added.
    pub fn app_emojis(&self) -> Vec<FakeEmoji> {
        self.state.lock().unwrap().emojis.list.clone()
    }

    /// Every emoji call answers 403 while set.
    pub fn refuse_app_emojis(&self, refuse: bool) {
        self.state.lock().unwrap().emojis.refuse = refuse;
    }

    /// Someone deleted the emoji named `name` in the developer portal.
    pub fn drop_app_emoji(&self, name: &str) {
        self.state
            .lock()
            .unwrap()
            .emojis
            .list
            .retain(|e| e.name != name);
    }

    /// `GET`, `POST` and `DELETE` under `/applications/<id>/emojis`.
    pub(super) fn emoji_route(
        &self,
        stream: TcpStream,
        (method, route): (&str, &str),
        id: Option<&str>,
        body: &Value,
    ) -> std::io::Result<()> {
        if self.state.lock().unwrap().emojis.refuse {
            self.record(seen(method, route, "refused"));
            return reply(stream, 403, &bad(50013, "Missing Permissions"));
        }
        match (method, id) {
            ("GET", None) => {
                self.record(seen(method, route, "listed"));
                let items: Vec<Value> = self.app_emojis().iter().map(json_of).collect();
                reply(stream, 200, &json!({ "items": items }))
            }
            ("POST", None) => {
                let name = body["name"].as_str().unwrap_or("").to_string();
                let Some((bytes, animated)) = body["image"].as_str().and_then(image_of) else {
                    self.record(seen(method, route, "bad_image"));
                    return reply(stream, 400, &bad(50035, "Invalid Form Body"));
                };
                let dup = self.app_emojis().iter().any(|e| e.name == name);
                let named = (2..=32).contains(&name.len())
                    && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
                if bytes > MAX_IMAGE_BYTES || dup || !named {
                    self.record(seen(method, route, "too_big_or_bad_name"));
                    return reply(stream, 400, &bad(50035, "Invalid Form Body"));
                }
                let e = FakeEmoji {
                    id: self.next_big(),
                    name,
                    animated,
                    bytes,
                };
                self.state.lock().unwrap().emojis.list.push(e.clone());
                self.record(seen(method, route, "created"));
                reply(stream, 201, &json_of(&e))
            }
            ("DELETE", Some(id)) => {
                let gone = {
                    let mut st = self.state.lock().unwrap();
                    let n = st.emojis.list.len();
                    st.emojis.list.retain(|e| e.id.to_string() != id);
                    st.emojis.list.len() != n
                };
                self.record(seen(
                    method,
                    route,
                    if gone { "deleted" } else { "unknown" },
                ));
                if gone {
                    reply_empty(stream)
                } else {
                    reply(stream, 404, &bad(10014, "Unknown Emoji"))
                }
            }
            _ => reply(stream, 405, &bad(0, "405: Method Not Allowed")),
        }
    }
}
