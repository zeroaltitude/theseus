//! The unit's text: which program it runs, and the one change this module makes to it.

use std::path::{Path, PathBuf};

use super::LIB_DIR;

/// A service whose program is in a per-commit directory of the root-owned program directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rooted {
    pub program: PathBuf,
}

/// `program` is `/usr/local/lib/theseus/<7 or more hex digits>/theseusd`: the layout the helper installs into. The
/// separate-user layout's `/usr/local/lib/theseus/theseusd` is not: it is a system unit this does not touch.
pub fn rooted(program: &Path) -> Option<Rooted> {
    let dir = program.parent()?;
    let name = dir.file_name()?.to_str()?;
    let in_lib = dir.parent()? == Path::new(LIB_DIR);
    let hex = name.len() >= 7
        && name
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    (in_lib && hex && program.file_name()? == "theseusd").then(|| Rooted {
        program: program.to_path_buf(),
    })
}

/// The program the one `ExecStart=` a file leaves names: its first word, which must be an absolute path. A bare
/// `ExecStart=` clears what came before it, so a drop-in that clears and sets leaves its own one line.
pub fn exec_program(text: &str) -> Option<String> {
    super::start::one_in(text).map(|(program, _)| program)
}

/// The file's text with the program of its one `ExecStart=` line changed from `old` to `new`, and not a byte else: the
/// line's arguments, its spacing, every other line. Refused when the program is not `old` (it changed since it was
/// read), or when the file leaves no single `ExecStart=`.
pub fn repoint(text: &str, old: &str, new: &str) -> Result<String, String> {
    match super::start::one_in(text) {
        Some((p, at)) if p == old => {
            let mut out = String::with_capacity(text.len() + new.len());
            out.push_str(&text[..at.start]);
            out.push_str(new);
            out.push_str(&text[at.end..]);
            Ok(out)
        }
        Some((p, _)) => Err(format!(
            "the start command runs {p}, not {old}: it changed since it was read"
        )),
        None => Err("the file has no single ExecStart= naming a program".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLD: &str = "/usr/local/lib/theseus/aaaaaaaa/theseusd";
    const NEW: &str = "/usr/local/lib/theseus/bbbbbbbb/theseusd";

    fn unit(exec: &str) -> String {
        format!("# Written by `theseusd install --user`.\n[Service]\nType=notify\nExecStart={exec}\nRestart=on-failure\n")
    }

    #[test]
    fn only_a_per_commit_directory_of_the_program_directory_is_rooted() {
        assert!(rooted(Path::new(OLD)).is_some());
        for p in [
            "/usr/local/lib/theseus/theseusd",
            "/usr/local/lib/theseus/aaaaaaaa/theseus",
            "/usr/local/lib/theseus/AAAAAAAA/theseusd",
            "/usr/local/lib/theseus/aa/theseusd",
            "/home/x/.local/bin/theseusd",
            "/opt/theseus/aaaaaaaa/theseusd",
            "/usr/local/lib/theseus/aaaaaaaa/b/theseusd",
        ] {
            assert!(rooted(Path::new(p)).is_none(), "{p}");
        }
    }

    #[test]
    fn the_unit_changes_by_its_program_and_nothing_else() {
        let text = unit(&format!(
            "{OLD} --config /etc/theseus/theseus.toml --op-token-file /home/x/tok"
        ));
        let want = text.replace(OLD, NEW);
        assert_eq!(repoint(&text, OLD, NEW).unwrap(), want);
        assert_eq!(exec_program(&want).as_deref(), Some(NEW));
        // Other lines that name the old program are not touched (a comment, an Environment).
        let odd = format!("# was {OLD}\n{}", unit(OLD));
        assert!(repoint(&odd, OLD, NEW)
            .unwrap()
            .starts_with(&format!("# was {OLD}\n")));
    }

    #[test]
    fn a_unit_that_is_not_what_was_read_is_refused() {
        assert!(repoint(&unit("/usr/bin/other"), OLD, NEW)
            .unwrap_err()
            .contains("changed"));
        assert!(repoint("[Service]\n", OLD, NEW).is_err());
        let two = format!("{}ExecStart={OLD}\n", unit(OLD));
        assert!(repoint(&two, OLD, NEW).is_err(), "two ExecStart lines");
        assert!(exec_program(&unit("relative/theseusd")).is_none());
    }
}
