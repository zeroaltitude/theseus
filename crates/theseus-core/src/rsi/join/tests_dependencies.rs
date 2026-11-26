//! A change of dependencies stops at the owner's card (issue 20, "The builder
//! cannot try a library the machine does not already hold"; the owner's decision
//! of 2026-11-15 17:46:56): the card names each package, its old and new version
//! and where it comes from; a lock file change that changes no package does not
//! stop a branch.

use crate::rsi::join::tests_publish::PassYes as _;
use theseus_protocol::rsi::SelfMode;

use super::tests::{git, rig, Rig, PROVED, SONNET};
use crate::rsi::deps::tests::{cargo_lock, npm_lock};

const CRATES_IO: &str = "registry+https://github.com/rust-lang/crates.io-index";
const SUM_A: &str = "92ecc6618181def0457392ccd0ee51198e065e016d1d527a7ac1b6dc7c1f09d2";
const SUM_B: &str = "0000000000000000000000000000000000000000000000000000000000000001";

/// Main gains `file`, and the repo has seen it.
fn main_gains(r: &Rig, file: &str, text: &str) {
    let root = r.f.place.state.parent().unwrap().to_path_buf();
    let work = root.join("work-main");
    git(
        &root,
        &[
            "clone",
            "-q",
            r.f.upstream.to_str().unwrap(),
            work.to_str().unwrap(),
        ],
    );
    let at = work.join(file);
    std::fs::create_dir_all(at.parent().unwrap()).unwrap();
    std::fs::write(at, text).unwrap();
    git(&work, &["add", "-A"]);
    git(&work, &["commit", "-q", "-m", "main's lock file"]);
    git(&work, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    git(&r.f.place.repo, &["fetch", "-q", "origin"]);
}

fn main_lock() -> String {
    cargo_lock(&[
        ("theseus-core", "0.1.0", None, None),
        ("gone", "0.2.0", Some(CRATES_IO), Some(SUM_A)),
        ("tokio", "1.40.0", Some(CRATES_IO), Some(SUM_A)),
        ("same", "1.0.0", Some(CRATES_IO), Some(SUM_A)),
    ])
}

/// **KEY: the card lists each package** with its names and versions in front of the
/// owner, for a change that adds, removes and changes one; main is untouched and the
/// head is never tried again.
#[tokio::test]
async fn a_change_of_dependencies_stops_at_the_owners_card_with_names_and_versions() {
    let r = rig(SelfMode::Act);
    main_gains(&r, "Cargo.lock", &main_lock());
    let changed = cargo_lock(&[
        ("theseus-core", "0.2.0", None, None),
        ("fresh", "2.1.0", Some(CRATES_IO), Some(SUM_B)),
        ("tokio", "1.41.0", Some(CRATES_IO), Some(SUM_B)),
        ("same", "1.0.0", Some(CRATES_IO), Some(SUM_A)),
    ]);
    let mut files = PROVED.to_vec();
    files.push(("Cargo.lock", &changed));
    let (branch, _) = r.seed("try-a-library", &files, SONNET);
    let main = r.upstream_main();
    let seams = r.seams();
    let pass = r.core.self_join_pass_yes(&seams).await.unwrap();
    assert_eq!(pass.stopped, [(branch, "keel".to_string())], "{pass:?}");
    let posts = r.posts();
    assert_eq!(posts.len(), 1, "{posts:?}");
    for line in [
        "- dependency: cargo: added fresh 2.1.0, from crates.io",
        "- dependency: cargo: removed gone 0.2.0 (it came from crates.io)",
        "- dependency: cargo: tokio 1.40.0 -> 1.41.0, from crates.io",
    ] {
        assert!(posts[0].contains(line), "{line}\n{}", posts[0]);
    }
    assert!(posts[0].contains("keel (dependency)"), "{}", posts[0]);
    assert!(
        !posts[0].contains("same 1.0.0"),
        "a package that did not change is not listed"
    );
    assert!(
        !posts[0].contains("theseus-core"),
        "a workspace member is not a dependency"
    );
    assert_eq!(r.upstream_main(), main, "nothing joined");
    let again = r.core.self_join_pass_yes(&seams).await.unwrap();
    assert!(
        again.stopped.is_empty() && again.joined.is_empty(),
        "{again:?}"
    );
    assert_eq!(r.posts().len(), 1, "never retried at that head");
}

/// An npm change is held the same way, and a source that may not be fetched is on the card.
#[tokio::test]
async fn an_npm_change_and_a_source_that_may_not_be_fetched_are_on_the_card() {
    let r = rig(SelfMode::Act);
    let ok = |n: &str, v: &str| format!("https://registry.npmjs.org/{n}/-/{n}-{v}.tgz");
    main_gains(
        &r,
        "cockpit/package-lock.json",
        &npm_lock(&[(
            "node_modules/left-pad",
            "1.2.0",
            &ok("left-pad", "1.2.0"),
            "sha512-AA",
        )]),
    );
    let npm = npm_lock(&[
        (
            "node_modules/left-pad",
            "1.3.0",
            &ok("left-pad", "1.3.0"),
            "sha512-BB",
        ),
        (
            "node_modules/odd",
            "1.0.0",
            "git+ssh://git@example.invalid/odd.git",
            "sha512-CC",
        ),
    ]);
    let git_dep = cargo_lock(&[(
        "x",
        "1.0.0",
        Some("git+https://example.invalid/x?rev=abc#abc"),
        None,
    )]);
    let mut files = PROVED.to_vec();
    files.push(("cockpit/package-lock.json", &npm));
    files.push(("Cargo.lock", &git_dep));
    r.seed("try-npm", &files, SONNET);
    r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    let posts = r.posts();
    assert_eq!(posts.len(), 1, "{posts:?}");
    assert!(
        posts[0].contains("- dependency: npm: left-pad 1.2.0 -> 1.3.0, from the npm registry"),
        "{}",
        posts[0]
    );
    assert!(
        posts[0].contains("odd 1.0.0: it comes from `git+ssh://git@example.invalid/odd.git`"),
        "{}",
        posts[0]
    );
    assert!(
        posts[0].contains("x 1.0.0 comes from `git+https://example.invalid/x?rev=abc#abc`"),
        "{}",
        posts[0]
    );
}

/// A lock file change that changes no dependency is not held: the branch joins.
#[tokio::test]
async fn a_lock_file_that_only_moved_is_not_held() {
    let r = rig(SelfMode::Act);
    main_gains(&r, "Cargo.lock", &main_lock());
    // The same packages, the workspace member's own entry changed, in another order.
    let moved = cargo_lock(&[
        ("same", "1.0.0", Some(CRATES_IO), Some(SUM_A)),
        ("tokio", "1.40.0", Some(CRATES_IO), Some(SUM_A)),
        ("gone", "0.2.0", Some(CRATES_IO), Some(SUM_A)),
        ("theseus-core", "0.9.0", None, None),
    ]);
    let mut files = PROVED.to_vec();
    files.push(("Cargo.lock", &moved));
    let (branch, _) = r.seed("move-the-lock", &files, SONNET);
    let pass = r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    assert!(pass.stopped.is_empty(), "{pass:?}: {:?}", r.posts());
    assert_eq!(pass.joined.len(), 1, "{pass:?}");
    assert_eq!(pass.joined[0].0, branch);
}

/// A lock file that cannot be read is held, never passed.
#[tokio::test]
async fn a_lock_file_that_cannot_be_read_is_held() {
    let r = rig(SelfMode::Act);
    let mut files = PROVED.to_vec();
    files.push(("Cargo.lock", "this is [not toml"));
    r.seed("break-the-lock", &files, SONNET);
    r.core.self_join_pass_yes(&r.seams()).await.unwrap();
    let posts = r.posts();
    assert!(
        posts.len() == 1 && posts[0].contains("Cargo.lock could not be read"),
        "{posts:?}"
    );
}

/// **KEY (review round 1): the other L1 builds of a branch's code get the same refusal as its
/// gate**: the planted revert's tests and the install's build ask for the packages of the tree's
/// lock files before they run, so a dependency that may not be fetched stops there with the
/// reason, and nothing is run. (A dependency that may be fetched is bound to the cache the
/// gate made: `deps::tests_fetch::every_build_of_the_same_lock_files_reads_one_cache`.)
#[tokio::test(flavor = "multi_thread")]
async fn the_planted_revert_and_the_install_ask_for_the_packages_too() {
    use std::sync::Arc;
    let f = crate::rsi::tests_tree::fixture();
    let mut place = f.place;
    let cfg = crate::sandbox::SandboxConfig::default();
    let sandbox = crate::sandbox::Sandbox::new(&cfg, &[std::path::PathBuf::from("/work")], &[]);
    let jail = crate::rsi::jail::Jail::new(Arc::new(sandbox));
    place.jail = Some(jail.clone());
    let t = crate::rsi::tree::open(&place, "git-crate", 1_792_000_000_000).unwrap();
    std::fs::write(
        t.dir.join("Cargo.lock"),
        cargo_lock(&[(
            "x",
            "1.0.0",
            Some("git+https://example.invalid/x?rev=abc#abc"),
            None,
        )]),
    )
    .unwrap();
    // On a blocking thread, as the pipeline calls them.
    let (p2, t2, j2) = (place.clone(), t.clone(), jail.clone());
    let planted = tokio::task::spawn_blocking(move || {
        super::planted::in_l1(&j2, &p2, &t2, "all()").unwrap_err()
    })
    .await
    .unwrap();
    assert!(
        format!("{planted:#}").contains("only crates.io is allowed"),
        "{planted:#}"
    );
    let (p2, t2) = (place.clone(), t.clone());
    let install =
        tokio::task::spawn_blocking(move || super::install::build(&p2, &t2, "abc123").unwrap_err())
            .await
            .unwrap();
    assert!(
        format!("{install:#}").contains("only crates.io is allowed"),
        "{install:#}"
    );
    assert!(
        !place
            .gates()
            .join(format!("planted-{}.log", t.slug))
            .exists(),
        "nothing ran"
    );
    assert!(
        !place
            .gates()
            .join(format!("install-{}.log", t.slug))
            .exists(),
        "nothing ran"
    );
}
