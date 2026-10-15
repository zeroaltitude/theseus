//! route.v5's `seal` verdict (theseus-c1uz; the owner, 2026-10-09: Jev
//! recommends, the owner seals). A `yes` at the confirm bar or above, in a
//! message of a thread, asks the core to propose sealing it
//! (`Core::thread_recommend_seal`: `seal_pending`, a card to the owner's DM,
//! the thread held out of other threads' recall until answered). Jev never
//! seals or unseals. Off the turn's path, on the blocking pool; the verdict
//! itself is already in the route judgment's row.

use super::JudgeService;
use crate::engage::SealAnswer;

impl JudgeService {
    /// The verdict, when it recommends and `[threads] seal_recommend` is on:
    /// spawned, never awaited.
    pub(crate) fn seal_verdict(&self, session: &str, a: &SealAnswer) {
        if !a.acts || !a.recommends() {
            return;
        }
        let Some(core) = self.core() else { return };
        if !core.cfg.threads.seal_recommend {
            return;
        }
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let (session, p) = (session.to_string(), a.p_yes);
        rt.spawn_blocking(move || recommend(&core, &session, p));
    }
}

/// Propose sealing the thread `session` serves, if it serves one.
fn recommend(core: &crate::Core, session: &str, p: f64) {
    let Ok(board) = core.runner.threads.all(&core.store) else {
        return;
    };
    let Some(slug) = board.of_session(session).map(|t| t.slug.clone()) else {
        return;
    };
    let reason = format!(
        "Jev read content that may need keeping private (an unfixed vulnerability or embargo, a \
         legal or personnel matter, a partner's data, or a credential), p {p:.2}"
    );
    if let Err(e) = core.thread_recommend_seal(&slug, &reason) {
        tracing::warn!(error = %format!("{e:#}"), "judge: the seal recommendation was not made");
    }
}
