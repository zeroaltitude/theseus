//! Who a self commit is by (theseus-smi8m): the self repo's `user.name` and
//! `user.email`, so GitHub can show a self-join Verified under the owner's
//! no-reply address, and Theseus's own only when the repo names none. Each
//! commit path is proved over `tests_tree`'s throwaway repository, with the
//! environment's own identity left out (an identity in the environment beats
//! any `-c`, and the daemon has none).
//!
//! Verified is the identity **and** the signature, so the other half is here
//! too (S22-2): every commit of the loop's landing path is signed with the
//! one key `user.signingkey` names, the content commit as well as the merge,
//! and a repo with no usable key still commits — unsigned, never wedged.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
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

/// A stand-in gpg at `name` beside the fixture's state dir, named by the
/// repo's `gpg.program`, so a commit is signed without a real key.
fn gpg_program(p: &Place, name: &str, script: &str) -> PathBuf {
    let path = p.state.parent().unwrap().join(name);
    std::fs::write(&path, script).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    git(&p.repo, &["config", "gpg.program", path.to_str().unwrap()]);
    path
}

/// [`FAKE_GPG`] with each call's arguments appended to `log`, one a line, so
/// a test can read which key a signature was actually made with.
fn recording_gpg(log: &Path) -> String {
    FAKE_GPG.replacen(
        "#!/bin/bash\n",
        &format!(
            "#!/bin/bash\nprintf '%s\\n' \"$*\" >> '{}'\n",
            log.display()
        ),
        1,
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
        let (p, _f) = place_without_identity();
        let (name, email) = if configured {
            git(&p.repo, &["config", "user.email", OWNER]);
            git(&p.repo, &["config", "user.name", "The Owner"]);
            ("The Owner", OWNER)
        } else {
            ("Theseus", "theseus@localhost")
        };
        let want = format!("{name} <{email}>");
        gpg_program(&p, "fake-gpg.sh", FAKE_GPG);

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

/// The loop's CONTENT commit carries a signature, made with the same key as
/// its merge (S22-2, the self-loop verification line). Before this, only the
/// merge was signed: `tree::commit` committed with `commit.gpgsign=false` and
/// nothing signed it afterwards, so every self branch's own commit reached
/// GitHub unsigned. `jgit::signed` is the predicate the join itself uses.
#[test]
fn a_self_commit_is_signed_with_the_same_key_as_the_merge() {
    let (p, _f) = place_without_identity();
    git(&p.repo, &["config", "user.name", "The Owner"]);
    git(&p.repo, &["config", "user.email", OWNER]);
    let calls = p.state.parent().unwrap().join("gpg-calls.txt");
    gpg_program(&p, "recording-gpg.sh", &recording_gpg(&calls));
    git(&p.repo, &["config", "user.signingkey", "FIXTUREKEY"]);
    assert_eq!(p.signing_key().as_deref(), Some("FIXTUREKEY"));

    let t = tree::open(&p, "replace the planks", NOW).unwrap();
    std::fs::write(t.dir.join("planks.txt"), "new planks\n").unwrap();
    let c = tree::commit(&p, &t, "replace the planks", "a test")
        .unwrap()
        .unwrap();
    assert!(
        jgit::signed(&p, &c).unwrap(),
        "the content commit {c} carries no signature:\n{}",
        git(&p.repo, &["cat-file", "commit", &c])
    );
    // The identity still the repo's, so the signature matches the key's user ID.
    let want = format!("The Owner <{OWNER}>");
    assert_eq!(who(&p.repo, &c), (want.clone(), want));
    let made = std::fs::read_to_string(&calls).unwrap();
    assert_eq!(made.lines().count(), 1, "one signature was made:\n{made}");
    assert!(
        made.contains("FIXTUREKEY"),
        "the content commit's signature was not made with the repo's key:\n{made}"
    );

    // The merge's signature is made with that same key, by the same reading.
    let base = jgit::main_of(&p, "origin").unwrap();
    let j = jgit::detached(&p, "join", &t.branch, &base).unwrap();
    let merge = jgit::merge(&p, &j, &c, "Merge self/replace-the-planks").unwrap();
    assert!(
        !jgit::signed(&p, &merge).unwrap(),
        "the merge waits for the gate"
    );
    let signed = jgit::sign(&p, &j, &p.signing_key().unwrap()).unwrap();
    assert!(jgit::signed(&p, &signed).unwrap());
    let made = std::fs::read_to_string(&calls).unwrap();
    let keyed: Vec<&str> = made.lines().filter(|l| l.contains("FIXTUREKEY")).collect();
    assert_eq!(
        keyed.len(),
        2,
        "the content commit and the merge are both signed with the one key:\n{made}"
    );
}

/// It fails safe, not closed (S22-2): a repo that names no key, and a repo
/// whose key its gpg cannot use, each still commit — unsigned, as before.
/// A loop that could not commit on a machine whose key is not unlocked would
/// be wedged with no card and no stop; an unsigned branch is held at the
/// join's sign phase instead, and `push_main` refuses it besides.
#[test]
fn a_self_commit_with_no_usable_key_is_unsigned_and_not_refused() {
    // No key at all: today's behaviour, kept.
    let (p, _f) = place_without_identity();
    gpg_program(&p, "fake-gpg.sh", FAKE_GPG);
    assert_eq!(p.signing_key(), None);
    let t = tree::open(&p, "caulk the hull", NOW).unwrap();
    std::fs::write(t.dir.join("hull.txt"), "caulked\n").unwrap();
    let c = tree::commit(&p, &t, "caulk the hull", "a test")
        .unwrap()
        .expect("a repo with no signing key still commits");
    assert!(!jgit::signed(&p, &c).unwrap(), "{c} with no key");

    // A key its gpg refuses: the commit is still made, and still unsigned.
    let (p, _f) = place_without_identity();
    gpg_program(
        &p,
        "broken-gpg.sh",
        "#!/bin/bash\necho 'gpg: no secret key' >&2\nexit 2\n",
    );
    git(&p.repo, &["config", "user.signingkey", "LOCKEDKEY"]);
    assert_eq!(p.signing_key().as_deref(), Some("LOCKEDKEY"));
    let t = tree::open(&p, "step the mast", NOW).unwrap();
    std::fs::write(t.dir.join("mast.txt"), "stepped\n").unwrap();
    let c = tree::commit(&p, &t, "step the mast", "a test")
        .unwrap()
        .expect("a repo whose key gpg refuses still commits");
    assert!(!jgit::signed(&p, &c).unwrap(), "{c} with a refused key");
    // And the tree is committed, not left dirty with the change still staged.
    assert_eq!(git(&t.dir, &["status", "--porcelain"]), "");
}
