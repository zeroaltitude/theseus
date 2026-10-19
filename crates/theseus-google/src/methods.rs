//! The methods `google.call` knows by name: each id with its HTTP verb and
//! path template. Gmail, Drive, Sheets, Slides, Docs and Calendar's own REST
//! (their discovery documents' `id`, `httpMethod` and `path`), the ones a
//! person or an agent reaches for. Any other call is made by its verb and
//! path (`"POST /gmail/v1/users/me/labels"`), classified the same way.

use serde_json::{Map, Value};

use crate::classify::classify;
use crate::client::{Body, Request};
use crate::scopes::Service;

/// `(id, verb, path)`. A `{name}` in a path is filled from `params`, or is
/// `me` for `{userId}`.
pub const TABLE: &[(&str, &str, &str)] = &[
    // Gmail
    (
        "gmail.users.getProfile",
        "GET",
        "/gmail/v1/users/{userId}/profile",
    ),
    (
        "gmail.users.messages.list",
        "GET",
        "/gmail/v1/users/{userId}/messages",
    ),
    (
        "gmail.users.messages.get",
        "GET",
        "/gmail/v1/users/{userId}/messages/{id}",
    ),
    (
        "gmail.users.messages.modify",
        "POST",
        "/gmail/v1/users/{userId}/messages/{id}/modify",
    ),
    (
        "gmail.users.messages.batchModify",
        "POST",
        "/gmail/v1/users/{userId}/messages/batchModify",
    ),
    (
        "gmail.users.messages.batchDelete",
        "POST",
        "/gmail/v1/users/{userId}/messages/batchDelete",
    ),
    (
        "gmail.users.messages.trash",
        "POST",
        "/gmail/v1/users/{userId}/messages/{id}/trash",
    ),
    (
        "gmail.users.messages.untrash",
        "POST",
        "/gmail/v1/users/{userId}/messages/{id}/untrash",
    ),
    (
        "gmail.users.messages.delete",
        "DELETE",
        "/gmail/v1/users/{userId}/messages/{id}",
    ),
    (
        "gmail.users.messages.send",
        "POST",
        "/gmail/v1/users/{userId}/messages/send",
    ),
    (
        "gmail.users.messages.import",
        "POST",
        "/gmail/v1/users/{userId}/messages/import",
    ),
    (
        "gmail.users.messages.insert",
        "POST",
        "/gmail/v1/users/{userId}/messages",
    ),
    (
        "gmail.users.messages.attachments.get",
        "GET",
        "/gmail/v1/users/{userId}/messages/{messageId}/attachments/{id}",
    ),
    (
        "gmail.users.threads.list",
        "GET",
        "/gmail/v1/users/{userId}/threads",
    ),
    (
        "gmail.users.threads.get",
        "GET",
        "/gmail/v1/users/{userId}/threads/{id}",
    ),
    (
        "gmail.users.threads.modify",
        "POST",
        "/gmail/v1/users/{userId}/threads/{id}/modify",
    ),
    (
        "gmail.users.threads.trash",
        "POST",
        "/gmail/v1/users/{userId}/threads/{id}/trash",
    ),
    (
        "gmail.users.threads.untrash",
        "POST",
        "/gmail/v1/users/{userId}/threads/{id}/untrash",
    ),
    (
        "gmail.users.threads.delete",
        "DELETE",
        "/gmail/v1/users/{userId}/threads/{id}",
    ),
    (
        "gmail.users.drafts.list",
        "GET",
        "/gmail/v1/users/{userId}/drafts",
    ),
    (
        "gmail.users.drafts.get",
        "GET",
        "/gmail/v1/users/{userId}/drafts/{id}",
    ),
    (
        "gmail.users.drafts.create",
        "POST",
        "/gmail/v1/users/{userId}/drafts",
    ),
    (
        "gmail.users.drafts.update",
        "PUT",
        "/gmail/v1/users/{userId}/drafts/{id}",
    ),
    (
        "gmail.users.drafts.delete",
        "DELETE",
        "/gmail/v1/users/{userId}/drafts/{id}",
    ),
    (
        "gmail.users.drafts.send",
        "POST",
        "/gmail/v1/users/{userId}/drafts/send",
    ),
    (
        "gmail.users.labels.list",
        "GET",
        "/gmail/v1/users/{userId}/labels",
    ),
    (
        "gmail.users.labels.get",
        "GET",
        "/gmail/v1/users/{userId}/labels/{id}",
    ),
    (
        "gmail.users.labels.create",
        "POST",
        "/gmail/v1/users/{userId}/labels",
    ),
    (
        "gmail.users.labels.update",
        "PUT",
        "/gmail/v1/users/{userId}/labels/{id}",
    ),
    (
        "gmail.users.labels.delete",
        "DELETE",
        "/gmail/v1/users/{userId}/labels/{id}",
    ),
    (
        "gmail.users.history.list",
        "GET",
        "/gmail/v1/users/{userId}/history",
    ),
    // Drive
    ("drive.about.get", "GET", "/drive/v3/about"),
    ("drive.files.list", "GET", "/drive/v3/files"),
    ("drive.files.get", "GET", "/drive/v3/files/{fileId}"),
    ("drive.files.create", "POST", "/drive/v3/files"),
    ("drive.files.update", "PATCH", "/drive/v3/files/{fileId}"),
    ("drive.files.copy", "POST", "/drive/v3/files/{fileId}/copy"),
    ("drive.files.delete", "DELETE", "/drive/v3/files/{fileId}"),
    (
        "drive.files.export",
        "GET",
        "/drive/v3/files/{fileId}/export",
    ),
    ("drive.files.emptyTrash", "DELETE", "/drive/v3/files/trash"),
    (
        "drive.permissions.list",
        "GET",
        "/drive/v3/files/{fileId}/permissions",
    ),
    (
        "drive.permissions.get",
        "GET",
        "/drive/v3/files/{fileId}/permissions/{permissionId}",
    ),
    (
        "drive.permissions.create",
        "POST",
        "/drive/v3/files/{fileId}/permissions",
    ),
    (
        "drive.permissions.update",
        "PATCH",
        "/drive/v3/files/{fileId}/permissions/{permissionId}",
    ),
    (
        "drive.permissions.delete",
        "DELETE",
        "/drive/v3/files/{fileId}/permissions/{permissionId}",
    ),
    (
        "drive.revisions.list",
        "GET",
        "/drive/v3/files/{fileId}/revisions",
    ),
    (
        "drive.revisions.get",
        "GET",
        "/drive/v3/files/{fileId}/revisions/{revisionId}",
    ),
    (
        "drive.comments.list",
        "GET",
        "/drive/v3/files/{fileId}/comments",
    ),
    (
        "drive.comments.create",
        "POST",
        "/drive/v3/files/{fileId}/comments",
    ),
    // Sheets
    ("sheets.spreadsheets.create", "POST", "/v4/spreadsheets"),
    (
        "sheets.spreadsheets.get",
        "GET",
        "/v4/spreadsheets/{spreadsheetId}",
    ),
    (
        "sheets.spreadsheets.batchUpdate",
        "POST",
        "/v4/spreadsheets/{spreadsheetId}:batchUpdate",
    ),
    (
        "sheets.spreadsheets.values.get",
        "GET",
        "/v4/spreadsheets/{spreadsheetId}/values/{range}",
    ),
    (
        "sheets.spreadsheets.values.batchGet",
        "GET",
        "/v4/spreadsheets/{spreadsheetId}/values:batchGet",
    ),
    (
        "sheets.spreadsheets.values.update",
        "PUT",
        "/v4/spreadsheets/{spreadsheetId}/values/{range}",
    ),
    (
        "sheets.spreadsheets.values.append",
        "POST",
        "/v4/spreadsheets/{spreadsheetId}/values/{range}:append",
    ),
    (
        "sheets.spreadsheets.values.clear",
        "POST",
        "/v4/spreadsheets/{spreadsheetId}/values/{range}:clear",
    ),
    // Slides
    ("slides.presentations.create", "POST", "/v1/presentations"),
    (
        "slides.presentations.get",
        "GET",
        "/v1/presentations/{presentationId}",
    ),
    (
        "slides.presentations.batchUpdate",
        "POST",
        "/v1/presentations/{presentationId}:batchUpdate",
    ),
    (
        "slides.presentations.pages.get",
        "GET",
        "/v1/presentations/{presentationId}/pages/{pageObjectId}",
    ),
    // Docs
    ("docs.documents.create", "POST", "/v1/documents"),
    ("docs.documents.get", "GET", "/v1/documents/{documentId}"),
    (
        "docs.documents.batchUpdate",
        "POST",
        "/v1/documents/{documentId}:batchUpdate",
    ),
    // Calendar
    (
        "calendar.calendarList.list",
        "GET",
        "/calendar/v3/users/me/calendarList",
    ),
    (
        "calendar.calendars.get",
        "GET",
        "/calendar/v3/calendars/{calendarId}",
    ),
    (
        "calendar.calendars.insert",
        "POST",
        "/calendar/v3/calendars",
    ),
    (
        "calendar.calendars.delete",
        "DELETE",
        "/calendar/v3/calendars/{calendarId}",
    ),
    (
        "calendar.calendars.clear",
        "POST",
        "/calendar/v3/calendars/{calendarId}/clear",
    ),
    (
        "calendar.events.list",
        "GET",
        "/calendar/v3/calendars/{calendarId}/events",
    ),
    (
        "calendar.events.get",
        "GET",
        "/calendar/v3/calendars/{calendarId}/events/{eventId}",
    ),
    (
        "calendar.events.insert",
        "POST",
        "/calendar/v3/calendars/{calendarId}/events",
    ),
    (
        "calendar.events.update",
        "PUT",
        "/calendar/v3/calendars/{calendarId}/events/{eventId}",
    ),
    (
        "calendar.events.patch",
        "PATCH",
        "/calendar/v3/calendars/{calendarId}/events/{eventId}",
    ),
    (
        "calendar.events.delete",
        "DELETE",
        "/calendar/v3/calendars/{calendarId}/events/{eventId}",
    ),
    (
        "calendar.events.move",
        "POST",
        "/calendar/v3/calendars/{calendarId}/events/{eventId}/move",
    ),
    (
        "calendar.events.quickAdd",
        "POST",
        "/calendar/v3/calendars/{calendarId}/events/quickAdd",
    ),
    (
        "calendar.events.import",
        "POST",
        "/calendar/v3/calendars/{calendarId}/events/import",
    ),
    (
        "calendar.acl.list",
        "GET",
        "/calendar/v3/calendars/{calendarId}/acl",
    ),
    (
        "calendar.acl.insert",
        "POST",
        "/calendar/v3/calendars/{calendarId}/acl",
    ),
    (
        "calendar.acl.update",
        "PUT",
        "/calendar/v3/calendars/{calendarId}/acl/{ruleId}",
    ),
    (
        "calendar.acl.patch",
        "PATCH",
        "/calendar/v3/calendars/{calendarId}/acl/{ruleId}",
    ),
    (
        "calendar.acl.delete",
        "DELETE",
        "/calendar/v3/calendars/{calendarId}/acl/{ruleId}",
    ),
    ("calendar.freebusy.query", "POST", "/calendar/v3/freeBusy"),
];

/// The service a method id or path belongs to.
pub fn service_of_id(id: &str) -> Option<Service> {
    Service::parse(id.split('.').next()?)
}

/// The service a path belongs to, by its API's own prefix.
pub fn service_of_path(path: &str) -> Option<Service> {
    let p = path.trim_start_matches('/');
    let first = p.split('/').next()?;
    match first {
        "gmail" => Some(Service::Gmail),
        "drive" | "upload" if p.contains("drive/") => Some(Service::Drive),
        "calendar" => Some(Service::Calendar),
        "v4" if p.starts_with("v4/spreadsheets") => Some(Service::Sheets),
        "v1" if p.starts_with("v1/presentations") => Some(Service::Slides),
        "v1" if p.starts_with("v1/documents") => Some(Service::Docs),
        _ => None,
    }
}

/// A call as the model states it, resolved to a request. `method` is a known
/// id, or `"VERB /path"`.
#[derive(Debug)]
pub struct Resolved {
    pub request: Request,
    /// The id when the method was one the table knows.
    pub id: Option<&'static str>,
    /// What the call is: its class, judged on the verb, the filled path and
    /// the id (`classify`).
    pub class: crate::classify::Classified,
    /// The thread, message, file, event or document the call names, for the
    /// ledger's row: an id, never content.
    pub resource: Option<String>,
}

fn fill(template: &str, params: &mut Map<String, Value>) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let close = rest[open..]
            .find('}')
            .ok_or("a path template is malformed")?
            + open;
        let name = &rest[open + 1..close];
        let value = match params.remove(name) {
            Some(Value::String(s)) => s,
            Some(Value::Number(n)) => n.to_string(),
            Some(_) => return Err(format!("params.{name} must be a string")),
            None if name == "userId" => "me".to_string(),
            None if name == "calendarId" => "primary".to_string(),
            None => return Err(format!("this method needs params.{name}")),
        };
        out.push_str(&percent_encoding::utf8_percent_encode(&value, SEGMENT).to_string());
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// A path segment's escapes: everything but the unreserved set, and `!$&'()*+,;=:@` kept
/// as Google's own examples keep them (a range's `A1:B2`).
const SEGMENT: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~')
    .remove(b':')
    .remove(b'@')
    .remove(b'!')
    .remove(b'$')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')')
    .remove(b'*')
    .remove(b',')
    .remove(b';')
    .remove(b'=')
    .remove(b'+');

fn query_value(v: &Value) -> Vec<String> {
    match v {
        Value::Array(a) => a.iter().flat_map(query_value).collect(),
        Value::String(s) => vec![s.clone()],
        Value::Null => Vec::new(),
        other => vec![other.to_string()],
    }
}

/// The request for `method`, `params` (path parameters and query parameters
/// together, as Google's reference gives them) and `body`.
pub fn resolve(
    method: &str,
    params: Option<&Value>,
    body: Option<&Value>,
) -> Result<Resolved, String> {
    let mut params: Map<String, Value> = match params {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(m)) => m.clone(),
        Some(_) => return Err("params must be an object".into()),
    };
    let (id, verb, template) = match TABLE.iter().find(|(id, _, _)| *id == method) {
        Some((id, verb, path)) => (Some(*id), (*verb).to_string(), (*path).to_string()),
        None => {
            let (verb, path) = method.split_once(' ').ok_or_else(|| {
                format!(
                    "unknown method {method:?}: name one google.call knows (drive.files.list, \
                     gmail.users.messages.get, …) or give the verb and path, as \"GET /gmail/v1/users/me/labels\""
                )
            })?;
            let verb = verb.to_ascii_uppercase();
            if !["GET", "POST", "PUT", "PATCH", "DELETE"].contains(&verb.as_str()) {
                return Err(format!("unknown HTTP verb {verb:?}"));
            }
            if !path.starts_with('/') || path.contains("..") || path.contains("://") {
                return Err("the path must begin with / and name one API's path".into());
            }
            // The class is read off the path, so nothing may hide a segment
            // from it: no query or fragment (params carry the query), no escape.
            if path.contains(['?', '#', '%', '\\']) || path.chars().any(char::is_whitespace) {
                return Err("the path must be a plain path: put the query in params".into());
            }
            (None, verb, path.trim().to_string())
        }
    };
    let service = id
        .and_then(service_of_id)
        .or_else(|| service_of_path(&template))
        .ok_or("the path is none of Gmail, Drive, Sheets, Slides, Docs or Calendar's")?;
    let resource = [
        "id",
        "messageId",
        "threadId",
        "fileId",
        "eventId",
        "spreadsheetId",
        "presentationId",
        "documentId",
        "calendarId",
    ]
    .iter()
    .find_map(|k| params.get(*k).and_then(Value::as_str))
    .map(str::to_string);
    let path = fill(&template, &mut params)?;
    let query: Vec<(String, String)> = params
        .iter()
        .flat_map(|(k, v)| query_value(v).into_iter().map(move |s| (k.clone(), s)))
        .collect();
    // The filled path, not the template: a path's own `{x}` filled with `send`
    // is a send.
    let class = classify(&verb, &path, id, &query);
    let body = match body {
        None | Some(Value::Null) => Body::None,
        Some(v) => Body::Json(v.clone()),
    };
    Ok(Resolved {
        request: Request {
            service,
            verb,
            path,
            query,
            body,
            repeat: class.repeat,
        },
        id,
        class,
        resource,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_known_method_fills_its_path_and_sends_the_rest_as_query() {
        let r = resolve(
            "drive.files.list",
            Some(&json!({"q": "name contains 'x'", "pageSize": 5})),
            None,
        )
        .unwrap();
        assert_eq!(
            (r.request.verb.as_str(), r.request.path.as_str()),
            ("GET", "/drive/v3/files")
        );
        assert_eq!(r.request.service, Service::Drive);
        assert!(r.request.query.contains(&("pageSize".into(), "5".into())));
        let r = resolve(
            "gmail.users.messages.get",
            Some(&json!({"id": "m1", "format": "metadata"})),
            None,
        )
        .unwrap();
        assert_eq!(r.request.path, "/gmail/v1/users/me/messages/m1");
        let r = resolve(
            "sheets.spreadsheets.values.get",
            Some(&json!({"spreadsheetId": "s1", "range": "Sheet1!A1:B2"})),
            None,
        )
        .unwrap();
        assert_eq!(
            r.request.path,
            "/v4/spreadsheets/s1/values/Sheet1%21A1:B2".replace("%21", "!")
        );
        assert!(resolve("drive.files.get", None, None)
            .unwrap_err()
            .contains("params.fileId"));
    }

    #[test]
    fn a_verb_and_path_is_the_long_tail() {
        let r = resolve(
            "POST /gmail/v1/users/me/labels",
            None,
            Some(&json!({"name": "x"})),
        )
        .unwrap();
        assert_eq!(r.request.service, Service::Gmail);
        assert_eq!(r.id, None);
        assert!(resolve("nonsense.method", None, None).is_err());
        assert!(resolve("GET /somewhere/else", None, None).is_err());
        assert!(resolve("GET https://evil.example/gmail/v1/x", None, None).is_err());
    }

    #[test]
    fn a_send_hidden_in_a_path_template_or_a_query_still_asks() {
        use crate::classify::Class;
        let r = resolve(
            "POST /gmail/v1/users/me/messages/{x}",
            Some(&json!({"x": "send"})),
            None,
        )
        .unwrap();
        assert_eq!(r.request.path, "/gmail/v1/users/me/messages/send");
        assert_eq!(r.class.class, Class::Authority);
        assert!(r.class.send);
        let r = resolve(
            "POST /drive/v3/files/f1/{p}",
            Some(&json!({"p": "permissions"})),
            None,
        )
        .unwrap();
        assert_eq!(r.class.class, Class::Authority);
        for method in [
            "POST /gmail/v1/users/me/messages/send?alt=json",
            "POST /gmail/v1/users/me/drafts/send#x",
            "POST /gmail/v1/users/me/messages/%73end",
            "POST /gmail/v1/users/me/messages/send x",
        ] {
            assert!(
                resolve(method, None, None)
                    .unwrap_err()
                    .contains("plain path"),
                "{method}"
            );
        }
    }

    #[test]
    fn every_table_row_resolves_to_a_service() {
        for (id, verb, path) in TABLE {
            assert!(service_of_id(id).is_some(), "{id}");
            assert_eq!(service_of_id(id), service_of_path(path), "{id}");
            assert!(["GET", "POST", "PUT", "PATCH", "DELETE"].contains(verb));
        }
    }
}
