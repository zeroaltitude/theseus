//! A summary's route (theseus-a1wvq.10, theseus-9ymz0): where a compaction's
//! range may go. A profile of US origin goes as asked; one of restricted
//! origin only when the session's labels allow it (`data_step::Cn`): the same
//! rule the turn's floor and the chokepoint hold, read fresh because a
//! compaction runs off the route's path. A range that may not go is not sent:
//! the turn's own restricted target keeps the ring, a Claude turn has it
//! written by the `[model]` default.

use theseus_judge::builders::{JobKind, RestrictedModels};

use super::super::data_step::{Cn, DEFAULT_PROFILE};
use super::super::{Target, Turn, TurnRunner};
use super::{SUMMARY_MAX_TOKENS, SUMMARY_SESSION};
use crate::compiler::Compiled;
use crate::config::memory::Summary;
use crate::job_model::{How, Site};
use crate::session::SessionRecord;

/// What a summary call is: its profile, its target when that is the turn's
/// own, and the words for a row when the gate moved it.
pub(super) struct Route {
    pub profile: String,
    pub own: Option<Target>,
    pub note: Option<String>,
}

impl TurnRunner {
    /// What the summary runs on: the pinned profile (`session` is the turn's
    /// own target), or with `[memory] summary_profile` unset, Jev's choice for
    /// the range (route.v10, theseus-ej6bj), which falls to the turn's own
    /// target (`session`, what the key meant unset) when Jev is not asked, is
    /// late, or names a model the data floor refuses. None: `off`.
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
                    restricted: RestrictedModels::allowed_if(self.cn_read(t) == Cn::Allowed),
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

    /// The summary's route, or why the ring stands.
    pub(super) async fn summary_route(
        &self,
        t: &Turn<'_>,
        (profile, own): (String, Option<Target>),
    ) -> Result<Route, String> {
        let mut route = Route {
            profile,
            own,
            note: None,
        };
        let restricted = match &route.own {
            Some(o) => self.restricted_model(&o.provider, &o.model),
            None => self.restricted_profile(&route.profile),
        };
        if !restricted {
            return Ok(route);
        }
        // A summary is a request that carries the session's items: it waits
        // for their verdicts as a turn bound for the same model does.
        let cn = self.cn_wait(t).await;
        if cn == Cn::Allowed {
            return Ok(route);
        }
        let word = cn.word();
        self.judge.record_data_gate("compaction", word);
        let why = format!("data_gate: {word}");
        if self.restricted_profile(&t.target.profile) {
            return Err(why);
        }
        if self.restricted_profile(DEFAULT_PROFILE) {
            return Err(format!(
                "{why}; `{DEFAULT_PROFILE}` is of restricted origin or does not resolve"
            ));
        }
        route.profile = DEFAULT_PROFILE.into();
        route.own = None;
        route.note = Some(why);
        Ok(route)
    }
}
