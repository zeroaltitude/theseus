//! A review's scratch is removed when the reviewer has answered, and so is a tree it left
//! under the self trees: one left there held the pipeline's only slot for thirteen hours
//! (`scratch`).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use theseus_protocol::rsi::SelfMode;

use super::review::{self, Ask, Fut, Review, Reviewer};
use super::tests::{rig_with, PROVED, SONNET};
use super::{scratch, Seams};
use crate::rpc::Core;
use crate::rsi::capacity::{trees_named, trees_open};
use crate::rsi::tree::{self, Place};

/// A reviewer that makes itself a checkout under the self trees and a build in its scratch,
/// as a model with a shell does, leaves both, and rejects. With `builds`, a build opens its
/// own tree while the review runs.
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
        let left = self.place.trees().join("review-0a1b2c-2");
        std::fs::create_dir_all(left.join("src")).unwrap();
        std::fs::write(left.join("src/lib.rs"), "// a checkout the reviewer made\n").unwrap();
        let build = PathBuf::from(&ask.scratch).join("target/debug");
        std::fs::create_dir_all(&build).unwrap();
        std::fs::write(build.join("out"), "a build\n").unwrap();
        if self.builds {
            tree::open(&self.place, "mend the nets", 1_794_700_000_000).unwrap();
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
/// Without the close, `review-0a1b2c-2` is still under the self trees and counts as one open.
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
}

/// A build that opens its tree while a review runs keeps it: the close removes what a
/// review left, never the pipeline's own.
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
    assert!(tree::find(&r.f.place, "mend the nets").is_some());
}

/// A tree that was open before the review, and a join's own, are not the review's to remove.
#[test]
fn only_what_appeared_during_the_review_is_removed() {
    let f = crate::rsi::tests_tree::fixture();
    let trees = f.place.trees();
    std::fs::create_dir_all(trees.join("left-by-an-earlier-run")).unwrap();
    let own = scratch::open(&f.place, "self/20261014-trim-the-sails").unwrap();
    assert!(own.dir.is_dir());
    std::fs::create_dir_all(trees.join("planted-trim")).unwrap();
    std::fs::create_dir_all(trees.join("review-0a1b2c-2")).unwrap();
    assert_eq!(scratch::close(&f.place, &own), ["review-0a1b2c-2"]);
    assert!(!own.dir.exists());
    assert_eq!(
        trees_named(&trees),
        ["left-by-an-earlier-run", "planted-trim"]
    );
    // A second close finds nothing more.
    assert_eq!(scratch::close(&f.place, &own), Vec::<String>::new());
}
