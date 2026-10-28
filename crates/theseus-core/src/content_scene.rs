//! What the daemon knows about where an item sits (`content.v3`, theseus-gyjyo.11):
//! who wrote it, whether the session speaks in a private or a shared place, the
//! session's directory shown from home, whether the operator lists the item's tree
//! as open source (`[privacy] open_source_paths`), and the operator's own paragraph
//! (`[content_based_security] context`). One place makes the five, and both the live
//! ask (`judge::content`) and a replay's rebuild (`learning::rebuild`) read it, so
//! a judgment replayed under v3 is asked what the daemon would ask now.
//!
//! **`public` is a cue the pack reads as the operator's own word, so it errs
//! toward false.** An item that names a path decides by its paths alone, and every
//! one of them must be under an open-source tree: a grep over a public project and a
//! private one is not public, and a session that sits in a public directory does not
//! make a read of some other directory public. Only an item that names no path takes
//! the session's directory. Trees are compared by path component, so
//! `~/projects/example-private` is not under `~/projects/example`, and `..` is
//! resolved before the comparison.

use std::path::{Component, Path, PathBuf};

use serde_json::Value;
use theseus_judge::builders::ContentInput;

use crate::content::ItemKind;

/// Who put an item there, as the pack reads it.
pub fn author_of(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::OperatorMessage => "owner",
        ItemKind::PersonMessage => "person",
        ItemKind::ToolResult
        | ItemKind::FetchedPage
        | ItemKind::SearchResult
        | ItemKind::TaskBrief
        | ItemKind::TaskReport => "tool",
    }
}

/// `path` with `home` shown as `~`: `~/projects/example`, or the path as it was.
pub fn home_shown(path: &str, home: Option<&Path>) -> String {
    let Some(home) = home else {
        return path.to_string();
    };
    match Path::new(path).strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.to_string(),
    }
}

/// `path` with `.` and `..` resolved by name, without touching the disk.
pub fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Whether `path` is `tree` or inside it, by component.
pub fn under(path: &Path, tree: &Path) -> bool {
    lexical(path).starts_with(lexical(tree))
}

/// The paths a `source` line names: each word that starts with `/` or `~/`, a
/// trailing `:line` or punctuation dropped. A URL names none.
pub fn paths_in(source: &str) -> Vec<String> {
    source
        .split_whitespace()
        .filter(|w| w.starts_with('/') || w.starts_with("~/"))
        .map(|w| {
            let w = w.trim_end_matches([',', ';', ')', '\'', '"']);
            // `a.rs:12:` and `a.rs:12:5`: a path never ends in a number after a colon.
            let mut end = w.len();
            while let Some(i) = w[..end].rfind(':') {
                if w[i + 1..end].chars().all(|c| c.is_ascii_digit()) {
                    end = i;
                } else {
                    break;
                }
            }
            w[..end].to_string()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

/// The paths a tool call's input names: the string values (and the strings of an
/// array) under `path`, `paths`, `file`, `files`, `dir`, `cwd` and `root`.
pub fn call_paths(input: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let Some(map) = input.as_object() else {
        return out;
    };
    // `fs.read`, `fs.write` and `fs.edit` also accept `file_path` and `filename`
    // (theseus-tools/src/fs.rs), and `file_path` is the spelling a model trained on
    // other harnesses reaches for. Missing them here made a read of a private file
    // from a session that sits in a listed tree fall back to the session's directory
    // and come out `public: true` -- the exact case this is meant to prevent.
    for key in [
        "path",
        "paths",
        "file",
        "files",
        "file_path",
        "filename",
        "dir",
        "cwd",
        "root",
    ] {
        match map.get(key) {
            Some(Value::String(s)) if !s.is_empty() => out.push(s.clone()),
            Some(Value::Array(a)) => out.extend(
                a.iter()
                    .filter_map(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string),
            ),
            _ => {}
        }
    }
    out
}

/// Where a session speaks and works.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scene {
    /// `private` or `shared`; none when the place cannot be read.
    pub place: Option<&'static str>,
    /// The session's directory, absolute: its record's, else the daemon's.
    pub dir: Option<String>,
}

/// The operator's config as the pack reads it, with the home it is shown from.
#[derive(Debug, Clone, Default)]
pub struct Around {
    open_source: Vec<PathBuf>,
    context: String,
    home: Option<PathBuf>,
}

impl Around {
    pub fn new(open_source: Vec<PathBuf>, context: &str, home: Option<PathBuf>) -> Self {
        Self {
            open_source,
            context: context.trim().to_string(),
            home,
        }
    }

    /// From the daemon's config: `[privacy] open_source_paths`,
    /// `[content_based_security] context`, and `$HOME`.
    pub fn from_config(cfg: &crate::Config) -> Self {
        Self::new(
            cfg.privacy.expanded_open_source_paths(),
            &cfg.content_based_security.context,
            std::env::var_os("HOME").map(PathBuf::from),
        )
    }

    /// Whether the operator lists any tree as open source at all.
    pub fn lists_any(&self) -> bool {
        !self.open_source.is_empty()
    }

    /// `path` as an absolute path: `~/` from home, a relative one from `dir`.
    fn resolve(&self, path: &str, dir: Option<&str>) -> Option<PathBuf> {
        if let Some(rest) = path.strip_prefix("~/") {
            return self.home.as_ref().map(|h| h.join(rest));
        }
        let p = Path::new(path);
        if p.is_absolute() {
            return Some(p.to_path_buf());
        }
        dir.map(|d| Path::new(d).join(p))
    }

    fn open(&self, path: &Path) -> bool {
        self.open_source.iter().any(|t| under(path, t))
    }

    /// Whether the operator lists the item's tree as open source (see the module
    /// note): `paths` are the ones the item names, each absolute, `~/`, or
    /// relative to `dir`; none names the session's directory.
    pub fn public(&self, dir: Option<&str>, paths: &[String]) -> bool {
        if self.open_source.is_empty() {
            return false;
        }
        if paths.is_empty() {
            return dir.is_some_and(|d| self.open(Path::new(d)));
        }
        paths
            .iter()
            .all(|p| self.resolve(p, dir).is_some_and(|p| self.open(&p)))
    }

    /// `input` with the five fields set.
    pub fn apply(&self, input: &mut ContentInput, kind: ItemKind, scene: &Scene, paths: &[String]) {
        input.author = Some(author_of(kind).to_string());
        input.place = scene.place.map(str::to_string);
        input.project = scene
            .dir
            .as_deref()
            .map(|d| home_shown(d, self.home.as_deref()));
        // Only what came from a path sits in a tree. A fetched page, a search
        // result, a person's message and the operator's own words name none, and
        // falling back to the session's directory would tell the pack that a page
        // fetched from anywhere is "from a tree the operator lists as open source".
        input.public = kind == ItemKind::ToolResult && self.public(scene.dir.as_deref(), paths);
        input.context = (!self.context.is_empty()).then(|| self.context.clone());
    }
}

impl crate::rpc::Core {
    /// Where `session` speaks and works, as the pack's state says it: the place
    /// rule's class of its target (a place that cannot be read counts as shared,
    /// as the rest of the daemon reads it), and its directory as its tools
    /// resolve it (`ToolRuntime::cwd_for`: the record's, else `[tools] cwd`).
    pub fn content_scene(&self, session: &str) -> Scene {
        use theseus_memory::recall::Place;
        let place = match self.runner.place_of(session) {
            Place::Private => "private",
            _ => "shared",
        };
        let dir = self
            .store
            .get_session::<crate::session::SessionRecord>(session)
            .ok()
            .flatten()
            .and_then(|r| r.dir);
        Scene {
            place: Some(place),
            dir: Some(
                self.tools
                    .cwd_for(dir.as_deref())
                    .to_string_lossy()
                    .into_owned(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn around() -> Around {
        Around::new(
            vec!["/home/owner/projects/lantern".into(), "/srv/open".into()],
            "A personal assistant. ~/projects/lantern is open source.",
            Some("/home/owner".into()),
        )
    }

    /// `fs.read`'s own spellings count as paths (theseus-gyjyo.11, the R9
    /// review): a model that sends `file_path` or `filename` must not fall
    /// through to the session's directory.
    #[test]
    fn a_calls_path_is_read_under_every_spelling_the_tools_accept() {
        for key in [
            "path",
            "paths",
            "file",
            "files",
            "file_path",
            "filename",
            "dir",
            "cwd",
            "root",
        ] {
            assert_eq!(
                call_paths(&json!({ key: "/a/b" })),
                vec!["/a/b".to_string()],
                "{key}"
            );
        }
    }

    /// Only what came from a path sits in a tree. A page, a search result and a
    /// message name none, so a session that happens to sit in a listed tree must
    /// not make them public (theseus-gyjyo.11, the R9 review).
    #[test]
    fn a_pathless_item_is_public_only_when_a_tool_result_read_the_tree() {
        let scene = Scene {
            place: Some("private"),
            dir: Some("/home/owner/projects/lantern".into()),
        };
        let public_of = |kind| {
            let mut i = ContentInput::default();
            around().apply(&mut i, kind, &scene, &[]);
            i.public
        };
        assert!(public_of(ItemKind::ToolResult), "a tool read the tree");
        for kind in [
            ItemKind::FetchedPage,
            ItemKind::PersonMessage,
            ItemKind::OperatorMessage,
        ] {
            assert!(!public_of(kind), "{kind:?} names no path");
        }
    }

    #[test]
    fn the_author_is_read_from_the_kind() {
        assert_eq!(author_of(ItemKind::OperatorMessage), "owner");
        assert_eq!(author_of(ItemKind::PersonMessage), "person");
        for k in [
            ItemKind::ToolResult,
            ItemKind::FetchedPage,
            ItemKind::SearchResult,
            ItemKind::TaskBrief,
            ItemKind::TaskReport,
        ] {
            assert_eq!(author_of(k), "tool", "{k:?}");
        }
    }

    #[test]
    fn a_home_directory_is_shown_as_a_tilde() {
        let home = Path::new("/home/owner");
        assert_eq!(
            home_shown("/home/owner/projects/lantern", Some(home)),
            "~/projects/lantern"
        );
        assert_eq!(home_shown("/home/owner", Some(home)), "~");
        assert_eq!(
            home_shown("/home/owner2/x", Some(home)),
            "/home/owner2/x",
            "a sibling of home is not under it"
        );
        assert_eq!(home_shown("/srv/open", Some(home)), "/srv/open");
        assert_eq!(home_shown("/srv/open", None), "/srv/open");
    }

    /// A tree is compared by component, and `..` is resolved first.
    #[test]
    fn a_tree_is_compared_by_component_after_dots_are_resolved() {
        let tree = Path::new("/home/owner/projects/lantern");
        for (p, want) in [
            ("/home/owner/projects/lantern", true),
            ("/home/owner/projects/lantern/src/main.rs", true),
            ("/home/owner/projects/lantern/", true),
            ("/home/owner/projects/lantern-private/x", false),
            ("/home/owner/projects", false),
            ("/home/owner/projects/lantern/../secret/x", false),
            ("/home/owner/projects/lantern/./src/../src/a", true),
            ("/home/owner/projects/other/../lantern/a", true),
        ] {
            assert_eq!(under(Path::new(p), tree), want, "{p}");
        }
        assert_eq!(lexical(Path::new("/a/b/../c/./d")), PathBuf::from("/a/c/d"));
    }

    #[test]
    fn a_source_line_names_its_paths_and_a_url_names_none() {
        assert_eq!(
            paths_in("fs.read /home/owner/projects/lantern/README.md"),
            ["/home/owner/projects/lantern/README.md"]
        );
        assert_eq!(
            paths_in("shell.run cd ~/projects/lantern && ls /srv/open"),
            ["~/projects/lantern", "/srv/open"]
        );
        assert_eq!(
            paths_in("fs.grep /home/owner/x/a.rs:12:"),
            ["/home/owner/x/a.rs"]
        );
        assert!(paths_in("http.fetch https://example.test/a/b").is_empty());
        assert!(paths_in("fs.grep").is_empty());
        assert!(paths_in("message").is_empty());
    }

    #[test]
    fn a_calls_input_names_its_paths() {
        assert_eq!(
            call_paths(&json!({"pattern": "TODO", "path": "src", "paths": ["a", "/srv/open/b"]})),
            ["src", "a", "/srv/open/b"]
        );
        assert_eq!(
            call_paths(&json!({"cwd": "/srv/open", "argv": ["ls"]})),
            ["/srv/open"]
        );
        assert!(call_paths(&json!({"pattern": "x"})).is_empty());
        assert!(call_paths(&json!("not an object")).is_empty());
    }

    /// The directory decides when the item names no path; the item's paths decide
    /// when it names any, and every one must be open.
    #[test]
    fn public_is_the_items_paths_or_else_the_sessions_directory() {
        let a = around();
        let dir = Some("/home/owner/projects/lantern");
        let s = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(a.public(dir, &[]), "no path: the directory decides");
        assert!(!a.public(Some("/home/owner/work/company"), &[]));
        assert!(!a.public(None, &[]));
        assert!(
            a.public(
                Some("/home/owner/work/company"),
                &s(&["~/projects/lantern/src"])
            ),
            "a private session reading a public tree"
        );
        assert!(
            !a.public(dir, &s(&["/home/owner/work/company/plan.md"])),
            "a public session reading a private file is not public"
        );
        assert!(
            !a.public(dir, &s(&["~/projects/lantern", "/home/owner/work/x"])),
            "every path must be open"
        );
        assert!(
            a.public(dir, &s(&["src/main.rs"])),
            "relative to the directory"
        );
        assert!(
            !a.public(Some("/home/owner/work/company"), &s(&["src/main.rs"])),
            "relative to a private directory"
        );
        assert!(a.public(None, &s(&["/srv/open/x"])));
        assert!(
            !a.public(dir, &s(&["../company/x"])),
            "a relative path out of the tree"
        );
        // Nothing listed: nothing is public.
        assert!(!Around::new(vec![], "", None).public(dir, &[]));
    }

    #[test]
    fn the_five_fields_are_set_on_an_input() {
        let mut i = ContentInput {
            text: "t".into(),
            kind: "tool_result".into(),
            source: "fs.grep".into(),
            ..ContentInput::default()
        };
        let scene = Scene {
            place: Some("private"),
            dir: Some("/home/owner/projects/lantern".into()),
        };
        around().apply(&mut i, ItemKind::ToolResult, &scene, &[]);
        assert_eq!(i.author.as_deref(), Some("tool"));
        assert_eq!(i.place.as_deref(), Some("private"));
        assert_eq!(i.project.as_deref(), Some("~/projects/lantern"));
        assert!(i.public);
        assert_eq!(
            i.context.as_deref(),
            Some("A personal assistant. ~/projects/lantern is open source.")
        );
        // An empty paragraph is not stated; a shared place and an owner's words.
        let mut j = i.clone();
        let scene = Scene {
            place: Some("shared"),
            dir: None,
        };
        Around::new(vec![], "  ", None).apply(&mut j, ItemKind::OperatorMessage, &scene, &[]);
        assert_eq!(
            (
                j.author.as_deref(),
                j.place.as_deref(),
                j.project.clone(),
                j.public,
                j.context.clone()
            ),
            (Some("owner"), Some("shared"), None, false, None)
        );
    }
}
