//! The calendar's stubs (contract 6): a create echoes the event it was given,
//! as an event the account organizes; a change echoes the patch for the id in
//! the path; a delete answers 204. A list answers empty until a test gives it
//! a body (`Fake::json("GET", "/calendar/v3/calendars/primary/events", …)`).

use serde_json::{json, Value};

use super::Reply;
use crate::http1::Req;

const EVENTS: &str = "/calendar/v3/calendars/primary/events";

/// The answer for a calendar request nothing else claimed, or None.
pub(super) fn stub(req: &Req, account: &str) -> Option<Reply> {
    let organizer = json!({"email": account, "self": true});
    let event = |id: &str| {
        let mut e = req.json();
        e["id"] = id.into();
        e["status"] = "confirmed".into();
        e["organizer"] = organizer.clone();
        e
    };
    match (req.method.as_str(), req.path()) {
        ("GET", EVENTS) => Some(Reply::json(200, json!({"items": []}))),
        ("POST", EVENTS) => Some(Reply::json(200, event("evt1"))),
        (method, path) => {
            let id = path.strip_prefix(EVENTS)?.strip_prefix('/')?;
            match method {
                "PATCH" | "PUT" => Some(Reply::json(200, event(id))),
                "DELETE" => Some(Reply::json(204, Value::Null)),
                _ => None,
            }
        }
    }
}
