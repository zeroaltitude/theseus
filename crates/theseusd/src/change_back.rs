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

/// The state directory the settings file names, `[server] state_dir`, read
/// as plain TOML (the file parsed when the change was written). None when it
/// names none or cannot be read.
fn configured_state_dir(config_file: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(config_file).ok()?;
    let doc: toml::Value = toml::from_str(&text).ok()?;
    let dir = doc.get("server")?.get("state_dir")?.as_str()?;
    Some(expand(dir))
}

/// The marker this start was given: the path the exec carried, else the one in
/// the state directory this daemon serves from, the one the floor holds:
/// `--state-dir`, else the settings file's `[server] state_dir`, else the
/// default. A marker in any other directory is never read, so a start that a
/// supervisor began after a crash finds its marker where the floor protects it
/// and nowhere a tool could have written one.
pub fn find(state_flag: Option<&Path>, config_file: &Path) -> Option<Found> {
    find_in(
        state_flag,
        config_file,
        &expand(theseus_core::config::DEFAULT_STATE_DIR),
    )
}

/// [`find`], with the default state directory given: what a test varies.
pub fn find_in(state_flag: Option<&Path>, config_file: &Path, default_dir: &Path) -> Option<Found> {
    let marker = match std::env::var(CHANGE_ENV) {
        Ok(p) if !p.is_empty() => PathBuf::from(p),
        _ => {
            let dir = match state_flag {
                Some(d) => expand(&d.to_string_lossy()),
                None => {
                    configured_state_dir(config_file).unwrap_or_else(|| default_dir.to_path_buf())
                }
            };
            restart::marker_path(&dir)
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
pub fn go_back(marker: &Path, mut p: Pending, why: &str, config: &Path) -> Option<Exit> {
    match restart::roll_back(marker, &mut p, why, config) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn marker_in(dir: &Path, config: &Path) {
        let (prev, _) = (dir.join("store/config-changes/previous-0001.toml"), ());
        std::fs::create_dir_all(prev.parent().unwrap()).unwrap();
        std::fs::write(&prev, "a = 1\n").unwrap();
        let p = Pending {
            id: "act_x".into(),
            file: config.to_path_buf(),
            previous: prev,
            previous_sha256: theseus_core::config_copy::sha256("a = 1\n"),
            digest_before: "b".into(),
            digest_after: theseus_core::config_copy::sha256("a = 2\n"),
            paths: vec!["a".into()],
            session_id: "ses_x".into(),
            place: None,
            approved_by: "cli".into(),
            via: "cli".into(),
            requested_ms: 1,
            stage: restart::Stage::Started,
            restart_only: false,
            took_ms: None,
            why: None,
        };
        restart::save(&restart::marker_path(dir), &p).unwrap();
    }

    /// A marker is read only from the state directory the daemon serves from,
    /// which the floor holds: when the settings name another, one planted in the
    /// default directory is never read (it could have been written by a turn).
    #[test]
    fn a_marker_in_a_directory_the_settings_do_not_name_is_never_read() {
        let t = tempfile::tempdir().unwrap();
        let (default_dir, named) = (t.path().join("default"), t.path().join("named"));
        let config = t.path().join("theseus.toml");
        std::fs::write(
            &config,
            format!("[server]\nstate_dir = \"{}\"\n", named.display()),
        )
        .unwrap();
        marker_in(&default_dir, &config);
        assert!(
            find_in(None, &config, &default_dir).is_none(),
            "planted under the default"
        );
        // The same marker in the directory the settings name is read.
        marker_in(&named, &config);
        let found = find_in(None, &config, &default_dir).expect("the named directory's marker");
        assert!(found.marker.starts_with(&named));
        // A flag decides before the settings do.
        assert!(find_in(Some(&default_dir), &config, &default_dir).is_some());
        // No `[server] state_dir`: the default.
        std::fs::write(&config, "narrative = false\n").unwrap();
        assert!(find_in(None, &config, &default_dir).is_some());
        assert!(find_in(None, &config, &named.join("elsewhere")).is_none());
    }
}
