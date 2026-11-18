//! The start command in force (`start.rs`): how the files of a user manager's unit path compose, which line wins, and
//! what changing it touches. Real files in a directory of the test's own; no systemd.

use std::path::Path;

use super::start::{self, Dir, Place};
use super::unit::{exec_program, repoint};

const OLD: &str = "/usr/local/lib/theseus/aaaaaaaa/theseusd";
const NEW: &str = "/usr/local/lib/theseus/bbbbbbbb/theseusd";
const ARGS: &str = "--config /home/x/theseus.toml --op-token-file /home/x/tok";

fn put(root: &Path, rel: &str, text: &str) {
    let at = root.join(rel);
    std::fs::create_dir_all(at.parent().unwrap()).unwrap();
    std::fs::write(at, text).unwrap();
}

/// The unit path of a test: the user's own two directories, the administrator's one.
fn dirs(root: &Path) -> Vec<Dir> {
    [
        ("own/user.control", Place::Own),
        ("own/user", Place::Own),
        ("admin/user", Place::Admin),
        ("lib/user", Place::Admin),
    ]
    .map(|(p, place)| Dir {
        path: root.join(p),
        place,
    })
    .to_vec()
}

fn unit(exec: &str) -> String {
    format!("[Unit]\nDescription=x\n\n[Service]\nType=notify\nExecStart={exec}\nRestart=on-failure\n\n[Install]\nWantedBy=default.target\n")
}

fn winner_of(root: &Path) -> (String, String) {
    let pieces = start::pieces(&dirs(root), "theseusd.service").unwrap();
    let (at, program) = start::winner(&pieces).unwrap();
    (
        pieces[at]
            .path
            .strip_prefix(root)
            .unwrap()
            .display()
            .to_string(),
        program,
    )
}

/// A drop-in that clears and sets the command is the one that wins, whatever the unit says.
#[test]
fn a_drop_in_that_clears_and_sets_wins_over_the_unit() {
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        "own/user/theseusd.service",
        &unit("/usr/bin/old --a"),
    );
    put(
        d.path(),
        "own/user/theseusd.service.d/args.conf",
        &format!("[Service]\nExecStart=\nExecStart={OLD} {ARGS}\n"),
    );
    assert_eq!(
        winner_of(d.path()),
        ("own/user/theseusd.service.d/args.conf".into(), OLD.into())
    );
}

/// With no drop-in, the unit's own line; a drop-in that sets other keys only leaves it be.
#[test]
fn the_unit_wins_when_no_drop_in_sets_a_start_command() {
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        "own/user/theseusd.service",
        &unit(&format!("{OLD} {ARGS}")),
    );
    assert_eq!(winner_of(d.path()).0, "own/user/theseusd.service");
    put(
        d.path(),
        "own/user/theseusd.service.d/env.conf",
        "[Service]\nEnvironment=\"A=1\"\nRestart=always\n",
    );
    assert_eq!(
        winner_of(d.path()),
        ("own/user/theseusd.service".into(), OLD.into())
    );
}

/// Drop-ins are applied by file name, across directories, and the last that sets the command is the winner; a name in
/// a directory of higher priority hides the same name below.
#[test]
fn drop_ins_apply_in_name_order_across_directories_and_the_last_wins() {
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        "own/user/theseusd.service",
        &unit("/usr/bin/unit"),
    );
    put(
        d.path(),
        "own/user/theseusd.service.d/20-b.conf",
        &format!("[Service]\nExecStart=\nExecStart={OLD} --b\n"),
    );
    put(
        d.path(),
        "admin/user/theseusd.service.d/10-a.conf",
        "[Service]\nExecStart=\nExecStart=/usr/bin/a\n",
    );
    // The last by name is `20-b`, though `10-a` is in a directory of lower priority.
    assert_eq!(
        winner_of(d.path()).0,
        "own/user/theseusd.service.d/20-b.conf"
    );
    // A same-named file in a higher directory hides the lower one: `20-b` in the administrator's is not read.
    put(
        d.path(),
        "admin/user/theseusd.service.d/20-b.conf",
        "[Service]\nExecStart=\nExecStart=/usr/bin/hidden\n",
    );
    assert_eq!(
        winner_of(d.path()),
        ("own/user/theseusd.service.d/20-b.conf".into(), OLD.into())
    );
    // And the administrator's, alone, wins where the user has none: the place says whose it is.
    std::fs::remove_file(d.path().join("own/user/theseusd.service.d/20-b.conf")).unwrap();
    let pieces = start::pieces(&dirs(d.path()), "theseusd.service").unwrap();
    let (at, program) = start::winner(&pieces).unwrap();
    assert_eq!(
        (pieces[at].place, program.as_str()),
        (Place::Admin, "/usr/bin/hidden")
    );
    assert!(start::refusal(&pieces[at], |_| true)
        .unwrap()
        .contains("administrator"));
}

/// The type-level `service.d` is read too, and a drop-in masked by a link to /dev/null is not.
#[test]
fn a_type_level_drop_in_is_read_and_a_masked_one_is_not() {
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        "own/user/theseusd.service",
        &unit("/usr/bin/unit"),
    );
    put(
        d.path(),
        "own/user/service.d/50-all.conf",
        &format!("[Service]\nExecStart=\nExecStart={OLD}\n"),
    );
    assert_eq!(winner_of(d.path()).0, "own/user/service.d/50-all.conf");
    let masked = d.path().join("own/user/theseusd.service.d");
    std::fs::create_dir_all(&masked).unwrap();
    std::os::unix::fs::symlink("/dev/null", masked.join("50-all.conf")).unwrap();
    // The unit's own directory hides the type-level name, and it is masked: the unit's line is back.
    assert_eq!(winner_of(d.path()).0, "own/user/theseusd.service");
}

/// More than one command left, none, or one that is not a plain path: refused in words, naming the files.
#[test]
fn a_start_command_that_cannot_be_told_is_refused_in_words() {
    let d = tempfile::tempdir().unwrap();
    let refused = |d: &Path| {
        let pieces = start::pieces(&dirs(d), "theseusd.service").unwrap();
        format!("{:#}", start::winner(&pieces).unwrap_err())
    };
    put(
        d.path(),
        "own/user/theseusd.service",
        "[Service]\nType=notify\n",
    );
    assert!(refused(d.path()).contains("no ExecStart"), "none");
    put(d.path(), "own/user/theseusd.service", &unit(OLD));
    put(
        d.path(),
        "own/user/theseusd.service.d/more.conf",
        "[Service]\nExecStart=/usr/bin/second\n",
    );
    let said = refused(d.path());
    assert!(
        said.contains("2 start commands") && said.contains("more.conf"),
        "{said}"
    );
    for odd in [
        "-/usr/bin/x",
        "@/usr/bin/x argv0",
        "\"/usr/bin/x\" a",
        "%h/bin/x",
        "bin/x",
    ] {
        put(
            d.path(),
            "own/user/theseusd.service.d/more.conf",
            &format!("[Service]\nExecStart=\nExecStart={odd}\n"),
        );
        assert!(refused(d.path()).contains("plain absolute path"), "{odd}");
    }
}

/// Only the `[Service]` section's `ExecStart=` counts; comments, spacing around `=`, and a continued command line
/// are read as systemd reads them.
#[test]
fn only_the_service_sections_lines_count_and_spacing_is_read() {
    let text = format!(
        "# ExecStart=/usr/bin/comment\n[Unit]\nExecStart=/usr/bin/elsewhere\n[Service]\n; ExecStart=/usr/bin/semi\nExecStart = {OLD} \\\n   {ARGS}\n"
    );
    assert_eq!(exec_program(&text).as_deref(), Some(OLD));
    let want = text.replace(OLD, NEW);
    assert_eq!(repoint(&text, OLD, NEW).unwrap(), want);
}

/// The one change is the program's path: every other byte of the file stays, the arguments and the clearing line
/// included, and a file whose line does not name `old` is not touched at all.
#[test]
fn only_the_programs_path_changes() {
    let text = format!(
        "# the drop-in, by hand\n[Service]\nEnvironment=\"X=1\"\nExecStart=\nExecStart={OLD} {ARGS}   \nRestart=always"
    );
    let got = repoint(&text, OLD, NEW).unwrap();
    assert_eq!(got, text.replace(OLD, NEW));
    let sans = |t: &str, p: &str| t.replacen(p, "<P>", 1);
    assert_eq!(
        sans(&got, NEW),
        sans(&text, OLD),
        "nothing but the path differs"
    );
    assert!(got.contains(&format!("{NEW} {ARGS}   \n")));
    // Not `old`: refused, and said why.
    let e = repoint(&text, "/usr/bin/other", NEW).unwrap_err();
    assert!(e.contains("changed since it was read"), "{e}");
    // A path that only begins with `old` is not `old`.
    let longer = format!("[Service]\nExecStart={OLD}x --a\n");
    assert!(repoint(&longer, OLD, NEW).is_err());
}

/// What systemd says it will run, read from `systemctl show -p ExecStart`: one path per command, an argument that
/// says `path=` is not one.
#[test]
fn what_systemd_says_it_will_run_is_read_from_the_braces() {
    let shown = format!(
        "ExecStart={{ path={NEW} ; argv[]={NEW} --config path=/x ; ignore_errors=no ; start_time=[n/a] ; stop_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }}"
    );
    assert_eq!(start::programs_of(&shown), [NEW]);
    let two =
        format!("{shown} {{ path=/usr/bin/second ; argv[]=/usr/bin/second ; ignore_errors=no }}");
    assert_eq!(start::programs_of(&two), [NEW, "/usr/bin/second"]);
    assert!(start::programs_of("ExecStart=\n").is_empty());
}

/// A winner Theseus may not write is refused with the reason: in the runtime directories, behind a link, or not the
/// user's to write.
#[test]
fn a_winner_in_a_place_that_is_not_the_users_is_refused() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "own/user/real.service", &unit(OLD));
    std::os::unix::fs::symlink(
        d.path().join("own/user/real.service"),
        d.path().join("own/user/theseusd.service"),
    )
    .unwrap();
    let pieces = start::pieces(&dirs(d.path()), "theseusd.service").unwrap();
    let (at, _) = start::winner(&pieces).unwrap();
    assert!(start::refusal(&pieces[at], |_| true)
        .unwrap()
        .contains("link"));
    let mut p = pieces[at].clone();
    p.path = d.path().join("own/user/real.service");
    assert!(
        start::refusal(&p, |_| true).is_none(),
        "the user's own file, writable"
    );
    assert!(start::refusal(&p, |_| false)
        .unwrap()
        .contains("cannot write"));
    p.place = Place::Volatile;
    assert!(start::refusal(&p, |_| true)
        .unwrap()
        .contains("restart of the machine"));
}
