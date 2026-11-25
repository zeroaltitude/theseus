//! The class of a `google.call` (Google design §3.2, and the owner's decision
//! that every send asks): a read, a write, or **authority**. `Plan.authority`
//! asks at every posture, so no `[policy.tools]` line, grant, trust or
//! judgment loosens it.
//!
//! It is an allow list (issue 57): a GET reads, as does Calendar's free/busy
//! query; every other call asks unless it is one of a few acts the owner can
//! have put back exactly, each checked on its verb, its path, its query and
//! its body, and run at the tool's own posture with the owner told:
//! - a Gmail message moved to the Trash or out of it, one message by id;
//! - a Drive file moved to the Trash or restored, and nothing else changed;
//! - a calendar made, with no more than its name, description, place and
//!   time zone.
//!
//! A call that carries anything more (another field, a parent, an attendee,
//! a notification, a thread rather than a message) is not that act: it asks.
//! What sends, deletes, shares or puts mail into the mailbox asks under its
//! own reason, whatever its shape. The path is judged as resolved, with its
//! trailing slashes gone ([`normal`]), and a method's id resolves to the same
//! path as its spelled-out form.

use serde_json::Value;

use crate::retry::Repeat;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Read,
    Write,
    Authority,
}

/// One of the acts that run at the tool's own posture and are told, with
/// what the owner's Undo needs to name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tidy {
    /// `POST /gmail/v1/users/me/messages/{id}/trash`.
    GmailTrash(String),
    /// `POST /gmail/v1/users/me/messages/{id}/untrash`.
    GmailUntrash(String),
    /// `PATCH /drive/v3/files/{id}` with the body `{"trashed": bool}` alone.
    DriveTrash { id: String, trashed: bool },
    /// `POST /calendar/v3/calendars` with a name and nothing that shares it.
    CalendarMade,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classified {
    pub class: Class,
    /// Authority methods are non-repeatable; so is any send.
    pub repeat: Repeat,
    /// Why it is authority, in the card's words.
    pub why: Option<&'static str>,
    /// A told act (never authority).
    pub tidy: Option<Tidy>,
    /// It sends mail (`messages.send`, `drafts.send`): the daily cap counts it.
    pub send: bool,
}

/// What a call off the allow list is, in the card's words.
pub const UNLISTED: &str =
    "changes something in Google that Theseus may not change unasked (it is not one of the few acts it can put back exactly)";

/// A path as it is judged and sent: its trailing slashes gone.
pub fn normal(path: &str) -> &str {
    match path.trim_end_matches('/') {
        "" => "/",
        p => p,
    }
}

/// The path's last segment, without a `:customVerb`.
fn last_segment(path: &str) -> (&str, Option<&str>) {
    let last = normal(path).rsplit('/').next().unwrap_or("");
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
    if mutating && segments.contains(&"calendars") && ends(&["clear"]) {
        return Some("clears a calendar");
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

/// An id as Gmail and Drive spell theirs: nothing a path could hide behind.
fn plain_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 200
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// A body that says nothing: none, `null` or `{}`.
fn empty(body: Option<&Value>) -> bool {
    match body {
        None | Some(Value::Null) => true,
        Some(Value::Object(m)) => m.is_empty(),
        Some(_) => false,
    }
}

/// One of the told acts, exactly: its verb, its path, its query and its body.
fn tidy(verb: &str, path: &str, query: &[(String, String)], body: Option<&Value>) -> Option<Tidy> {
    let segs: Vec<&str> = path.split('/').skip(1).collect();
    match (verb, segs.as_slice()) {
        ("POST", ["gmail", "v1", "users", "me", "messages", id, op])
            if plain_id(id) && query.is_empty() && empty(body) =>
        {
            match *op {
                "trash" => Some(Tidy::GmailTrash((*id).to_string())),
                "untrash" => Some(Tidy::GmailUntrash((*id).to_string())),
                _ => None,
            }
        }
        ("PATCH", ["drive", "v3", "files", id]) if plain_id(id) => {
            let all_drives = |(k, v): &(String, String)| {
                k == "supportsAllDrives" && matches!(v.as_str(), "true" | "false")
            };
            let fields = body?.as_object()?;
            let trashed = fields.get("trashed")?.as_bool()?;
            (fields.len() == 1 && query.iter().all(all_drives)).then(|| Tidy::DriveTrash {
                id: (*id).to_string(),
                trashed,
            })
        }
        ("POST", ["calendar", "v3", "calendars"]) if query.is_empty() => {
            let fields = body?.as_object()?;
            let known = ["summary", "description", "location", "timeZone"];
            let plain = fields
                .iter()
                .all(|(k, v)| known.contains(&k.as_str()) && v.is_string());
            (plain && fields.contains_key("summary")).then_some(Tidy::CalendarMade)
        }
        _ => None,
    }
}

/// A call that reads though it is not a GET: Calendar's free/busy query.
fn reads(verb: &str, path: &str) -> bool {
    verb == "GET" || (verb == "POST" && path == "/calendar/v3/freeBusy")
}

/// The class of a call by its verb, its path (the template or the resolved
/// path), its id when it is a known one, its query and its body.
pub fn classify(
    verb: &str,
    path: &str,
    id: Option<&str>,
    query: &[(String, String)],
    body: Option<&Value>,
) -> Classified {
    let verb = verb.to_ascii_uppercase();
    let path = normal(path);
    let repeat = if matches!(verb.as_str(), "GET" | "HEAD" | "PUT") {
        Repeat::Safe
    } else {
        Repeat::NonRepeatable
    };
    let class = |class, repeat, why, tidy| Classified {
        class,
        repeat,
        why,
        tidy,
        send: false,
    };
    if let Some(why) = authority(&verb, path, id, query) {
        let send = id.is_some_and(|i| i.ends_with("messages.send") || i.ends_with("drafts.send"))
            || (verb == "POST" && last_segment(path).0 == "send");
        return Classified {
            send,
            ..class(Class::Authority, Repeat::NonRepeatable, Some(why), None)
        };
    }
    if reads(&verb, path) {
        return class(Class::Read, repeat, None, None);
    }
    match tidy(&verb, path, query, body) {
        Some(t) => class(Class::Write, Repeat::NonRepeatable, None, Some(t)),
        None => class(Class::Authority, repeat, Some(UNLISTED), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::methods::TABLE;
    use serde_json::json;

    fn of(id: &str) -> Classified {
        let (_, verb, path) = TABLE.iter().find(|(i, _, _)| *i == id).unwrap();
        classify(verb, path, Some(id), &[], None)
    }

    /// The class of a call as `google.call` resolves it: its path filled,
    /// its query and its body.
    fn resolved(method: &str, params: Value, body: Option<Value>) -> Classified {
        crate::methods::resolve(method, Some(&params), body.as_ref())
            .unwrap()
            .class
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
        // Reads are not. Every other write asks unless it is one of the told
        // acts, exactly (issue 57): these ordinary writes now ask.
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
    fn the_told_acts_are_exactly_their_verb_path_query_and_body() {
        let t = |m: &str, p: Value, b: Option<Value>| resolved(m, p, b);
        let told = |c: Classified| {
            assert_eq!((c.class, c.why, c.repeat), (Class::Write, None, Repeat::NonRepeatable));
            c.tidy
        };
        assert_eq!(
            told(t("gmail.users.messages.trash", json!({"id": "m1"}), None)),
            Some(Tidy::GmailTrash("m1".into()))
        );
        assert_eq!(
            told(t("gmail.users.messages.untrash", json!({"id": "m1"}), Some(json!({})))),
            Some(Tidy::GmailUntrash("m1".into()))
        );
        assert_eq!(
            told(t(
                "drive.files.update",
                json!({"fileId": "f1", "supportsAllDrives": true}),
                Some(json!({"trashed": true}))
            )),
            Some(Tidy::DriveTrash { id: "f1".into(), trashed: true })
        );
        assert_eq!(
            told(t(
                "PATCH /drive/v3/files/f1",
                json!({}),
                Some(json!({"trashed": false}))
            )),
            Some(Tidy::DriveTrash { id: "f1".into(), trashed: false })
        );
        assert_eq!(
            told(t(
                "calendar.calendars.insert",
                json!({}),
                Some(json!({"summary": "Trips", "timeZone": "UTC"}))
            )),
            Some(Tidy::CalendarMade)
        );
        // One normal form: a trailing slash is the same path.
        assert_eq!(
            told(t("POST /gmail/v1/users/me/messages/m1/trash/", json!({}), None)),
            Some(Tidy::GmailTrash("m1".into()))
        );
        assert_eq!(normal("/a/b//"), "/a/b");
        assert_eq!(normal("/"), "/");
    }

    #[test]
    fn a_told_act_that_carries_anything_more_asks() {
        let asks = |m: &str, p: Value, b: Option<Value>| {
            let c = resolved(m, p.clone(), b.clone());
            assert_eq!(c.class, Class::Authority, "{m} {p} {b:?}");
            assert_eq!(c.tidy, None, "{m}");
        };
        // A Drive trash with a parent move, another field, a query, or no trash.
        let f = || json!({"fileId": "f1"});
        asks(
            "drive.files.update",
            json!({"fileId": "f1", "addParents": "shared"}),
            Some(json!({"trashed": true})),
        );
        asks(
            "drive.files.update",
            f(),
            Some(json!({"trashed": true, "name": "x"})),
        );
        asks(
            "drive.files.update",
            f(),
            Some(json!({"trashed": true, "parents": ["shared"]})),
        );
        asks("drive.files.update", f(), Some(json!({"trashed": "yes"})));
        asks("drive.files.update", f(), Some(json!({"name": "x"})));
        asks("drive.files.update", f(), None);
        asks(
            "drive.files.create",
            json!({}),
            Some(json!({"name": "x", "parents": ["shared"]})),
        );
        asks(
            "drive.files.copy",
            f(),
            Some(json!({"parents": ["shared"]})),
        );
        asks(
            "PATCH /drive/v2/files/f1",
            json!({}),
            Some(json!({"trashed": true})),
        );
        // A Gmail trash of a thread, of someone else's mailbox, with a body
        // or a query.
        asks("gmail.users.threads.trash", json!({"id": "t1"}), None);
        asks("gmail.users.threads.untrash", json!({"id": "t1"}), None);
        asks(
            "gmail.users.messages.trash",
            json!({"id": "m1", "userId": "someone@example.test"}),
            None,
        );
        asks(
            "gmail.users.messages.trash",
            json!({"id": "m1"}),
            Some(json!({"x": 1})),
        );
        asks(
            "gmail.users.messages.trash",
            json!({"id": "m1", "fields": "id"}),
            None,
        );
        // Labels can hide mail.
        asks(
            "gmail.users.messages.modify",
            json!({"id": "m1"}),
            Some(json!({"removeLabelIds": ["INBOX"]})),
        );
        asks(
            "gmail.users.messages.batchModify",
            json!({}),
            Some(json!({"ids": ["m1"], "addLabelIds": ["SPAM"]})),
        );
        asks(
            "gmail.users.threads.modify",
            json!({"id": "t1"}),
            Some(json!({"addLabelIds": ["TRASH"]})),
        );
        // Every settings write: filters, forwarding, vacation, send-as,
        // delegates, IMAP and POP.
        for (m, b) in [
            ("POST /gmail/v1/users/me/settings/filters", json!({"action": {}})),
            (
                "POST /gmail/v1/users/me/settings/forwardingAddresses",
                json!({"forwardingEmail": "x@example.test"}),
            ),
            (
                "PUT /gmail/v1/users/me/settings/autoForwarding",
                json!({"enabled": true}),
            ),
            (
                "PUT /gmail/v1/users/me/settings/vacation",
                json!({"enableAutoReply": true}),
            ),
            ("POST /gmail/v1/users/me/settings/sendAs", json!({})),
            ("POST /gmail/v1/users/me/settings/delegates", json!({})),
            ("PUT /gmail/v1/users/me/settings/imap", json!({})),
            ("PUT /gmail/v1/users/me/settings/pop", json!({})),
        ] {
            asks(m, json!({}), Some(b));
        }
        // Events with attendees or a notification, whatever its value.
        let attendees = json!({"summary": "x", "attendees": [{"email": "a@example.test"}]});
        asks("calendar.events.insert", json!({}), Some(attendees.clone()));
        asks(
            "calendar.events.patch",
            json!({"eventId": "e1"}),
            Some(attendees.clone()),
        );
        asks(
            "calendar.events.update",
            json!({"eventId": "e1"}),
            Some(attendees),
        );
        asks(
            "calendar.events.insert",
            json!({"sendUpdates": "none"}),
            Some(json!({"summary": "x"})),
        );
        // A calendar made with more than its name, or with none.
        asks(
            "calendar.calendars.insert",
            json!({}),
            Some(json!({"summary": "x", "conferenceProperties": {}})),
        );
        asks("calendar.calendars.insert", json!({}), Some(json!({"description": "x"})));
        asks(
            "calendar.calendars.insert",
            json!({"x": "y"}),
            Some(json!({"summary": "x"})),
        );
        // An unknown write.
        asks("POST /drive/v3/files/f1/watch", json!({}), Some(json!({})));
        asks("POST /v4/spreadsheets/s1:batchUpdate", json!({}), Some(json!({})));
    }

    #[test]
    fn a_call_by_path_is_classified_like_one_by_id() {
        let c = |v: &str, p: &str| classify(v, p, None, &[], None).class;
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
            Class::Write
        );
        // A KEEP reason is judged first: a trash under a permission, a clear,
        // a delete, are authority under their own reason.
        let why = |v: &str, p: &str| classify(v, p, None, &[], None).why;
        assert_eq!(
            why("POST", "/drive/v3/files/abc/permissions/p1:trash"),
            Some("changes who can see or edit")
        );
        assert_eq!(
            why("DELETE", "/gmail/v1/users/me/messages/abc/trash"),
            Some("deletes")
        );
        assert_eq!(
            why("POST", "/calendar/v3/calendars/c1/clear"),
            Some("clears a calendar")
        );
        assert_eq!(
            why("POST", "/gmail/v1/users/me/messages/import"),
            Some("puts a message into the mailbox")
        );
        assert_eq!(c("GET", "/gmail/v1/users/me/messages"), Class::Read);
        assert_eq!(c("POST", "/gmail/v1/users/me/labels"), Class::Authority);
        assert_eq!(
            c("POST", "/v4/spreadsheets/s1:batchUpdate"),
            Class::Authority
        );
        assert_eq!(why("POST", "/v4/spreadsheets/s1:batchUpdate"), Some(UNLISTED));
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
                &q("sendUpdates", "all"),
                None
            )
            .class,
            Class::Authority
        );
        let why = |q: &[(String, String)]| {
            classify("POST", path, Some("calendar.events.insert"), q, None).why
        };
        assert_eq!(
            why(&q("sendUpdates", "all")),
            Some("sends invitations or updates to attendees")
        );
        // An event made with no notification is no told act either: it asks.
        assert_eq!(why(&q("sendUpdates", "none")), Some(UNLISTED));
        assert_eq!(why(&[]), Some(UNLISTED));
    }

    #[test]
    fn generic_creates_are_not_repeatable() {
        assert_eq!(
            classify("POST", "/spreadsheets/x/values/y:append", None, &[], None).repeat,
            Repeat::NonRepeatable
        );
        assert_eq!(
            classify("GET", "/spreadsheets/x", None, &[], None).repeat,
            Repeat::Safe
        );
    }

    #[test]
    fn generic_patch_requests_need_an_idempotence_guarantee() {
        assert_eq!(
            classify("PATCH", "/resources/item", None, &[], None).repeat,
            Repeat::NonRepeatable
        );
    }
}
