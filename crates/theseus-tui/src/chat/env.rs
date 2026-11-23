//! What the chat reads of the machine, with no daemon and no model call: the
//! directory it opens in and its git branch, and the history of what was
//! typed there (a file of the client's, like the seen file).

use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::editor::HISTORY_KEPT;

/// `dir` with the home directory written `~`.
pub fn shown_dir(dir: &str, home: Option<&str>) -> String {
    match home.filter(|h| !h.is_empty() && *h != "/") {
        Some(h) if dir == h => "~".to_string(),
        Some(h) => dir
            .strip_prefix(h)
            .filter(|rest| rest.starts_with('/'))
            .map_or_else(|| dir.to_string(), |rest| format!("~{rest}")),
        None => dir.to_string(),
    }
}

/// The branch checked out in the repository holding `dir`, read from `.git/HEAD`
/// (or a worktree's `gitdir:` file); a detached head is its short hash. None
/// outside a repository.
pub fn git_branch(dir: &Path) -> Option<String> {
    for d in dir.ancestors() {
        let dot = d.join(".git");
        let head = if dot.is_dir() {
            dot.join("HEAD")
        } else if dot.is_file() {
            let text = std::fs::read_to_string(&dot).ok()?;
            let gitdir = text.trim().strip_prefix("gitdir:")?.trim();
            let p = Path::new(gitdir);
            let p = if p.is_absolute() {
                p.to_path_buf()
            } else {
                d.join(p)
            };
            p.join("HEAD")
        } else {
            continue;
        };
        let text = std::fs::read_to_string(head).ok()?;
        let text = text.trim();
        return Some(match text.strip_prefix("ref: refs/heads/") {
            Some(b) => b.to_string(),
            None => text.chars().take(7).collect(),
        });
    }
    None
}

/// `$XDG_STATE_HOME/theseus/history/`, else under `~/.local/state`: one file
/// for each directory (`file_for`), so each project keeps its own.
pub fn history_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("state"))
        })?;
    Some(base.join("theseus").join("history"))
}

/// The file in `folder` that holds `dir`'s messages: its last component for a
/// person to recognise, and a hash of the whole path (FNV-1a, which is the
/// same on every build) so two directories of one name stay apart.
pub fn file_for(folder: &Path, dir: &str) -> PathBuf {
    let hash = dir.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    let name: String = dir
        .rsplit('/')
        .find(|c| !c.is_empty())
        .unwrap_or("root")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(40)
        .collect();
    folder.join(format!("{name}-{hash:016x}.json"))
}

/// This directory's messages, oldest first: an unreadable or foreign file is
/// no history. Before per-directory files, one file beside the folder held
/// every directory's (`chat-history.json`); it is read when the directory has
/// no file of its own yet, and never changed.
pub fn load_history(folder: &Path, dir: &str) -> Vec<String> {
    let own = std::fs::read_to_string(file_for(folder, dir))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok());
    let v = match own {
        Some(Value::Array(a)) => return strings(&a),
        _ => folder
            .parent()
            .map(|p| p.join("chat-history.json"))
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str::<Value>(&t).ok()),
    };
    v.and_then(|v| v.get(dir).and_then(Value::as_array).map(|a| strings(a)))
        .unwrap_or_default()
}

fn strings(a: &[Value]) -> Vec<String> {
    a.iter()
        .filter_map(|m| m.as_str().map(String::from))
        .collect()
}

/// Append newly typed messages under this directory's lock, then atomically
/// replace its bounded history. Callers must pass new entries, not a stale
/// window's whole snapshot. Both the lock and temporary file are private (0600).
pub fn save_history(folder: &Path, dir: &str, messages: &[String]) -> std::io::Result<()> {
    let path = file_for(folder, dir);
    std::fs::create_dir_all(folder)?;
    let lock = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path.with_extension("json.lock"))?;
    lock.lock()?;
    let mut merged = load_history(folder, dir);
    for message in messages {
        if merged.last() != Some(message) {
            merged.push(message.clone());
        }
    }
    let from = merged.len().saturating_sub(HISTORY_KEPT);
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let (tmp, mut f) = loop {
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tmp = path.with_extension(format!("json.{}.{n}.tmp", std::process::id()));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)
        {
            Ok(f) => break (tmp, f),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    };
    let result = (|| {
        f.write_all(&serde_json::to_vec(&json!(&merged[from..]))?)?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_home_directory_is_a_tilde() {
        assert_eq!(
            shown_dir("/home/ada/work/tides", Some("/home/ada")),
            "~/work/tides"
        );
        assert_eq!(shown_dir("/home/ada", Some("/home/ada")), "~");
        assert_eq!(shown_dir("/home/adam/x", Some("/home/ada")), "/home/adam/x");
        assert_eq!(shown_dir("/srv/x", None), "/srv/x");
    }

    #[test]
    fn a_branch_is_read_from_head_and_a_detached_head_is_a_short_hash() {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join(".git")).unwrap();
        std::fs::create_dir_all(t.path().join("src/deep")).unwrap();
        std::fs::write(
            t.path().join(".git/HEAD"),
            "ref: refs/heads/feature/tides\n",
        )
        .unwrap();
        assert_eq!(
            git_branch(&t.path().join("src/deep")).as_deref(),
            Some("feature/tides")
        );
        std::fs::write(t.path().join(".git/HEAD"), "0123456789abcdef\n").unwrap();
        assert_eq!(git_branch(t.path()).as_deref(), Some("0123456"));
        let bare = tempfile::tempdir().unwrap();
        assert_eq!(git_branch(bare.path()), None);
    }

    #[test]
    fn concurrent_history_saves_keep_both_writers() {
        let t = tempfile::tempdir().unwrap();
        save_history(t.path(), "/project", &["base".into()]).unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let workers: Vec<_> = ["first", "second"]
            .into_iter()
            .map(|line| {
                let folder = t.path().to_path_buf();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    save_history(&folder, "/project", &[line.into()]).unwrap();
                })
            })
            .collect();
        barrier.wait();
        for worker in workers {
            worker.join().unwrap();
        }
        let lines = load_history(t.path(), "/project");
        assert_eq!(lines.len(), 3);
        assert!(lines.contains(&"first".into()) && lines.contains(&"second".into()));
    }

    #[test]
    fn independent_history_snapshots_merge_without_losing_lines() {
        let t = tempfile::tempdir().unwrap();
        save_history(
            t.path(),
            "/project",
            &["base".into(), "first window".into()],
        )
        .unwrap();
        // The second window contributes only its new entry, never its stale base.
        save_history(t.path(), "/project", &["second window".into()]).unwrap();
        assert_eq!(
            load_history(t.path(), "/project"),
            ["base", "first window", "second window"]
        );
    }

    #[test]
    fn history_is_kept_per_directory_and_survives_a_foreign_file() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("state/history");
        assert!(load_history(&p, "/a").is_empty());
        save_history(&p, "/a", &["one".into(), "two".into()]).unwrap();
        save_history(&p, "/b", &["other".into()]).unwrap();
        assert_eq!(load_history(&p, "/a"), ["one", "two"]);
        assert_eq!(load_history(&p, "/b"), ["other"]);
        // Each directory has a file of its own, and two of one name differ.
        assert_ne!(file_for(&p, "/a"), file_for(&p, "/b"));
        assert_ne!(file_for(&p, "/x/work"), file_for(&p, "/y/work"));
        assert_eq!(
            std::fs::read_dir(&p)
                .unwrap()
                .filter(|e| e
                    .as_ref()
                    .is_ok_and(|e| e.path().extension().is_some_and(|x| x == "json")))
                .count(),
            2
        );
        std::fs::write(file_for(&p, "/a"), "not json").unwrap();
        assert!(load_history(&p, "/a").is_empty());
        save_history(&p, "/a", &["three".into()]).unwrap();
        assert_eq!(load_history(&p, "/a"), ["three"]);
        assert_eq!(load_history(&p, "/b"), ["other"], "the other is untouched");
    }

    #[test]
    fn a_directory_without_a_file_reads_the_one_file_of_before() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("state/history");
        std::fs::create_dir_all(t.path().join("state")).unwrap();
        std::fs::write(
            t.path().join("state/chat-history.json"),
            r#"{"/a":["old one","old two"],"/b":["elsewhere"]}"#,
        )
        .unwrap();
        assert_eq!(load_history(&p, "/a"), ["old one", "old two"]);
        save_history(&p, "/a", &["new".into()]).unwrap();
        assert_eq!(
            load_history(&p, "/a"),
            ["old one", "old two", "new"],
            "the first append carries the legacy lines into the directory file"
        );
        assert_eq!(load_history(&p, "/b"), ["elsewhere"]);
        assert!(t.path().join("state/chat-history.json").exists());
    }

    #[test]
    fn the_history_file_is_the_owners_alone() {
        use std::os::unix::fs::PermissionsExt;
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("state/history");
        save_history(&p, "/a", &["a pasted token".into()]).unwrap();
        let mode = std::fs::metadata(file_for(&p, "/a"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "{mode:o}");
    }
}
