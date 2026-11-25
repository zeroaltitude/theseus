//! A note that tells the owner of an act Theseus did without asking, with its
//! **Undo** (issue 57). One helper, [`stage`], used by every act that stopped
//! asking: an FYI to the operator in the caller's fixed words, carrying an
//! [`Undo`]. Its text ends with the command that undoes it.
//!
//! **Where it is seen (the conductor's scope, 2026-11-25):** the note is an
//! operator post that Discord delivers, to the notices place when one is
//! bound, else the owner's DM, with an Undo button. No other surface (the
//! TUI, the cockpit, `theseus answer`) shows a told note today; that is a
//! later ticket.
//!
//! The press is `told.undo` as the presser (`rpc/told.rs`): one act, whatever
//! the undo calls underneath, judged by the general rule, the owner from a
//! private place as the daemon judges it, and refused first to a connection
//! whose own peer reading names an agent or a job. A local process the
//! daemon reads as the owner's terminal counts as the owner, so a counted
//! undo is not proof of a human press. The core keeps the note (its post) and
//! how its undo settled, so a press works after a restart and counts once.
//! The stored words stay as written: every Discord send and edit, and every
//! copy `told.undo` returns, masks them at the exit.

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
    /// `standing.revoke`: a standing order or record stops holding.
    #[serde(rename = "standing.revoke")]
    StandingRevoke(String),
    /// `self.drop`: a self backlog item is dropped.
    #[serde(rename = "self.drop")]
    SelfDrop(String),
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
            Undo::StandingRevoke(_) => method::STANDING_REVOKE,
            Undo::SelfDrop(_) => method::SELF_DROP,
            #[cfg(test)]
            Undo::Probe(_) => method::TOLD_UNDO,
        }
    }

    /// The method's params as the presser's: their name, and where they
    /// pressed wherever the method takes it.
    pub fn params(&self, author: &str, discord: Option<DiscordOrigin>) -> Value {
        match self {
            Undo::StandingRevoke(id) => {
                json!({"id": id, "author": author, "discord": discord})
            }
            Undo::SelfDrop(id) => json!({"item": id, "author": author, "discord": discord}),
            #[cfg(test)]
            Undo::Probe(id) => json!({"id": id, "author": author}),
        }
    }

    /// The command that does the same from a shell. An undo with no method
    /// of its own answers [`by_door`]: the note's own `theseus told undo`.
    pub fn command(&self) -> String {
        match self {
            Undo::StandingRevoke(id) => format!("theseus standing revoke {id}"),
            Undo::SelfDrop(id) => format!("theseus self drop {id}"),
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
///
/// **The contract.** `title`, `by` and `body` are the caller's fixed words
/// with ids, counts, schedules, settings names and the like: never
/// model-written or outside text. A conversation is named by its short id,
/// never by its title, which is text someone else may have written. The
/// exit's masking is a backstop for the vault's registered secret values
/// only; it is not a filter for outside text, and it is not applied here.
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

/// Who settled a note's undo, when, and the line the note ended with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Undone {
    pub by: String,
    pub at_ms: u64,
    /// The note's last line once settled ([`undone_line`], [`found_line`] or
    /// [`moved_line`]); none on a mark this lane wrote before it was kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
}

impl Undone {
    /// The note's last line once settled.
    pub fn line(&self) -> String {
        self.line.clone().unwrap_or_else(|| undone_line(&self.by))
    }

    /// Whether the undo ran (rather than finding nothing left to do, or the
    /// act moved on).
    pub fn ran(&self) -> bool {
        self.line() == undone_line(&self.by)
    }
}

fn undone_key(post: &str) -> String {
    format!("told.undone:{post}")
}

/// Whether the note `post` was undone, and by whom.
pub fn undone(store: &Store, post: &str) -> Result<Option<Undone>> {
    store.get_meta(&undone_key(post))
}

/// Write that the note `post`'s undo settled, by `by`, with `line` its last.
pub fn mark_undone(store: &Store, post: &str, by: &str, line: &str) -> Result<()> {
    let u = Undone {
        by: by.to_string(),
        at_ms: theseus_protocol::now_unix_ms(),
        line: Some(line.to_string()),
    };
    store.append(&[NewRecord::json(kinds::META, Some(&undone_key(post)), &u)?])?;
    Ok(())
}

/// The note's text without its Undo line.
fn without_undo(text: &str) -> String {
    let kept: Vec<&str> = text.lines().filter(|l| !l.starts_with("Undo: `")).collect();
    kept.join("\n")
}

/// The note's text once settled: the Undo line goes, `line` ends it.
pub fn settled_text(text: &str, line: &str) -> String {
    format!("{}\n{line}", without_undo(text))
}

/// The last line of a note whose undo ran.
pub fn undone_line(by: &str) -> String {
    format!("↩️ Undone by {by}.")
}

/// The last line of a note whose undo's outcome was found already there.
pub fn found_line(found: &str) -> String {
    format!("↩️ Nothing left to undo: {found}.")
}

/// The last line of a note whose act moved on before its Undo was pressed:
/// the undo no longer applies, and `command` is the ordinary way now.
pub fn moved_line(command: &str) -> String {
    format!(
        "↩️ This Undo no longer applies: the item has moved on since it was filed. To drop it \
         as it is now: `{command}`."
    )
}

/// The note's text once undone: the Undo line goes, a line says who undid it.
pub fn undone_text(text: &str, by: &str) -> String {
    settled_text(text, &undone_line(by))
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

    /// A restart: the core gone and its store opened again under a new one.
    /// The note's words and undo, and that it was undone, are the store's.
    #[tokio::test]
    async fn the_note_and_its_undone_survive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let core = crate::tests_unlabel::core_in(dir.path());
        let undo = Undo::StandingRevoke("std_1".into());
        let (a, frame) = stage(
            &core.outbox,
            None,
            "Kept a standing order",
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
            "ℹ️ FYI · Kept a standing order\nIn “Tides”, without asking you\n\
             Undo: `theseus standing revoke std_1`"
        );
        assert!(undone(&core.store, &post).unwrap().is_none());
        mark_undone(&core.store, &post, "ana", &found_line("it is gone")).unwrap();
        let core = reopened(core, dir.path()).await;
        let u = undone(&core.store, &post).unwrap().unwrap();
        assert_eq!(u.by, "ana");
        assert_eq!(u.line(), "↩️ Nothing left to undo: it is gone.");
        assert!(!u.ran());
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

    /// A stored note whose undo names a tag this build no longer has (the cut
    /// `wake.cancel`, `routine.delete`) is refused when read: an error, no
    /// panic, nothing run.
    #[tokio::test]
    async fn a_note_with_a_cut_undo_refuses_to_decode() {
        let dir = tempfile::tempdir().unwrap();
        let core = crate::tests_unlabel::core_in(dir.path());
        for gone in ["wake.cancel", "routine.delete"] {
            let (a, frame) = core
                .outbox
                .stage_to_operator(
                    None,
                    json!({"kind": KIND, "text": "ℹ️ FYI · old",
                           "undo": {"method": gone, "id": "x_1"}}),
                )
                .unwrap();
            core.store.append(&frame).unwrap();
            core.outbox.posted(&a);
            let e = of(&core.kernel, &a.correlation_id).unwrap_err();
            assert!(e.to_string().contains(gone), "{e}");
        }
    }

    /// A mark this lane wrote before it kept the line reads as an undo that
    /// ran.
    #[test]
    fn a_mark_with_no_line_is_an_undo_that_ran() {
        let u: Undone = serde_json::from_value(json!({"by": "ana", "at_ms": 1})).unwrap();
        assert_eq!(u.line(), "↩️ Undone by ana.");
        assert!(u.ran());
    }
}
