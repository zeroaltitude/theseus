//! At the next start: the daemon that is the build an install named confirms itself once healthy; the daemon that is
//! not was put back, and says so. The install's whole memory is one META key in the store: which commit, which program
//! the service ran before, and the unit as it was. Nothing of it is read from a file.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;

use super::hand::hand_words;
use super::{Pending, Stage, HEALTHY_WITHIN_SECS, LOOK_EVERY};
use crate::config_change::restart::watch_health;
use crate::rpc::Core;

/// The META key of the install that is under way, or the last one.
pub const KEY: &str = "self.install.pending";

impl Core {
    /// The install under way, or the last one.
    pub(crate) fn root_pending(&self) -> Option<Pending> {
        self.store.get_meta::<Pending>(KEY).ok().flatten()
    }

    pub(crate) fn root_save(&self, p: &Pending) -> anyhow::Result<()> {
        self.store.put_meta(KEY, p)
    }

    /// Whether an install of `commit` has been started and has not ended: its notice waits for the confirmation.
    pub(crate) fn root_install_pending_for(&self, commit: &str) -> bool {
        self.root_pending().is_some_and(|p| {
            p.merge == commit && matches!(p.stage, Stage::Restarting | Stage::Started)
        })
    }

    /// Once serving. An install that has not ended: if this daemon is the build it named, wait for health and confirm;
    /// if it is not, the install went back (or never happened), and the owner is told.
    pub fn root_install_after_serving(self: &Arc<Self>, socket: PathBuf) {
        self.root_install_after_serving_as(crate::build().commit.as_deref(), socket);
    }

    /// [`Self::root_install_after_serving`], for a daemon that is the build `running` names.
    pub(crate) fn root_install_after_serving_as(
        self: &Arc<Self>,
        running: Option<&str>,
        socket: PathBuf,
    ) {
        let Some(mut p) = self.root_pending() else {
            return;
        };
        if !matches!(p.stage, Stage::Restarting | Stage::Started) {
            return;
        }
        if running == Some(p.merge.as_str()) {
            p.stage = Stage::Started;
            if let Err(e) = self.root_save(&p) {
                tracing::warn!(error = %format!("{e:#}"), "self-install: the marker could not be kept");
            }
            tokio::spawn(self.clone().confirm_root_install(p, socket));
            return;
        }
        let why = "the new build is not what is running".to_string();
        p.stage = Stage::RolledBack;
        p.why = Some(why.clone());
        if let Err(e) = self.root_save(&p) {
            tracing::warn!(error = %format!("{e:#}"), "self-install: the marker could not be kept");
        }
        let step = self.root_hand_step(&p.merge, &p.old_program);
        self.root_row(
            "rolled_back",
            &p.merge,
            "the install did not take: the service runs the build before",
            &why,
            json!({"old_program": p.old_program, "new_program": p.new_program, "file": p.unit, "step": step}),
        );
        self.root_tell(format!(
            "The change Theseus joined ({what}, {c8}) is not what the service runs: the install did not take. The \
             service is running the build before ({old}), not the new one: the new build did not come up healthy \
             within {mins} minutes, or the restart never started it. The change is joined to main and not installed. \
             Theseus will not try again by itself. {by_hand}",
            what = p.what,
            c8 = super::short(&p.merge),
            mins = super::CONFIRM_WITHIN_SECS / 60,
            old = p.old_program,
            by_hand = hand_words(step.as_deref()),
        ));
    }

    /// The new daemon, serving: wait until it is healthy, then confirm (the restart's unit looks for it), record it, and
    /// say "installed and running". Not healthy in time: say so, and leave the confirmation unwritten, so the unit goes
    /// back.
    pub(crate) async fn confirm_root_install(self: Arc<Self>, mut p: Pending, socket: PathBuf) {
        let probe = || {
            let core = self.clone();
            let socket = socket.clone();
            async move { core.root_healthy(&socket).await }
        };
        let within = Duration::from_secs(HEALTHY_WITHIN_SECS);
        match watch_health(probe, within, LOOK_EVERY).await {
            Ok(took) => {
                let board = &self.tools.root_install;
                let signalled =
                    crate::config_change::write::atomic(&board.signal(), &p.merge, None);
                if let Err(e) = signalled {
                    tracing::warn!(error = %e, "self-install: the confirmation could not be written");
                    return;
                }
                p.stage = Stage::Confirmed;
                p.took_ms = Some(took.as_millis() as u64);
                if let Err(e) = self.root_save(&p) {
                    tracing::warn!(error = %format!("{e:#}"), "self-install: the marker could not be kept");
                }
                self.root_row(
                    "verified",
                    &p.merge,
                    &format!("the running build is {}", super::short(&p.merge)),
                    &format!(
                        "it answered healthy {} s after it started",
                        took.as_secs().max(1)
                    ),
                    json!({"took_ms": p.took_ms}),
                );
                // The notice of the join ("installed and running") waited for this.
                let noticed = self.self_installed_as(&p.merge);
                if !matches!(noticed, Ok(Some(_))) {
                    self.root_tell(format!(
                        "Theseus is back, running the change it joined ({what}, {c8}).",
                        what = p.what,
                        c8 = super::short(&p.merge)
                    ));
                }
            }
            Err(why) => {
                self.root_row(
                    "unhealthy",
                    &p.merge,
                    "the new build did not answer healthy in time",
                    &why,
                    json!({"within_secs": HEALTHY_WITHIN_SECS}),
                );
            }
        }
    }

    /// Healthy as an install judges it: it serves its socket, and the index tender is off or not `down` or `stalled`.
    async fn root_healthy(&self, socket: &std::path::Path) -> Result<(), String> {
        tokio::net::UnixStream::connect(socket)
            .await
            .map_err(|e| format!("it does not serve its socket yet: {e}"))?;
        let h = self.health_now().await;
        match h.index.as_ref().map(|i| i.state.as_str()) {
            Some(s @ ("down" | "stalled")) => Err(format!("the index tender is {s}")),
            _ => Ok(()),
        }
    }
}
