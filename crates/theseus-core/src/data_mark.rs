//! A session's data mark, and the one question the model menu and the floor
//! share: may this profile run this session's next turn? (theseus-jid7t.2.)
//!
//! - **The mark** is what the floor reads of a session, its level from
//!   `google::private::level`: `sensitive` when it, or any task above it, has
//!   read a calendar the config marks sensitive, `private` when it has read
//!   other Google content, else `open`. The data judgment keeps no verdict per
//!   session (it admits each turn, `turn/open_step.rs`), so there is none to
//!   read here.
//! - **The refusal** is the route step's own: a private or sensitive session
//!   runs only on a profile of US origin (`Config::profile_cleared_for_sensitive`,
//!   which `TurnRunner::uncleared_profile` reads too), and a profile that does
//!   not resolve runs nowhere. A profile of restricted origin on an open
//!   session is not refused: a turn admitted open may run on it, and the floor
//!   decides per turn (`turn/data_step.rs`).

use theseus_protocol::ops::DataMark;

use crate::catalog::Catalog;
use crate::store::Store;
use crate::Config;

/// What the floor reads of `session_id`'s data.
pub(crate) fn session_mark(store: &Store, cfg: &Config, session_id: &str) -> DataMark {
    use crate::google::private::{self, Level};
    match private::level(store, cfg, session_id) {
        Level::Open => DataMark::Open,
        Level::Private => DataMark::Private,
        Level::Sensitive => DataMark::Sensitive,
    }
}

/// Whether `profile` may not be sent private content: its origin is not on
/// the allowlist, or it does not resolve.
pub(crate) fn uncleared(cfg: &Config, catalog: &Catalog, profile: &str) -> bool {
    !cfg.profile_cleared_for_sensitive(profile, catalog)
}

/// Why a session marked `mark` cannot run on `profile`, from what the floor
/// reads of it: whether it resolves (`known`) and whether its origin is
/// cleared for private content (`cleared`).
pub(crate) fn refusal_of(
    mark: DataMark,
    profile: &str,
    known: bool,
    cleared: bool,
) -> Option<String> {
    match (known, mark, cleared) {
        (false, ..) => Some(format!("`{profile}` is not a configured profile")),
        (true, DataMark::Open, _) | (true, _, true) => None,
        _ => Some(format!(
            "this conversation holds {} content, and `{profile}` is not a model of US origin",
            match mark {
                DataMark::Sensitive => "sensitive",
                _ => "private",
            }
        )),
    }
}

/// [`refusal_of`] for a configured daemon: the check `session.profile` makes
/// and a model menu greys out by.
pub(crate) fn profile_refusal(
    cfg: &Config,
    catalog: &Catalog,
    mark: DataMark,
    profile: &str,
) -> Option<String> {
    refusal_of(
        mark,
        profile,
        cfg.all_profiles().contains_key(profile),
        !uncleared(cfg, catalog, profile),
    )
}

/// What an open session's restricted-origin profile cannot promise: the floor
/// keeps such a profile to a turn admitted open, so it may move a turn.
pub(crate) fn per_turn_note(
    cfg: &Config,
    catalog: &Catalog,
    mark: DataMark,
    profile: &str,
) -> Option<String> {
    (mark == DataMark::Open && cfg.profile_origin(profile, catalog).restricted()).then(|| {
        format!(
            "`{profile}` is a model of restricted origin: the floor decides each turn and may move a turn that is not cleared for it"
        )
    })
}

impl crate::rpc::Core {
    /// The work board's snapshot (`Push::work_snapshot`), its views stamped.
    pub(crate) fn work_snapshot_marked(
        &self,
        limit: usize,
        scope: &crate::push::WorkScope,
    ) -> (u64, Vec<theseus_protocol::work::WorkView>, u64) {
        let (position, mut views, total) = self.push.work_snapshot(limit, scope);
        self.stamp_data(&mut views);
        (position, views, total)
    }

    /// Stamp each conversation's and task's view with its data mark. The work
    /// board holds none (it reads no store): a snapshot and each frame it
    /// sends are stamped here as they go out.
    pub(crate) fn stamp_data(&self, views: &mut [theseus_protocol::work::WorkView]) {
        use theseus_protocol::work::WorkKind;
        for v in views {
            if let (WorkKind::Conversation | WorkKind::Task, Some(sid)) = (v.kind, &v.session_id) {
                v.data = Some(session_mark(&self.store, &self.cfg, sid));
            }
        }
    }
}

#[cfg(test)]
#[path = "tests_data_mark.rs"]
mod tests;
