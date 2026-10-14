//! What the chat reads of the machine, with no daemon and no model call: the
//! directory it opens in and its git branch, and the history of what was
//! typed there (a file of the client's, like the seen file).

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

/// `$XDG_STATE_HOME/theseus/chat-history.json`, else under `~/.local/state`.
pub fn history_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("state"))
        })?;
    Some(base.join("theseus").join("chat-history.json"))
}

/// This directory's messages, oldest first: an unreadable or foreign file is
/// no history.
pub fn load_history(path: &Path, dir: &str) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    v.get(dir)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|m| m.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Write this directory's messages, keeping the other directories' as the file
/// holds them: a temporary file beside it, then a rename.
pub fn save_history(path: &Path, dir: &str, messages: &[String]) -> std::io::Result<()> {
    let mut all: serde_json::Map<String, Value> = std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let from = messages.len().saturating_sub(HISTORY_KEPT);
    all.insert(dir.to_string(), json!(&messages[from..]));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(&all)?)?;
    std::fs::rename(&tmp, path)
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
    fn history_is_kept_per_directory_and_survives_a_foreign_file() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("state/chat-history.json");
        assert!(load_history(&p, "/a").is_empty());
        save_history(&p, "/a", &["one".into(), "two".into()]).unwrap();
        save_history(&p, "/b", &["other".into()]).unwrap();
        assert_eq!(load_history(&p, "/a"), ["one", "two"]);
        assert_eq!(load_history(&p, "/b"), ["other"]);
        std::fs::write(&p, "not json").unwrap();
        assert!(load_history(&p, "/a").is_empty());
        save_history(&p, "/a", &["three".into()]).unwrap();
        assert_eq!(load_history(&p, "/a"), ["three"]);
    }
}
