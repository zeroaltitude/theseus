//! The revert itself (theseus-pw1q.10), under the join lock the self-join
//! holds: fetch, `git revert -m 1 <merge>` in a detached tree off main, main's
//! gate on it through the door with the benches, the signature, the push of
//! main (never forced; main moved redoes the try), the row and the owner's
//! notice in one frame, and the install by the self-join's path.
//!
//! An automatic revert asks `rsi::gate` before every phase and stops on
//! anything but `Allowed`; the owner's veto asks nothing, it is their act. A
//! revert that cannot reach main (a conflict, a red gate, a keel finding, no
//! signing key, main moving every try) posts the owner's card and holds: the
//! change is still on main, and the owner decides.

use std::sync::Arc;

use anyhow::{bail, Result};
use serde_json::{json, Value};

use super::attribute::Joined;
use super::{Cause, Seams};
use crate::fact::rsi_join::SelfJoinStopped;
use crate::fact::rsi_revert::{SelfReverted, SelfVetoed};
use crate::rpc::Core;
use crate::rsi::join::{blocking, git, lock, lock_dir, refused, remote_of, TRIES};
use crate::rsi::tree::{self, Place, Tree, Verdict};

/// How a revert ended.
#[derive(Debug, Clone, PartialEq)]
pub enum Done {
    /// The revert commit is on main.
    Reverted(String),
    /// It stopped before main, and why. With a `card` the owner's card says so
    /// and the revert is theirs; without one (a halt, a lock that stayed held,
    /// a gate that did not run) nothing was decided and it may be tried again.
    Held {
        phase: String,
        why: String,
        card: bool,
    },
}

/// A revert made in its tree: the tree, the main it is on, its commit.
struct Made {
    t: Tree,
    base: String,
}

impl Core {
    /// Revert `j` off main under the join lock.
    pub(super) async fn revert_join(
        self: &Arc<Self>,
        seams: &Seams,
        j: &Joined,
        cause: &Cause,
    ) -> Result<Done> {
        let Some(repo) = self.cfg.self_improve.repo.clone() else {
            bail!("[self] repo is not set, so there is no repository to revert in");
        };
        let place = self.tools.self_pr.place(&repo);
        let remote = remote_of(&self.cfg);
        let auto = matches!(cause, Cause::Auto { .. });
        let core = self.clone();
        let holder = format!(
            "theseusd self-revert {} (pid {})",
            short(&j.merge),
            std::process::id()
        );
        let held = lock::take(
            &lock_dir(&self.cfg),
            &holder,
            seams.lock_wait,
            seams.lock_every,
            move || auto.then(|| refused(&core)).flatten(),
        )
        .await?;
        let held = match held {
            Ok(h) => h,
            Err(why) => return Ok(self.revert_held(j, "lock", &why, false)),
        };
        let done = self.reverting(seams, &place, &remote, j, cause).await;
        let said = match &done {
            Ok(Done::Reverted(r)) => format!("reverted {} as {r}", short(&j.merge)),
            Ok(Done::Held { phase, .. }) => {
                format!("stopped reverting {} at {phase}", short(&j.merge))
            }
            Err(e) => format!("failed reverting {}: {e:#}", short(&j.merge)),
        };
        if let Err(e) = held.release(&said) {
            tracing::warn!(error = %format!("{e:#}"), "self-revert: the join lock was not released");
        }
        let done = done?;
        if let Done::Reverted(r) = &done {
            if self.cfg.self_improve.install && (!auto || refused(self).is_none()) {
                let (p, i, r) = (place.clone(), seams.installer.clone(), r.clone());
                if let Err(e) = blocking(move || i.start(&p, &r)).await {
                    self.revert_held(
                        j,
                        "install",
                        &format!("the install did not start: {e:#}"),
                        false,
                    );
                }
            }
        }
        Ok(done)
    }

    /// Under the lock: the tries.
    async fn reverting(
        self: &Arc<Self>,
        seams: &Seams,
        place: &Place,
        remote: &str,
        j: &Joined,
        cause: &Cause,
    ) -> Result<Done> {
        let auto = matches!(cause, Cause::Auto { .. });
        for _ in 0..TRIES {
            if let Some(held) = self.revert_phase(seams, j, "revert", auto) {
                return Ok(held);
            }
            let made = {
                let (p, r, jj) = (place.clone(), remote.to_string(), j.clone());
                blocking(move || make(&p, &r, &jj)).await?
            };
            let made = match made {
                Ok(m) => m,
                Err(why) => return Ok(self.revert_held(j, "revert", &why, true)),
            };
            let closing = |t: &Tree| {
                let _ = tree::close(place, t);
            };
            if let Some(held) = self.revert_phase(seams, j, "gate", auto) {
                closing(&made.t);
                return Ok(held);
            }
            let run = match tree::gate_with_benches(place, &made.t).await {
                Ok(tree::Gated::Ran(r)) => r,
                Ok(tree::Gated::Stopped(why)) => {
                    closing(&made.t);
                    let why = format!("stopped while main's gate on the revert waited: {why}");
                    return Ok(self.revert_held(j, "gate", &why, false));
                }
                Err(e) => {
                    closing(&made.t);
                    let why = format!("main's gate on the revert did not run: {e:#}");
                    return Ok(self.revert_held(j, "gate", &why, false));
                }
            };
            if run.verdict != Verdict::Green {
                closing(&made.t);
                let phase = if run.verdict == Verdict::Keel {
                    "keel"
                } else {
                    "gate"
                };
                let why = format!(
                    "main's gate on the revert is {} (exit {:?}), so it needs the owner's yes; \
                     gate log {}",
                    run.verdict.name(),
                    run.exit,
                    run.log.display()
                );
                return Ok(self.revert_held(j, phase, &why, true));
            }
            if let Some(held) = self.revert_phase(seams, j, "sign", auto) {
                closing(&made.t);
                return Ok(held);
            }
            let Some(key) = git::signing_key(place) else {
                closing(&made.t);
                let why = "[self] repo names no user.signingkey, and an unsigned commit never \
                           reaches main";
                return Ok(self.revert_held(j, "sign", why, true));
            };
            let (p, t) = (place.clone(), made.t.clone());
            let signed = match blocking(move || git::sign(&p, &t, &key)).await {
                Ok(s) => s,
                Err(e) => {
                    closing(&made.t);
                    return Ok(self.revert_held(j, "sign", &format!("{e:#}"), true));
                }
            };
            if let Some(held) = self.revert_phase(seams, j, "push", auto) {
                closing(&made.t);
                return Ok(held);
            }
            let (p, t, r, base, s) = (
                place.clone(),
                made.t.clone(),
                remote.to_string(),
                made.base.clone(),
                signed.clone(),
            );
            let pushed = blocking(move || git::push_main(&p, &t, &r, &base, &s)).await;
            closing(&made.t);
            match pushed {
                Ok(git::Pushed::Done) => {
                    self.revert_recorded(j, cause, &signed, &run.secs.to_string());
                    return Ok(Done::Reverted(signed));
                }
                Ok(git::Pushed::Moved(_)) => {}
                Err(e) => return Ok(self.revert_held(j, "push", &format!("{e:#}"), true)),
            }
        }
        let why = format!("main moved before each of {TRIES} pushes");
        Ok(self.revert_held(j, "push", &why, true))
    }

    /// Before a phase: the test's hook, then (an automatic revert) the gate.
    fn revert_phase(&self, seams: &Seams, j: &Joined, name: &str, auto: bool) -> Option<Done> {
        if let Some(f) = &seams.on_phase {
            f(name);
        }
        let why = if auto { refused(self) } else { None }?;
        Some(self.revert_held(j, name, &why, false))
    }

    /// A stop: the row, and (`card`) the owner's card.
    fn revert_held(&self, j: &Joined, phase: &str, why: &str, card: bool) -> Done {
        self.rec(None).record(&SelfJoinStopped {
            branch: format!("revert {}", short(&j.merge)),
            head: j.merge.clone(),
            phase: phase.into(),
            why: why.into(),
            numbers: if card {
                json!({"held": true})
            } else {
                Value::Null
            },
        });
        if card {
            self.revert_card(held_text(j, phase, why));
        }
        Done::Held {
            phase: phase.into(),
            why: why.into(),
            card,
        }
    }

    fn revert_card(&self, text: String) {
        let sent = (|| -> Result<()> {
            let (post, frame) = self
                .outbox
                .stage_to_operator(None, json!({"kind": "self_join", "text": text}))?;
            self.store.append(&frame)?;
            self.outbox.posted(&post);
            Ok(())
        })();
        if let Err(e) = sent {
            tracing::warn!(error = %format!("{e:#}"), "self-revert: the owner's card was not posted");
        }
    }

    /// The row and the owner's notice, in one frame; then the fact's sentences.
    fn revert_recorded(&self, j: &Joined, cause: &Cause, revert: &str, gate_secs: &str) {
        let window = self.cfg.self_improve.revert_window_hours;
        match cause {
            Cause::Auto {
                signal,
                detail,
                tested,
            } => {
                let f = SelfReverted {
                    merge: j.merge.clone(),
                    revert: revert.into(),
                    signal: (*signal).into(),
                    window,
                    branch: j.branch.clone(),
                    item: j.item.clone(),
                    numbers: json!({"detail": detail, "bisected": tested, "gate_secs": gate_secs}),
                };
                let text = reverted_text(
                    j,
                    revert,
                    signal,
                    window,
                    detail,
                    *tested,
                    self.cfg.self_improve.install,
                );
                self.write_with_notice(&f, text);
            }
            Cause::Veto { by, place, dropped } => {
                let f = SelfVetoed {
                    merge: j.merge.clone(),
                    revert: revert.into(),
                    by: by.clone(),
                    place: place.clone(),
                    branch: j.branch.clone(),
                    item: j.item.clone(),
                    dropped: *dropped,
                };
                let text = vetoed_text(j, revert);
                self.write_with_notice(&f, text);
            }
        }
    }

    fn write_with_notice<F: crate::fact::Fact>(&self, fact: &F, text: String) {
        let rec = self.rec(None);
        let staged = (|| -> Result<_> {
            let (post, mut frame) = self
                .outbox
                .stage_to_operator(None, json!({"kind": "self_join", "text": text}))?;
            frame.push(rec.row(fact)?);
            self.store.append(&frame)?;
            self.outbox.posted(&post);
            Ok(())
        })();
        match staged {
            Ok(()) => rec.announce(fact),
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "self-revert: the row and the notice were not written");
            }
        }
    }
}

/// Fetch, check the merge is on main, and revert it in a detached tree off
/// main: the tree, or why not (a conflict, or not on main).
fn make(p: &Place, remote: &str, j: &Joined) -> Result<std::result::Result<Made, String>> {
    git::fetch(p, remote)?;
    let base = git::main_of(p, remote)?;
    let on_main = p.git(&p.repo, &["merge-base", "--is-ancestor", &j.merge, &base])?;
    if !on_main.status.success() {
        return Ok(Err(format!("{} is not on main", short(&j.merge))));
    }
    // A revert whose row was never written (a crash after its push) must not be
    // made again: its message names the join it undoes.
    let undone = p.git_ok(
        &p.repo,
        &[
            "log",
            "-1",
            "--format=%H",
            "--fixed-strings",
            &format!("--grep=undoes the self-join {}", j.merge),
            &base,
        ],
    )?;
    if !undone.trim().is_empty() {
        return Ok(Err(format!(
            "{} is already reverted on main by {}, though no row says so: check main, and do \
             not revert it again",
            short(&j.merge),
            short(undone.trim())
        )));
    }
    let t = git::detached(p, "revert", &j.branch, &base)?;
    let who = p.ident_args();
    let mut ident: Vec<&str> = who.iter().map(String::as_str).collect();
    ident.extend(["-c", "commit.gpgsign=false"]);
    let mut args = ident.clone();
    args.extend(["revert", "-m", "1", "--no-commit", &j.merge]);
    let reverted = p.git(&t.dir, &args)?;
    if !reverted.status.success() {
        let paths = p
            .git_ok(&t.dir, &["diff", "--name-only", "--diff-filter=U"])
            .unwrap_or_default();
        let _ = p.git(&t.dir, &["revert", "--abort"]);
        let _ = tree::close(p, &t);
        return Ok(Err(format!(
            "reverting {} conflicts with what came after it, in {}",
            short(&j.merge),
            paths.lines().collect::<Vec<_>>().join(", ")
        )));
    }
    let message = format!(
        "Revert \"Merge {}\" ({})\n\nA self-revert (theseus-pw1q.10): undoes the self-join {}.",
        j.branch, j.item, j.merge
    );
    let mut args = ident.clone();
    args.extend([
        "commit",
        "-q",
        "--no-verify",
        "--allow-empty",
        "-m",
        &message,
    ]);
    p.git_ok(&t.dir, &args)?;
    Ok(Ok(Made { t, base }))
}

fn short(c: &str) -> &str {
    &c[..c.len().min(12)]
}

/// The card for a revert that stopped (theseus-x8b9k).
pub(crate) fn held_text(j: &Joined, phase: &str, why: &str) -> String {
    crate::owner_note::note(
        crate::owner_note::Kind::Action,
        &format!(
            "Theseus could not revert its change {}: revert it by hand",
            short(&j.merge)
        ),
        &format!(
            "its revert of self branch {} ({}) stopped at the {phase}: {why}",
            j.branch, j.item
        ),
        &format!(
            "The change is still on main. Revert it by hand when you agree: `git revert -m 1 {}`.",
            j.merge
        ),
    )
}

/// The notice of an automatic revert.
pub(crate) fn reverted_text(
    j: &Joined,
    revert: &str,
    signal: &str,
    window: impl std::fmt::Display,
    detail: &str,
    tested: u32,
    install: bool,
) -> String {
    let body = format!(
        "Self branch {branch} ({item}), joined as {m}, is undone as {r}.\nEvidence: {detail}. \
         {bisect}\nThe revert passed main's gate and is signed and pushed{installed}.\nTo put the \
         change back, revert the revert: `git revert {r}`.",
        branch = j.branch,
        item = j.item,
        m = short(&j.merge),
        r = short(revert),
        bisect = if tested > 0 {
            format!("Found by testing {tested} commits of main.")
        } else {
            "Attributed by time: no outer-loop commit came between.".into()
        },
        installed = if install {
            ", and the install is under way"
        } else {
            ""
        },
    );
    crate::owner_note::note(
        crate::owner_note::Kind::Alert,
        "Theseus reverted its own change",
        &format!("the {signal} signal turned red within {window} h of the join"),
        &body,
    )
}

/// The notice of the owner's veto, done.
pub(crate) fn vetoed_text(j: &Joined, revert: &str) -> String {
    crate::owner_note::note(
        crate::owner_note::Kind::Fyi,
        &format!("Your veto of self branch {} is done", j.branch),
        &format!(
            "you vetoed it ({}); it was joined as {}",
            j.item,
            short(&j.merge)
        ),
        &format!(
            "Reverted as {}, signed and pushed to main. Its backlog item is dropped and will not \
             be filed again.",
            short(revert)
        ),
    )
}
