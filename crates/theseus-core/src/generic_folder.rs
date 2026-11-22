//! A generic folder name names no thread (theseus-2uhsy; the owner, 2026-11-07:
//! option (b)). A working set keeps the folders a thread's turns touched as its
//! `roots`, and the recall lens's naming waiver (`thread_recall::named`) takes a
//! root for a name. (The router's entity hits and the growth that put roots
//! there went with threads, 2026-11-22.) A folder every stream writes to (`notes`) made
//! one thread's notes cross into every other thread, because any message that
//! said "notes" named it. A generic folder is the one a person types when they
//! have not decided, so it is no name.
//!
//! What counts as generic is not a list of this module's alone. A folder is
//! generic when it is any of:
//! - **a placeholder** ([`PLACEHOLDERS`]): `notes`, `misc`, `stuff` and the
//!   words like them;
//! - **layout** (`theseus_judge::mine::is_layout_dir`): `src`, `docs`, `tmp`,
//!   `workspace` and the rest of what the judge's name mining already skips in
//!   a path;
//! - **a generic word** ([`GENERIC`], the router's list until it went):
//!   `main`, `test`, `core` and the like.
//!
//! A root that is generic is not added to a set, and one a set already holds
//! (an older thread's) is read as no name, so no stored thread needs mending.
//! A thread left with no roots is scored as before. The test below walks every
//! entry through the reader, so a list and a reader that drift apart fail
//! naming this file.

use theseus_judge::mine::is_layout_dir;

/// Words too generic to name a thread's work.
pub(crate) const GENERIC: &[&str] = &[
    "main", "master", "develop", "dev", "src", "lib", "test", "tests", "docs", "doc", "home",
    "tmp", "bin", "app", "api", "web", "core", "build", "target", "node", "git", "the", "and",
];

/// The placeholder names: what someone types for a folder when they have not
/// decided what it is. The owner's three first (`notes`, `misc`, `stuff`).
/// Lower case, sorted; a name here is a whole folder name, never a part of one.
pub const PLACEHOLDERS: &[&str] = &[
    "archive",
    "backup",
    "backups",
    "desktop",
    "documents",
    "downloads",
    "drafts",
    "files",
    "general",
    "misc",
    "miscellaneous",
    "new",
    "note",
    "notes",
    "old",
    "other",
    "random",
    "scratch",
    "stuff",
    "temp",
    "things",
    "untitled",
];

/// Whether the folder `name` (a bare name, any case) is too generic to name a
/// thread.
pub fn is_generic_folder(name: &str) -> bool {
    let n = name.trim().trim_matches('/').to_lowercase();
    PLACEHOLDERS.contains(&n.as_str()) || is_layout_dir(&n) || GENERIC.contains(&n.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thread_recall::named;
    use crate::threads::Board;
    use theseus_protocol::threads::{ThreadInfo, ThreadState, WorkingSet};

    const FILE: &str = "crates/theseus-core/src/generic_folder.rs";

    fn thread_holding(root: &str) -> ThreadInfo {
        ThreadInfo {
            slug: "homesvc".into(),
            title: "Home service".into(),
            goal: String::new(),
            topics: vec![],
            working_set: WorkingSet {
                roots: vec![root.into()],
                ..Default::default()
            },
            card: vec![],
            persona: None,
            state: ThreadState::Active,
            sealed: None,
            seal_pending: None,
            surfaces: vec![],
            parent: None,
            merged_into: None,
            split_from: None,
            links: vec![],
            session_id: None,
            last_active_ms: 1,
            created_at_ms: 1,
        }
    }

    /// Whether recall's waiver takes a root named `name` for a name.
    fn waived(name: &str) -> bool {
        let board = Board::build(vec![thread_holding(name)], &Default::default());
        !named(&board, &format!("put it in {name} please")).is_empty()
    }

    /// A generic folder names no thread for recall's waiver; a specific one, or one that only
    /// holds a generic word, still does.
    #[test]
    fn a_generic_folder_is_no_name_to_recalls_waiver() {
        assert!(
            !waived("notes") && !waived("Notes"),
            "notes is generic, in any case"
        );
        assert!(waived("tarnlib"), "a specific folder names one");
        assert!(waived("harbour-notes"), "a name that holds one is a name");
    }

    /// The owner's three are in the list, and the list is well formed.
    #[test]
    fn the_list_holds_the_owners_three_and_is_well_formed() {
        for w in ["notes", "misc", "stuff"] {
            assert!(PLACEHOLDERS.contains(&w), "{w} is in {FILE}");
        }
        let mut sorted = PLACEHOLDERS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            sorted, PLACEHOLDERS,
            "{FILE}: PLACEHOLDERS sorted, no repeats"
        );
        for p in PLACEHOLDERS {
            assert!(
                !p.is_empty() && *p == p.trim() && *p == p.to_lowercase() && !p.contains('/'),
                "{FILE}: {p:?} must be a lower-case bare folder name"
            );
        }
    }

    /// Recall's waiver, list by list: a placeholder, a generic word and a layout name are each
    /// read as generic, and none of them names a thread to the waiver. A list that gains a name
    /// the rule cannot see fails here naming this file.
    #[test]
    fn recalls_waiver_takes_no_name_from_any_generic_list() {
        let lists: [(&str, &[&str]); 3] = [
            ("PLACEHOLDERS", PLACEHOLDERS),
            ("GENERIC", GENERIC),
            ("LAYOUT_PROBES", LAYOUT_PROBES),
        ];
        let read_wrong: Vec<String> = (lists.iter())
            .flat_map(|(list, names)| names.iter().map(move |n| (*list, *n)))
            .filter(|(_, n)| !is_generic_folder(n) || waived(n))
            .map(|(list, n)| format!("{list}: {n}"))
            .collect();
        assert!(
            read_wrong.is_empty(),
            "{FILE}: these are not read as generic, or thread_recall::named still names a thread \
             by them: {read_wrong:?}"
        );
    }

    /// The judge's layout names, probed through this rule: if the judge's list
    /// loses one, this fails naming the judge's file rather than passing on a
    /// shorter list.
    const LAYOUT_PROBES: &[&str] = &["src", "docs", "tmp", "workspace", "node_modules"];

    #[test]
    fn the_judges_layout_names_are_still_generic_here() {
        for n in LAYOUT_PROBES {
            assert!(
                is_layout_dir(n),
                "{n} left crates/theseus-judge/src/mine.rs's GENERIC: correct that list, or drop \
                 it from LAYOUT_PROBES in {FILE} if it was meant to go"
            );
        }
    }
}
