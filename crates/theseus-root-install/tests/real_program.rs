//! The real program, run as itself: nothing the caller sets changes where it reads or writes, and anything but one
//! commit is refused. Run without root, it refuses at its first step, which is the point: the inputs below are the
//! ones an owner's program could try. (`LD_PRELOAD` and the other loader variables are read by the dynamic loader before
//! any of this program runs, so they are not tested here: sudo's `env_reset` removes them before the helper starts, and
//! the rule says `Defaults!<helper> env_reset` so a site that turned it off globally still gets it for this program.)

use std::process::Command;

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

fn run(args: &[&str], envs: &[(&str, &str)]) -> (i32, String) {
    let scratch = tempfile::tempdir().unwrap();
    let mut c = Command::new(env!("CARGO_BIN_EXE_theseus-root-install"));
    c.args(args).current_dir(scratch.path());
    for (k, v) in envs {
        c.env(k, v);
    }
    let out = c.output().unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn anything_but_one_commit_is_refused() {
    for args in [
        vec![],
        vec![COMMIT, COMMIT],
        vec!["--stage=/tmp/x", COMMIT],
        vec!["--help"],
        vec!["../../tmp/x"],
        vec!["0123456789ABCDEF0123456789abcdef01234567"],
    ] {
        let (code, said) = run(&args, &[]);
        assert_eq!(code, 1, "{args:?}: {said}");
        assert!(said.contains("refused"), "{args:?}: {said}");
    }
}

#[test]
fn the_environment_names_no_path_and_changes_nothing() {
    let scratch = tempfile::tempdir().unwrap();
    let p = scratch.path().to_string_lossy().into_owned();
    let polluted = [
        ("THESEUS_ROOT_INSTALL_STAGE", p.as_str()),
        ("THESEUS_STAGE", p.as_str()),
        ("TMPDIR", p.as_str()),
        ("HOME", p.as_str()),
        ("PATH", p.as_str()),
    ];
    let (plain_code, plain) = run(&[COMMIT], &[]);
    let (code, said) = run(&[COMMIT], &polluted);
    assert_eq!(
        (code, &said),
        (plain_code, &plain),
        "the same refusal with or without"
    );
    assert_eq!(code, 1, "{said}");
    assert!(
        std::fs::read_dir(scratch.path()).unwrap().next().is_none(),
        "nothing was written where the environment pointed"
    );
}
