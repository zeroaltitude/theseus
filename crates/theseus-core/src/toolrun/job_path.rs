//! A job's `PATH`: the daemon's, the login shell's, and the operator's own
//! directories first (theseus-v4cj, at parity with OpenClaw).
//!
//! A systemd user service never sources the shell's rc files, so the daemon's
//! own `PATH` is the unit's and not the one the owner has in a terminal. The
//! login-shell capture (`super::login_env`) reads the terminal's, and until
//! this module every captured name but `PATH` reached a job: `job_base_env`
//! let the daemon's value of a name win, and `PATH` is a name, so a program
//! the owner can run in his own shell was not on a job's `PATH`.
//!
//! The owner asked for parity with OpenClaw's practice, which merges the login
//! shell's `PATH` into the exec environment (his words of 2026-11-12 15:42:45,
//! quoted in the commit). He was told explicitly that this is OpenClaw's
//! practice and not a proven general standard; that qualification is part of
//! the record.
//!
//! **Merge, in this order**, and the order is the whole of the decision here:
//!
//! 1. `[tools] path_prepend`, the operator's own directories (OpenClaw spells
//!    this `tools.exec.pathPrepend`), unset by default;
//! 2. the daemon's own `PATH`, entry by entry, in its own order;
//! 3. the login shell's entries that 1 and 2 do not already hold.
//!
//! The daemon's entries keep their places, so **nothing a job could resolve
//! before resolves to a different program now**: the merge only adds
//! directories after the ones that were already searched. The other order
//! (the shell's first) was rejected for exactly that reason — it would move
//! every lookup a job makes, which is a change nobody asked for and which no
//! test could have caught. The operator who wants a directory searched first
//! says so with `path_prepend`, which is what part 4 of the ask is for.
//!
//! An empty entry is dropped: an empty element of `PATH` means the current
//! directory to every shell and to `execvp`, and a job's current directory is
//! the model's to choose.
//!
//! A **per-call `PATH`** is refused by name, never merged and never honoured
//! (`super::job::job_path_refusal`): a call's `env` is the model's, and a
//! model that could re-point `PATH` could put any directory it can write in
//! front of every program the gate judged by argv.

/// The merged `PATH` for a job, from the operator's `prepend` directories,
/// the daemon's `own` value and the login shell's `shell` value (each
/// `None` where there is none). `None` when every source is absent or empty:
/// the caller then sets no `PATH` at all, as it did before this module.
pub(crate) fn merge(prepend: &[String], own: Option<&str>, shell: Option<&str>) -> Option<String> {
    let mut out: Vec<&str> = Vec::new();
    let sources = prepend.iter().map(String::as_str).chain(
        [own, shell]
            .into_iter()
            .flatten()
            .flat_map(|v| v.split(':')),
    );
    for dir in sources {
        // An empty element means the current directory to every shell and to
        // `execvp`, and a job's current directory is the model's to choose.
        if !dir.is_empty() && !out.contains(&dir) {
            out.push(dir);
        }
    }
    (!out.is_empty()).then(|| out.join(":"))
}

#[cfg(test)]
mod tests {
    use super::merge;

    #[test]
    fn the_shells_directories_follow_the_daemons_and_neither_loses_a_place() {
        let got = merge(
            &[],
            Some("/usr/bin:/bin"),
            Some("/opt/example/bin:/usr/bin"),
        );
        assert_eq!(got.as_deref(), Some("/usr/bin:/bin:/opt/example/bin"));
    }

    #[test]
    fn the_operators_prepend_goes_first_and_is_not_repeated() {
        let got = merge(
            &["/opt/example/first".into(), "/usr/bin".into()],
            Some("/usr/bin:/bin"),
            Some("/opt/example/shell"),
        );
        assert_eq!(
            got.as_deref(),
            Some("/opt/example/first:/usr/bin:/bin:/opt/example/shell")
        );
    }

    #[test]
    fn an_empty_entry_is_dropped_because_it_means_the_current_directory() {
        let got = merge(&[String::new()], Some("/usr/bin::/bin"), Some(":"));
        assert_eq!(got.as_deref(), Some("/usr/bin:/bin"));
    }

    #[test]
    fn no_source_at_all_sets_no_path() {
        assert_eq!(merge(&[], None, None), None);
        assert_eq!(merge(&[], Some(""), Some("")), None);
    }

    #[test]
    fn the_shell_alone_is_the_whole_path() {
        assert_eq!(
            merge(&[], None, Some("/opt/example/shell/bin")).as_deref(),
            Some("/opt/example/shell/bin")
        );
    }
}
