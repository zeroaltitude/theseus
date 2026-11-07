//! A generic folder name names no thread (theseus-2uhsy; the owner, 2026-11-07:
//! option (b)). A working set keeps the folders a thread's turns touched as its
//! `roots`, and three readers take a root for a name: the router's entity hits
//! (`thread_route::entity_hits`), the recall lens's naming waiver
//! (`thread_recall::named`), and the growth that puts one there
//! (`thread_grow::touched_in`). A folder every stream writes to (`notes`) made
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
//! - **a generic word** of the router (`thread_route::GENERIC`): `main`,
//!   `test`, `core` and the like, which it already ignores for every other
//!   kind of entity.
//!
//! A root that is generic is not added to a set, and one a set already holds
//! (an older thread's) is read as no name, so no stored thread needs mending.
//! A thread left with no roots is scored as before. The test below walks every
//! entry through all three readers, so a list and a reader that drift apart
//! fail naming this file.

use theseus_judge::mine::is_layout_dir;

use crate::thread_route::GENERIC;

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
    use crate::thread_grow::touched_in;
    use crate::thread_recall::named;
    use crate::thread_route::entity_hits;
    use crate::threads::Board;
    use std::path::Path;
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

    /// What each reader makes of a root named `name`: grown from a touched
    /// file, taken by the router's entity hits, taken by recall's waiver.
    fn takes(name: &str) -> [bool; 3] {
        let calls = WorkingSet {
            files: vec![format!("/work/proj/{name}/a.md")],
            ..Default::default()
        };
        let grown = !touched_in(&calls, Path::new("/work/proj")).roots.is_empty();
        let w = WorkingSet {
            roots: vec![name.into()],
            ..Default::default()
        };
        let routed = !entity_hits(&format!("look in {name} again"), &w).is_empty();
        let board = Board::build(vec![thread_holding(name)], &Default::default());
        let waived = !named(&board, &format!("put it in {name} please")).is_empty();
        [grown, routed, waived]
    }

    /// The red test: before this change `notes` grew into a set, hit the
    /// router and waived recall's distance, as any specific folder does.
    #[test]
    fn a_generic_folder_names_no_thread_and_a_specific_one_still_does() {
        assert_eq!(takes("notes"), [false; 3], "notes is generic");
        assert_eq!(takes("Notes"), [false; 3], "in any case");
        assert_eq!(takes("tarnlib"), [true; 3], "a specific folder names one");
        // Only a whole folder name is generic: a name that holds one is a name.
        assert_eq!(takes("harbour-notes"), [true; 3]);
    }

    /// An absolute root's last part is read the same way.
    #[test]
    fn an_absolute_root_is_read_by_its_last_part() {
        let hit = |root: &str, said: &str| {
            let w = WorkingSet {
                roots: vec![root.into()],
                ..Default::default()
            };
            !entity_hits(said, &w).is_empty()
        };
        assert!(!hit("/work/proj/notes", "work in notes now"));
        assert!(!hit("/work/proj/notes/", "work in notes now"));
        assert!(hit("/work/proj/tidewatch", "work in tidewatch now"));
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

    /// The completeness test: every entry the rule reads as generic, from any
    /// of its three sources, is taken by no reader. A reader that stops
    /// calling [`is_generic_folder`], or a list that gains a name a reader
    /// cannot see, fails here and says which file to correct.
    #[test]
    fn every_generic_name_is_refused_by_every_reader() {
        let all: Vec<&str> = PLACEHOLDERS
            .iter()
            .chain(GENERIC.iter())
            .copied()
            .chain(LAYOUT_PROBES.iter().copied())
            .collect();
        for name in all {
            assert!(
                is_generic_folder(name),
                "{name} is listed as generic but {FILE}'s is_generic_folder does not read it so"
            );
            let [grown, routed, waived] = takes(name);
            assert!(
                !grown,
                "{name}: thread_grow::touched_in still grows it as a root; it must call \
                 generic_folder::is_generic_folder (crates/theseus-core/src/thread_grow.rs)"
            );
            assert!(
                !routed,
                "{name}: thread_route::entity_hits still hits on it as a root \
                 (crates/theseus-core/src/thread_route.rs)"
            );
            assert!(
                !waived,
                "{name}: thread_recall::named still names a thread by it \
                 (crates/theseus-core/src/thread_recall.rs)"
            );
        }
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
