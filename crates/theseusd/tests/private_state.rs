//! Review 2's H3 (theseus-wz2): Theseus's state is its operator's alone. The
//! daemon runs under umask 077, so what it creates has no group or other
//! bits; the state dir, the store, and the spool are made 0700, and when an
//! older build left them open (the owner's: 0775, 0755, 0755) they are tightened
//! at start.

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use common::Served;

fn mode(p: &Path) -> u32 {
    std::fs::metadata(p).unwrap().permissions().mode() & 0o7777
}

/// Every file and directory under `dir` whose mode lets anyone but the owner
/// in, with its mode.
fn open_to_others(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let m = e.metadata().unwrap();
        if m.permissions().mode() & 0o077 != 0 {
            out.push(format!(
                "{} {:o}",
                e.path().display(),
                m.permissions().mode() & 0o7777
            ));
        }
        if m.is_dir() {
            out.extend(open_to_others(&e.path()));
        }
    }
    out
}

#[test]
fn a_new_state_dir_and_everything_in_it_are_the_operators_alone() {
    let s = Served::start(|_| {}, |_| {});
    let state = s.path("state");
    for d in ["", "store", "spool"] {
        assert_eq!(mode(&state.join(d)), 0o700, "{d:?}");
    }
    assert_eq!(open_to_others(&state), Vec::<String>::new(), "{}", s.log());
    assert_eq!(mode(&s.path("sock")) & 0o777, 0o600);
}

#[test]
fn an_old_state_dir_left_open_is_tightened_at_start() {
    let s = Served::start(
        |dir| {
            for (d, m) in [
                ("state", 0o775),
                ("state/store", 0o755),
                ("state/spool", 0o755),
            ] {
                std::fs::create_dir_all(dir.join(d)).unwrap();
                std::fs::set_permissions(dir.join(d), std::fs::Permissions::from_mode(m)).unwrap();
            }
        },
        |_| {},
    );
    let state = s.path("state");
    for d in ["", "store", "spool"] {
        assert_eq!(mode(&state.join(d)), 0o700, "{d:?}:\n{}", s.log());
    }
    assert!(s.log().contains("tightened"), "{}", s.log());
}
