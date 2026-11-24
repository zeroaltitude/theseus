//! The pipeline's fetch against a registry the test owns: what it downloads,
//! what it refuses, and what the gate is left with. The real registries are
//! never contacted.

use std::path::Path;

use base64::Engine as _;
use sha2::{Digest, Sha256, Sha512};

use super::fetch::{Fail, Fetcher, Stats};
use super::lock::Pkg;
use super::stand_in::{Registry, Reply};
use super::tests::{cargo_lock, npm_lock};
use super::{provide_with, Packages, Provided, CARGO_LOCK, NPM_LOCK};
use crate::rsi::tests_tree::{fixture, git, Fixture};
use crate::rsi::tree;

const CRATES_IO: &str = "registry+https://github.com/rust-lang/crates.io-index";
const NOW: u64 = 1_792_000_000_000;

/// A `.crate` of `tide 1.0.0`, and its sha256 as a lock file records it.
fn tide() -> (Vec<u8>, String) {
    let bytes = theseus_files::archive::sample_tar_gz(&[
        (
            "tide-1.0.0/Cargo.toml",
            b"[package]\nname = \"tide\"\nversion = \"1.0.0\"\n",
        ),
        ("tide-1.0.0/src/lib.rs", b"pub fn high() {}\n"),
    ]);
    let sum = hex::encode(Sha256::digest(&bytes));
    (bytes, sum)
}

fn crate_pkg(sum: &str) -> Pkg {
    Pkg {
        name: "tide".into(),
        version: "1.0.0".into(),
        from: "crates.io".into(),
        check: sum.into(),
        url: String::new(),
    }
}

fn integrity(bytes: &[u8]) -> String {
    format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(Sha512::digest(bytes))
    )
}

#[tokio::test]
async fn a_crate_is_fetched_checked_unpacked_and_not_fetched_twice() {
    let reg = Registry::start().await;
    let (bytes, sum) = tide();
    reg.serve("/crates/tide/tide-1.0.0.crate", Reply::Body(bytes));
    let dir = crate::scratch::tempdir();
    let vendor = dir.path().join("vendor");
    let f = Fetcher::new(reg.endpoints()).unwrap();
    let stats = f.cargo(&vendor, &[crate_pkg(&sum)]).await.unwrap();
    assert_eq!(
        stats,
        Stats {
            fetched: 1,
            kept: 0
        }
    );
    assert_eq!(
        std::fs::read_to_string(vendor.join("tide-1.0.0/src/lib.rs")).unwrap(),
        "pub fn high() {}\n"
    );
    let marker: serde_json::Value = serde_json::from_slice(
        &std::fs::read(vendor.join("tide-1.0.0/.cargo-checksum.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        marker["package"],
        sum.as_str(),
        "cargo's directory source reads this"
    );
    assert!(marker["files"].as_object().unwrap().is_empty());
    assert!(
        !vendor.join(".part-tide-1.0.0").exists(),
        "no half-made leftovers"
    );
    // A second run finds it there with the checksum and asks for nothing.
    let again = f.cargo(&vendor, &[crate_pkg(&sum)]).await.unwrap();
    assert_eq!(
        again,
        Stats {
            fetched: 0,
            kept: 1
        }
    );
    assert_eq!(reg.seen(), ["/crates/tide/tide-1.0.0.crate"]);
}

#[tokio::test]
async fn bytes_other_than_the_locks_checksum_are_refused_and_never_kept() {
    let reg = Registry::start().await;
    let (_, sum) = tide();
    let other = theseus_files::archive::sample_tar_gz(&[("tide-1.0.0/src/lib.rs", b"evil\n")]);
    reg.serve("/crates/tide/tide-1.0.0.crate", Reply::Body(other));
    let dir = crate::scratch::tempdir();
    let vendor = dir.path().join("vendor");
    let got = Fetcher::new(reg.endpoints())
        .unwrap()
        .cargo(&vendor, &[crate_pkg(&sum)])
        .await;
    let Err(Fail::Refused(why)) = got else {
        panic!("{got:?}")
    };
    assert!(
        why.contains("not the package the lock file asked for"),
        "{why}"
    );
    assert!(
        why.contains(&sum),
        "it names the checksum the lock holds: {why}"
    );
    assert!(!vendor.join("tide-1.0.0").exists());
}

#[tokio::test]
async fn a_crate_the_registry_does_not_have_is_refused_with_its_name() {
    let reg = Registry::start().await;
    let (_, sum) = tide();
    let dir = crate::scratch::tempdir();
    let got = Fetcher::new(reg.endpoints())
        .unwrap()
        .cargo(&dir.path().join("v"), &[crate_pkg(&sum)])
        .await;
    let Err(Fail::Refused(why)) = got else {
        panic!("{got:?}")
    };
    assert!(
        why.contains("the registry has no") && why.contains("tide-1.0.0.crate"),
        "{why}"
    );
    assert_eq!(reg.seen().len(), 1, "a missing package is not retried");
}

/// **A redirect is never followed**, so no answer can send the fetch to a host that is
/// not on the list: the second server is never asked.
#[tokio::test]
async fn a_redirect_is_never_followed() {
    let (reg, elsewhere) = (Registry::start().await, Registry::start().await);
    let (bytes, sum) = tide();
    elsewhere.serve("/anything", Reply::Body(bytes));
    reg.serve(
        "/crates/tide/tide-1.0.0.crate",
        Reply::Redirect(format!("{}/anything", elsewhere.base)),
    );
    let dir = crate::scratch::tempdir();
    let got = Fetcher::new(reg.endpoints())
        .unwrap()
        .cargo(&dir.path().join("v"), &[crate_pkg(&sum)])
        .await;
    let Err(Fail::Refused(why)) = got else {
        panic!("{got:?}")
    };
    assert!(why.contains("redirect"), "{why}");
    assert!(elsewhere.seen().is_empty(), "{:?}", elsewhere.seen());
}

#[tokio::test]
async fn a_registry_that_errs_is_the_machines_fault_after_three_tries() {
    let reg = Registry::start().await;
    let (_, sum) = tide();
    reg.serve("/crates/tide/tide-1.0.0.crate", Reply::Status(503));
    let dir = crate::scratch::tempdir();
    let got = Fetcher::new(reg.endpoints())
        .unwrap()
        .cargo(&dir.path().join("v"), &[crate_pkg(&sum)])
        .await;
    let Err(Fail::Infra(why)) = got else {
        panic!("{got:?}")
    };
    assert!(why.contains("did not answer after 3 tries"), "{why}");
    assert_eq!(reg.seen().len(), 3);
}

#[tokio::test]
async fn a_stale_crate_is_removed_and_one_with_another_checksum_is_fetched_again() {
    let reg = Registry::start().await;
    let (bytes, sum) = tide();
    reg.serve("/crates/tide/tide-1.0.0.crate", Reply::Body(bytes));
    let dir = crate::scratch::tempdir();
    let vendor = dir.path().join("vendor");
    std::fs::create_dir_all(vendor.join("old-0.1.0")).unwrap();
    std::fs::create_dir_all(vendor.join("tide-1.0.0")).unwrap();
    std::fs::write(
        vendor.join("tide-1.0.0/.cargo-checksum.json"),
        r#"{"files":{},"package":"0000"}"#,
    )
    .unwrap();
    let stats = Fetcher::new(reg.endpoints())
        .unwrap()
        .cargo(&vendor, &[crate_pkg(&sum)])
        .await
        .unwrap();
    assert_eq!(stats.fetched, 1);
    assert!(!vendor.join("old-0.1.0").exists());
    assert!(vendor.join("tide-1.0.0/src/lib.rs").exists());
}

#[tokio::test]
async fn an_npm_tarball_is_kept_by_its_integrity_where_npm_looks_and_a_wrong_one_is_refused() {
    let reg = Registry::start().await;
    let bytes =
        theseus_files::archive::sample_tar_gz(&[("package/index.js", b"module.exports = 1\n")]);
    let check = integrity(&bytes);
    let pkg = |url: &str| Pkg {
        name: "left-pad".into(),
        version: "1.3.0".into(),
        from: "the npm registry".into(),
        check: check.clone(),
        url: format!("{}{url}", reg.base),
    };
    reg.serve("/left-pad/-/left-pad-1.3.0.tgz", Reply::Body(bytes.clone()));
    let dir = crate::scratch::tempdir();
    let cache = dir.path().join("npm");
    let f = Fetcher::new(reg.endpoints()).unwrap();
    let stats = f
        .npm(&cache, &[pkg("/left-pad/-/left-pad-1.3.0.tgz")])
        .await
        .unwrap();
    assert_eq!(
        stats,
        Stats {
            fetched: 1,
            kept: 0
        }
    );
    let hexed = hex::encode(Sha512::digest(&bytes));
    let at = cache
        .join("_cacache/content-v2/sha512")
        .join(&hexed[..2])
        .join(&hexed[2..4])
        .join(&hexed[4..]);
    assert_eq!(
        std::fs::read(&at).unwrap(),
        bytes,
        "npm's cache holds it by digest"
    );
    let again = f
        .npm(&cache, &[pkg("/left-pad/-/left-pad-1.3.0.tgz")])
        .await
        .unwrap();
    assert_eq!(
        again,
        Stats {
            fetched: 0,
            kept: 1
        }
    );
    assert_eq!(reg.seen().len(), 1);

    // The same integrity served other bytes: refused, and nothing is written.
    reg.serve("/other.tgz", Reply::Body(b"not the tarball".to_vec()));
    std::fs::remove_dir_all(&cache).unwrap();
    let got = f.npm(&cache, &[pkg("/other.tgz")]).await;
    let Err(Fail::Refused(why)) = got else {
        panic!("{got:?}")
    };
    assert!(
        why.contains("not the package the lock file asked for"),
        "{why}"
    );
    assert!(!at.exists());
}

// ---- provide: the cache a tree's gate reads ----

/// Main gains `file` (so a tree's copy of it can equal main's).
fn main_gains(f: &Fixture, file: &str, text: &str) {
    let root = f.place.repo.parent().unwrap().to_path_buf();
    git(
        &root,
        &["clone", "-q", f.upstream.to_str().unwrap(), "work-main"],
    );
    let work = root.join("work-main");
    if let Some(parent) = work.join(file).parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(work.join(file), text).unwrap();
    git(&work, &["add", "-A"]);
    git(&work, &["commit", "-q", "-m", "main gains a file"]);
    git(&work, &["push", "-q", "origin", "main"]);
    git(&f.place.repo, &["fetch", "-q", "origin"]);
}

fn write(t: &tree::Tree, rel: &str, text: &str) {
    let at = t.dir.join(rel);
    std::fs::create_dir_all(at.parent().unwrap()).unwrap();
    std::fs::write(at, text).unwrap();
}

fn tide_lock(sum: &str) -> String {
    cargo_lock(&[
        ("theseus-core", "0.1.0", None, None),
        ("tide", "1.0.0", Some(CRATES_IO), Some(sum)),
    ])
}

fn home() -> tempfile::TempDir {
    let h = crate::scratch::tempdir();
    std::fs::create_dir_all(h.path().join(".cargo/advisory-dbs")).unwrap();
    h
}

#[tokio::test]
async fn a_tree_whose_lock_files_are_mains_needs_no_packages() {
    let f = fixture();
    let (_, sum) = tide();
    main_gains(&f, CARGO_LOCK, &tide_lock(&sum));
    let t = tree::open(&f.place, "same-locks", NOW).unwrap();
    assert_eq!(
        std::fs::read_to_string(t.dir.join(CARGO_LOCK)).unwrap(),
        tide_lock(&sum)
    );
    let reg = Registry::start().await;
    let got = provide_with(&f.place, &t, &reg.endpoints(), home().path(), true).await;
    assert!(matches!(got, Provided::Nothing), "{got:?}");
    assert!(reg.seen().is_empty());
    assert!(!Packages::dir_for(&f.place, &t).unwrap().exists());
}

#[tokio::test]
async fn a_changed_cargo_lock_is_fetched_into_the_cache_the_gate_reads() {
    let f = fixture();
    let (bytes, sum) = tide();
    let reg = Registry::start().await;
    reg.serve("/crates/tide/tide-1.0.0.crate", Reply::Body(bytes));
    let t = tree::open(&f.place, "adds-tide", NOW).unwrap();
    write(&t, CARGO_LOCK, &tide_lock(&sum));
    let h = home();
    let Provided::Ready(pk) = provide_with(&f.place, &t, &reg.endpoints(), h.path(), true).await
    else {
        panic!("not ready")
    };
    assert_eq!(Some(pk.dir.clone()), Packages::dir_for(&f.place, &t));
    assert!(pk.cargo && !pk.npm);
    let config = std::fs::read_to_string(pk.dir.join("cargo-home/config.toml")).unwrap();
    assert!(
        config.contains("replace-with")
            && config.contains(&pk.dir.join("cargo-home/vendor").display().to_string()),
        "{config}"
    );
    assert!(pk
        .dir
        .join("cargo-home/vendor/tide-1.0.0/src/lib.rs")
        .exists());
    assert_eq!(
        std::fs::read_link(pk.dir.join("cargo-home/advisory-dbs")).unwrap(),
        h.path().join(".cargo/advisory-dbs")
    );
    assert_eq!(
        pk.env(),
        [(
            "CARGO_HOME".to_string(),
            pk.dir.join("cargo-home").display().to_string()
        )]
    );
    assert_eq!(Packages::of(&f.place, &t), Some(pk.clone()));
    // The cache is for these lock files: another lock file is not one it holds.
    write(&t, CARGO_LOCK, &format!("{}\n# moved\n", tide_lock(&sum)));
    assert_eq!(Packages::of(&f.place, &t), None);
    let Provided::Ready(_) = provide_with(&f.place, &t, &reg.endpoints(), h.path(), true).await
    else {
        panic!("not ready again")
    };
    assert_eq!(
        reg.seen().len(),
        2,
        "other lock files are another cache, made with its own download"
    );
    // The cache is the lock files' own, not the tree's: it stays when the tree closes, so the
    // planted revert's tree, the merge's and the install's, which have the same lock files, read it.
    tree::close(&f.place, &t).unwrap();
    assert!(pk.dir.exists());
}

/// **KEY (review round 1): every build of the same lock files reads one verified cache**,
/// not just the branch's own gate: a second tree with the same lock files (the planted
/// revert's, the install's) is bound to it without a download, and a tree cut from a main
/// that has since taken those lock files finds it too.
#[tokio::test]
async fn every_build_of_the_same_lock_files_reads_one_cache() {
    let f = fixture();
    let (bytes, sum) = tide();
    let reg = Registry::start().await;
    reg.serve("/crates/tide/tide-1.0.0.crate", Reply::Body(bytes));
    let t = tree::open(&f.place, "adds-tide", NOW).unwrap();
    write(&t, CARGO_LOCK, &tide_lock(&sum));
    let h = home();
    let Provided::Ready(pk) = provide_with(&f.place, &t, &reg.endpoints(), h.path(), true).await
    else {
        panic!("not ready")
    };
    // Another tree (the planted revert's, the install's) with the same lock file: bound, not fetched.
    let other = tree::open(&f.place, "planted-adds-tide", NOW).unwrap();
    assert_eq!(
        Packages::of(&f.place, &other),
        None,
        "its lock is main's: no cache yet"
    );
    write(&other, CARGO_LOCK, &tide_lock(&sum));
    assert_eq!(Packages::of(&f.place, &other), Some(pk.clone()));
    let Provided::Ready(again) =
        provide_with(&f.place, &other, &reg.endpoints(), h.path(), true).await
    else {
        panic!("not ready for the second tree")
    };
    assert_eq!(again, pk);
    assert_eq!(reg.seen().len(), 1, "the crate was fetched once for both");
    // The branch joins: main now has that lock file, and a tree cut from it reads the cache.
    tree::close(&f.place, &t).unwrap();
    tree::close(&f.place, &other).unwrap();
    main_gains(&f, CARGO_LOCK, &tide_lock(&sum));
    let later = tree::open(&f.place, "a-later-item", NOW).unwrap();
    assert!(
        provide_with(&f.place, &later, &reg.endpoints(), h.path(), true)
            .await
            .is_nothing(),
        "its lock is main's, so nothing is fetched"
    );
    assert_eq!(
        Packages::of(&f.place, &later),
        Some(pk),
        "and it still reads the cache"
    );
    assert_eq!(reg.seen().len(), 1);
}

/// Old caches are removed, and main's own and the newest stay.
#[tokio::test]
async fn old_caches_are_pruned_and_mains_stays() {
    let f = fixture();
    let reg = Registry::start().await;
    let h = home();
    let mut dirs = Vec::new();
    for n in 0..6 {
        let (bytes, sum) = tide();
        reg.serve("/crates/tide/tide-1.0.0.crate", Reply::Body(bytes));
        let t = tree::open(&f.place, &format!("lock-{n}"), NOW).unwrap();
        // A different lock file each time: a dependency list that only differs in a comment
        // is still other bytes, so another cache.
        write(&t, CARGO_LOCK, &format!("{}# {n}\n", tide_lock(&sum)));
        let Provided::Ready(pk) =
            provide_with(&f.place, &t, &reg.endpoints(), h.path(), true).await
        else {
            panic!("not ready")
        };
        dirs.push(pk.dir);
        tree::close(&f.place, &t).unwrap();
        // Distinct modified times, so "newest" is what it says.
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let kept: Vec<bool> = dirs.iter().map(|d| d.exists()).collect();
    assert_eq!(
        kept,
        [false, false, true, true, true, true],
        "the newest and the three before it stay"
    );
}

#[tokio::test]
async fn a_changed_npm_lock_is_fetched_into_npms_cache() {
    let f = fixture();
    let reg = Registry::start().await;
    let bytes = theseus_files::archive::sample_tar_gz(&[("package/index.js", b"1\n")]);
    reg.serve("/left-pad/-/left-pad-1.3.0.tgz", Reply::Body(bytes.clone()));
    let t = tree::open(&f.place, "adds-left-pad", NOW).unwrap();
    write(
        &t,
        NPM_LOCK,
        &npm_lock(&[(
            "node_modules/left-pad",
            "1.3.0",
            &format!("{}/left-pad/-/left-pad-1.3.0.tgz", reg.base),
            &integrity(&bytes),
        )]),
    );
    let Provided::Ready(pk) =
        provide_with(&f.place, &t, &reg.endpoints(), home().path(), true).await
    else {
        panic!("not ready")
    };
    assert!(pk.npm && !pk.cargo);
    assert_eq!(
        pk.env(),
        [(
            "npm_config_cache".to_string(),
            pk.dir.join("npm").display().to_string()
        )]
    );
    assert!(pk.dir.join("npm/_cacache/content-v2/sha512").is_dir());
}

/// A scenario: what it is, a lock file, a tracked file beside it, and what the refusal says.
type Case<'a> = (&'a str, &'a str, Option<(&'a str, &'a str)>, &'a str);

/// **A dependency from anywhere but the registries is refused at the build, with the
/// reason, and nothing is fetched**: the stand-in registry is never asked, and no cache
/// is made.
#[tokio::test]
async fn a_dependency_from_anywhere_else_is_refused_before_anything_is_fetched() {
    let (_, sum) = tide();
    let git_lock = cargo_lock(&[(
        "x",
        "1.0.0",
        Some("git+https://example.invalid/x?rev=abc#abc"),
        None,
    )]);
    let elsewhere = npm_lock(&[(
        "node_modules/x",
        "1.0.0",
        "https://example.invalid/x.tgz",
        "sha512-AA",
    )]);
    let tide_locked = tide_lock(&sum);
    let cases: [Case; 5] = [
        (
            "a git source in Cargo.lock",
            &git_lock,
            None,
            "only crates.io is allowed",
        ),
        ("a lock that is not TOML", "[[package", None, "Cargo.lock"),
        (
            "a path leaving the tree",
            &tide_locked,
            Some((
                "crates/a/Cargo.toml",
                "[dependencies]\nx = { path = \"../../../x\" }\n",
            )),
            "leaves the tree",
        ),
        (
            "a replaced registry",
            &tide_locked,
            Some((
                ".cargo/config.toml",
                "[source.crates-io]\nreplace-with = \"mine\"\n",
            )),
            "changes where cargo finds packages",
        ),
        (
            "npm from another host",
            "",
            None,
            "it comes from `http://127.0.0.1",
        ),
    ];
    for (n, (what, lock, tracked, said)) in cases.into_iter().enumerate() {
        let f = fixture();
        let reg = Registry::start().await;
        let t = tree::open(&f.place, &format!("refused-{n}"), NOW).unwrap();
        if what.starts_with("npm") {
            write(
                &t,
                NPM_LOCK,
                &elsewhere.replace(
                    "https://example.invalid/x.tgz",
                    &format!("{}-other/x.tgz", reg.base),
                ),
            );
        } else {
            write(&t, CARGO_LOCK, lock);
        }
        if let Some((rel, text)) = tracked {
            write(&t, rel, text);
            tree::commit(&f.place, &t, "self: a dependency (refused)", "body").unwrap();
        }
        let got = provide_with(&f.place, &t, &reg.endpoints(), home().path(), true).await;
        let Provided::Refused { why, infra } = got else {
            panic!("{what}: {got:?}")
        };
        assert!(!infra, "{what}: a refused source is the change's");
        assert!(why.contains(said), "{what}: {why}");
        assert!(reg.seen().is_empty(), "{what}: {:?}", reg.seen());
        assert!(!Packages::dir_for(&f.place, &t).unwrap().exists(), "{what}");
    }
}

#[tokio::test]
async fn a_registry_that_does_not_answer_is_not_the_changes_fault_and_leaves_no_ready_cache() {
    let f = fixture();
    let (_, sum) = tide();
    let reg = Registry::start().await;
    reg.serve("/crates/tide/tide-1.0.0.crate", Reply::Status(503));
    let t = tree::open(&f.place, "registry-down", NOW).unwrap();
    write(&t, CARGO_LOCK, &tide_lock(&sum));
    let got = provide_with(&f.place, &t, &reg.endpoints(), home().path(), true).await;
    let Provided::Refused { why, infra } = got else {
        panic!("{got:?}")
    };
    assert!(infra, "{why}");
    assert!(why.contains("could not be fetched"), "{why}");
    assert_eq!(
        Packages::of(&f.place, &t),
        None,
        "an unfinished fetch is never ready"
    );
}

/// The gate itself: a branch that may not be fetched is red with the reason in the
/// tail its builder reads, and the gate never starts.
#[tokio::test]
async fn the_gate_does_not_start_for_a_dependency_that_may_not_be_fetched() {
    let f = fixture();
    let t = tree::open(&f.place, "adds-a-git-crate", NOW).unwrap();
    write(
        &t,
        CARGO_LOCK,
        &cargo_lock(&[(
            "x",
            "1.0.0",
            Some("git+https://example.invalid/x?rev=abc#abc"),
            None,
        )]),
    );
    tree::commit(&f.place, &t, "self: a git crate (adds-a-git-crate)", "body").unwrap();
    let run = tree::gate(&f.place, &t).await.unwrap().ran().unwrap();
    assert_eq!(run.verdict, tree::Verdict::Red);
    assert!(!run.unrunnable, "the change's fault");
    assert!(
        run.tail.contains("only crates.io is allowed"),
        "{}",
        run.tail
    );
    let log = std::fs::read_to_string(&run.log).unwrap();
    assert!(!log.contains("stand-in gate: root"), "the gate ran: {log}");
    assert!(Path::new(&run.log).exists());
}

/// Judging is not fetching: a place with no sandbox (a test's stand-in gate) reads no
/// cache, so nothing is downloaded, and the registry is never asked.
#[tokio::test]
async fn a_place_with_no_sandbox_judges_and_fetches_nothing() {
    let f = fixture();
    let (_, sum) = tide();
    let reg = Registry::start().await;
    let t = tree::open(&f.place, "no-sandbox", NOW).unwrap();
    write(&t, CARGO_LOCK, &tide_lock(&sum));
    let got = provide_with(&f.place, &t, &reg.endpoints(), home().path(), false).await;
    assert!(matches!(got, Provided::Nothing), "{got:?}");
    assert!(reg.seen().is_empty());
    assert!(!Packages::dir_for(&f.place, &t).unwrap().exists());
    assert!(
        super::provide(&f.place, &t).await.is_nothing(),
        "no sandbox, no fetch"
    );
}

#[tokio::test]
async fn a_ready_cache_is_not_rebuilt_under_its_consumer() {
    let f = fixture();
    let reg = Registry::start().await;
    let h = home();
    let (bytes, sum) = tide();
    reg.serve("/crates/tide/tide-1.0.0.crate", Reply::Body(bytes));
    let t = tree::open(&f.place, "ready-cache", NOW).unwrap();
    write(&t, CARGO_LOCK, &tide_lock(&sum));
    let Provided::Ready(pk) = provide_with(&f.place, &t, &reg.endpoints(), h.path(), true).await
    else {
        panic!("not ready")
    };
    let witness = pk.dir.join("cargo-home/vendor/consumer-witness");
    std::fs::create_dir(&witness).unwrap();
    let Provided::Ready(again) = provide_with(&f.place, &t, &reg.endpoints(), h.path(), true).await
    else {
        panic!("not ready again")
    };
    assert!(
        witness.exists(),
        "a complete cache must not be destructively fetched again"
    );
    assert_eq!(pk, again);
    assert_eq!(reg.seen().len(), 1);
}

#[test]
fn a_consumer_pins_its_cache_against_later_pruning() {
    let f = fixture();
    let t = tree::open(&f.place, "held-cache", NOW).unwrap();
    write(&t, CARGO_LOCK, "# pinned lock\n");
    let dir = Packages::dir_for(&f.place, &t).unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("ready.json"), r#"{"cargo":true,"npm":false}"#).unwrap();
    std::fs::File::open(&dir)
        .unwrap()
        .set_times(
            std::fs::FileTimes::new()
                .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1)),
        )
        .unwrap();
    let held = Packages::of(&f.place, &t).unwrap();
    for n in 2..8 {
        let p = f.place.packages().join(format!("{n:032x}"));
        std::fs::create_dir_all(&p).unwrap();
        std::fs::File::open(&p)
            .unwrap()
            .set_times(
                std::fs::FileTimes::new()
                    .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(n)),
            )
            .unwrap();
    }
    super::prune(&f.place, "current", None);
    assert!(
        dir.exists(),
        "an active consumer still reads this old cache"
    );
    drop(held);
    super::prune(&f.place, "current", None);
    assert!(!dir.exists(), "an unused old cache is still pruned");
}
