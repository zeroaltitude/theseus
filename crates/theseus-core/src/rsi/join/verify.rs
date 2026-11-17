//! What the install says of itself (issue 20): the marker the unit leaves, the new daemon's check
//! of its own build against it, and the owner's notices for an install that did not happen.
//!
//! An install that restarts the service leaves a marker naming the merge it was for. The new
//! daemon, once serving, reads it: its own commit is the merge (`self.install.verified`, with the
//! notice "installed and running"), or it is not (`self.install.mismatch`, and the owner is told).
//! An install that cannot write where the service runs copies nothing and restarts nothing
//! (`self.install.not_installed`); the owner is told once, with the one step that installs it.
//! Neither path ever says a build is installed that is not what runs.

use anyhow::Result;
use serde_json::json;

use super::install::NotInstalled;
use crate::fact::rsi_install::{SelfInstallMismatch, SelfInstallNotInstalled};
use crate::owner_note::{self, Kind};
use crate::rpc::Core;
use crate::rsi::tree::Place;

/// The marker's name in the self pipeline's own directory.
const MARKER: &str = "install-pending";

/// Where the unit leaves the marker for the install of a merge.
pub(super) fn marker_of(p: &Place) -> std::path::PathBuf {
    p.root().join(MARKER)
}

/// A commit as a marker holds it: hexadecimal, nothing else.
fn commit_of(text: &str) -> Option<String> {
    let c = text.trim();
    (c.len() >= 7 && c.bytes().all(|b| b.is_ascii_hexdigit())).then(|| c.to_string())
}

fn short(c: &str) -> &str {
    &c[..c.len().min(12)]
}

impl Core {
    /// The place the self pipeline works in, when there is a repository.
    fn install_place(&self) -> Option<Place> {
        let repo = self.cfg.self_improve.repo.clone()?;
        Some(self.tools.self_pr.place(&repo))
    }

    /// The merge the unit's marker names, if there is a marker.
    fn self_install_marker(&self) -> Option<String> {
        let text = std::fs::read_to_string(marker_of(&self.install_place()?)).ok()?;
        commit_of(&text)
    }

    /// Whether the marker names `commit`.
    pub(super) fn self_install_marked(&self, commit: &str) -> bool {
        self.self_install_marker().as_deref() == Some(commit)
    }

    /// The marker has been read and answered: it goes, so nothing is told twice.
    pub(super) fn self_install_spent(&self) {
        if let Some(p) = self.install_place() {
            let _ = std::fs::remove_file(marker_of(&p));
        }
    }

    /// A marker that names a merge this build is not: the install restarted the service, and it
    /// came back on another build (or on one that names no commit, which cannot be checked). The
    /// row and the owner's notice in one frame, then the marker is spent. The merge told of.
    pub(super) fn self_install_mismatch(&self, running: Option<&str>) -> Result<Option<String>> {
        if self.cfg.self_improve.mode != theseus_protocol::rsi::SelfMode::Act {
            return Ok(None);
        }
        let Some(merge) = self.self_install_marker() else {
            return Ok(None);
        };
        if running == Some(merge.as_str()) {
            return Ok(None);
        }
        let Some(rows) = super::rows_of(&self.store, &["self.joined"])? else {
            anyhow::bail!("the ledger's index is still being built");
        };
        let branch = rows
            .iter()
            .rev()
            .find(|r| r.data["merge"] == merge.as_str())
            .and_then(|r| r.data["branch"].as_str())
            .unwrap_or("")
            .to_string();
        let fact = SelfInstallMismatch {
            merge: merge.clone(),
            branch: branch.clone(),
            running: running.map(str::to_string),
        };
        let text = mismatch_notice(&branch, &merge, running);
        let (post, mut frame) = self
            .outbox
            .stage_to_operator(None, json!({"kind": "self_join", "text": text}))?;
        let rec = self.rec(None);
        frame.push(rec.row(&fact)?);
        self.store.append(&frame)?;
        self.outbox.posted(&post);
        rec.announce(&fact);
        self.self_install_spent();
        Ok(Some(merge))
    }

    /// A joined build that was not installed: the row and the owner's notice in one frame, once
    /// for each merge. Whether it was written (false: this merge was told of before).
    pub(crate) fn self_install_not_installed(
        &self,
        branch: &str,
        item: &str,
        merge: &str,
        n: &NotInstalled,
    ) -> Result<bool> {
        let told = super::rows_of(&self.store, &["self.install.not_installed"])?
            .is_some_and(|rows| rows.iter().any(|r| r.data["merge"] == merge));
        if told {
            return Ok(false);
        }
        let fact = SelfInstallNotInstalled {
            merge: merge.into(),
            branch: branch.into(),
            why: n.why.clone(),
            target: n.target.display().to_string(),
            step: n.step.clone(),
        };
        let text = not_installed_notice(branch, item, merge, n);
        let (post, mut frame) = self
            .outbox
            .stage_to_operator(None, json!({"kind": "self_join", "text": text}))?;
        let rec = self.rec(None);
        frame.push(rec.row(&fact)?);
        self.store.append(&frame)?;
        self.outbox.posted(&post);
        rec.announce(&fact);
        Ok(true)
    }
}

/// What reached main, as the owner's notices say it: a join's branch (and item), which joined
/// main, or a revert (which has neither), which reached it.
fn change(branch: &str, item: &str) -> String {
    match (branch.is_empty(), item.is_empty()) {
        (true, _) => "A revert reached main".to_string(),
        (false, true) => format!("The change from self branch {branch} joined main"),
        (false, false) => format!("The change from self branch {branch} ({item}) joined main"),
    }
}

/// How to take a join back out of main: a join's own `self revert`. A revert has no such word (it
/// would revert the revert), so it has none.
fn undo(branch: &str, merge: &str) -> String {
    if branch.is_empty() {
        String::new()
    } else {
        format!(" To take the change back out of main instead: `theseus self revert {merge}`")
    }
}

/// The owner's notice for a build on main that was not installed: an action, because it needs
/// him. What happened, why, and the one step.
pub(crate) fn not_installed_notice(
    branch: &str,
    item: &str,
    merge: &str,
    n: &NotInstalled,
) -> String {
    let what = change(branch, item);
    let body = format!(
        "{what} as {m}, and it is not installed: {why}. So Theseus copied nothing \
         and restarted nothing; the service is still running the build from before it, and the \
         command-line programs are unchanged too.\nThe new programs are built and waiting in \
         {target}/release.\nThe one step that installs them, which needs the administrator's \
         rights:\n`{step}`\nTheseus says so when the service is running this build.{undo}",
        m = short(merge),
        why = n.why,
        target = n.target.display(),
        step = n.step,
        undo = undo(branch, merge),
    );
    owner_note::note(
        Kind::Action,
        "A change on main is not installed",
        &format!("{what} as {} and is not running", short(merge)),
        &body,
    )
}

/// The owner's notice for an install whose service came back on another build.
pub(crate) fn mismatch_notice(branch: &str, merge: &str, running: Option<&str>) -> String {
    let what = change(branch, "");
    let now = running.map_or_else(
        || "a build that does not name its commit, so Theseus cannot tell which".to_string(),
        |r| format!("the build {}", short(r)),
    );
    let body = format!(
        "{what} as {m}. Theseus copied its build in and restarted the service, but \
         the service came back running {now}, not {m}. So the change is joined and is not what \
         runs. Nothing else was changed.\nTo see what the service starts: `systemctl --user cat \
         theseusd.service` (its ExecStart line); the build it is running is in `theseus health`.\
         {undo}",
        m = short(merge),
        undo = undo(branch, merge),
    );
    owner_note::note(
        Kind::Alert,
        "An install did not take",
        &format!(
            "the service restarted on {} instead of {}",
            running.map_or("a build with no commit", short),
            short(merge)
        ),
        &body,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_marker_holds_a_commit_and_nothing_else() {
        assert_eq!(commit_of("abc1234\n").as_deref(), Some("abc1234"));
        assert_eq!(
            commit_of("ABCDEF0123456789"),
            Some("ABCDEF0123456789".into())
        );
        for bad in ["", "abc12", "abc1234; rm -rf /", "../x/y/zzzzzzz"] {
            assert_eq!(commit_of(bad), None, "{bad:?}");
        }
    }
    /// A revert's notices carry no `self revert` of the revert's own commit (that would bring the
    /// reverted change back); a join's do.
    #[test]
    fn only_a_joins_notices_offer_the_revert() {
        let merge = "0123456789abcdef0123456789abcdef01234567";
        let n = NotInstalled {
            why: "w".into(),
            target: "/t".into(),
            step: "s".into(),
        };
        let revert = not_installed_notice("", "", merge, &n);
        assert!(
            revert.contains("A revert reached main as 0123456789ab"),
            "{revert}"
        );
        assert!(!revert.contains("self revert"), "{revert}");
        assert!(!mismatch_notice("", merge, None).contains("self revert"));
        let join = not_installed_notice("b", "i", merge, &n);
        assert!(
            join.contains(&format!("`theseus self revert {merge}`")),
            "{join}"
        );
        assert!(mismatch_notice("b", merge, None).contains("theseus self revert"));
    }
}
