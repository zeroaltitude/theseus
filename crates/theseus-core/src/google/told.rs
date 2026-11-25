//! A tidy Google act tells the owner (issue 57): moving mail or a file to the
//! Trash or back, removing someone's access to a file, and through
//! `google.call` a trash or untrash or a calendar made. None of them reaches anyone or loses anything the owner
//! cannot take back, so they run at the tool's own posture rather than asking
//! every time; the result carries what to tell him under [`KEY`], and
//! `toolrun/complete.rs` posts it to the operator once, beside the result.
//! A call the owner approved himself carries nothing: he already knows.
//!
//! The words name only ids, addresses and roles the call itself was given or
//! Google echoed back for Theseus's own act: never a file's name, a subject or
//! anything else someone else wrote. Where the act has an exact opposite, the
//! note carries it as `google.call` inputs under `undo`; where it has none
//! that runs unasked (giving access back is a share, which asks), the note
//! says in words how to take it back.

use serde_json::{json, Value};

/// The `ToolOutput.meta` key a tidy act's result carries.
pub const KEY: &str = "told";

/// The note's parts: its title, what was done, how to take it back in words,
/// and the calls that take it back exactly (empty: none runs unasked).
fn told(title: &str, did: String, back: String, undo: Vec<Value>) -> Value {
    json!({"title": title, "did": did, "back": back, "undo": undo})
}

/// The `google.call` that runs `verb path` with no body.
fn call(verb: &str, path: &str) -> Value {
    json!({"method": format!("{verb} {path}")})
}

/// Gmail threads or messages moved to the Trash by `gmail.label`: each id
/// with the kind (`threads` or `messages`) that took it.
pub fn mail_trashed(done: &[(&str, &str)]) -> Value {
    let ids: Vec<&str> = done.iter().map(|(_, id)| *id).collect();
    let undo = done
        .iter()
        .map(|(kind, id)| call("POST", &format!("/gmail/v1/users/me/{kind}/{id}/untrash")))
        .collect();
    told(
        "Moved mail to the Trash",
        format!(
            "Moved {} Gmail thread(s) to the Trash: {}.",
            ids.len(),
            ids.join(", ")
        ),
        "Undo takes them out of the Trash.".into(),
        undo,
    )
}

/// A Drive file moved to the Trash (`trashed`) or restored from it.
pub fn drive_trashed(id: &str, trashed: bool) -> Value {
    let (title, did) = match trashed {
        true => (
            "Moved a Drive file to the Trash",
            "Moved Drive file {id} to the Trash.",
        ),
        false => (
            "Restored a Drive file from the Trash",
            "Restored Drive file {id} from the Trash.",
        ),
    };
    let back = match trashed {
        true => "Undo restores it.",
        false => "Undo moves it back to the Trash.",
    };
    let undo = json!({
        "method": "drive.files.update",
        "params": {"fileId": id, "supportsAllDrives": "true"},
        "body": {"trashed": !trashed},
    });
    told(title, did.replace("{id}", id), back.into(), vec![undo])
}

/// Someone's access to a Drive file removed: `who` is the address or
/// "anyone with the link", `role` the access it gave. Giving it back is a
/// share, which asks the owner, so the note says how and carries no undo.
pub fn drive_revoked(id: &str, who: &str, role: &str) -> Value {
    told(
        "Stopped sharing a Drive file",
        format!("Removed {who}'s access ({role}) to Drive file {id}."),
        format!(
            "To give it back, ask Theseus to share Drive file {id} with {who} as {role}: that \
             asks you first."
        ),
        vec![],
    )
}

/// A `google.call` tidy act: `tidy` is its class's words, `label` the
/// method as the call named it, `verb` and `path` the resolved request, and
/// `echo` Google's answer to it (a made calendar's `id`).
pub fn called(tidy: &str, label: &str, verb: &str, path: &str, echo: &Value) -> Value {
    let did = format!("{label}: {verb} {path} ({tidy}).");
    let made = echo
        .get("id")
        .and_then(Value::as_str)
        .filter(|i| i.len() < 200 && !i.contains(char::is_whitespace));
    let swapped = swap_trash(path);
    let (back, undo) = match (swapped, made) {
        (Some(p), _) => ("Undo runs its opposite.".to_string(), vec![call(verb, &p)]),
        (None, Some(id)) if path.starts_with("/calendar/") => (
            format!("To take it back, delete calendar {id}: that asks you first."),
            vec![],
        ),
        (None, _) => ("Google gave no id to take it back by.".to_string(), vec![]),
    };
    told("A Google call was made", did, back, undo)
}

/// The calls that take back the act a result node tells of: its note's
/// `undo`. None for a node that is no succeeded Google result with one.
pub fn undo_calls(node: &crate::node::Node) -> Option<Vec<Value>> {
    let crate::node::Body::ToolResult { meta, status, .. } = &node.body else {
        return None;
    };
    if *status != crate::node::ResultStatus::Ok || !super::private::marked(meta) {
        return None;
    }
    let calls = meta[KEY]["undo"].as_array()?;
    (!calls.is_empty()).then(|| calls.clone())
}

/// The path of a trash's opposite: `…/trash` is `…/untrash` and back, as a
/// last segment or a `:verb`. None for any other path.
fn swap_trash(path: &str) -> Option<String> {
    for sep in ['/', ':'] {
        if let Some(head) = path.strip_suffix(&format!("{sep}untrash")) {
            return Some(format!("{head}{sep}trash"));
        }
        if let Some(head) = path.strip_suffix(&format!("{sep}trash")) {
            return Some(format!("{head}{sep}untrash"));
        }
    }
    None
}

/// A note's three parts, as `owner_note` heads them: its title, the
/// conversation it was done in (by its title, as the ontology's notes name
/// it), and what was done and how to take it back.
pub struct Parts {
    pub title: String,
    pub by: String,
    pub body: String,
}

/// The note's parts. None for a value that is not a told note.
pub fn parts(told: &Value, title: Option<&str>) -> Option<Parts> {
    let head = told.get("title")?.as_str()?;
    let did = told.get("did")?.as_str()?;
    let back = told.get("back").and_then(Value::as_str).unwrap_or_default();
    let by = format!(
        "{}, without asking you",
        crate::judge::onto::words::in_conversation(title)
    );
    let body = [did, back]
        .into_iter()
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    Some(Parts {
        title: head.to_string(),
        by,
        body,
    })
}

/// The note as it posts when it carries no Undo: an FYI head, then its body.
pub fn text(told: &Value, title: Option<&str>) -> Option<String> {
    let p = parts(told, title)?;
    Some(crate::owner_note::note(
        crate::owner_note::Kind::Fyi,
        &p.title,
        &p.by,
        &p.body,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trash_swaps_with_its_opposite_and_nothing_else_does() {
        assert_eq!(
            swap_trash("/gmail/v1/users/me/threads/t1/trash").as_deref(),
            Some("/gmail/v1/users/me/threads/t1/untrash")
        );
        assert_eq!(
            swap_trash("/gmail/v1/users/me/messages/m1/untrash").as_deref(),
            Some("/gmail/v1/users/me/messages/m1/trash")
        );
        assert_eq!(swap_trash("/x/y:trash").as_deref(), Some("/x/y:untrash"));
        assert_eq!(swap_trash("/gmail/v1/users/me/messages"), None);
        assert_eq!(swap_trash("/x/untrashed"), None);
    }

    #[test]
    fn every_note_is_headed_and_names_its_conversation() {
        for t in [
            mail_trashed(&[("threads", "t1"), ("messages", "m2")]),
            drive_trashed("doc1", true),
            drive_trashed("doc1", false),
            drive_revoked("doc1", "a@example.test", "writer"),
            called(
                "makes a calendar",
                "calendar.calendars.insert",
                "POST",
                "/calendar/v3/calendars",
                &json!({"id": "c9"}),
            ),
        ] {
            let s = text(&t, Some("tidy my inbox")).unwrap();
            assert!(crate::owner_note::is_headed(&s), "{s}");
            assert!(s.starts_with("ℹ️ FYI · "), "{s}");
            assert!(
                s.contains("\nIn \"tidy my inbox\", without asking you\n"),
                "{s}"
            );
            let s = text(&t, None).unwrap();
            assert!(
                s.contains("\nIn a conversation with no title yet, without asking you\n"),
                "{s}"
            );
        }
        assert_eq!(text(&json!({"x": 1}), None), None);
    }
}
