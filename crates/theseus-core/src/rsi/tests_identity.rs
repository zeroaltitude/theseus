//! Who a self commit is by (theseus-smi8m): the self repo's `user.name` and
//! `user.email`, so GitHub can show a self-join Verified under the owner's
//! no-reply address, and Theseus's own only when the repo names none. Each
//! commit path is proved over `tests_tree`'s throwaway repository, with the
//! environment's own identity left out (an identity in the environment beats
//! any `-c`, and the daemon has none).

use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

use super::join::git as jgit;
use super::tests_tree::{fixture, hermetic};
use super::tree::{self, Place};

const NOW: u64 = 1_792_000_000_000;
const OWNER: &str = "102749+zeroaltitude@users.noreply.github.com";
const FAKE_GPG: &str = "#!/bin/bash\n\
echo '[GNUPG:] KEY_CONSIDERED FIXTURE 0' >&2\n\
echo '[GNUPG:] SIG_CREATED D 1 8 00 1792000000 FIXTURE' >&2\n\
cat >/dev/null\n\
printf -- '-----BEGIN PGP SIGNATURE-----\\n\\nZml4dHVyZQ==\\n-----END PGP SIGNATURE-----\\n'\n";

/// The hermetic environment without its author and committer: git's own
/// lookup, as in a daemon.
pub(super) fn bare() -> Vec<(OsString, OsString)> {
    hermetic()
        .into_iter()
        .filter(|(k, _)| {
            let k = k.to_string_lossy();
            !(k.starts_with("GIT_AUTHOR_") || k.starts_with("GIT_COMMITTER_"))
        })
        .collect()
}

fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .envs(hermetic())
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// A commit's author and committer: `name <email>` each.
fn who(dir: &Path, rev: &str) -> (String, String) {
    (
        git(dir, &["log", "-1", "--format=%an <%ae>", rev]),
        git(dir, &["log", "-1", "--format=%cn <%ce>", rev]),
    )
}

fn place_without_identity() -> (Place, super::tests_tree::Fixture) {
    let f = fixture();
    let mut p = f.place.clone();
    p.env = bare();
    (p, f)
}

#[test]
fn the_identity_is_the_repos_and_falls_back_to_theseus() {
    let (p, _f) = place_without_identity();
    assert_eq!(
        p.identity(),
        ("Theseus".to_string(), "theseus@localhost".to_string())
    );
    git(&p.repo, &["config", "user.email", OWNER]);
    assert_eq!(
        p.identity(),
        ("Theseus".to_string(), OWNER.to_string()),
        "a name left unset falls back alone"
    );
    git(&p.repo, &["config", "user.name", "The Owner"]);
    assert_eq!(p.identity(), ("The Owner".to_string(), OWNER.to_string()));
    assert_eq!(
        p.ident_args(),
        [
            "-c".to_string(),
            "user.name=The Owner".into(),
            "-c".into(),
            format!("user.email={OWNER}")
        ]
    );
}

/// The self-pr's commit, the self-join's merge and its signing amend each
/// carry the repo's identity, and Theseus's own when the repo has none.
#[test]
fn a_self_commit_merge_and_signature_carry_the_repos_identity() {
    for configured in [true, false] {
        let (p, f) = place_without_identity();
        let (name, email) = if configured {
            git(&p.repo, &["config", "user.email", OWNER]);
            git(&p.repo, &["config", "user.name", "The Owner"]);
            ("The Owner", OWNER)
        } else {
            ("Theseus", "theseus@localhost")
        };
        let want = format!("{name} <{email}>");
        let gpg = f.place.state.parent().unwrap().join("fake-gpg.sh");
        std::fs::write(&gpg, FAKE_GPG).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&gpg, std::fs::Permissions::from_mode(0o755)).unwrap();
        git(&p.repo, &["config", "gpg.program", gpg.to_str().unwrap()]);

        // The self-pr's commit (`tree::commit`).
        let t = tree::open(&p, "mend sails", NOW).unwrap();
        std::fs::write(t.dir.join("sails.txt"), "mended\n").unwrap();
        let c = tree::commit(&p, &t, "mend the sails", "a test")
            .unwrap()
            .unwrap();
        assert_eq!(
            who(&p.repo, &c),
            (want.clone(), want.clone()),
            "{configured}"
        );

        // The self-join's merge, then its signature.
        let base = jgit::main_of(&p, "origin").unwrap();
        let j = jgit::detached(&p, "join", &t.branch, &base).unwrap();
        let merge = jgit::merge(&p, &j, &c, "Merge self/mend-sails").unwrap();
        assert_eq!(who(&p.repo, &merge).1, want, "{configured}");
        let signed = jgit::sign(&p, &j, "FIXTUREKEY").unwrap();
        let (author, committer) = who(&p.repo, &signed);
        assert_eq!(committer, want, "{configured}: the signed merge");
        assert_eq!(author, want, "{configured}: the signed merge's author");
        assert!(jgit::signed(&p, &signed).unwrap());
    }
}
