//! The build stamp `theseus-root-install` checks (issue 22): the built `theseusd` carries `theseus-build-stamp:<commit>:`
//! exactly once, read back out of its own bytes with the helper's scanner, so what the daemon writes and what the
//! helper reads cannot drift apart.

#[test]
fn the_built_program_carries_one_stamp_naming_its_commit() {
    let commit = env!("THESEUS_COMMIT");
    let bytes = std::fs::read(env!("CARGO_BIN_EXE_theseusd")).unwrap();
    let found = theseus_root_install::stamp::commits_in(&bytes);
    assert_eq!(
        found.len(),
        1,
        "exactly one stamp in the program: {found:?}"
    );
    if commit.is_empty() {
        // A build outside a checkout names no commit: the helper refuses such a program, which is right.
        assert!(found[0].is_err(), "{found:?}");
    } else {
        assert_eq!(found[0].as_deref(), Ok(commit));
        assert!(theseus_root_install::stamp::names(&bytes, commit).is_ok());
    }
}
