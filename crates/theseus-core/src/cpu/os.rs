//! The OS and the architecture a command runs on (theseus-wadu;
//! `docs/design/project-context.md` §4). A model that knows the CPU count
//! but not the system is still guessing at the shell, the path separator
//! and what a downloaded binary will run on. These are **static for the
//! process**, so §4 puts them in block 3 beside the CPU fact, and not in the
//! messages' tail where the date and the git state go: nothing per request
//! belongs inside a layer, and these are not per request.
//!
//! **The fact is the same bytes on every machine of a family**, by choice.
//! The distribution's own name (`/etc/os-release`'s `PRETTY_NAME`) was
//! weighed and left out: block 3's bytes are pinned by four artefacts — the
//! token counts in `tests_tasks_wake`, `own_bench`'s first-request golden,
//! `tests_private_view`'s request golden and theseus-discord's DM flow
//! golden — and a name whose length is the machine's would make every one of
//! them fail on any machine but the one they were written on, every gate, for
//! ever. The CPU fact already carries the machine's own numbers; this one
//! carries what is true of every machine running this build. A distribution
//! name can join it later behind a seam the tests pin.

/// The system a command runs on: what this build targets, which for a native
/// daemon is what it runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Os {
    /// The OS family (`linux`, `macos`, …).
    pub name: &'static str,
    /// The architecture (`x86_64`, `aarch64`, …).
    pub arch: &'static str,
}

impl Os {
    /// This machine's.
    pub fn of_machine() -> Self {
        Self {
            name: std::env::consts::OS,
            arch: std::env::consts::ARCH,
        }
    }

    /// The fact as the model reads it, one line beside the CPU fact.
    pub fn fact(&self) -> String {
        format!("OS: {} ({}).", self.name, self.arch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One line, the family and the architecture, and the same bytes on
    /// every machine of a family: block 3's own bytes are pinned elsewhere.
    #[test]
    fn the_fact_is_one_line_of_the_family_and_the_architecture() {
        let os = Os::of_machine();
        assert_eq!(os.name, std::env::consts::OS);
        assert_eq!(os.arch, std::env::consts::ARCH);
        let fact = os.fact();
        assert_eq!(fact.lines().count(), 1);
        assert!(fact.starts_with("OS: "), "{fact}");
        assert!(fact.ends_with(").") && fact.contains(" ("), "{fact}");
        assert_eq!(Os::of_machine().fact(), fact, "the same every time");
    }

    /// A worked example, so a reader of the test knows the shape without
    /// running it on the machine that wrote it.
    #[test]
    fn a_linux_machines_fact_reads_like_this() {
        let os = Os {
            name: "linux",
            arch: "x86_64",
        };
        assert_eq!(os.fact(), "OS: linux (x86_64).");
    }
}
