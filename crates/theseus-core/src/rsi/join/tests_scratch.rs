//! A review's scratch is removed when the reviewer has answered, and so is a tree it left
//! under the self trees: one left there held the pipeline's only slot for thirteen hours.
//! A build's tree is never the price of that (`scratch`).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use theseus_protocol::rsi::SelfMode;

use super::review::{self, Ask, Fut, Review, Reviewer};
use super::tests::{git, rig_with, PROVED, SONNET};
use super::{scratch, Seams};
use crate::rpc::Core;
use crate::rsi::capacity::{trees_named, trees_open};
use crate::rsi::tree::{self, Place};

/// The tree the reviewer leaves, named as the one of 2026-11-14 was.
const LEFT: &str = "review-0a1b2c-2";

/// A reviewer that makes itself a checkout under the self trees (a detached worktree of the
/// self repo, as a model with a shell does) and a build in its scratch, leaves both, and
/// rejects. With `builds`, a build opens its own tree while the review runs, and its
/// builder checks a commit out in it.
struct Litter {
    place: Place,
    builds: bool,
    /// The scratch it was given, and the text it was asked with.
    seen: Mutex<Option<(String, String)>>,
}

impl Litter {
    fn at(place: &Place, builds: bool) -> Arc<Litter> {
        Arc::new(Litter {
            place: place.clone(),
            builds,
            seen: Mutex::default(),
        })
    }
}

impl Reviewer for Litter {
    fn review<'a>(&'a self, _core: &'a Arc<Core>, ask: &'a Ask) -> Fut<'a> {
        let left = self.place.trees().join(LEFT);
        std::fs::create_dir_all(self.place.trees()).unwrap();
        git(
            &self.place.repo,
            &[
                "worktree",
                "add",
                "-q",
                "--detach",
                left.to_str().unwrap(),
                "origin/main",
            ],
        );
        let build = PathBuf::from(&ask.scratch).join("target/debug");
        std::fs::create_dir_all(&build).unwrap();
        std::fs::write(build.join("out"), "a build\n").unwrap();
        if self.builds {
            let t = tree::open(&self.place, "mend the nets", 1_794_700_000_000).unwrap();
            std::fs::write(t.dir.join("half-done.txt"), "not committed yet\n").unwrap();
            git(&t.dir, &["checkout", "-q", "--detach"]);
        }
        *self.seen.lock().unwrap() = Some((ask.scratch.clone(), review::prompt(ask)));
        Box::pin(async {
            Ok(Review {
                accept: false,
                reasons: vec!["its test passes without the fix".into()],
                cost_usd: 0.1,
                session: None,
            })
        })
    }
}

/// KEY: what the reviewer leaves is gone when the pass ends, so the next start has its slot.
/// Without the close, its tree is still under the self trees and counts as one open.
#[tokio::test]
async fn a_tree_the_reviewer_leaves_holds_no_slot() {
    let r = rig_with(SelfMode::Act, |_| {});
    let (branch, _) = r.seed("trim-the-sails", PROVED, SONNET);
    let litter = Litter::at(&r.f.place, false);
    let seams = Seams {
        reviewer: litter.clone(),
        ..r.seams()
    };
    r.core.self_join_pass(&seams).await.unwrap();
    let (given, asked) = litter.seen.lock().unwrap().clone().expect("it was asked");
    // It was told where to write, in its ask, and that place is the review's own.
    assert_eq!(PathBuf::from(&given), scratch::dir(&r.f.place, &branch));
    assert!(asked.contains(&given), "the ask names the scratch: {asked}");
    assert!(
        !given.contains("/trees/"),
        "{given} is outside the self trees"
    );
    // Both are gone: the build it made in its scratch, and the tree it left where it may not.
    assert!(!PathBuf::from(&given).exists(), "{given} was left");
    assert_eq!(trees_named(&r.f.place.trees()), Vec::<String>::new());
    assert_eq!(trees_open(&r.f.place.trees()), 0);
    // The self repo no longer lists it, and the review is closed: nothing says it is open.
    let listed = git(&r.f.place.repo, &["worktree", "list"]);
    assert!(!listed.contains(LEFT), "{listed}");
    assert!(!PathBuf::from(&given).with_extension("before").exists());
}

/// KEY: a build that opens its tree while a review runs keeps it, with its uncommitted
/// work, whatever its builder has checked out in it: the close removes what a review left,
/// never the pipeline's own.
#[tokio::test]
async fn a_builds_tree_opened_during_a_review_is_kept() {
    let r = rig_with(SelfMode::Act, |_| {});
    r.seed("trim-the-sails", PROVED, SONNET);
    let litter = Litter::at(&r.f.place, true);
    let seams = Seams {
        reviewer: litter.clone(),
        ..r.seams()
    };
    r.core.self_join_pass(&seams).await.unwrap();
    assert_eq!(trees_named(&r.f.place.trees()), ["mend-the-nets"]);
    let kept = r.f.place.trees().join("mend-the-nets/half-done.txt");
    assert!(kept.exists(), "the builder's uncommitted work stays");
}

/// A tree that was open before the review, a join's own, and one git cannot answer for are
/// not the review's to remove.
#[test]
fn only_what_appeared_during_the_review_and_is_not_the_pipelines_is_removed() {
    let f = crate::rsi::tests_tree::fixture();
    let trees = f.place.trees();
    std::fs::create_dir_all(trees.join("left-by-an-earlier-run")).unwrap();
    let own = scratch::open(&f.place, "self/20261014-trim-the-sails");
    assert!(own.dir.is_dir());
    for join in ["planted-trim", "dry-trim"] {
        std::fs::create_dir_all(trees.join(join)).unwrap();
    }
    // A checkout whose `.git` git cannot read: a doubt, so it stays.
    std::fs::create_dir_all(trees.join("unreadable")).unwrap();
    std::fs::write(trees.join("unreadable/.git"), "gitdir: /nowhere/at/all\n").unwrap();
    std::fs::create_dir_all(trees.join(LEFT)).unwrap();
    assert_eq!(scratch::close(&f.place, &own), [LEFT]);
    assert!(!own.dir.exists());
    assert_eq!(
        trees_named(&trees),
        [
            "dry-trim",
            "left-by-an-earlier-run",
            "planted-trim",
            "unreadable"
        ]
    );
    // A second close finds nothing more.
    assert_eq!(scratch::close(&f.place, &own), Vec::<String>::new());
}

/// A review the daemon stopped under never closed; the next review to open closes it.
#[test]
fn a_review_that_never_closed_is_closed_by_the_next_to_open() {
    let f = crate::rsi::tests_tree::fixture();
    let trees = f.place.trees();
    let first = scratch::open(&f.place, "self/20261014-trim-the-sails");
    std::fs::create_dir_all(trees.join(LEFT)).unwrap();
    std::fs::write(first.dir.join("notes"), "half a review\n").unwrap();
    // No close: the daemon stopped here. Another branch's review opens later.
    let next = scratch::open(&f.place, "self/20261014-mend-the-nets");
    assert!(!first.dir.exists(), "the first review's scratch was left");
    assert_eq!(trees_named(&trees), Vec::<String>::new());
    assert!(next.dir.is_dir());
    assert_eq!(scratch::close(&f.place, &next), Vec::<String>::new());
}
