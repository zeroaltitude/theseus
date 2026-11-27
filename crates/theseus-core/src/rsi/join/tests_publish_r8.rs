//! Round 8 (issue 53): the yes is tied to the review file's bytes; only text that can be shown exactly is asked; an
//! address rewrite or a credential outside the user part holds publication; a review's scratch is closed however the
//! review ends.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use theseus_protocol::rsi::SelfMode;

use super::publish::Answer;
use super::tests::{git, rig, Rig, PROVED, SONNET};
use super::tests_publish::{cards, owner_says_yes, publish_stops, seams, watch};
use super::tests_publish_r6::{another_remote, pass, proved, received, repo_git, review_text, write};
use crate::Core;

/// The review file the waiting card names.
fn card_file(core: &Core) -> PathBuf {
    PathBuf::from(cards(core).pop().expect("a card").input["review_file"].as_str().unwrap())
}

// ---- ruling 1: the yes is tied to the file he read --------------------------------------------------------------

/// The review file changed while its card waits: the card ends, the file is made again from the objects, one new
/// card asks, and nothing is sent.
#[tokio::test]
async fn a_review_file_changed_while_its_card_waits_asks_a_new_card() {
    let r = rig(SelfMode::Act);
    let w = watch(&r, &r.core);
    r.seed("trim-the-sails", PROVED, SONNET);
    pass(&r).await;
    let first = cards(&r.core).pop().unwrap().correlation_id;
    let file = card_file(&r.core);
    std::fs::write(&file, "not what the owner read\n").unwrap();
    pass(&r).await;
    let now = cards(&r.core);
    assert_eq!(now.len(), 1, "one new card");
    assert_ne!(now[0].correlation_id, first);
    assert_eq!(r.core.publish_answer(&first).unwrap(), Answer::Ended);
    assert!(review_text(&r.core).starts_with("THESEUS SELF-PUBLISH REVIEW FILE"), "made again from the objects");
    assert!(w.received().is_empty());
}

/// The review file changed, or removed, after the owner's yes, also across a restart: the yes is void, nothing is
/// sent, and one new card asks.
#[tokio::test]
async fn a_review_file_changed_or_removed_after_the_yes_voids_it() {
    for (case, restart) in [("changed", false), ("removed", false), ("changed", true)] {
        let r = rig(SelfMode::Act);
        let w = watch(&r, &r.core);
        r.seed("trim-the-sails", PROVED, SONNET);
        pass(&r).await;
        let file = card_file(&r.core);
        assert_eq!(owner_says_yes(&r.core), 1, "{case}");
        match case {
            "removed" => std::fs::remove_file(&file).unwrap(),
            _ => std::fs::write(&file, "replaced\n").unwrap(),
        }
        let core = if restart { r.restarted() } else { r.core.clone() };
        let p = core.self_join_pass(&seams(&r)).await.unwrap();
        assert!(p.joined.is_empty(), "{case}, restart {restart}: {p:?}");
        assert!(w.received().is_empty(), "{case}: nothing sent");
        assert_eq!(cards(&core).len(), 1, "{case}: one new card");
    }
}

/// The owner says yes to a card whose review file changed: the yes is not taken; the card ends.
#[tokio::test]
async fn a_yes_to_a_card_whose_file_changed_is_not_taken() {
    let r = rig(SelfMode::Act);
    r.seed("trim-the-sails", PROVED, SONNET);
    pass(&r).await;
    let card = cards(&r.core).pop().unwrap();
    std::fs::write(card_file(&r.core), "changed\n").unwrap();
    let said = r.core.confirm_action(&card.correlation_id, true, None, "cli").unwrap();
    assert!(!said.approved, "the yes is not taken");
    assert_eq!(r.core.publish_answer(&card.correlation_id).unwrap(), Answer::Ended);
}

// ---- ruling 2: only what can be shown exactly is asked ----------------------------------------------------------

/// A branch whose bytes the review file cannot show exactly is held with the fixed notice and no card.
async fn held_inexact(make: impl FnOnce(&Path)) {
    let r = rig(SelfMode::Act);
    let (_, head) = r.seed_by("trim-the-sails", SONNET, |wt| {
        proved(wt);
        make(wt);
    });
    let p = pass(&r).await;
    assert!(p.parked.is_empty() && p.joined.is_empty(), "{p:?}");
    assert!(cards(&r.core).is_empty(), "no card");
    let held: Vec<_> = (publish_stops(&r).into_iter())
        .filter(|d| d["numbers"]["held"] == true && d["head"] == head.as_str())
        .collect();
    assert_eq!(held.len(), 1, "{held:?}");
    assert!(
        r.posts().iter().any(|t| t.contains("text that cannot be shown exactly")),
        "{:?}",
        r.posts()
    );
}

fn commit_all(wt: &Path, message: &str) {
    git(wt, &["add", "-A"]);
    git(wt, &["commit", "-q", "-m", message]);
}

#[tokio::test]
async fn an_invalid_utf8_blob_is_held() {
    held_inexact(|wt| {
        write(wt, "bytes.txt", b"valid then \xff\xfe invalid\n");
        commit_all(wt, "bytes");
    })
    .await;
}

#[tokio::test]
async fn an_invalid_utf8_commit_message_is_held() {
    held_inexact(|wt| {
        write(wt, "plain.txt", b"plain\n");
        git(wt, &["add", "-A"]);
        let msg = wt.join("..").join("message.bin");
        std::fs::write(&msg, b"subject \xff\n").unwrap();
        git(wt, &["commit", "-q", "-F", msg.to_str().unwrap()]);
    })
    .await;
}

#[tokio::test]
async fn a_symlink_whose_target_holds_a_control_byte_is_held() {
    held_inexact(|wt| {
        std::os::unix::fs::symlink("target\u{1}name", wt.join("link")).unwrap();
        commit_all(wt, "link");
    })
    .await;
}

#[tokio::test]
async fn a_path_with_a_direction_override_is_held() {
    held_inexact(|wt| {
        write(wt, "name\u{202e}txt.rs", b"fn main() {}\n");
        commit_all(wt, "name");
    })
    .await;
}

/// A replacement ref that would show other objects: the review file shows the commit itself.
#[tokio::test]
async fn a_replacement_ref_does_not_change_what_the_review_file_shows() {
    let r = rig(SelfMode::Act);
    let (_, head) = r.seed_by("trim-the-sails", SONNET, |wt| {
        proved(wt);
        write(wt, "real.txt", b"SECRET-R the real content\n");
        commit_all(wt, "real");
    });
    let innocent = {
        let wt = r.f.upstream.parent().unwrap().join("innocent");
        repo_git(&r, &["worktree", "add", "-q", "--detach", wt.to_str().unwrap(), &format!("{head}^")]);
        write(&wt, "real.txt", b"nothing to see\n");
        commit_all(&wt, "real");
        let c = git(&wt, &["rev-parse", "HEAD"]);
        repo_git(&r, &["worktree", "remove", "--force", wt.to_str().unwrap()]);
        c
    };
    repo_git(&r, &["replace", &head, &innocent]);
    pass(&r).await;
    let text = review_text(&r.core);
    assert!(text.contains("+SECRET-R the real content"), "{text}");
    assert!(!text.contains("nothing to see"), "{text}");
}

/// A `.gitattributes` in the self repository's checked-out tree naming a text conversion: the review file shows the
/// line itself.
#[tokio::test]
async fn a_worktree_gitattributes_textconv_does_not_hide_a_line() {
    let r = rig(SelfMode::Act);
    repo_git(&r, &["config", "diff.hide.textconv", "sh -c 'echo HIDDEN-BY-TEXTCONV'"]);
    std::fs::write(r.f.place.repo.join(".gitattributes"), "secret.txt diff=hide\n").unwrap();
    r.seed_by("trim-the-sails", SONNET, |wt| {
        proved(wt);
        write(wt, "secret.txt", b"SECRET-W under a worktree textconv\n");
        commit_all(wt, "secret");
    });
    pass(&r).await;
    let text = review_text(&r.core);
    assert!(text.contains("+SECRET-W under a worktree textconv"), "{text}");
    assert!(!text.contains("HIDDEN-BY-TEXTCONV"), "{text}");
}

// ---- ruling 3: what the card shows is where the push goes -------------------------------------------------------

/// An explicit push URL with a pushInsteadOf rule, and a rewrite rule alone: publication is held with fixed words,
/// no card, and nothing is received anywhere.
#[tokio::test]
async fn an_address_rewrite_holds_publication() {
    for case in ["pushurl-and-pushinsteadof", "insteadof-alone"] {
        let r = rig(SelfMode::Act);
        let w = watch(&r, &r.core);
        let (other, other_log) = another_remote(&r, "other");
        let up = r.f.upstream.to_str().unwrap().to_string();
        match case {
            "pushurl-and-pushinsteadof" => {
                repo_git(&r, &["config", "remote.origin.pushurl", &up]);
                let key = format!("url.{}.pushInsteadOf", other.display());
                repo_git(&r, &["config", &key, &up]);
            }
            _ => {
                repo_git(&r, &["config", "url.https://example.invalid/.insteadOf", "nowhere:"]);
            }
        }
        r.seed("trim-the-sails", PROVED, SONNET);
        let p = pass(&r).await;
        assert!(p.parked.is_empty() && p.joined.is_empty(), "{case}: {p:?}");
        assert!(cards(&r.core).is_empty(), "{case}: no card");
        assert!(r.posts().iter().any(|t| t.contains("git rewrites addresses here")), "{case}: {:?}", r.posts());
        assert!(w.received().is_empty() && received(&other_log).is_empty(), "{case}");
    }
}

/// A destination with a token in its query, or in a path segment, is held: it is never shown.
#[tokio::test]
async fn a_credential_outside_the_user_part_holds_the_destination() {
    let r = rig(SelfMode::Act);
    for url in [
        "https://example.invalid/org/repo.git?access_token=abc",
        "https://example.invalid/x-access-token:abc/repo.git",
    ] {
        repo_git(&r, &["remote", "set-url", "origin", url]);
        let d = super::git::dest(&r.f.place, "origin").unwrap();
        assert_eq!(d, Err(super::git::URL_CREDENTIAL), "{url}");
    }
}

// ---- ruling 5: the reviewer's checkout -------------------------------------------------------------------------

/// A reviewer that never answers.
struct Never;

impl super::review::Reviewer for Never {
    fn review<'a>(&'a self, _core: &'a Arc<Core>, _ask: &'a super::review::Ask) -> super::review::Fut<'a> {
        Box::pin(async move {
            std::future::pending::<()>().await;
            unreachable!()
        })
    }
}

/// A reviewer that makes a file under its scratch the daemon cannot remove, then accepts.
struct Stuck;

impl super::review::Reviewer for Stuck {
    fn review<'a>(&'a self, _core: &'a Arc<Core>, ask: &'a super::review::Ask) -> super::review::Fut<'a> {
        let locked = Path::new(&ask.scratch).join("locked");
        std::fs::create_dir_all(&locked).unwrap();
        std::fs::write(locked.join("f"), "x").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
        let (profile, model) = (ask.target.profile.clone(), ask.target.model.clone());
        Box::pin(async move {
            Ok(super::review::Review {
                accept: false,
                fix: false,
                reasons: vec!["no".into()],
                cost_usd: 0.0,
                session: None,
                profile,
                model,
            })
        })
    }
}

fn reviews(r: &Rig) -> PathBuf {
    r.f.place.root().join("review")
}

fn marks(r: &Rig) -> Vec<PathBuf> {
    (std::fs::read_dir(reviews(r)).into_iter().flatten().flatten())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "before"))
        .collect()
}

/// A review cancelled between its checkout and its close: its scratch and checkout are closed all the same.
#[tokio::test]
async fn a_review_cancelled_after_its_checkout_leaves_nothing_open() {
    let r = rig(SelfMode::Act);
    r.seed("trim-the-sails", PROVED, SONNET);
    let s = super::Seams {
        reviewer: Arc::new(Never),
        ..r.seams()
    };
    let cut = tokio::time::timeout(std::time::Duration::from_secs(3), r.core.self_join_pass(&s)).await;
    assert!(cut.is_err(), "the pass was cut while the reviewer ran");
    let left: Vec<_> = (std::fs::read_dir(reviews(&r)).into_iter().flatten().flatten())
        .map(|e| e.path())
        .collect();
    assert!(left.is_empty(), "nothing open: {left:?}");
    let listed = repo_git(&r, &["worktree", "list", "--porcelain"]);
    assert!(!listed.contains("/review/"), "{listed}");
}

/// A scratch the daemon could not remove keeps its mark, and the next review (after a restart) closes it first.
#[tokio::test]
async fn a_scratch_that_could_not_be_removed_keeps_its_mark_until_the_next_review_closes_it() {
    let r = rig(SelfMode::Act);
    r.seed("trim-the-sails", PROVED, SONNET);
    let s = super::Seams {
        reviewer: Arc::new(Stuck),
        ..r.seams()
    };
    r.core.self_join_pass(&s).await.unwrap();
    let kept = marks(&r);
    assert_eq!(kept.len(), 1, "the mark stays: {kept:?}");
    use std::os::unix::fs::PermissionsExt;
    let dir = kept[0].with_extension("");
    std::fs::set_permissions(dir.join("locked"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let core = r.restarted();
    r.seed("reef-the-main", &[("reef.txt", "reefed\n"), ("tests/reef.sh", "test -f reef.txt\n")], SONNET);
    core.self_join_pass(&seams(&r)).await.unwrap();
    assert!(!kept[0].exists() && !dir.join("locked").exists(), "closed by the next review");
}
