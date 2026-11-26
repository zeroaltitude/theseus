//! The class of a `google.call` (Google design §3.2, and the owner's decision
//! that every send asks): a read, a write, or **authority**. `Plan.authority`
//! asks at every posture, so no `[policy.tools]` line, grant, trust or
//! judgment loosens it.
//!
//! It is an allow list: a GET reads, as does Calendar's free/busy query, and
//! every other call is authority. A send, a delete or trash, a mailbox
//! insert, a permission or ACL change, a calendar made or cleared and an
//! invitation say so in their own words; any other change says
//! [`UNLISTED`]. It is judged on the resolved verb and path as well as on the
//! method's name, so a call by path is classified like one by id, and the
//! path is judged as it is sent, with its trailing slashes gone ([`normal`]).

use crate::retry::Repeat;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Read,
    Write,
    Authority,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classified {
    pub class: Class,
    /// Authority methods are non-repeatable; so is any send.
    pub repeat: Repeat,
    /// Why it is authority, in the card's words.
    pub why: Option<&'static str>,
    /// It sends mail (`messages.send`, `drafts.send`): the daily cap counts it.
    pub send: bool,
}

/// What a change with no reason of its own is, in the card's words.
pub const UNLISTED: &str = "changes something in Google";

/// A path as it is judged and sent: its trailing slashes gone.
pub fn normal(path: &str) -> &str {
    match path.trim_end_matches('/') {
        "" => "/",
        p => p,
    }
}

/// The path's last segment, without a `:customVerb`.
fn last_segment(path: &str) -> (&str, Option<&str>) {
    let last = path.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    match last.split_once(':') {
        Some((seg, action)) => (seg, Some(action)),
        None => (last, None),
    }
}

fn authority(
    verb: &str,
    path: &str,
    id: Option<&str>,
    query: &[(String, String)],
) -> Option<&'static str> {
    let segments: Vec<&str> = path.split('/').collect();
    let (last, action) = last_segment(path);
    let mutating = verb != "GET";
    let ends = |names: &[&str]| names.contains(&last) || action.is_some_and(|a| names.contains(&a));
    if id.is_some_and(|i| i.ends_with(".send")) || (mutating && ends(&["send"])) {
        return Some("sends mail");
    }
    if verb == "DELETE" || id.is_some_and(|i| i.ends_with(".delete")) {
        return Some("deletes");
    }
    if mutating && ends(&["trash", "untrash"]) {
        return Some("moves to or from the trash");
    }
    if mutating && ends(&["import"]) && segments.contains(&"messages") {
        return Some("puts a message into the mailbox");
    }
    if verb == "POST" && last == "messages" {
        return Some("puts a message into the mailbox");
    }
    if mutating && ends(&["batchDelete"]) {
        return Some("deletes");
    }
    if mutating && segments.iter().any(|s| matches!(*s, "permissions" | "acl")) {
        return Some("changes who can see or edit");
    }
    if mutating
        && segments.contains(&"calendars")
        && (ends(&["clear"]) || (verb == "POST" && last == "calendars"))
    {
        return Some("changes or clears a calendar");
    }
    if mutating && segments.contains(&"events") {
        let loud = |k: &str| {
            query
                .iter()
                .any(|(n, v)| n == k && !v.is_empty() && v != "none" && v != "false")
        };
        if loud("sendUpdates") || loud("sendNotifications") {
            return Some("sends invitations or updates to attendees");
        }
    }
    None
}

/// A call that reads though it is not a GET: Calendar's free/busy query.
fn reads(verb: &str, path: &str) -> bool {
    verb == "GET" || (verb == "POST" && path == "/calendar/v3/freeBusy")
}

/// The class of a call by its verb, its path (the template or the resolved
/// path), its id when it is a known one, and its query.
pub fn classify(
    verb: &str,
    path: &str,
    id: Option<&str>,
    query: &[(String, String)],
) -> Classified {
    let verb = verb.to_ascii_uppercase();
    let path = normal(path);
    let repeat = if matches!(verb.as_str(), "GET" | "HEAD" | "PUT") {
        Repeat::Safe
    } else {
        Repeat::NonRepeatable
    };
    if let Some(why) = authority(&verb, path, id, query) {
        let send = id.is_some_and(|i| i.ends_with("messages.send") || i.ends_with("drafts.send"))
            || (verb == "POST" && last_segment(path).0 == "send");
        return Classified {
            class: Class::Authority,
            repeat: Repeat::NonRepeatable,
            why: Some(why),
            send,
        };
    }
    let (class, why) = match reads(&verb, path) {
        true => (Class::Read, None),
        false => (Class::Authority, Some(UNLISTED)),
    };
    Classified {
        class,
        repeat,
        why,
        send: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::methods::{resolve, TABLE};
    use serde_json::{json, Value};

    fn of(id: &str) -> Classified {
        let (_, verb, path) = TABLE.iter().find(|(i, _, _)| *i == id).unwrap();
        classify(verb, path, Some(id), &[])
    }

    #[test]
    fn google_call_classifies_delete_permissions_and_acl_as_authority() {
        for id in [
            "gmail.users.messages.send",
            "gmail.users.drafts.send",
            "gmail.users.messages.import",
            "gmail.users.messages.insert",
            "gmail.users.messages.trash",
            "gmail.users.threads.untrash",
            "gmail.users.messages.delete",
            "gmail.users.messages.batchDelete",
            "drive.files.delete",
            "drive.files.emptyTrash",
            "drive.permissions.create",
            "drive.permissions.update",
            "drive.permissions.delete",
            "calendar.acl.insert",
            "calendar.acl.delete",
            "calendar.calendars.insert",
            "calendar.calendars.delete",
            "calendar.calendars.clear",
            "calendar.events.delete",
            "gmail.users.labels.delete",
        ] {
            let c = of(id);
            assert_eq!(c.class, Class::Authority, "{id}");
            assert_eq!(c.repeat, Repeat::NonRepeatable, "{id}");
        }
        // Reads are not; every other write is.
        for (id, class) in [
            ("drive.files.list", Class::Read),
            ("drive.permissions.list", Class::Read),
            ("calendar.acl.list", Class::Read),
            ("gmail.users.messages.list", Class::Read),
            ("calendar.freebusy.query", Class::Read),
            ("gmail.users.drafts.create", Class::Authority),
            ("gmail.users.messages.modify", Class::Authority),
            ("sheets.spreadsheets.batchUpdate", Class::Authority),
            ("drive.files.update", Class::Authority),
            ("calendar.events.insert", Class::Authority),
        ] {
            assert_eq!(of(id).class, class, "{id}");
        }
        assert!(of("gmail.users.messages.send").send && of("gmail.users.drafts.send").send);
        assert!(!of("gmail.users.messages.insert").send);
    }

    #[test]
    fn a_call_by_path_is_classified_like_one_by_id() {
        let c = |v: &str, p: &str| classify(v, p, None, &[]).class;
        assert_eq!(
            c("POST", "/gmail/v1/users/me/messages/send"),
            Class::Authority
        );
        assert_eq!(
            c("POST", "/gmail/v1/users/me/drafts/send"),
            Class::Authority
        );
        assert_eq!(c("POST", "/gmail/v1/users/me/messages"), Class::Authority);
        assert_eq!(c("DELETE", "/drive/v3/files/abc"), Class::Authority);
        assert_eq!(
            c("POST", "/drive/v3/files/abc/permissions"),
            Class::Authority
        );
        assert_eq!(
            c("POST", "/calendar/v3/calendars/primary/clear"),
            Class::Authority
        );
        assert_eq!(
            c("POST", "/gmail/v1/users/me/messages/abc/trash"),
            Class::Authority
        );
        assert_eq!(c("GET", "/gmail/v1/users/me/messages"), Class::Read);
        assert_eq!(c("POST", "/gmail/v1/users/me/labels"), Class::Authority);
        assert_eq!(
            c("POST", "/v4/spreadsheets/s1:batchUpdate"),
            Class::Authority
        );
    }

    #[test]
    fn an_event_that_sends_updates_is_authority() {
        let q = |k: &str, v: &str| vec![(k.to_string(), v.to_string())];
        let path = "/calendar/v3/calendars/{calendarId}/events";
        assert_eq!(
            classify(
                "POST",
                path,
                Some("calendar.events.insert"),
                &q("sendUpdates", "all")
            )
            .class,
            Class::Authority
        );
        let why =
            |q: &[(String, String)]| classify("POST", path, Some("calendar.events.insert"), q).why;
        assert_eq!(
            why(&q("sendUpdates", "all")),
            Some("sends invitations or updates to attendees")
        );
        assert_eq!(why(&q("sendUpdates", "none")), Some(UNLISTED));
        assert_eq!(why(&[]), Some(UNLISTED));
    }

    #[test]
    fn generic_creates_are_not_repeatable() {
        assert_eq!(
            classify("POST", "/spreadsheets/x/values/y:append", None, &[]).repeat,
            Repeat::NonRepeatable
        );
        assert_eq!(
            classify("GET", "/spreadsheets/x", None, &[]).repeat,
            Repeat::Safe
        );
    }

    #[test]
    fn generic_patch_requests_need_an_idempotence_guarantee() {
        assert_eq!(
            classify("PATCH", "/resources/item", None, &[]).repeat,
            Repeat::NonRepeatable
        );
    }

    /// The class of a call as `google.call` resolves it.
    fn resolved(method: &str, params: Value, body: Option<Value>) -> Classified {
        resolve(method, Some(&params), body.as_ref()).unwrap().class
    }

    fn asks(method: &str, params: Value, body: Option<Value>) -> &'static str {
        let c = resolved(method, params.clone(), body.clone());
        assert_eq!(c.class, Class::Authority, "{method} {params} {body:?}");
        c.why.expect(method)
    }

    #[test]
    fn a_message_trash_a_trashed_patch_and_a_calendar_made_ask() {
        assert_eq!(
            asks("gmail.users.messages.trash", json!({"id": "m1"}), None),
            "moves to or from the trash"
        );
        assert_eq!(
            asks("gmail.users.messages.untrash", json!({"id": "m1"}), None),
            "moves to or from the trash"
        );
        for (m, p) in [
            ("drive.files.update", json!({"fileId": "f1"})),
            (
                "drive.files.update",
                json!({"fileId": "f1", "supportsAllDrives": true}),
            ),
            ("PATCH /drive/v3/files/f1", json!({})),
        ] {
            assert_eq!(asks(m, p, Some(json!({"trashed": true}))), UNLISTED);
        }
        assert_eq!(
            asks(
                "calendar.calendars.insert",
                json!({}),
                Some(json!({"summary": "Trips"}))
            ),
            "changes or clears a calendar"
        );
    }

    #[test]
    fn main_s_reasons_come_first_and_every_other_change_is_unlisted() {
        for (m, p, why) in [
            ("gmail.users.messages.send", json!({}), "sends mail"),
            ("drive.files.delete", json!({"fileId": "f1"}), "deletes"),
            (
                "gmail.users.messages.insert",
                json!({}),
                "puts a message into the mailbox",
            ),
            (
                "gmail.users.messages.import",
                json!({}),
                "puts a message into the mailbox",
            ),
            (
                "drive.permissions.create",
                json!({"fileId": "f1"}),
                "changes who can see or edit",
            ),
            (
                "calendar.calendars.clear",
                json!({}),
                "changes or clears a calendar",
            ),
            (
                "calendar.events.insert",
                json!({"sendUpdates": "all"}),
                "sends invitations or updates to attendees",
            ),
            // Every other change: drafts, labels, settings, events, Drive,
            // Sheets, Docs and Slides.
            ("gmail.users.drafts.create", json!({}), UNLISTED),
            ("POST /gmail/v1/users/me/labels", json!({}), UNLISTED),
            ("gmail.users.messages.modify", json!({"id": "m1"}), UNLISTED),
            ("gmail.users.messages.batchModify", json!({}), UNLISTED),
            ("gmail.users.threads.modify", json!({"id": "t1"}), UNLISTED),
            (
                "POST /gmail/v1/users/me/settings/filters",
                json!({}),
                UNLISTED,
            ),
            (
                "POST /gmail/v1/users/me/settings/forwardingAddresses",
                json!({}),
                UNLISTED,
            ),
            (
                "PUT /gmail/v1/users/me/settings/autoForwarding",
                json!({}),
                UNLISTED,
            ),
            (
                "PUT /gmail/v1/users/me/settings/vacation",
                json!({}),
                UNLISTED,
            ),
            (
                "POST /gmail/v1/users/me/settings/sendAs",
                json!({}),
                UNLISTED,
            ),
            (
                "POST /gmail/v1/users/me/settings/delegates",
                json!({}),
                UNLISTED,
            ),
            ("PUT /gmail/v1/users/me/settings/imap", json!({}), UNLISTED),
            ("PUT /gmail/v1/users/me/settings/pop", json!({}), UNLISTED),
            ("calendar.events.insert", json!({}), UNLISTED),
            (
                "calendar.events.patch",
                json!({"eventId": "e1", "sendUpdates": "none"}),
                UNLISTED,
            ),
            ("drive.files.create", json!({}), UNLISTED),
            ("drive.files.copy", json!({"fileId": "f1"}), UNLISTED),
            ("POST /drive/v3/files/f1/watch", json!({}), UNLISTED),
            (
                "sheets.spreadsheets.batchUpdate",
                json!({"spreadsheetId": "s1"}),
                UNLISTED,
            ),
            ("POST /v1/documents/d1:batchUpdate", json!({}), UNLISTED),
            ("POST /v1/presentations/p1:batchUpdate", json!({}), UNLISTED),
        ] {
            assert_eq!(asks(m, p.clone(), Some(json!({}))), why, "{m} {p}");
        }
    }

    /// The spellings a write might hide behind: each is refused before it is
    /// sent, or asks; a GET reads whatever it carries.
    #[test]
    fn no_spelling_of_a_write_reads() {
        // Refused: an escape, a dot segment, a query or fragment in the path,
        // a backslash, whitespace, a full URL, a service it is none of.
        for m in [
            "POST /gmail/v1/users/me/messages/m1%2ftrash",
            "PATCH /drive/v3/files/%2e%2e/f1",
            "POST /gmail/v1/users/me/messages/../labels",
            "POST /gmail/v1/users/me/messages/m1/trash?alt=json",
            "POST /gmail/v1/users/me/messages/m1/trash#x",
            "POST /gmail/v1/users/me/messages\\m1",
            "POST /gmail/v1/users/me/messages /m1",
            "POST https://www.googleapis.com/gmail/v1/users/me/labels",
            "POST /batch/gmail/v1",
            "PATCH /Drive/v3/Files/f1",
            "TRACE /gmail/v1/users/me/labels",
        ] {
            assert!(resolve(m, None, None).is_err(), "{m}");
        }
        let b = || Some(json!({"trashed": true}));
        for (m, p, body) in [
            // A path parameter that would add a segment is encoded.
            ("drive.files.update", json!({"fileId": "f1/../x"}), b()),
            ("gmail.users.messages.trash", json!({"id": "m1/x"}), None),
            // Doubled and trailing slashes, case, another user, v2, the
            // upload and batch prefixes.
            ("PATCH /drive/v3//files/f1", json!({}), b()),
            ("PATCH /drive/v3/files/f1//", json!({}), b()),
            ("patch /drive/v3/files/f1", json!({}), b()),
            ("POST /gmail/v1/users/me/Messages/m1/Trash", json!({}), None),
            (
                "gmail.users.messages.trash",
                json!({"id": "m1", "userId": "someone@example.test"}),
                None,
            ),
            ("PATCH /drive/v2/files/f1", json!({}), b()),
            ("PATCH /upload/drive/v3/files/f1", json!({}), b()),
            ("POST /gmail/v1/batch/messages", json!({}), None),
            // A method-override query, `alt`, `uploadType`, `fields`.
            (
                "drive.files.update",
                json!({"fileId": "f1", "$.xgafv": "2", "X-HTTP-Method-Override": "GET"}),
                b(),
            ),
            (
                "drive.files.update",
                json!({"fileId": "f1", "alt": "json", "uploadType": "media", "fields": "id"}),
                b(),
            ),
            // The Drive parent parameters.
            (
                "drive.files.update",
                json!({"fileId": "f1", "addParents": "team", "removeParents": "root"}),
                b(),
            ),
            (
                "drive.files.update",
                json!({"fileId": "f1", "enforceSingleParent": true, "keepRevisionForever": true}),
                b(),
            ),
            (
                "drive.files.update",
                json!({"fileId": "f1", "ocrLanguage": "en", "useContentAsIndexableText": true}),
                b(),
            ),
            // Bodies that are arrays, scalars, or carry extra keys.
            (
                "drive.files.update",
                json!({"fileId": "f1"}),
                Some(json!([{"trashed": true}])),
            ),
            (
                "drive.files.update",
                json!({"fileId": "f1"}),
                Some(json!("{\"trashed\":true}")),
            ),
            (
                "drive.files.update",
                json!({"fileId": "f1"}),
                Some(json!({"trashed": true, "parents": ["team"]})),
            ),
            (
                "calendar.calendars.insert",
                json!({}),
                Some(json!({"summary": "x", "Summary": "y"})),
            ),
        ] {
            asks(m, p, body);
        }
        // The path judged is the path sent: trailing slashes gone.
        let r = resolve("PATCH /drive/v3/files/f1//", None, b().as_ref()).unwrap();
        assert_eq!(r.request.path, "/drive/v3/files/f1");
        assert_eq!(r.class.class, Class::Authority);
        // A GET reads, whatever body it carries; so does free/busy, exactly.
        for (m, body) in [
            ("GET /drive/v3/files/f1", Some(json!({"trashed": true}))),
            ("get /gmail/v1/users/me/labels", None),
            ("calendar.freebusy.query", Some(json!({"items": []}))),
        ] {
            assert_eq!(resolved(m, json!({}), body).class, Class::Read, "{m}");
        }
        assert_eq!(
            resolved("POST /calendar/v3/freeBusy/", json!({}), None).class,
            Class::Read
        );
        asks("POST /calendar/v3/freebusy", json!({}), None);
        asks("PUT /calendar/v3/freeBusy", json!({}), None);
    }
}
