//! A told Google act tells the owner (issue 57): Gmail messages moved to the
//! Trash or out of it, a Drive file moved to the Trash or restored, and a
//! calendar made. Each runs at its tool's posture rather than asking every
//! time; the result carries what to tell him under [`KEY`], and
//! `toolrun/complete.rs` posts it to the operator once, beside the result.
//! A call the owner approved himself carries nothing: he already knows.
//!
//! The words are fixed, with counts: no address, no subject, no file or
//! calendar name, no path, and the conversation by its short id, never its
//! title, since a notices place may be shared. Where the act can be put back
//! exactly, the note carries what each thing was before it
//! ([`super::take_back`]) under `undo`, and its Undo restores that.

use serde_json::{json, Value};

pub use super::take_back::{drive_before, gmail_before};

/// The `ToolOutput.meta` key a told act's result carries.
pub const KEY: &str = "told";

/// The note's parts: its title, what was done, how to take it back in words,
/// and what each thing was before it (empty: no Undo).
fn told(title: &str, did: String, back: &str, undo: Vec<Value>) -> Value {
    json!({"title": title, "did": did, "back": back, "undo": undo})
}

/// "1 Gmail message" or "3 Gmail messages".
fn messages(n: usize) -> String {
    match n {
        1 => "1 Gmail message".into(),
        n => format!("{n} Gmail messages"),
    }
}

/// Gmail messages moved to the Trash: `before` holds each one moved, as it
/// was; `of` is how many the act meant to move. Fewer than `of` is a call that
/// failed partway: what moved is told, and only that is undone.
pub fn mail_trashed(before: Vec<Value>, of: usize) -> Value {
    let did = match before.len() {
        n if n == of => format!("Moved {} to the Trash.", messages(n)),
        n => format!(
            "Moved {n} of {of} Gmail messages to the Trash; then Google refused one, and the \
             rest are as they were."
        ),
    };
    told(
        "Moved mail to the Trash",
        did,
        "Undo puts each one back exactly as it was.",
        before,
    )
}

/// A Gmail message taken out of the Trash, as it was before.
pub fn mail_untrashed(before: Vec<Value>) -> Value {
    let did = format!("Took {} out of the Trash.", messages(before.len()));
    told(
        "Took mail out of the Trash",
        did,
        "Undo puts it back exactly as it was.",
        before,
    )
}

/// A Drive file moved to the Trash (`trashed`) or restored from it, and
/// whether it was in the Trash before (`was`).
pub fn drive_trashed(id: &str, trashed: bool, was: bool) -> Value {
    let (title, did) = match trashed {
        true => (
            "Moved a Drive file to the Trash",
            "Moved 1 Drive file to the Trash.",
        ),
        false => (
            "Restored a Drive file from the Trash",
            "Restored 1 Drive file from the Trash.",
        ),
    };
    let did = match was == trashed {
        true => format!("{did} It was there already, so nothing changed."),
        false => did.to_string(),
    };
    told(
        title,
        did,
        "Undo puts it back exactly as it was.",
        vec![drive_before(id, was)],
    )
}

/// A calendar made. It has no Undo: taking it back is a delete, which asks.
pub fn calendar_made() -> Value {
    told(
        "Made a calendar",
        "Made 1 Google calendar.".into(),
        "To take it back, delete it: that asks you first.",
        vec![],
    )
}

/// What each thing a told result changed was before it: its note's `undo`.
/// None for a node that is no private Google result with one. A call that
/// failed partway carries the part that was done.
pub fn undo_before(node: &crate::node::Node) -> Option<Vec<Value>> {
    let crate::node::Body::ToolResult { meta, .. } = &node.body else {
        return None;
    };
    if !super::private::marked(meta) {
        return None;
    }
    let before = meta[KEY]["undo"].as_array()?;
    (!before.is_empty()).then(|| before.clone())
}

/// A note's three parts, as `owner_note` heads them: its title, the
/// conversation it was done in (by its short id), and what was done and how
/// to take it back.
pub struct Parts {
    pub title: String,
    pub by: String,
    pub body: String,
}

/// The note's parts. None for a value that is not a told note.
pub fn parts(told: &Value, session_id: &str) -> Option<Parts> {
    let head = told.get("title")?.as_str()?;
    let did = told.get("did")?.as_str()?;
    let back = told.get("back").and_then(Value::as_str).unwrap_or_default();
    let by = format!(
        "In conversation {}, without asking you",
        theseus_protocol::short_id(session_id)
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
pub fn text(told: &Value, session_id: &str) -> Option<String> {
    let p = parts(told, session_id)?;
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
    fn every_note_is_headed_and_names_its_conversation_by_its_short_id() {
        let sid = "ses_0123456789abcdef";
        let short = theseus_protocol::short_id(sid);
        for t in [
            mail_trashed(vec![gmail_before("m1", &[])], 1),
            mail_trashed(vec![gmail_before("m1", &[])], 3),
            mail_untrashed(vec![gmail_before("m1", &[])]),
            drive_trashed("doc1", true, false),
            drive_trashed("doc1", false, true),
            calendar_made(),
        ] {
            let s = text(&t, sid).unwrap();
            assert!(crate::owner_note::is_headed(&s), "{s}");
            assert!(s.starts_with("ℹ️ FYI · "), "{s}");
            assert!(
                s.contains(&format!("\nIn conversation {short}, without asking you\n")),
                "{s}"
            );
            for id in ["m1", "doc1", sid] {
                assert!(!s.contains(id), "{id} in {s}");
            }
        }
        assert_eq!(text(&json!({"x": 1}), sid), None);
    }

    #[test]
    fn a_partial_trash_says_how_many_of_how_many() {
        let t = mail_trashed(vec![gmail_before("m1", &[])], 3);
        assert_eq!(
            t["did"],
            "Moved 1 of 3 Gmail messages to the Trash; then Google refused one, and the rest \
             are as they were."
        );
        let t = drive_trashed("doc1", true, true);
        assert_eq!(
            t["did"],
            "Moved 1 Drive file to the Trash. It was there already, so nothing changed."
        );
        assert_eq!(t["undo"], json!([{"drive": "doc1", "trashed": true}]));
    }
}
