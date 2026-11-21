//! A summary's route (theseus-a1wvq.10, route.v10, theseus-ej6bj): what a
//! compaction's range is sent to. No profile is kept off a range for its
//! model's origin: that rule was deleted on 2026-11-21.

use theseus_judge::builders::JobKind;

use super::super::{Target, Turn, TurnRunner};
use super::{SUMMARY_MAX_TOKENS, SUMMARY_SESSION};
use crate::compiler::Compiled;
use crate::config::memory::Summary;
use crate::job_model::{How, Site};
use crate::session::SessionRecord;

/// What a summary call is: its profile, and its target when that is the
/// turn's own.
pub(super) struct Route {
    pub profile: String,
    pub own: Option<Target>,
}

impl TurnRunner {
    /// What the summary runs on: the pinned profile (`session` is the turn's
    /// own target), or with `[memory] summary_profile` unset, Jev's choice for
    /// the range (route.v10, theseus-ej6bj), which falls to the turn's own
    /// target (`session`, what the key meant unset) when Jev is not asked or
    /// is late. None: `off`.
    pub(super) async fn summary_pick(
        &self,
        t: &Turn<'_>,
        ring: &Compiled,
    ) -> Option<(String, Option<Target>)> {
        let own = || (t.target.profile.clone(), Some(t.target.clone()));
        match self.memory.cfg().summary() {
            Summary::Off => None,
            Summary::Pin(w) if w == SUMMARY_SESSION => Some(own()),
            Summary::Pin(p) => Some((p.to_string(), None)),
            Summary::Jev => {
                let site = Site {
                    name: "compaction",
                    pin: None,
                    job: JobKind::Compaction {
                        oldest_tokens: ring
                            .budget
                            .dropped
                            .iter()
                            .filter(|d| d.tier == "ring")
                            .map(|d| d.tokens)
                            .sum(),
                        summary_tokens: u64::from(SUMMARY_MAX_TOKENS),
                        labels: Self::session_labels(t),
                    },
                };
                let (choice, how) = self.job_choice(&t.target.profile, site).await;
                match (how, self.choice_target(&t.target.profile, &choice)) {
                    (How::Jev { .. }, Ok(picked)) => Some((picked.profile.clone(), Some(picked))),
                    _ => Some(own()),
                }
            }
        }
    }

    /// The words of the labels the session carries (`confidential`, ...).
    fn session_labels(t: &Turn<'_>) -> Vec<String> {
        let Ok(Some(rec)) = t.tc.store.get_session::<SessionRecord>(t.tc.session_id) else {
            return Vec::new();
        };
        let Some(flags) = rec.data.as_deref() else {
            return Vec::new();
        };
        crate::content::Flag::ALL
            .into_iter()
            .filter(|f| flags.has(*f))
            .map(|f| f.as_str().to_string())
            .collect()
    }
}
