//! A thread's working set grows from its own turns (theseus-98bz.5). A
//! thread starts with what its owner, a split or an import gave it, and the
//! router's entity hits, the recall lens's naming waiver and Jev's thread
//! inputs all read that set: one nothing grows stays empty, and every one of
//! them stays blind. After each turn that ran in a thread, what the turn
//! touched and what its message named join the set (`rpc/thread_grow.rs`).
//!
//! - **Touched** is what the turn's tool calls named (`thread_split::touched`):
//!   a path made relative to the session's directory gives its file and its
//!   first component as a root (`tarnlib/reviews/comment-1.md` gives the file
//!   and the root `tarnlib`); a path outside that directory, or one that climbs
//!   out with `..`, gives nothing. A directory (`cwd`, `dir`) gives its first
//!   component; a URL its host.
//! - **Named** is the message's entities, as the import's extractor reads them
//!   (`threads_import::extract::working_set`).
//! - **Streams stay apart**: an entity another live thread holds (a sealed one
//!   included), compared without case as `thread_route::new_entities` does, is
//!   not added; that thread keeps it, and nothing is removed here.
//! - **A full list takes nothing more**: at [`MAX_SET`] a list is left as it
//!   is (eviction is not built).

use std::path::{Component, Path};

use theseus_protocol::threads::WorkingSet;

use crate::thread_join::MAX_SET;

/// A path as it reads from the session's directory: a relative one as it
/// is, an absolute one under `dir` made relative to it, `/`-joined; none for
/// one outside, one that climbs out, or the directory itself.
fn relative(path: &str, dir: &Path) -> Option<String> {
    let p = Path::new(path);
    let p = match p.is_absolute() {
        true => p.strip_prefix(dir).ok()?,
        false => p,
    };
    let mut parts = Vec::new();
    for c in p.components() {
        match c {
            Component::Normal(s) => parts.push(s.to_str()?),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// The first component of a relative path that has more than one.
fn root_of(rel: &str) -> Option<&str> {
    rel.split_once('/').map(|(r, _)| r)
}

/// What a turn's calls touched (`thread_split::touched`, as written), read
/// from the session's directory `dir`: files and roots relative to it, the
/// hosts as they are.
pub fn touched_in(calls: &WorkingSet, dir: &Path) -> WorkingSet {
    let mut w = WorkingSet {
        hosts: calls.hosts.clone(),
        ..WorkingSet::default()
    };
    for f in calls.files.iter().filter_map(|f| relative(f, dir)) {
        if let Some(r) = root_of(&f) {
            add(&mut w.roots, r);
        }
        add(&mut w.files, &f);
    }
    for d in calls.roots.iter().filter_map(|d| relative(d, dir)) {
        add(&mut w.roots, root_of(&d).unwrap_or(&d));
    }
    w
}

fn add(list: &mut Vec<String>, e: &str) {
    if !list.iter().any(|x| x == e) {
        list.push(e.to_string());
    }
}

/// Whether any of `sets` holds `e`, in any of its lists, without case.
fn held(sets: &[&WorkingSet], e: &str) -> bool {
    sets.iter().any(|w| {
        [&w.roots, &w.repos, &w.branches, &w.refs, &w.files, &w.hosts]
            .iter()
            .any(|l| l.iter().any(|x| x.eq_ignore_ascii_case(e)))
    })
}

/// `current` with what the turn `touched` and what its message `named` added
/// (touched first), but for what `elsewhere` (every other live thread's set)
/// holds; each list takes nothing past [`MAX_SET`].
pub fn grow(
    current: &WorkingSet,
    touched: &WorkingSet,
    named: &WorkingSet,
    elsewhere: &[&WorkingSet],
) -> WorkingSet {
    let mut next = current.clone();
    let lists = |w: &WorkingSet| {
        [
            w.roots.clone(),
            w.repos.clone(),
            w.branches.clone(),
            w.refs.clone(),
            w.files.clone(),
            w.hosts.clone(),
        ]
    };
    let (t, n) = (lists(touched), lists(named));
    let into = [
        &mut next.roots,
        &mut next.repos,
        &mut next.branches,
        &mut next.refs,
        &mut next.files,
        &mut next.hosts,
    ];
    for ((list, t), n) in into.into_iter().zip(t).zip(n) {
        for e in t.iter().chain(&n) {
            let mine = list.iter().any(|x| x.eq_ignore_ascii_case(e));
            if !mine && list.len() < MAX_SET && !held(elsewhere, e) {
                list.push(e.clone());
            }
        }
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws(roots: &[&str], refs: &[&str], files: &[&str]) -> WorkingSet {
        let v = |x: &[&str]| x.iter().map(|s| (*s).to_string()).collect();
        WorkingSet {
            roots: v(roots),
            refs: v(refs),
            files: v(files),
            ..Default::default()
        }
    }

    /// A path under the session's directory, or relative to it, gives its
    /// file and its first component; one outside, or climbing out, nothing.
    #[test]
    fn a_touched_path_gives_its_file_and_its_root() {
        let dir = Path::new("/work/proj");
        let calls = WorkingSet {
            files: vec![
                "/work/proj/tarnlib/reviews/comment-1.md".into(),
                "./datatidy/clean.py".into(),
                "README.md".into(),
                "/etc/hosts".into(),
                "../other/x.rs".into(),
            ],
            roots: vec!["/work/proj/homesvc/conf".into(), "/work/proj".into()],
            hosts: vec!["ci.example.test".into()],
            ..Default::default()
        };
        let w = touched_in(&calls, dir);
        assert_eq!(w.roots, ["tarnlib", "datatidy", "homesvc"]);
        assert_eq!(
            w.files,
            [
                "tarnlib/reviews/comment-1.md",
                "datatidy/clean.py",
                "README.md"
            ]
        );
        assert_eq!(w.hosts, ["ci.example.test"]);
    }

    /// What the turn touched and named joins the set, touched first, nothing
    /// twice (without case).
    #[test]
    fn a_turn_adds_what_it_touched_and_named() {
        let cur = ws(&[], &["prr-4142"], &[]);
        let touched = ws(&["tarnlib"], &[], &["tarnlib/reviews/comment-1.md"]);
        let named = ws(
            &[],
            &["PRR-4142", "OPS-77"],
            &["tarnlib/reviews/comment-1.md"],
        );
        let w = grow(&cur, &touched, &named, &[]);
        assert_eq!(w.roots, ["tarnlib"]);
        assert_eq!(w.refs, ["prr-4142", "OPS-77"]);
        assert_eq!(w.files, ["tarnlib/reviews/comment-1.md"]);
    }

    /// An entity another live thread holds stays that thread's, a sealed
    /// one's included (the caller passes every other live thread's set).
    #[test]
    fn an_entity_another_thread_holds_is_not_added() {
        let other = ws(&["Homesvc"], &[], &[]);
        let sealed = ws(&[], &["OPS-4214"], &[]);
        let touched = ws(&["homesvc", "tarnlib"], &[], &[]);
        let named = ws(&[], &["ops-4214", "PRR-4142"], &[]);
        let w = grow(&WorkingSet::default(), &touched, &named, &[&other, &sealed]);
        assert_eq!(w.roots, ["tarnlib"]);
        assert_eq!(w.refs, ["PRR-4142"]);
        assert_eq!(other.roots, ["Homesvc"], "the other thread keeps it");
    }

    /// A list at its cap takes nothing more; the others still grow.
    #[test]
    fn a_full_list_takes_nothing() {
        let full: Vec<String> = (0..MAX_SET).map(|i| format!("f{i}.rs")).collect();
        let cur = WorkingSet {
            files: full.clone(),
            ..Default::default()
        };
        let touched = ws(&["tarnlib"], &[], &["tarnlib/new.rs"]);
        let w = grow(&cur, &touched, &WorkingSet::default(), &[]);
        assert_eq!(w.files, full);
        assert_eq!(w.roots, ["tarnlib"]);
    }

    /// Nothing new leaves the set as it was, byte for byte.
    #[test]
    fn nothing_new_changes_nothing() {
        let cur = ws(&["tarnlib"], &["PRR-4142"], &["tarnlib/a.md"]);
        let again = ws(&["TARNLIB"], &["prr-4142"], &["tarnlib/a.md"]);
        assert_eq!(grow(&cur, &again, &again, &[]), cur);
    }
}
