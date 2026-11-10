//! The one walk `fs.list`, `fs.glob` and `fs.grep` share, and what their
//! results say it left out.

use std::path::Path;

/// The one walk `fs.list`, `fs.glob` and `fs.grep` share, so "ignored" means
/// one thing. `all` shows hidden and ignored entries too; `.git` is never
/// walked. The global gitignore is never read, since a listing must not
/// depend on the daemon's account, and the scope lines name only
/// ".gitignore'd" paths.
pub(crate) fn walk_builder(
    base: &Path,
    all: bool,
    max_depth: Option<usize>,
) -> ignore::WalkBuilder {
    let mut b = ignore::WalkBuilder::new(base);
    b.hidden(!all)
        .git_ignore(!all)
        .git_global(false)
        .git_exclude(!all)
        .ignore(!all)
        .parents(!all)
        .require_git(false)
        .max_depth(max_depth);
    // Never descend into VCS internals even when hidden files are shown.
    b.filter_entry(|e| e.file_name() != ".git");
    b
}

pub(crate) fn walker(base: &Path, all: bool, max_depth: Option<usize>) -> ignore::Walk {
    walk_builder(base, all, max_depth).build()
}

/// What `fs.glob` and `fs.grep` never walk, which their results state as part
/// of their scope (Appendix F's rule, theseus-8ye): an empty or narrow result
/// must not read as "there is none".
pub(crate) const NOT_WALKED: &str = "hidden files and .gitignore'd paths are not searched";

/// The same, for a call that gave `all`: only VCS internals are left out.
pub(crate) const NOT_WALKED_ALL: &str =
    "hidden and .gitignore'd paths included; .git is never searched";

pub(crate) fn not_walked(all: bool) -> &'static str {
    match all {
        true => NOT_WALKED_ALL,
        false => NOT_WALKED,
    }
}
