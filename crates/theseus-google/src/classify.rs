//! The class of a `google.call` (Google design §3.2, and the owner's decision
//! that every send asks): a read, a write, or **authority**. Authority is what
//! reaches other people or cannot be undone: a send, a delete, a permission or
//! ACL change, an invitation, a cleared calendar. `Plan.authority` asks at
//! every posture, so no `[policy.tools]` line, grant, trust or judgment
//! loosens it. A **tidy** write is one the owner can take back himself (a
//! trash or untrash, a message put into the mailbox, a calendar made): it runs
//! at the tool's own posture and the owner is told of it (issue 57). It is
//! judged on the resolved verb and path as well as on the method's name, so a
//! call by path is classified like one by id.

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
    /// A tidy write (never authority): what it does, in the note's words.
    pub tidy: Option<&'static str>,
    /// It sends mail (`messages.send`, `drafts.send`): the daily cap counts it.
    pub send: bool,
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

/// A tidy write, judged only once [`authority`] found none: one the owner can
/// take back himself, so it runs at the tool's posture and is told.
fn tidy(verb: &str, path: &str) -> Option<&'static str> {
    let segments: Vec<&str> = path.split('/').collect();
    let (last, action) = last_segment(path);
    let mutating = verb != "GET";
    let ends = |names: &[&str]| names.contains(&last) || action.is_some_and(|a| names.contains(&a));
    if mutating && ends(&["trash", "untrash"]) {
        return Some("moves to or from the trash");
    }
    if mutating && ends(&["import"]) && segments.contains(&"messages") {
        return Some("puts a message into the mailbox");
    }
    if verb == "POST" && last == "messages" {
        return Some("puts a message into the mailbox");
    }
    if verb == "POST" && last == "calendars" {
        return Some("makes a calendar");
    }
    None
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
    if let Some(why) = authority(&verb, path, id, query) {
        let send = id.is_some_and(|i| i.ends_with("messages.send") || i.ends_with("drafts.send"))
            || (verb == "POST" && last_segment(path).0 == "send");
        return Classified {
            class: Class::Authority,
            repeat: Repeat::NonRepeatable,
            why: Some(why),
            tidy: None,
            send,
        };
    }
    if let Some(tidy) = tidy(&verb, path) {
        return Classified {
            class: Class::Write,
            repeat: Repeat::NonRepeatable,
            why: None,
            tidy: Some(tidy),
            send: false,
        };
    }
    let class = if verb == "GET" {
        Class::Read
    } else {
        Class::Write
    };
    Classified {
        class,
        repeat: if matches!(verb.as_str(), "GET" | "HEAD" | "PUT") {
            Repeat::Safe
        } else {
            Repeat::NonRepeatable
        },
        why: None,
        tidy: None,
        send: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::methods::TABLE;

    fn of(id: &str) -> Classified {
        let (_, verb, path) = TABLE.iter().find(|(i, _, _)| *i == id).unwrap();
        classify(verb, path, Some(id), &[])
    }

    #[test]
    fn google_call_classifies_delete_permissions_and_acl_as_authority() {
        for id in [
            "gmail.users.messages.send",
            "gmail.users.drafts.send",
            "gmail.users.messages.delete",
            "gmail.users.messages.batchDelete",
            "drive.files.delete",
            "drive.files.emptyTrash",
            "drive.permissions.create",
            "drive.permissions.update",
            "drive.permissions.delete",
            "calendar.acl.insert",
            "calendar.acl.delete",
            "calendar.calendars.delete",
            "calendar.calendars.clear",
            "calendar.events.delete",
            "gmail.users.labels.delete",
        ] {
            let c = of(id);
            assert_eq!(c.class, Class::Authority, "{id}");
            assert_eq!(c.repeat, Repeat::NonRepeatable, "{id}");
            assert_eq!(c.tidy, None, "{id}");
        }
        // What the owner can take back himself is a tidy write: it is told,
        // and does not ask (issue 57).
        for (id, tidy) in [
            ("gmail.users.messages.import", "puts a message into the mailbox"),
            ("gmail.users.messages.insert", "puts a message into the mailbox"),
            ("gmail.users.messages.trash", "moves to or from the trash"),
            ("gmail.users.messages.untrash", "moves to or from the trash"),
            ("gmail.users.threads.trash", "moves to or from the trash"),
            ("gmail.users.threads.untrash", "moves to or from the trash"),
            ("calendar.calendars.insert", "makes a calendar"),
        ] {
            let c = of(id);
            assert_eq!(c.class, Class::Write, "{id}");
            assert_eq!(c.why, None, "{id}");
            assert_eq!(c.tidy, Some(tidy), "{id}");
            assert_eq!(c.repeat, Repeat::NonRepeatable, "{id}");
        }
        // Reads and ordinary writes are not.
        for (id, class) in [
            ("drive.files.list", Class::Read),
            ("drive.permissions.list", Class::Read),
            ("calendar.acl.list", Class::Read),
            ("gmail.users.messages.list", Class::Read),
            ("gmail.users.drafts.create", Class::Write),
            ("gmail.users.messages.modify", Class::Write),
            ("sheets.spreadsheets.batchUpdate", Class::Write),
            ("drive.files.update", Class::Write),
            ("calendar.events.insert", Class::Write),
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
        assert_eq!(c("POST", "/gmail/v1/users/me/messages"), Class::Write);
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
        let tidy = |v: &str, p: &str| classify(v, p, None, &[]).tidy;
        assert_eq!(
            tidy("POST", "/gmail/v1/users/me/messages/abc/trash"),
            Some("moves to or from the trash")
        );
        assert_eq!(
            tidy("POST", "/gmail/v1/users/me/messages"),
            Some("puts a message into the mailbox")
        );
        assert_eq!(
            tidy("POST", "/calendar/v3/calendars"),
            Some("makes a calendar")
        );
        // A KEEP reason is judged first: a trash under a permission, a clear,
        // a delete, are authority whatever they end in.
        assert_eq!(
            c("POST", "/drive/v3/files/abc/permissions/p1:trash"),
            Class::Authority
        );
        assert_eq!(c("DELETE", "/gmail/v1/users/me/messages/abc/trash"), Class::Authority);
        assert_eq!(c("POST", "/calendar/v3/calendars/c1/clear"), Class::Authority);
        assert_eq!(tidy("POST", "/calendar/v3/calendars/c1/clear"), None);
        assert_eq!(c("GET", "/gmail/v1/users/me/messages"), Class::Read);
        assert_eq!(c("POST", "/gmail/v1/users/me/labels"), Class::Write);
        assert_eq!(c("POST", "/v4/spreadsheets/s1:batchUpdate"), Class::Write);
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
        assert_eq!(
            classify(
                "POST",
                path,
                Some("calendar.events.insert"),
                &q("sendUpdates", "none")
            )
            .class,
            Class::Write
        );
        assert_eq!(
            classify("POST", path, Some("calendar.events.insert"), &[]).class,
            Class::Write
        );
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
}
