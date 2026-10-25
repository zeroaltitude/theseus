//! A join that conflicts with main goes back to the backlog (theseus-o1h3f.6)
//! instead of waiting for the owner: the item is `proposed`, a note says the
//! next build cuts a fresh branch from the new main, that head is never
//! joined, and a third conflict drops the item with the way back. The keel
//! check beside it is unchanged: a ceiling's file still posts the owner's
//! card and the head waits.

use theseus_protocol::rsi::SelfMode;
use theseus_protocol::tasks::{TaskEvidence, TaskRecord, TaskState};

use super::tests::{git, rig, PROVED, SONNET};

/// The item as the builder left it: in progress, with `conflicts` earlier
/// conflicts in its evidence.
fn item(r: &super::tests::Rig, id: &str, conflicts: usize) {
    let t = TaskRecord {
        id: id.into(),
        version: 3,
        title: "Trim the sails".into(),
        state: TaskState::InProgress,
        session: Some("ses_builder".into()),
        owner: "self".into(),
        evidence: (0..conflicts)
            .map(|i| TaskEvidence {
                node: None,
                identity: format!("conflict:self/old-{i}@000000000000"),
                note: Some("it conflicts with main in same.txt".into()),
                by: "self-join".into(),
                at_ms: 1,
            })
            .collect(),
        ..TaskRecord::default()
    };
    r.core
        .store
        .append(&[crate::task_graph::record(&t).unwrap()])
        .unwrap();
}

/// Main moves on with its own `same.txt`, so a branch adding another conflicts.
fn main_moves(r: &super::tests::Rig) {
    let root = r.f.place.state.parent().unwrap().to_path_buf();
    let other = root.join("mover");
    git(
        &root,
        &[
            "clone",
            "-q",
            r.f.upstream.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    std::fs::write(other.join("same.txt"), "theirs\n").unwrap();
    git(&other, &["add", "-A"]);
    git(&other, &["commit", "-q", "-m", "main moves"]);
    git(&other, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
}

fn conflicting() -> Vec<(&'static str, &'static str)> {
    let mut files = PROVED.to_vec();
    files.push(("same.txt", "mine\n"));
    files
}

/// The conflict returns the item to `proposed` with a note naming the fresh
/// branch; main is untouched, the head is not tried again, and the note goes
/// once.
#[tokio::test]
async fn a_conflict_with_main_returns_the_item_to_proposed_with_a_notice() {
    let r = rig(SelfMode::Act);
    item(&r, "tsk_sails", 0);
    let (branch, _) = r.seed("tsk_sails", &conflicting(), SONNET);
    main_moves(&r);
    let main = r.upstream_main();
    let seams = r.seams();
    let pass = r.core.self_join_pass(&seams).await.unwrap();
    assert_eq!(
        pass.stopped,
        [(branch.clone(), "merge".to_string())],
        "{pass:?}"
    );
    assert!(pass.joined.is_empty(), "{pass:?}");
    assert_eq!(r.upstream_main(), main, "nothing joined");
    let t = crate::task_graph::get(&r.core.store, "tsk_sails")
        .unwrap()
        .unwrap();
    assert_eq!(
        t.state,
        TaskState::Proposed,
        "ready again, not held for the owner"
    );
    assert_eq!(t.session, None);
    let e = t.evidence.last().unwrap();
    assert!(
        e.identity.starts_with(&format!("conflict:{branch}@")),
        "{e:?}"
    );
    let posts = r.posts();
    assert_eq!(posts.len(), 1, "{posts:?}");
    assert!(
        posts[0].contains("fresh branch from") && posts[0].contains("same.txt"),
        "{}",
        posts[0]
    );
    let stopped = &r.rows("self.join.stopped")[0];
    assert_eq!(stopped["numbers"]["conflict"], true, "{stopped}");
    let again = r.core.self_join_pass(&seams).await.unwrap();
    assert!(
        again.stopped.is_empty() && again.joined.is_empty(),
        "{again:?}"
    );
    assert_eq!(r.posts().len(), 1, "that head is never tried again");
}

/// The bound on a loop: the third conflict of one item drops it, `failed`,
/// and the note quotes the command back.
#[tokio::test]
async fn the_third_conflict_drops_the_item_with_the_way_back() {
    let r = rig(SelfMode::Act);
    item(&r, "tsk_sails", 2);
    r.seed("tsk_sails", &conflicting(), SONNET);
    main_moves(&r);
    r.core.self_join_pass(&r.seams()).await.unwrap();
    let t = crate::task_graph::get(&r.core.store, "tsk_sails")
        .unwrap()
        .unwrap();
    assert_eq!(t.state, TaskState::Failed);
    let posts = r.posts();
    assert!(
        posts[0].contains(&format!(
            "`{}`",
            theseus_protocol::rsi::retry_command("tsk_sails")
        )),
        "{}",
        posts[0]
    );
}

/// The keel is unchanged: a ceiling's file posts the owner's card, the head
/// waits (held, not a conflict), and the item is not sent back to build.
#[tokio::test]
async fn a_self_join_in_a_keel_class_still_posts_the_owners_card() {
    let r = rig(SelfMode::Act);
    item(&r, "tsk_sails", 0);
    let mut files = PROVED.to_vec();
    files.push(("scripts/long-files.txt", "crates/x.rs 9999\n"));
    let (branch, _) = r.seed("tsk_sails", &files, SONNET);
    let main = r.upstream_main();
    let pass = r.core.self_join_pass(&r.seams()).await.unwrap();
    assert_eq!(pass.stopped, [(branch, "keel".to_string())], "{pass:?}");
    let posts = r.posts();
    assert_eq!(posts.len(), 1, "{posts:?}");
    assert!(posts[0].contains("keel ("), "{}", posts[0]);
    assert_eq!(r.upstream_main(), main);
    let stopped = &r.rows("self.join.stopped")[0];
    assert_eq!(stopped["numbers"]["held"], true);
    assert!(stopped["numbers"]["conflict"].is_null(), "{stopped}");
}
