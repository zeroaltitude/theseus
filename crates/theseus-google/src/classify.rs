//! The class of a `google.call` (Google design §3.2, and the owner's decision
//! that every send asks): a read, a write, or **authority**. Authority is what
//! reaches other people or cannot be undone: a send, a delete or trash, a
//! permission or ACL change, an invitation. `Plan.authority` asks at every
//! posture, so no `[policy.tools]` line, grant, trust or judgment loosens it.
//! It is judged on the resolved verb and path as well as on the method's name,
//! so a call by path is classified like one by id.

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
            send,
        };
    }
    let class = if verb == "GET" {
        Class::Read
    } else {
        Class::Write
    };
    Classified {
        class,
        repeat: Repeat::Safe,
        why: None,
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
}
