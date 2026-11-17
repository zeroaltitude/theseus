//! A start onto a change to the settings (issue 22, line 2): what `theseusd`
//! does about the marker the change left, so a change that does not load, or
//! that leaves the daemon unhealthy, is put back. The logic and its tests are
//! `theseus_core::config_change::restart`; this is only where a start reads
//! the marker and what it does with the answer.

use std::path::{Path, PathBuf};

use theseus_core::config::expand;
use theseus_core::config_change::restart::{self, Begin, Pending, CHANGE_ENV};

use crate::Exit;

/// What a start found: the marker, and what to do about it.
pub struct Found {
    pub marker: PathBuf,
    pub begin: Begin,
}

/// The marker this start was given: the path the exec carried, else the one
/// in the state directory a start would use, so a start that a supervisor
/// began after a crash finds it too.
pub fn find(state_flag: Option<&Path>) -> Option<Found> {
    let marker = match std::env::var(CHANGE_ENV) {
        Ok(p) if !p.is_empty() => PathBuf::from(p),
        _ => {
            let dir = state_flag.map_or(theseus_core::config::DEFAULT_STATE_DIR.into(), |d| {
                d.to_string_lossy().into_owned()
            });
            restart::marker_path(&expand(&dir))
        }
    };
    match restart::begin(&marker) {
        Begin::Normal => None,
        begin => Some(Found { marker, begin }),
    }
}

/// Go back, and exec onto the settings before the change. Refused (logged):
/// the start goes on as it would have, since the file is not what the change
/// wrote, or the copy is not what was kept.
pub fn go_back(marker: &Path, mut p: Pending, why: &str) -> Option<Exit> {
    match restart::roll_back(marker, &mut p, why) {
        Ok(()) => {
            tracing::warn!(why = %why, "the new settings are put back; restarting onto the old ones");
            Some(Exit::Exec(CHANGE_ENV, marker.display().to_string()))
        }
        Err(e) => {
            tracing::warn!(error = %e, why = %why, "the settings were not put back");
            None
        }
    }
}
