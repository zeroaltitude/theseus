//! A note that tells the owner of an act Theseus did without asking, with its
//! **Undo** (issue 57). One helper, [`stage`], used by every act that stopped
//! asking: an FYI to the operator in the caller's plain words, carrying an
//! [`Undo`]. The note is an operator post: Discord delivers it (to the
//! notices place when one is bound, else the owner's DM), with an Undo
//! button; other surfaces show it only where they already show operator
//! posts, and today none of them reads a told note's words. Its text ends
//! with the command that undoes it, so wherever the words are read they
//! carry the way back.
//!
//! The press is `told.undo` as the presser (`rpc/told.rs`): one act, whatever
//! the undo calls underneath, judged by the general rule, the owner from a
//! private place as the daemon judges it. A local process on the owner's
//! socket counts as the owner's terminal, as for every owner-only command, so
//! a counted undo is not proof of a human press. The core keeps the note (its
//! post) and whether it was undone, so a press works after a restart and
//! counts once. The stored words stay as written: every send and edit masks
//! them at the exit.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use theseus_kernel::{Action, Kernel};
use theseus_protocol::DiscordOrigin;
use theseus_store::{kinds, NewRecord};

use super::{note, Kind};
use crate::outbox::{body_of, Outbox};
use crate::store::Store;

/// The note's post kind.
pub const KIND: &str = "told";

/// The existing call that undoes what a note tells: the method, and the id it
/// takes. Nothing else is carried, so a note can only ask for what the method
/// itself allows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", content = "id")]
pub enum Undo {
    /// `routine.delete`: a routine's schedule and any run going end.
    #[serde(rename = "routine.delete")]
    RoutineDelete(String),
    /// `wake.cancel`: a pending wake or watch fires nothing.
    #[serde(rename = "wake.cancel")]
    WakeCancel(String),
    /// `standing.revoke`: a standing order or record stops holding.
    #[serde(rename = "standing.revoke")]
    StandingRevoke(String),
    /// `self.drop`: a self backlog item is dropped.
    #[serde(rename = "self.drop")]
    SelfDrop(String),
    /// A tidy Google act's own opposite (an untrash, a restore, a trash
    /// again), named by the act's result node (`google::told`). It has no
    /// method of its own: `told.undo` runs it inside the core.
    #[serde(rename = "google.take_back")]
    GoogleTakeBack(String),
    /// An undo with no method of its own, for the tests: its arm in
    /// `rpc::told` counts the times it ran.
    #[cfg(test)]
    #[serde(rename = "test.probe")]
    Probe(String),
}

impl Undo {
    /// The method a press calls.
    pub fn method(&self) -> &'static str {
        use theseus_protocol::method;
        match self {
            Undo::RoutineDelete(_) => method::ROUTINE_DELETE,
            Undo::WakeCancel(_) => method::WAKE_CANCEL,
            Undo::StandingRevoke(_) => method::STANDING_REVOKE,
            Undo::SelfDrop(_) => method::SELF_DROP,
            Undo::GoogleTakeBack(_) => method::TOLD_UNDO,
            #[cfg(test)]
            Undo::Probe(_) => method::TOLD_UNDO,
        }
    }

    /// The method's params as the presser's: their name, and where they
    /// pressed wherever the method takes it.
    pub fn params(&self, author: &str, discord: Option<DiscordOrigin>) -> Value {
        match self {
            Undo::RoutineDelete(id) | Undo::StandingRevoke(id) => {
                json!({"id": id, "author": author, "discord": discord})
            }
            Undo::GoogleTakeBack(id) => json!({"id": id, "author": author}),
            Undo::WakeCancel(id) => json!({"wake": id, "author": author}),
            Undo::SelfDrop(id) => json!({"item": id, "author": author, "discord": discord}),
            #[cfg(test)]
            Undo::Probe(id) => json!({"id": id, "author": author}),
        }
    }

    /// The command that does the same from a shell. An undo with no method
    /// of its own answers [`by_door`]: the note's own `theseus told undo`.
    pub fn command(&self) -> String {
        match self {
            Undo::RoutineDelete(id) => format!("theseus routine rm {id}"),
            Undo::WakeCancel(id) => format!("theseus cancel {id}"),
            Undo::StandingRevoke(id) => format!("theseus standing revoke {id}"),
            Undo::SelfDrop(id) => format!("theseus self drop {id}"),
            Undo::GoogleTakeBack(_) => by_door(),
            #[cfg(test)]
            Undo::Probe(_) => by_door(),
        }
    }
}

/// Where a note names its own post id. The id is minted when the note is
/// staged, after its words are written, so the words carry this mark and
/// every reader of a note fills it ([`filled`]: [`of`], and Discord's send).
/// The stored words keep the mark.
pub const THIS_POST: &str = "<this-note>";

/// The command that undoes a note through its own door from a terminal,
/// `theseus told undo <its post id>`: the `command()` of an undo with no
/// method of its own.
pub fn by_door() -> String {
    format!("theseus told undo {THIS_POST}")
}

/// A note's words as read: its own post id where they name it.
pub fn filled(text: &str, post: &str) -> String {
    text.replace(THIS_POST, post)
}

/// Stage the note to the operator: `title` (what was done), `by` (which
/// conversation did it, the note's second line), `body` (more, or empty),
/// then the command that undoes it. Append the records with the act's own (or
/// alone), then `outbox.posted` the action, as `stage_to_operator`'s callers do.
pub fn stage(
    outbox: &Outbox,
    session: Option<&str>,
    title: &str,
    by: &str,
    body: &str,
    undo: &Undo,
) -> Result<(Action, Vec<NewRecord>)> {
    let undo_line = format!("Undo: `{}`", undo.command());
    let body = match body.trim_end() {
        "" => undo_line,
        b => format!("{b}\n{undo_line}"),
    };
    let text = note(Kind::Fyi, title, by, &body);
    outbox.stage_to_operator(session, json!({"kind": "told", "text": text, "undo": undo}))
}

/// A note's text and its undo, by its post's id: none for any other post.
pub fn of(kernel: &Kernel, post: &str) -> Result<Option<(String, Undo)>> {
    let Some(a) = kernel.outbox_action(post)? else {
        return Ok(None);
    };
    let body = body_of(&a);
    if body["kind"] != KIND {
        return Ok(None);
    }
    let undo = serde_json::from_value(body["undo"].clone())?;
    Ok(Some((
        filled(body["text"].as_str().unwrap_or(""), post),
        undo,
    )))
}

/// The note `name` names: its post id, or the end of one no other told
/// note's id shares. None when no told note has it.
pub fn resolve(kernel: &Kernel, name: &str) -> Result<Option<String>> {
    if of(kernel, name)?.is_some() {
        return Ok(Some(name.to_string()));
    }
    if name.is_empty() {
        return Ok(None);
    }
    let all: Vec<String> = kernel
        .outbox_actions()?
        .into_iter()
        .filter(|a| body_of(a)["kind"] == KIND && a.correlation_id.ends_with(name))
        .map(|a| a.correlation_id)
        .collect();
    match all.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some(one.clone())),
        many => anyhow::bail!(
            "`{name}` ends {} notes' ids ({}): give more of it",
            many.len(),
            many.join(", ")
        ),
    }
}

/// Who undid a note, and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Undone {
    pub by: String,
    pub at_ms: u64,
}

fn undone_key(post: &str) -> String {
    format!("told.undone:{post}")
}

/// Whether the note `post` was undone, and by whom.
pub fn undone(store: &Store, post: &str) -> Result<Option<Undone>> {
    store.get_meta(&undone_key(post))
}

/// Write that the note `post` was undone by `by`.
pub fn mark_undone(store: &Store, post: &str, by: &str) -> Result<()> {
    let u = Undone {
        by: by.to_string(),
        at_ms: theseus_protocol::now_unix_ms(),
    };
    store.append(&[NewRecord::json(kinds::META, Some(&undone_key(post)), &u)?])?;
    Ok(())
}

/// The note's text without its Undo line.
fn without_undo(text: &str) -> String {
    let kept: Vec<&str> = text.lines().filter(|l| !l.starts_with("Undo: `")).collect();
    kept.join("\n")
}

/// The note's text once undone: the Undo line goes, a line says who undid it.
pub fn undone_text(text: &str, by: &str) -> String {
    format!("{}\n↩️ Undone by {by}.", without_undo(text))
}

/// The note's text when its undo's outcome was found already there (an
/// earlier press got that far): the Undo line goes, a line says what was found.
pub fn found_text(text: &str, found: &str) -> String {
    format!("{}\n↩️ Nothing left to undo: {found}.", without_undo(text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::method;

    /// Each undo is a method the core has, with the id under the name the
    /// method reads, and the command a shell types.
    #[test]
    fn each_undo_names_an_existing_method_its_params_and_its_command() {
        let id = "rtn_1".to_string();
        let cases = [
            (
                Undo::RoutineDelete(id.clone()),
                method::ROUTINE_DELETE,
                "id",
                "theseus routine rm rtn_1",
            ),
            (
                Undo::WakeCancel(id.clone()),
                method::WAKE_CANCEL,
                "wake",
                "theseus cancel rtn_1",
            ),
            (
                Undo::StandingRevoke(id.clone()),
                method::STANDING_REVOKE,
                "id",
                "theseus standing revoke rtn_1",
            ),
            (
                Undo::SelfDrop(id.clone()),
                method::SELF_DROP,
                "item",
                "theseus self drop rtn_1",
            ),
        ];
        for (u, m, field, cmd) in cases {
            assert_eq!(u.method(), m);
            assert!(method::ALL.contains(&m), "{m}");
            assert_eq!(
                serde_json::to_value(&u).unwrap(),
                json!({"method": m, "id": id})
            );
            let p = u.params("ana", None);
            assert_eq!(p[field], json!(id), "{m}");
            assert_eq!(p["author"], json!("ana"));
            assert_eq!(u.command(), cmd);
        }
    }

    /// An undo with no method of its own runs inside `told.undo`: its method
    /// is that door's, its tag names it in the note's body alone, and its
    /// way back from a terminal is the door's own verb.
    #[test]
    fn a_google_take_back_has_no_method_of_its_own() {
        let u = Undo::GoogleTakeBack("nod_1".into());
        assert_eq!(u.method(), method::TOLD_UNDO);
        assert_eq!(
            serde_json::to_value(&u).unwrap(),
            json!({"method": "google.take_back", "id": "nod_1"})
        );
        assert!(!method::ALL.contains(&"google.take_back"));
        assert_eq!(
            u.params("ana", None),
            json!({"id": "nod_1", "author": "ana"})
        );
        assert_eq!(u.command(), "theseus told undo <this-note>");
    }

    /// A restart: the core gone and its store opened again under a new one.
    /// The note's words and undo, and that it was undone, are the store's.
    #[tokio::test]
    async fn the_note_and_its_undone_survive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let core = crate::tests_unlabel::core_in(dir.path());
        let undo = Undo::WakeCancel("wke_1".into());
        let (a, frame) = stage(
            &core.outbox,
            None,
            "Set a watch",
            "In “Tides”, without asking you",
            "",
            &undo,
        )
        .unwrap();
        core.store.append(&frame).unwrap();
        core.outbox.posted(&a);
        let post = a.correlation_id.clone();
        let core = reopened(core, dir.path()).await;
        let (text, got) = of(&core.kernel, &post).unwrap().unwrap();
        assert_eq!(got, undo);
        assert_eq!(
            text,
            "ℹ️ FYI · Set a watch\nIn “Tides”, without asking you\nUndo: `theseus cancel wke_1`"
        );
        assert!(undone(&core.store, &post).unwrap().is_none());
        mark_undone(&core.store, &post, "ana").unwrap();
        let core = reopened(core, dir.path()).await;
        assert_eq!(
            undone(&core.store, &post).unwrap().map(|u| u.by).as_deref(),
            Some("ana")
        );
        assert!(of(&core.kernel, "no_such_post").unwrap().is_none());
    }

    async fn reopened(
        core: std::sync::Arc<crate::rpc::Core>,
        dir: &std::path::Path,
    ) -> std::sync::Arc<crate::rpc::Core> {
        drop(core);
        for _ in 0..50 {
            if let Ok(s) = Store::open(&dir.join("store")) {
                drop(s);
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        crate::tests_unlabel::core_in(dir)
    }

    #[test]
    fn an_undone_note_loses_its_undo_line_and_says_who() {
        let t = note(
            Kind::Fyi,
            "Made a routine",
            "In “Trip”, without asking you",
            "Undo: `theseus routine rm r1`",
        );
        assert_eq!(
            undone_text(&t, "ana"),
            "ℹ️ FYI · Made a routine\nIn “Trip”, without asking you\n↩️ Undone by ana."
        );
    }
}
