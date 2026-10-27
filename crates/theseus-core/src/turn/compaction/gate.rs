//! A summary's route (theseus-a1wvq.10, theseus-9ymz0): where a compaction's
//! range may go. A profile of US origin goes as asked; one of restricted
//! origin only when the session's labels allow it (`data_step::Cn`): the same
//! rule the turn's floor and the chokepoint hold, read fresh because a
//! compaction runs off the route's path. A range that may not go is not sent:
//! the turn's own restricted target keeps the ring, a Claude turn has it
//! written by the `[model]` default.

use super::super::data_step::{Cn, DEFAULT_PROFILE};
use super::super::{Target, Turn, TurnRunner};

/// What a summary call is: its profile, its target when that is the turn's
/// own, and the words for a row when the gate moved it.
pub(super) struct Route {
    pub profile: String,
    pub own: Option<Target>,
    pub note: Option<String>,
}

impl TurnRunner {
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
        if !self.restricted_profile(&route.profile) {
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
