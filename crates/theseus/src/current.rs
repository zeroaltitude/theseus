//! The current thread of a terminal (theseus-j0yh): a client-side choice,
//! never the store's. Two places hold it:
//!
//! - **`THESEUS_THREAD`** in the shell's environment. `theseus thread switch`
//!   prints the `export` line that sets it, as tools that change a shell's
//!   state do. It is the only thing `theseus ask` reads, so a script that
//!   never set it asks the home conversation, whatever a terminal chose.
//! - **A small file** beside the seen file (`$XDG_STATE_HOME/theseus/
//!   thread.json`), a map from a terminal's name (its tty) to its thread: the
//!   interactive loop and the TUI keep their choice there, across restarts and
//!   per terminal. One writer may lose a race with another terminal's write;
//!   the file holds a convenience, not a record.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use theseus_protocol::threads::ThreadRow;

use crate::resolve::SHORTEST;

pub const ENV: &str = "THESEUS_THREAD";
const FILE_NAME: &str = "thread.json";
/// Terminals remembered; the oldest names go first.
const KEEP: usize = 32;

/// The environment's thread: set, not empty, and not `home`.
pub fn from_env() -> Option<String> {
    named(std::env::var(ENV).ok().as_deref())
}

fn named(raw: Option<&str>) -> Option<String> {
    let s = raw?.trim();
    (!s.is_empty() && s != crate::render::thread::HOME).then(|| s.to_string())
}

/// The file's path: beside the seen file.
pub fn default_path() -> Option<PathBuf> {
    Some(crate::seen::Seen::default_path()?.with_file_name(FILE_NAME))
}

/// This terminal's name: its tty, else `-`.
pub fn terminal() -> String {
    std::fs::read_link("/proc/self/fd/0")
        .ok()
        .filter(|p| p.starts_with("/dev/pts/") || p.starts_with("/dev/tty"))
        .map_or_else(|| "-".to_string(), |p| p.display().to_string())
}

fn load(path: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// What the file holds for `terminal`.
pub fn read(path: &Path, terminal: &str) -> Option<String> {
    named(load(path).get(terminal).map(String::as_str))
}

/// Put `slug` (none: home) in the file for `terminal`, replacing it whole and
/// atomically.
pub fn write(path: &Path, terminal: &str, slug: Option<&str>) -> io::Result<()> {
    let mut map = load(path);
    map.remove(terminal);
    if let Some(s) = slug {
        map.insert(terminal.to_string(), s.to_string());
    }
    while map.len() > KEEP {
        let Some(first) = map.keys().next().cloned() else {
            break;
        };
        map.remove(&first);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.new");
    std::fs::write(&tmp, serde_json::to_string(&map).map_err(io::Error::other)?)?;
    std::fs::rename(tmp, path)
}

/// The interactive loop's and the TUI's current thread: the environment's,
/// else this terminal's in the file.
pub fn of_terminal() -> Option<String> {
    from_env().or_else(|| read(&default_path()?, &terminal()))
}

/// Remember this terminal's choice. Best effort: a read-only state dir loses
/// the memory, not the switch.
pub fn remember(slug: Option<&str>) {
    if let Some(p) = default_path() {
        let _ = write(&p, &terminal(), slug);
    }
}

/// What `/t X` and `thread switch X` name.
#[derive(Debug, PartialEq)]
pub enum Pick<'a> {
    Home,
    Thread(&'a ThreadRow),
}

/// `given` among `rows`, as listed: `home`; a number, counted from 1 in the
/// order shown; a slug whole; or a unique start of a slug or a title, of at
/// least four characters.
pub fn pick<'a>(given: &str, rows: &'a [ThreadRow]) -> Result<Pick<'a>> {
    let s = given.trim();
    if s.eq_ignore_ascii_case(crate::render::thread::HOME) {
        return Ok(Pick::Home);
    }
    if let Ok(n) = s.parse::<usize>() {
        return match n.checked_sub(1).and_then(|i| rows.get(i)) {
            Some(r) => Ok(Pick::Thread(r)),
            None => bail!("there is no thread {n}: `/t` lists {} of them", rows.len()),
        };
    }
    if let Some(r) = rows.iter().find(|r| r.thread.slug == s) {
        return Ok(Pick::Thread(r));
    }
    if s.chars().count() < SHORTEST {
        bail!("`{given}` is too short to name a thread: give a slug whole, or at least four characters of it or of its title");
    }
    let lower = s.to_lowercase();
    let found: Vec<&ThreadRow> = rows
        .iter()
        .filter(|r| {
            r.thread.slug.starts_with(&lower) || r.thread.title.to_lowercase().starts_with(&lower)
        })
        .collect();
    match found.as_slice() {
        [] => bail!("no thread is named `{given}`: `theseus thread list` names them"),
        [one] => Ok(Pick::Thread(one)),
        many => bail!(
            "`{given}` names {} threads ({}): give more of it",
            many.len(),
            many.iter()
                .take(5)
                .map(|r| r.thread.slug.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::thread::tests::row;

    fn rows() -> Vec<ThreadRow> {
        vec![
            row("pr", "ex-ai pr round"),
            row("tides-gauge", "Tide gauge four"),
            row("tides-log", "Tide log rewrite"),
        ]
    }

    #[test]
    fn a_thread_is_named_by_home_a_number_a_slug_or_a_unique_start() {
        let rs = rows();
        let slug = |p: Result<Pick>| match p.unwrap() {
            Pick::Home => "home".to_string(),
            Pick::Thread(r) => r.thread.slug.clone(),
        };
        assert_eq!(slug(pick("home", &rs)), "home");
        assert_eq!(slug(pick("2", &rs)), "tides-gauge");
        assert_eq!(slug(pick("pr", &rs)), "pr", "a slug whole needs no length");
        assert_eq!(slug(pick("ex-a", &rs)), "pr", "a title's start");
        assert_eq!(
            slug(pick("tide ga", &rs)),
            "tides-gauge",
            "a title's start, any case"
        );
        assert_eq!(slug(pick("tides-l", &rs)), "tides-log");
        let e = |g: &str| pick(g, &rs).unwrap_err().to_string();
        assert!(e("tide").contains("names 2 threads"), "{}", e("tide"));
        assert!(e("ex").contains("too short"), "{}", e("ex"));
        assert!(e("9").contains("no thread 9"), "{}", e("9"));
        assert!(e("0").contains("no thread 0"), "{}", e("0"));
        assert!(e("zzzz").contains("no thread is named"), "{}", e("zzzz"));
    }

    #[test]
    fn the_environment_names_the_thread_and_home_or_nothing_names_none() {
        assert_eq!(named(Some("pr")), Some("pr".into()));
        assert_eq!(named(Some(" pr ")), Some("pr".into()));
        assert_eq!(named(Some("home")), None);
        assert_eq!(named(Some("")), None);
        assert_eq!(named(None), None);
    }

    #[test]
    fn a_terminals_choice_is_kept_beside_the_others_and_cleared_by_home() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("theseus").join("thread.json");
        assert_eq!(read(&p, "/dev/pts/1"), None);
        write(&p, "/dev/pts/1", Some("pr")).unwrap();
        write(&p, "/dev/pts/2", Some("tides-log")).unwrap();
        assert_eq!(read(&p, "/dev/pts/1").as_deref(), Some("pr"));
        assert_eq!(read(&p, "/dev/pts/2").as_deref(), Some("tides-log"));
        write(&p, "/dev/pts/1", None).unwrap();
        assert_eq!(read(&p, "/dev/pts/1"), None);
        assert_eq!(read(&p, "/dev/pts/2").as_deref(), Some("tides-log"));
    }
}
