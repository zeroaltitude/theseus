//! Whether the build that runs contains a joined commit (issue 20): after a hand install of a later build of main, the
//! join runs inside it, and the words must say so. The daemon tells from its own clone: the running build's commit
//! (its stamp) has the join as an ancestor (`git merge-base --is-ancestor`). When the clone cannot say, it says why.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use super::words::Contained;
use crate::rpc::Core;
use crate::rsi::tree::Place;

/// Answers that history cannot change: a commit's ancestry is fixed, so `Yes` and `No` are kept (by clone, join and
/// running commit); `CannotTell` is not, since a fetch may answer it.
type Known = HashMap<(PathBuf, String, String), Contained>;
static KNOWN: Mutex<Option<Known>> = Mutex::new(None);

fn is_hex(c: &str) -> bool {
    (7..=40).contains(&c.len()) && c.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Whether the commit `c` is in the clone.
fn present(p: &Place, c: &str) -> bool {
    p.git(
        &p.repo,
        &[
            "--no-replace-objects",
            "cat-file",
            "-e",
            &format!("{c}^{{commit}}"),
        ],
    )
    .is_ok_and(|o| o.status.success())
}

/// The answer for `merge` against the build whose commit is `running`, from the clone `p`.
pub(super) fn contained_in(p: &Place, merge: &str, running: Option<&str>) -> Contained {
    let Some(running) = running else {
        return Contained::CannotTell("this build does not name its commit".into());
    };
    if running == merge {
        return Contained::Yes(running.into());
    }
    if !is_hex(running) || !is_hex(merge) {
        return Contained::CannotTell("a commit is not written as a commit".into());
    }
    let short = |c: &str| c[..c.len().min(12)].to_string();
    if !present(p, merge) {
        return Contained::CannotTell(format!(
            "this daemon's copy of the repository does not have the join, {}",
            short(merge)
        ));
    }
    if !present(p, running) {
        return Contained::CannotTell(format!(
            "this daemon's copy of main does not have the running build's commit, {}: it has not fetched since that build was made",
            short(running)
        ));
    }
    classify(
        p.git(
            &p.repo,
            &[
                "--no-replace-objects",
                "merge-base",
                "--is-ancestor",
                merge,
                running,
            ],
        ),
        running,
    )
}

/// What `git merge-base --is-ancestor <join> <running>` came to: exit 0 is yes and exit 1 is no, and anything else (any
/// other code, a signal, a command that could not be run) is that it cannot tell, never a yes.
pub(super) fn classify(result: anyhow::Result<std::process::Output>, running: &str) -> Contained {
    match result {
        Ok(o) if o.status.success() => Contained::Yes(running.into()),
        Ok(o) if o.status.code() == Some(1) => Contained::No(running.into()),
        Ok(o) => Contained::CannotTell(format!(
            "git could not compare them ({}): {}",
            o.status,
            String::from_utf8_lossy(&o.stderr).trim()
        )),
        Err(e) => Contained::CannotTell(format!("git could not be run: {e:#}")),
    }
}

impl Core {
    /// Whether the build `running` contains `merge`, by this daemon's own clone (`[self] repo`); kept once known.
    pub(crate) fn running_contains(&self, merge: &str, running: Option<&str>) -> Contained {
        let Some(repo) = self.cfg.self_improve.repo.clone() else {
            return Contained::CannotTell("this daemon has no [self] repo to look in".into());
        };
        // The build that is the join, while the install of it waits for the new daemon's confirmation, may yet go back.
        if self.root_install_pending_for(merge) {
            return Contained::CannotTell(
                "its install is still waiting for the new daemon to confirm itself".into(),
            );
        }
        let place = self.tools.self_pr.place(&repo);
        let key = running.map(|r| (place.repo.clone(), merge.to_string(), r.to_string()));
        if let Some(k) = &key {
            let known = KNOWN
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(a) = known.as_ref().and_then(|m| m.get(k)) {
                return a.clone();
            }
        }
        let answer = contained_in(&place, merge, running);
        if let (Some(k), Contained::Yes(_) | Contained::No(_)) = (key, &answer) {
            KNOWN
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get_or_insert_with(HashMap::new)
                .insert(k, answer.clone());
        }
        answer
    }
}
