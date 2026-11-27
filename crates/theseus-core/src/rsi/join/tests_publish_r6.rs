//! What the owner reads is everything that is sent, the push sends one ref to one destination, and a yes is bound to
//! what it was asked about (issue 53, round 6). Every test reads what each destination received from a `pre-receive`
//! hook of its own.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use theseus_protocol::rsi::SelfMode;

use super::publish::Answer;
use super::tests::{git, rig, rig_with, Rig, PROVED, SONNET};
use super::tests_publish::{cards, owner_says_yes, publish_stops, seams, watch};
use crate::Core;

/// A bare repository beside the rig's remote, cloned from it, with a hook that writes down every ref it receives.
pub(super) fn another_remote(r: &Rig, name: &str) -> (PathBuf, PathBuf) {
    let root = r.f.upstream.parent().unwrap().to_path_buf();
    let dir = root.join(format!("{name}.git"));
    git(
        &root,
        &[
            "clone",
            "-q",
            "--bare",
            r.f.upstream.to_str().unwrap(),
            dir.to_str().unwrap(),
        ],
    );
    (dir.clone(), hooked(&dir, name))
}

/// A `pre-receive` hook in `bare` writing what it receives to a log; the log's path.
pub(super) fn hooked(bare: &Path, name: &str) -> PathBuf {
    let log = bare.parent().unwrap().join(format!("{name}-received.log"));
    let hook = bare.join("hooks").join("pre-receive");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(&hook, format!("#!/bin/sh\ncat >> '{}'\n", log.display())).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    log
}

pub(super) fn received(log: &Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

pub(super) fn repo_git(r: &Rig, args: &[&str]) -> String {
    git(&r.f.place.repo, args)
}

/// The review file the waiting card names, read.
pub(super) fn review_text(core: &Core) -> String {
    let card = cards(core).pop().expect("a card");
    std::fs::read_to_string(card.input["review_file"].as_str().unwrap()).unwrap()
}

/// The proving files committed, as every branch here needs them for its review.
pub(super) fn proved(wt: &Path) {
    for (p, t) in PROVED {
        let p = wt.join(p);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, t).unwrap();
    }
    git(wt, &["add", "-A"]);
    git(
        wt,
        &["commit", "-q", "-m", "self: the fix (trim-the-sails)"],
    );
}

pub(super) fn write(wt: &Path, path: &str, text: &[u8]) {
    let p = wt.join(path);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

pub(super) async fn pass(r: &Rig) -> super::Pass {
    r.core.self_join_pass(&seams(r)).await.unwrap()
}

/// (a) A merge commit's own resolution, which `git log -p` does not show, is in the review file.
#[tokio::test]
async fn the_review_file_holds_a_merges_own_change() {
    let r = rig(SelfMode::Act);
    r.seed_by("trim-the-sails", SONNET, |wt| {
        proved(wt);
        git(wt, &["checkout", "-q", "-b", "side", "HEAD~1"]);
        write(wt, "side.txt", b"side\n");
        git(wt, &["add", "-A"]);
        git(wt, &["commit", "-q", "-m", "side"]);
        git(wt, &["checkout", "-q", "-"]);
        git(wt, &["merge", "-q", "--no-ff", "--no-commit", "side"]);
        write(wt, "evil.txt", b"SECRET-A in the merge alone\n");
        git(wt, &["add", "-A"]);
        git(wt, &["commit", "-q", "-m", "merge side"]);
    });
    pass(&r).await;
    let text = review_text(&r.core);
    assert!(text.contains("+SECRET-A in the merge alone"), "{text}");
}

/// (b) A file an intermediate commit adds and a later one removes is in the review file.
#[tokio::test]
async fn the_review_file_holds_a_file_added_then_removed() {
    let r = rig(SelfMode::Act);
    r.seed_by("trim-the-sails", SONNET, |wt| {
        write(wt, "gone.txt", b"SECRET-B for a moment\n");
        git(wt, &["add", "-A"]);
        git(wt, &["commit", "-q", "-m", "add"]);
        git(wt, &["rm", "-q", "gone.txt"]);
        git(wt, &["commit", "-q", "-m", "remove"]);
        proved(wt);
    });
    pass(&r).await;
    let text = review_text(&r.core);
    assert!(text.contains("+SECRET-B for a moment"), "{text}");
}

/// (c) A header git keeps in a commit object, which no `log` format shows, is in the review file.
#[tokio::test]
async fn the_review_file_holds_a_commits_extra_header() {
    let r = rig(SelfMode::Act);
    r.seed_by("trim-the-sails", SONNET, |wt| {
        proved(wt);
        let raw = git(wt, &["cat-file", "commit", "HEAD"]);
        let (head, body) = raw.split_once("\n\n").unwrap();
        let extra = format!("{head}\nx-extra SECRET-C in a header\n\n{body}\n");
        let path = wt.join("..").join("extra-commit.txt");
        std::fs::write(&path, extra).unwrap();
        let c = git(
            wt,
            &["hash-object", "-t", "commit", "-w", path.to_str().unwrap()],
        );
        git(wt, &["reset", "-q", "--hard", &c]);
    });
    pass(&r).await;
    let text = review_text(&r.core);
    assert!(text.contains("x-extra SECRET-C in a header"), "{text}");
}

/// (d) A file a `.gitattributes` text conversion would show as something else is in the review file as it is.
#[tokio::test]
async fn the_review_file_holds_what_a_textconv_would_hide() {
    let r = rig(SelfMode::Act);
    repo_git(
        &r,
        &[
            "config",
            "diff.hide.textconv",
            "sh -c 'echo HIDDEN-BY-TEXTCONV'",
        ],
    );
    // Where git reads attributes for this operation: the repository's own `info/attributes`. With it there,
    // `--textconv` would show HIDDEN-BY-TEXTCONV; the review file's `--no-textconv` shows the line itself.
    let info = PathBuf::from(repo_git(
        &r,
        &["rev-parse", "--git-path", "info/attributes"],
    ));
    let info = if info.is_absolute() {
        info
    } else {
        r.f.place.repo.join(info)
    };
    std::fs::create_dir_all(info.parent().unwrap()).unwrap();
    std::fs::write(&info, "secret.txt diff=hide\n").unwrap();
    r.seed_by("trim-the-sails", SONNET, |wt| {
        proved(wt);
        write(wt, ".gitattributes", b"secret.txt diff=hide\n");
        write(wt, "secret.txt", b"SECRET-D under a textconv\n");
        git(wt, &["add", "-A"]);
        git(wt, &["commit", "-q", "-m", "attributes"]);
    });
    pass(&r).await;
    let text = review_text(&r.core);
    assert!(text.contains("+SECRET-D under a textconv"), "{text}");
    assert!(!text.contains("HIDDEN-BY-TEXTCONV"), "{text}");
}

/// A binary file and a NUL byte past git's own look hold the branch with the fixed notice, and no card is asked.
#[tokio::test]
async fn a_file_the_review_file_cannot_show_holds_the_branch_with_no_card() {
    for (name, bytes) in [
        ("binary", vec![0u8, 1, 2, 3, 255, 0, 7]),
        (
            "late-nul",
            [vec![b'a'; 9000], vec![0u8], b"tail\n".to_vec()].concat(),
        ),
    ] {
        let r = rig(SelfMode::Act);
        let w = watch(&r, &r.core);
        let (branch, head) = r.seed_by("trim-the-sails", SONNET, |wt| {
            proved(wt);
            write(wt, &format!("{name}.dat"), &bytes);
            git(wt, &["add", "-A"]);
            git(wt, &["commit", "-q", "-m", name]);
        });
        let p = pass(&r).await;
        assert!(p.parked.is_empty() && p.joined.is_empty(), "{name}: {p:?}");
        assert!(cards(&r.core).is_empty(), "{name}: no card");
        let held: Vec<_> = (publish_stops(&r).into_iter())
            .filter(|d| d["numbers"]["held"] == true && d["head"] == head.as_str())
            .collect();
        assert_eq!(held.len(), 1, "{name}: {held:?}");
        let posts = r.posts();
        assert!(
            posts
                .iter()
                .any(|t| t.contains("cannot be shown as text") && t.contains(&head)),
            "{name}: {posts:?}"
        );
        assert!(w.received().is_empty(), "{name}");
        let again = pass(&r).await;
        assert!(
            again.parked.is_empty() && cards(&r.core).is_empty(),
            "{name}: held, {branch}"
        );
    }
}

/// `push.followTags=true` and an annotated tag on the approved head made after the yes: the push sends main alone.
#[tokio::test]
async fn follow_tags_sends_no_tag() {
    let r = rig(SelfMode::Act);
    let w = watch(&r, &r.core);
    repo_git(&r, &["config", "push.followTags", "true"]);
    let (_, head) = r.seed("trim-the-sails", PROVED, SONNET);
    pass(&r).await;
    assert_eq!(owner_says_yes(&r.core), 1);
    repo_git(
        &r,
        &[
            "tag",
            "-a",
            "v-unreviewed",
            "-m",
            "SECRET tag message",
            &head,
        ],
    );
    let p = pass(&r).await;
    assert_eq!(p.joined.len(), 1, "{p:?}");
    let got = w.received();
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(got[0].ends_with(" refs/heads/main"), "{got:?}");
    let tags = git(&r.f.upstream, &["tag", "--list"]);
    assert!(tags.is_empty(), "no tag at the remote: {tags}");
}

/// A push URL that is not the fetch URL, two push URLs, and a `pushInsteadOf` rewrite each hold the branch with fixed
/// words: no card, and nothing received at any destination.
#[tokio::test]
async fn a_remote_without_one_destination_is_held() {
    for case in ["pushurl", "two-pushurls", "push-instead-of"] {
        let r = rig(SelfMode::Act);
        let w = watch(&r, &r.core);
        let (other, other_log) = another_remote(&r, "other");
        let (third, third_log) = another_remote(&r, "third");
        let other_url = other.to_str().unwrap().to_string();
        match case {
            "pushurl" => {
                repo_git(&r, &["config", "remote.origin.pushurl", &other_url]);
            }
            "two-pushurls" => {
                repo_git(
                    &r,
                    &[
                        "config",
                        "--add",
                        "remote.origin.pushurl",
                        r.f.upstream.to_str().unwrap(),
                    ],
                );
                repo_git(
                    &r,
                    &[
                        "config",
                        "--add",
                        "remote.origin.pushurl",
                        third.to_str().unwrap(),
                    ],
                );
            }
            _ => {
                let key = format!("url.{other_url}.pushInsteadOf");
                repo_git(&r, &["config", &key, r.f.upstream.to_str().unwrap()]);
            }
        }
        r.seed("trim-the-sails", PROVED, SONNET);
        let p = pass(&r).await;
        assert!(p.parked.is_empty() && p.joined.is_empty(), "{case}: {p:?}");
        assert!(cards(&r.core).is_empty(), "{case}: no card");
        let posts = r.posts();
        assert!(
            posts
                .iter()
                .any(|t| t.contains("not offered for publishing")),
            "{case}: {posts:?}"
        );
        assert!(
            w.received().is_empty()
                && received(&other_log).is_empty()
                && received(&third_log).is_empty(),
            "{case}"
        );
    }
}

/// A `remote.<name>.push` refspec that would push every branch: the push sends main alone.
#[tokio::test]
async fn a_configured_push_refspec_sends_main_alone() {
    let r = rig(SelfMode::Act);
    let w = watch(&r, &r.core);
    repo_git(
        &r,
        &["config", "remote.origin.push", "refs/heads/*:refs/heads/*"],
    );
    r.seed("trim-the-sails", PROVED, SONNET);
    pass(&r).await;
    assert_eq!(owner_says_yes(&r.core), 1);
    let p = pass(&r).await;
    assert_eq!(p.joined.len(), 1, "{p:?}");
    let got = w.received();
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(got[0].ends_with(" refs/heads/main"), "{got:?}");
}

/// The remote's URL changed between the yes and the push: the yes is void, a new card shows the new destination,
/// and nothing is received anywhere until the owner answers it.
#[tokio::test]
async fn a_config_change_after_the_yes_voids_it() {
    let r = rig(SelfMode::Act);
    let w = watch(&r, &r.core);
    let (other, other_log) = another_remote(&r, "other");
    r.seed("trim-the-sails", PROVED, SONNET);
    pass(&r).await;
    let first = cards(&r.core).pop().unwrap();
    assert_eq!(owner_says_yes(&r.core), 1);
    repo_git(
        &r,
        &["config", "remote.origin.url", other.to_str().unwrap()],
    );
    let p = pass(&r).await;
    assert!(p.joined.is_empty(), "{p:?}");
    assert!(w.received().is_empty() && received(&other_log).is_empty());
    let p = pass(&r).await;
    assert!(p.joined.is_empty(), "{p:?}");
    let now = cards(&r.core);
    assert_eq!(now.len(), 1, "one new card");
    assert_ne!(now[0].correlation_id, first.correlation_id);
    assert!(
        now[0]
            .reason
            .contains(&format!("DESTINATION: {}", other.display())),
        "{}",
        now[0].reason
    );
    assert!(w.received().is_empty() && received(&other_log).is_empty());
}

/// Main moved forward at the remote after the yes, with none of the head's commits: the yes stands and the head
/// joins onto the new main.
#[tokio::test]
async fn a_fast_forward_of_main_keeps_the_yes() {
    let r = rig(SelfMode::Act);
    let w = watch(&r, &r.core);
    let (branch, head) = r.seed("trim-the-sails", PROVED, SONNET);
    pass(&r).await;
    assert_eq!(owner_says_yes(&r.core), 1);
    let moved = onto_upstream_main(&r, "ahead", false);
    let core = r.restarted();
    let p = core.self_join_pass(&seams(&r)).await.unwrap();
    assert_eq!(p.joined.len(), 1, "{p:?}");
    let merge = git(&r.f.upstream, &["rev-parse", "refs/heads/main"]);
    let parents = git(&r.f.upstream, &["rev-list", "--parents", "-n", "1", &merge]);
    assert_eq!(parents, format!("{merge} {moved} {head}"), "{branch}");
    assert_eq!(w.received().len(), 2, "the outside commit, then the join");
}

/// Main at the remote rewritten after the yes: the yes is void and one new card is asked, also after a restart.
#[tokio::test]
async fn a_rewritten_main_voids_the_yes() {
    let r = rig(SelfMode::Act);
    // Main gets a second commit, so its tip has a parent a rewrite keeps.
    onto_upstream_main(&r, "second", false);
    repo_git(&r, &["fetch", "-q", "origin"]);
    r.seed("trim-the-sails", PROVED, SONNET);
    pass(&r).await;
    let first = cards(&r.core).pop().unwrap().correlation_id;
    assert_eq!(owner_says_yes(&r.core), 1);
    onto_upstream_main(&r, "rewritten", true);
    let core = r.restarted();
    let s = seams(&r);
    let p = core.self_join_pass(&s).await.unwrap();
    assert!(p.joined.is_empty(), "{p:?}");
    let p = core.self_join_pass(&s).await.unwrap();
    assert!(p.joined.is_empty(), "{p:?}");
    let now = cards(&core);
    assert_eq!(now.len(), 1, "one new card");
    assert_ne!(now[0].correlation_id, first);
    assert_eq!(
        core.publish_answer(&first).unwrap(),
        Answer::Yes,
        "the old yes stands only for the old main"
    );
}

/// A commit onto the remote's main from another clone: a fast-forward of it, or (`rewrite`) main's tip amended and
/// force-pushed. The new main.
pub(super) fn onto_upstream_main(r: &Rig, name: &str, rewrite: bool) -> String {
    let root = r.f.upstream.parent().unwrap().to_path_buf();
    let work = root.join(name);
    git(
        &root,
        &[
            "clone",
            "-q",
            r.f.upstream.to_str().unwrap(),
            work.to_str().unwrap(),
        ],
    );
    write(&work, &format!("{name}.txt"), b"outside\n");
    git(&work, &["add", "-A"]);
    match rewrite {
        // Main's own tip replaced: the card's main is no ancestor of the new one.
        true => git(&work, &["commit", "-q", "--amend", "-m", name]),
        false => git(&work, &["commit", "-q", "-m", name]),
    };
    git(
        &work,
        &["push", "-q", "--force", "origin", "HEAD:refs/heads/main"],
    );
    git(&work, &["rev-parse", "HEAD"])
}

/// The configured remote is not `origin`: the card's main, the scrub's base and the push are that remote's.
#[tokio::test]
async fn a_remote_that_is_not_origin_is_the_one_read_and_pushed() {
    let r = rig_with(SelfMode::Act, |c| {
        c.self_improve.remote = Some("pub".into())
    });
    let (published, published_log) = another_remote(&r, "published");
    repo_git(&r, &["remote", "add", "pub", published.to_str().unwrap()]);
    repo_git(&r, &["fetch", "-q", "pub"]);
    let w = watch(&r, &r.core);
    r.seed("trim-the-sails", PROVED, SONNET);
    pass(&r).await;
    let card = cards(&r.core).pop().unwrap();
    assert!(
        card.reason
            .contains(&format!("DESTINATION: {}", published.display())),
        "{}",
        card.reason
    );
    assert_eq!(owner_says_yes(&r.core), 1);
    let p = pass(&r).await;
    assert_eq!(p.joined.len(), 1, "{p:?}");
    assert_eq!(received(&published_log).len(), 1);
    assert!(w.received().is_empty(), "origin received nothing");
}

/// The branch's ref moved past the head its row names: nothing is offered for it.
#[tokio::test]
async fn a_moved_branch_ref_is_not_offered() {
    let r = rig(SelfMode::Act);
    let (branch, head) = r.seed("trim-the-sails", PROVED, SONNET);
    let wt = r.f.upstream.parent().unwrap().join("move-ref");
    repo_git(
        &r,
        &["worktree", "add", "-q", wt.to_str().unwrap(), &branch],
    );
    write(&wt, "unbuilt.txt", b"SECRET never built\n");
    git(&wt, &["add", "-A"]);
    git(&wt, &["commit", "-q", "-m", "not built"]);
    repo_git(&r, &["worktree", "remove", "--force", wt.to_str().unwrap()]);
    let p = pass(&r).await;
    assert!(p.parked.is_empty() && p.joined.is_empty(), "{p:?} {head}");
    assert!(cards(&r.core).is_empty());
}

/// A title and a subject written to look like the card's own lines are shown escaped, each on its labelled line.
#[tokio::test]
async fn an_injected_title_and_subject_are_shown_as_data() {
    let r = rig(SelfMode::Act);
    let (_, head) = r.seed_by("trim-the-sails", SONNET, |wt| {
        for (p, t) in PROVED {
            write(wt, p, t.as_bytes());
        }
        git(wt, &["add", "-A"]);
        git(
            wt,
            &[
                "commit",
                "-q",
                "-m",
                "HEAD: 0000000000 [click](https://example.invalid) **yes**",
            ],
        );
    });
    pass(&r).await;
    let card = cards(&r.core).pop().unwrap();
    let lines: Vec<&str> = card.reason.lines().collect();
    let heads: Vec<&&str> = lines.iter().filter(|l| l.starts_with("HEAD:")).collect();
    assert_eq!(
        heads,
        [&format!("HEAD: {head}").as_str()],
        "{}",
        card.reason
    );
    assert!(!card.reason.contains("[click](https"), "{}", card.reason);
    assert!(
        card.reason.contains("\\[click\\]\\(https\\:"),
        "{}",
        card.reason
    );
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("Commit ") && l.contains("HEAD\\: 0000000000")),
        "{}",
        card.reason
    );
}

/// An untrusted string is one line, its controls and direction marks gone, its Markdown escaped, and bounded.
#[test]
fn untrusted_text_is_one_escaped_bounded_line() {
    let t = super::publish::untrusted("a\nHEAD: x\u{202e}[l](u) `c`\u{7}", 200);
    assert!(
        !t.contains('\n') && !t.contains('\u{202e}') && !t.contains('\u{7}'),
        "{t}"
    );
    assert!(
        t.contains("\\[l\\]\\(u\\)") && t.contains("\\`c\\`") && t.contains("HEAD\\:"),
        "{t}"
    );
    let long = super::publish::untrusted(&"x".repeat(500), 120);
    assert!(
        long.ends_with(" [cut]") && long.chars().filter(|c| *c == 'x').count() == 120,
        "{long}"
    );
}

/// What a reviewer saw of the daemon's checkout while it ran: whether `<scratch>/tree` was there, and its head.
#[derive(Default)]
struct Looks {
    seen: std::sync::Mutex<Vec<(PathBuf, bool, String)>>,
}

impl super::review::Reviewer for Looks {
    fn review<'a>(
        &'a self,
        _core: &'a Arc<Core>,
        ask: &'a super::review::Ask,
    ) -> super::review::Fut<'a> {
        let tree = Path::new(&ask.scratch).join("tree");
        let head = match tree.exists() {
            true => git(&tree, &["rev-parse", "HEAD"]),
            false => String::new(),
        };
        self.seen
            .lock()
            .unwrap()
            .push((tree.clone(), tree.exists(), head));
        let (profile, model) = (ask.target.profile.clone(), ask.target.model.clone());
        Box::pin(async move {
            Ok(super::review::Review {
                accept: true,
                fix: false,
                reasons: vec!["it reads the daemon's checkout".into()],
                cost_usd: 0.0,
                session: None,
                profile,
                model,
            })
        })
    }
}

/// STOP 1 (issue 53): the daemon checks out the reviewed head in the review's scratch before the review, and it is
/// gone (the directory and its worktree entry) when the review has answered.
#[tokio::test]
async fn the_daemons_checkout_is_there_during_the_review_and_gone_after() {
    let r = rig(SelfMode::Act);
    let (_, head) = r.seed("trim-the-sails", PROVED, SONNET);
    let looks = Arc::new(Looks::default());
    let s = super::Seams {
        reviewer: looks.clone(),
        ..r.seams()
    };
    r.core.self_join_pass(&s).await.unwrap();
    let seen = looks.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 1, "{seen:?}");
    let (tree, there, at) = &seen[0];
    assert!(
        *there,
        "the checkout was there during the review: {}",
        tree.display()
    );
    assert_eq!(at, &head, "at the reviewed head");
    assert!(!tree.exists(), "and gone after");
    let listed = repo_git(&r, &["worktree", "list", "--porcelain"]);
    assert!(!listed.contains(&tree.display().to_string()), "{listed}");
}
