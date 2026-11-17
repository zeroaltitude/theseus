//! The helper's logic against directories a test owns, without root. The real paths are compiled into
//! [`Config::real`]; these tests build a [`Config`] of their own, which only a test can (`tests/real_program.rs` holds
//! the real program to that).

use std::ffi::OsString;
use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use crate::{parse_args, run, Config, Outcome, Refusal, MAX_BUILDS, PROGRAMS};

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
const OTHER: &str = "fedcba9876543210fedcba9876543210fedcba98";

fn mode(p: &Path, m: u32) {
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(m)).unwrap();
}

/// A program: an ELF header, some bytes, and the stamp of `commit` (none for an empty `commit`).
fn program(name: &str, commit: &str) -> Vec<u8> {
    let mut b = b"\x7fELF fake ".to_vec();
    b.extend_from_slice(name.as_bytes());
    if !commit.is_empty() {
        b.extend_from_slice(format!(" theseus-build-stamp:{commit}: ").as_bytes());
    }
    b
}

struct Rig {
    _dir: tempfile::TempDir,
    cfg: Config,
}

impl Rig {
    /// An anchor, a staging directory with the four programs of `COMMIT`, and an empty program directory.
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        mode(&root, 0o755);
        let (stage_parent, lib) = (root.join("var"), root.join("lib"));
        std::fs::create_dir_all(stage_parent.join("stage")).unwrap();
        std::fs::create_dir(&lib).unwrap();
        for d in [&stage_parent, &lib] {
            mode(d, 0o755);
        }
        mode(&stage_parent.join("stage"), 0o700);
        let cfg = Config {
            stage: stage_parent.join("stage"),
            lib_dir: lib,
            anchor: root,
            trusted_uid: std::fs::metadata(dir.path()).unwrap().uid(),
            real: false,
        };
        let rig = Self { _dir: dir, cfg };
        rig.stage_all(COMMIT);
        rig
    }

    fn stage_all(&self, commit: &str) {
        for p in PROGRAMS {
            std::fs::write(self.cfg.stage.join(p), program(p, commit)).unwrap();
        }
    }

    fn run(&self, commit: &str) -> (i32, Vec<String>) {
        let mut lines = Vec::new();
        let code = run(&self.cfg, &[commit.into()], &mut |l| {
            lines.push(l.to_string())
        });
        (code, lines)
    }

    fn refused(&self, commit: &str) -> String {
        let (code, lines) = self.run(commit);
        assert_eq!(code, 1, "{lines:?}");
        assert_eq!(lines.len(), 1, "one line for a refusal: {lines:?}");
        lines.into_iter().next().unwrap()
    }

    fn lib_entries(&self) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(&self.cfg.lib_dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    fn install(&self) -> Outcome {
        crate::install::install(&self.cfg, COMMIT).unwrap()
    }
}

fn args(a: &[&str]) -> Vec<OsString> {
    a.iter().map(OsString::from).collect()
}

#[test]
fn the_argument_is_one_commit_of_forty_lowercase_hex_digits() {
    assert_eq!(parse_args(&args(&[COMMIT])).unwrap(), COMMIT);
    let bad: [&[&str]; 9] = [
        &[],
        &[COMMIT, COMMIT],
        &[COMMIT, "--stage=/tmp/x"],
        &["--help"],
        &["0123456789abcdef0123456789abcdef0123456"],
        &["0123456789abcdef0123456789abcdef012345678"],
        &["0123456789ABCDEF0123456789abcdef01234567"],
        &["/usr/local/lib/theseus/0123456789abcdef0123456789abcdef01234567"],
        &["0123456789abcdef0123456789abcdef0123456g"],
    ];
    for a in bad {
        assert!(parse_args(&args(a)).is_err(), "{a:?} must be refused");
    }
}

#[test]
fn it_installs_the_four_programs_into_one_new_directory() {
    let rig = Rig::new();
    let (code, lines) = rig.run(COMMIT);
    assert_eq!((code, lines.len()), (0, 1), "{lines:?}");
    assert!(
        lines[0].starts_with(&format!("installed {COMMIT} in ")),
        "{lines:?}"
    );
    assert_eq!(
        rig.lib_entries(),
        ["01234567"],
        "the one directory, and no temporary one left"
    );
    let dir = rig.cfg.lib_dir.join("01234567");
    assert_eq!(std::fs::metadata(&dir).unwrap().mode() & 0o7777, 0o755);
    for p in PROGRAMS {
        let m = std::fs::metadata(dir.join(p)).unwrap();
        assert_eq!(m.mode() & 0o7777, 0o755, "{p}");
        assert_eq!(
            std::fs::read(dir.join(p)).unwrap(),
            program(p, COMMIT),
            "{p}"
        );
    }
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), PROGRAMS.len());
}

#[test]
fn it_does_not_touch_the_staging_directory() {
    let rig = Rig::new();
    rig.install();
    for p in PROGRAMS {
        assert_eq!(
            std::fs::read(rig.cfg.stage.join(p)).unwrap(),
            program(p, COMMIT),
            "{p}"
        );
    }
}

#[test]
fn the_same_content_again_changes_nothing_and_says_so() {
    let rig = Rig::new();
    rig.install();
    let before = std::fs::metadata(rig.cfg.lib_dir.join("01234567/theseusd"))
        .unwrap()
        .ino();
    let (code, lines) = rig.run(COMMIT);
    assert_eq!(code, 0, "{lines:?}");
    assert!(lines[0].starts_with("already installed"), "{lines:?}");
    let after = std::fs::metadata(rig.cfg.lib_dir.join("01234567/theseusd"))
        .unwrap()
        .ino();
    assert_eq!(before, after, "the existing file is the same file");
    assert_eq!(rig.lib_entries(), ["01234567"]);
}

#[test]
fn an_existing_directory_whose_content_differs_is_refused_and_kept() {
    let rig = Rig::new();
    rig.install();
    // The same eight digits, another commit: the stamp names it, the directory is the same.
    let same8 = format!("01234567{}", &OTHER[8..]);
    rig.stage_all(&same8);
    let line = rig.refused(&same8);
    assert!(line.contains("already exists and differs"), "{line}");
    let kept = std::fs::read(rig.cfg.lib_dir.join("01234567/theseusd")).unwrap();
    assert_eq!(
        kept,
        program("theseusd", COMMIT),
        "the existing build is untouched"
    );
    assert_eq!(rig.lib_entries(), ["01234567"]);
}

#[test]
fn a_symlink_among_the_programs_is_refused() {
    let rig = Rig::new();
    let secret = rig.cfg.anchor.join("secret");
    std::fs::write(&secret, program("theseus", COMMIT)).unwrap();
    std::fs::remove_file(rig.cfg.stage.join("theseus")).unwrap();
    symlink(&secret, rig.cfg.stage.join("theseus")).unwrap();
    let line = rig.refused(COMMIT);
    assert!(line.contains("not a regular file"), "{line}");
    assert!(
        rig.lib_entries().is_empty(),
        "nothing written, nothing left"
    );
}

#[test]
fn a_hard_link_is_refused() {
    let rig = Rig::new();
    std::fs::hard_link(
        rig.cfg.stage.join("theseus"),
        rig.cfg.anchor.join("elsewhere"),
    )
    .unwrap();
    let line = rig.refused(COMMIT);
    assert!(line.contains("more than one link"), "{line}");
    assert!(rig.lib_entries().is_empty());
}

#[test]
fn an_extra_file_a_missing_file_or_a_directory_is_refused() {
    let rig = Rig::new();
    std::fs::write(rig.cfg.stage.join("notes.txt"), "x").unwrap();
    assert!(rig.refused(COMMIT).contains("extra"));
    std::fs::remove_file(rig.cfg.stage.join("notes.txt")).unwrap();
    std::fs::remove_file(rig.cfg.stage.join("theseus-tui")).unwrap();
    assert!(rig.refused(COMMIT).contains("missing"));
    std::fs::create_dir(rig.cfg.stage.join("theseus-tui")).unwrap();
    assert!(rig.refused(COMMIT).contains("not a regular file"));
    assert!(rig.lib_entries().is_empty());
}

#[test]
fn a_program_that_is_not_an_elf_or_is_empty_is_refused() {
    let rig = Rig::new();
    std::fs::write(rig.cfg.stage.join("theseus-index"), "#!/bin/sh\necho hi\n").unwrap();
    assert!(rig.refused(COMMIT).contains("no ELF header"));
    std::fs::write(rig.cfg.stage.join("theseus-index"), "").unwrap();
    assert!(rig.refused(COMMIT).contains("empty"));
    assert!(rig.lib_entries().is_empty());
}

#[test]
fn a_build_stamp_that_is_missing_or_names_another_commit_is_refused() {
    let rig = Rig::new();
    rig.stage_all("");
    assert!(rig.refused(COMMIT).contains("no build stamp"));
    rig.stage_all(OTHER);
    assert!(rig.refused(COMMIT).contains(OTHER));
    // One right stamp and one wrong one is a wrong one.
    let mut two = program("theseusd", COMMIT);
    two.extend_from_slice(format!(" theseus-build-stamp:{OTHER}: ").as_bytes());
    rig.stage_all(COMMIT);
    std::fs::write(rig.cfg.stage.join("theseusd"), two).unwrap();
    assert!(rig.refused(COMMIT).contains(OTHER));
    let mut odd = program("theseusd", COMMIT);
    odd.extend_from_slice(b" theseus-build-stamp:abc: ");
    std::fs::write(rig.cfg.stage.join("theseusd"), odd).unwrap();
    assert!(rig.refused(COMMIT).contains("malformed"));
    assert!(
        rig.lib_entries().is_empty(),
        "a refusal leaves no temporary directory"
    );
}

#[test]
fn the_stage_and_the_program_directory_must_be_plain_trusted_directories() {
    // The staging directory is a link.
    let rig = Rig::new();
    let real = rig.cfg.anchor.join("var/real-stage");
    std::fs::rename(&rig.cfg.stage, &real).unwrap();
    symlink(&real, &rig.cfg.stage).unwrap();
    assert!(rig.refused(COMMIT).contains("opening"));
    // The staging directory is writable by others.
    let rig = Rig::new();
    mode(&rig.cfg.stage, 0o770);
    assert!(rig
        .refused(COMMIT)
        .contains("writable by its group or others"));
    // The program directory is writable by others, and a link.
    let rig = Rig::new();
    mode(&rig.cfg.lib_dir, 0o777);
    assert!(rig.refused(COMMIT).contains("writable by no one else"));
    let rig = Rig::new();
    let real = rig.cfg.anchor.join("real-lib");
    std::fs::rename(&rig.cfg.lib_dir, &real).unwrap();
    symlink(&real, &rig.cfg.lib_dir).unwrap();
    assert!(rig.refused(COMMIT).contains("opening"));
    assert_eq!(
        std::fs::read_dir(&real).unwrap().count(),
        0,
        "nothing written through the link"
    );
}

#[test]
fn a_parent_that_others_can_write_is_refused() {
    let rig = Rig::new();
    mode(&rig.cfg.anchor.join("var"), 0o757);
    assert!(rig.refused(COMMIT).contains("writable by no one else"));
}

#[test]
fn a_program_directory_over_the_count_is_refused() {
    let rig = Rig::new();
    for i in 0..MAX_BUILDS {
        std::fs::create_dir(rig.cfg.lib_dir.join(format!("{i:08x}"))).unwrap();
    }
    assert!(rig.refused(COMMIT).contains("builds (at most"));
    assert_eq!(rig.lib_entries().len(), MAX_BUILDS, "nothing added");
}

#[test]
fn the_leftovers_of_a_killed_run_are_swept_and_a_live_ones_are_not() {
    let rig = Rig::new();
    let dead = rig.cfg.lib_dir.join(".install-aaaaaaaa-2147483000");
    let live = rig
        .cfg
        .lib_dir
        .join(format!(".install-bbbbbbbb-{}", std::process::id()));
    for d in [&dead, &live] {
        std::fs::create_dir(d).unwrap();
        std::fs::write(d.join("theseusd"), "x").unwrap();
    }
    // The live one is this very process's name: the run's own temporary directory has the same pid but another commit.
    rig.install();
    assert!(!dead.exists(), "a dead run's directory is removed");
    assert!(live.exists(), "this process's own is never touched");
}

#[test]
fn nothing_but_the_one_directory_is_ever_written() {
    // A file and a directory beside the target are left exactly as they were.
    let rig = Rig::new();
    std::fs::write(
        rig.cfg.lib_dir.join("theseusd"),
        "the separate layout's binary",
    )
    .unwrap();
    std::fs::create_dir(rig.cfg.lib_dir.join("abcdef01")).unwrap();
    rig.install();
    assert_eq!(rig.lib_entries(), ["01234567", "abcdef01", "theseusd"]);
    assert_eq!(
        std::fs::read(rig.cfg.lib_dir.join("theseusd")).unwrap(),
        b"the separate layout's binary"
    );
}

#[test]
fn the_real_configuration_is_the_one_with_the_real_paths_and_the_checks_on() {
    let c = Config::real();
    assert_eq!(
        c.stage,
        PathBuf::from("/var/lib/theseus-root-install/stage")
    );
    assert_eq!(c.lib_dir, PathBuf::from("/usr/local/lib/theseus"));
    assert_eq!(c.anchor, PathBuf::from("/"));
    assert_eq!((c.trusted_uid, c.real), (0, true));
}

#[test]
fn only_main_reads_the_process_and_only_to_clear_it() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for entry in std::fs::read_dir(&src).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(&path).unwrap();
        // This file names the calls it forbids.
        if name == "tests.rs" {
            continue;
        }
        for read in [
            "env::var(",
            "env::var_os(",
            "getenv",
            "env::current_dir",
            "env::args(",
        ] {
            assert!(!text.contains(read), "{name} reads the process with {read}");
        }
        if name == "main.rs" {
            // The only configuration `main` names, and the only things it does with its environment.
            assert!(text.contains("Config::real()"));
            assert!(
                !text.contains("Config {"),
                "main builds no configuration of its own"
            );
            assert!(!text.contains("trusted_uid") && !text.contains("anchor"));
        } else {
            assert!(
                !text.contains("env::set_var") && !text.contains("env::remove_var"),
                "{name}"
            );
        }
    }
}

#[test]
fn a_refusal_is_one_plain_line() {
    assert_eq!(Refusal("why".into()).to_string(), "why");
}
