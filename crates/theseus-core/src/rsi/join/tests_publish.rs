//! Nothing of the self loop reaches the remote without the owner's yes (issue 53). The join asks one card for the
//! head it would publish and parks; his yes publishes that head and only it; his no holds it; no answer waits,
//! across passes and a restart. What reached the remote is read twice: git's own trace of every command the loop
//! ran (`GIT_TRACE`), and a `pre-receive` hook on the remote that writes down every ref it is sent.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;
use theseus_protocol::rsi::SelfMode;

use super::publish::{self, Answer};
use super::tests::{git, rig, Rig, PROVED, SONNET};
use super::Seams;
use crate::fact::rsi_pr::SelfBranchBuilt;
use crate::Core;

/// Where the loop's git commands and the remote's received refs are written.
struct Watched {
    trace: PathBuf,
    received: PathBuf,
}

/// Watch what `core`'s git commands do and what the rig's remote receives.
fn watch(r: &Rig, core: &Core) -> Watched {
    let root = r.f.upstream.parent().unwrap().to_path_buf();
    let (trace, received) = (root.join("git-trace.log"), root.join("received.log"));
    let hook = r.f.upstream.join("hooks").join("pre-receive");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(
        &hook,
        format!("#!/bin/sh\ncat >> '{}'\n", received.display()),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut env = crate::rsi::tests_tree::hermetic();
    env.push(("GIT_TRACE".into(), trace.clone().into()));
    core.tools.self_pr.set_env(env);
    Watched { trace, received }
}

impl Watched {
    /// Every git command traced that sends to a remote: a push, or the receive end it runs.
    fn sends(&self) -> Vec<String> {
        let text = std::fs::read_to_string(&self.trace).unwrap_or_default();
        text.lines()
            .filter(|l| l.contains("trace: "))
            .filter(|l| {
                l.contains(" push ") || l.contains("receive-pack") || l.contains("send-pack")
            })
            .map(str::to_string)
            .collect()
    }

    /// Every ref update the remote received, one `old new ref` a line.
    fn received(&self) -> Vec<String> {
        let text = std::fs::read_to_string(&self.received).unwrap_or_default();
        text.lines().map(str::to_string).collect()
    }

    /// Whether the trace saw the loop's git at all (so an empty `sends` is not an empty trace).
    fn traced(&self) -> bool {
        std::fs::read_to_string(&self.trace).is_ok_and(|t| t.contains("trace: "))
    }
}

/// The owner's yes to every publish card waiting on `core`, as the CLI answers: what a test that is about something
/// else does where the join now waits for him. How many it answered.
pub(crate) fn owner_says_yes(core: &Core) -> usize {
    let waiting = cards(core);
    for c in &waiting {
        core.confirm_action(&c.correlation_id, true, None, "cli")
            .unwrap();
    }
    waiting.len()
}

/// A join pass as a test about something else needs it: when the pass parks on the owner's card, the owner says
/// yes and the pass runs once more. What the two did, as one pass: the reviews of both, the second's joins and parks,
/// and the stops of both but the first's wait for the card.
pub(crate) trait PassYes {
    async fn self_join_pass_yes(&self, seams: &Seams) -> anyhow::Result<super::Pass>;
}

impl PassYes for Arc<Core> {
    async fn self_join_pass_yes(&self, seams: &Seams) -> anyhow::Result<super::Pass> {
        let mut first = self.self_join_pass(seams).await?;
        if first.parked.is_empty() {
            return Ok(first);
        }
        owner_says_yes(self);
        let second = self.self_join_pass(seams).await?;
        first.reviewed.extend(second.reviewed);
        first.joined = second.joined;
        first.stopped.retain(|(_, phase)| phase != publish::PHASE);
        first.stopped.extend(second.stopped);
        first.parked = second.parked;
        Ok(first)
    }
}

fn seams(r: &Rig) -> Seams {
    Seams {
        reviewer: Arc::new(super::review::CheckTurn),
        ..r.seams()
    }
}

/// The publish cards waiting on `core`.
fn cards(core: &Core) -> Vec<theseus_protocol::ConfirmRequest> {
    core.confirm_list()
        .unwrap()
        .into_iter()
        .filter(|c| c.tool == publish::ACK)
        .collect()
}

/// The remote's branches.
fn remote_heads(r: &Rig) -> String {
    git(
        &r.f.upstream,
        &["for-each-ref", "--format=%(refname)", "refs/heads/"],
    )
}

fn publish_stops(r: &Rig) -> Vec<Value> {
    (r.rows("self.join.stopped").into_iter())
        .filter(|d| d["phase"] == publish::PHASE)
        .collect()
}

/// The join parks on one card for the head and sends nothing before the owner's yes: not on the first pass, not on
/// a second. The card names the change and the command that prints all of it.
#[tokio::test]
async fn the_join_parks_on_one_card_and_sends_nothing_before_the_yes() {
    let r = rig(SelfMode::Act);
    let w = watch(&r, &r.core);
    let (branch, head) = r.seed("trim-the-sails", PROVED, SONNET);
    let base = r.upstream_main();
    let seams = seams(&r);
    for n in 1..=2 {
        let pass = r.core.self_join_pass(&seams).await.unwrap();
        assert!(pass.joined.is_empty(), "pass {n}: {pass:?}");
        assert_eq!(
            pass.parked,
            [(branch.clone(), head.clone())],
            "pass {n}: {pass:?}"
        );
        assert_eq!(cards(&r.core).len(), 1, "pass {n}: one card");
        assert_eq!(r.upstream_main(), base, "pass {n}: main at the remote");
        assert_eq!(
            remote_heads(&r),
            "refs/heads/main",
            "pass {n}: no branch at the remote"
        );
    }
    assert!(w.traced(), "the loop's git was traced");
    assert_eq!(
        w.sends(),
        Vec::<String>::new(),
        "no git command sent anything"
    );
    assert_eq!(
        w.received(),
        Vec::<String>::new(),
        "the remote received nothing"
    );
    assert_eq!(publish_stops(&r).len(), 1, "asked once");
    let card = &cards(&r.core)[0];
    for want in [
        "Trim the sails before the wind.",
        branch.as_str(),
        head.as_str(),
        "1 commit(s), 2 file(s)",
        &format!("log --reverse --format=fuller --patch --stat {base}..{head}"),
        "Nothing of it has left this machine",
    ] {
        assert!(card.reason.contains(want), "{want}: {}", card.reason);
    }
    assert!(!card.needs_proof, "the card alone: no authenticator code");
}

/// The owner's yes publishes that head and only it: one push, of main, holding the branch's head.
#[tokio::test]
async fn a_yes_publishes_that_head_and_only_that_head() {
    let r = rig(SelfMode::Act);
    let w = watch(&r, &r.core);
    let (branch, head) = r.seed("trim-the-sails", PROVED, SONNET);
    let base = r.upstream_main();
    let seams = seams(&r);
    r.core.self_join_pass(&seams).await.unwrap();
    let q = cards(&r.core).pop().expect("the card").correlation_id;
    r.core.confirm_action(&q, true, None, "cli").unwrap();
    assert_eq!(r.core.publish_answer(&q).unwrap(), Answer::Yes);
    assert_eq!(
        w.received(),
        Vec::<String>::new(),
        "the answer sends nothing itself"
    );
    let pass = r.core.self_join_pass(&seams).await.unwrap();
    let merge = r.upstream_main();
    assert_eq!(pass.joined, [(branch.clone(), merge.clone())], "{pass:?}");
    let parents = git(&r.f.upstream, &["rev-list", "--parents", "-n", "1", &merge]);
    assert_eq!(parents, format!("{merge} {base} {head}"));
    assert_eq!(
        remote_heads(&r),
        "refs/heads/main",
        "no branch at the remote"
    );
    let got = w.received();
    assert_eq!(got.len(), 1, "one ref sent: {got:?}");
    assert_eq!(got[0], format!("{base} {merge} refs/heads/main"));
    let pushes: Vec<String> = (w.sends().into_iter())
        .filter(|l| l.contains(" push "))
        .collect();
    assert_eq!(pushes.len(), 1, "{pushes:?}");
    assert!(
        pushes[0].contains(&format!("{merge}:refs/heads/main")),
        "{pushes:?}"
    );
    assert!(cards(&r.core).is_empty(), "the card is answered");
}

/// A new head of the branch after the yes: the yes is the old head's, so the new one is asked again and nothing is
/// sent; the old card is no longer waiting.
#[tokio::test]
async fn a_moved_head_voids_the_yes() {
    let r = rig(SelfMode::Act);
    let w = watch(&r, &r.core);
    let (branch, head) = r.seed("trim-the-sails", PROVED, SONNET);
    let base = r.upstream_main();
    let seams = seams(&r);
    r.core.self_join_pass(&seams).await.unwrap();
    let q = cards(&r.core).pop().expect("the card").correlation_id;
    r.core.confirm_action(&q, true, None, "cli").unwrap();
    // The branch moves on: a commit more, built green, as a later attempt leaves it.
    let wt = r.f.upstream.parent().unwrap().join("moved");
    let repo = &r.f.place.repo;
    git(
        repo,
        &["worktree", "add", "-q", wt.to_str().unwrap(), &branch],
    );
    std::fs::write(wt.join("more.txt"), "more\n").unwrap();
    git(&wt, &["add", "-A"]);
    git(&wt, &["commit", "-q", "-m", "self: more (trim-the-sails)"]);
    let moved = git(&wt, &["rev-parse", "HEAD"]);
    git(
        repo,
        &["worktree", "remove", "--force", wt.to_str().unwrap()],
    );
    r.core.rec(None).record(&SelfBranchBuilt {
        item: "trim-the-sails",
        issue: "theseus-sail1",
        branch: &branch,
        head: Some(&moved),
        gate: "green",
        report: None,
        cost_usd: 1.0,
        stopped_at: None,
        why: "Trim the sails before the wind.",
        numbers: Value::Null,
    });
    assert_ne!(moved, head);
    let pass = r.core.self_join_pass(&seams).await.unwrap();
    assert!(pass.joined.is_empty(), "{pass:?}");
    assert_eq!(pass.parked, [(branch.clone(), moved.clone())], "{pass:?}");
    assert_eq!(r.upstream_main(), base);
    assert_eq!(w.received(), Vec::<String>::new());
    let waiting = cards(&r.core);
    assert_eq!(waiting.len(), 1, "one card, for the new head");
    assert!(waiting[0].reason.contains(&moved), "{}", waiting[0].reason);
    assert_ne!(waiting[0].correlation_id, q);
}

/// The owner's no holds the head: nothing is sent, and nothing asks again, on the next pass or after a restart.
#[tokio::test]
async fn a_no_holds_and_is_never_asked_again() {
    let r = rig(SelfMode::Act);
    let w = watch(&r, &r.core);
    let (branch, head) = r.seed("trim-the-sails", PROVED, SONNET);
    let base = r.upstream_main();
    let seams = seams(&r);
    r.core.self_join_pass(&seams).await.unwrap();
    let q = cards(&r.core).pop().expect("the card").correlation_id;
    r.core
        .confirm_action(&q, false, Some("not this one"), "cli")
        .unwrap();
    assert_eq!(r.core.publish_answer(&q).unwrap(), Answer::No);
    let held: Vec<Value> = (publish_stops(&r).into_iter())
        .filter(|d| d["numbers"]["held"] == true)
        .collect();
    assert_eq!(held.len(), 1, "{held:?}");
    assert_eq!(
        (held[0]["branch"].as_str(), held[0]["head"].as_str()),
        (Some(branch.as_str()), Some(head.as_str()))
    );
    let again = r.core.self_join_pass(&seams).await.unwrap();
    assert!(
        again.joined.is_empty() && again.parked.is_empty(),
        "{again:?}"
    );
    assert!(cards(&r.core).is_empty(), "no new card");
    let core = r.restarted();
    let w2 = watch(&r, &core);
    let after = core.self_join_pass(&seams).await.unwrap();
    assert!(
        after.joined.is_empty() && after.parked.is_empty() && after.reviewed.is_empty(),
        "{after:?}"
    );
    assert!(cards(&core).is_empty(), "no new card after a restart");
    assert_eq!(publish_stops(&r).len(), 2, "the one ask and the one no");
    assert_eq!(r.upstream_main(), base);
    assert_eq!((w.received(), w2.received()), (vec![], vec![]));
}

/// No answer waits: the questions' expiry passes over the card, and after a restart the same card is the one that
/// waits, nothing is sent, and the owner's yes on the new core still publishes.
#[tokio::test]
async fn no_answer_survives_a_restart_and_still_sends_nothing() {
    let r = rig(SelfMode::Act);
    let w = watch(&r, &r.core);
    let (branch, head) = r.seed("trim-the-sails", PROVED, SONNET);
    let base = r.upstream_main();
    let seams = seams(&r);
    r.core.self_join_pass(&seams).await.unwrap();
    let q = cards(&r.core).pop().expect("the card").correlation_id;
    assert_eq!(
        r.core.expire_questions(u64::MAX / 2),
        0,
        "nothing expires it"
    );
    assert_eq!(r.core.publish_answer(&q).unwrap(), Answer::Waiting);
    let core = r.restarted();
    let w2 = watch(&r, &core);
    let pass = core.self_join_pass(&seams).await.unwrap();
    assert_eq!(pass.parked, [(branch.clone(), head.clone())], "{pass:?}");
    let waiting = cards(&core);
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].correlation_id, q, "the same card, not a new one");
    assert_eq!(r.upstream_main(), base);
    assert_eq!((w.received(), w2.received()), (vec![], vec![]));
    core.confirm_action(&q, true, None, "cli").unwrap();
    let pass = core.self_join_pass(&seams).await.unwrap();
    assert_eq!(pass.joined.len(), 1, "{pass:?}");
    assert_eq!(w2.received().len(), 1);
}

/// While one join waits for the owner, the pass goes on: a second accepted branch is reviewed, and does not join
/// before the first (one join a pass, oldest first).
#[tokio::test]
async fn another_branch_is_reviewed_while_a_join_is_parked() {
    let r = rig(SelfMode::Act);
    let w = watch(&r, &r.core);
    let (first, head) = r.seed("trim-the-sails", PROVED, SONNET);
    let (second, _) = r.seed(
        "reef-the-main",
        &[
            ("reef.txt", "reefed\n"),
            ("tests/reef.sh", "test -f reef.txt\n"),
        ],
        SONNET,
    );
    let seams = seams(&r);
    let pass = r.core.self_join_pass(&seams).await.unwrap();
    assert_eq!(pass.parked, [(first.clone(), head)], "{pass:?}");
    let reviewed: Vec<&str> = pass.reviewed.iter().map(|(b, _)| b.as_str()).collect();
    assert_eq!(reviewed, [first.as_str(), second.as_str()], "{pass:?}");
    assert!(pass.joined.is_empty(), "{pass:?}");
    assert_eq!(
        cards(&r.core).len(),
        1,
        "the second waits behind the first, unasked"
    );
    assert_eq!(w.received(), Vec::<String>::new());
}

/// An agent's process is not the owner: its yes is refused and the card still waits.
#[tokio::test]
async fn an_agents_yes_is_refused() {
    let r = rig(SelfMode::Act);
    r.seed("trim-the-sails", PROVED, SONNET);
    let seams = seams(&r);
    r.core.self_join_pass(&seams).await.unwrap();
    let q = cards(&r.core).pop().expect("the card").correlation_id;
    let mut agent = crate::approval::Answerer::from("cli");
    agent.agent = Some("sess-agent".into());
    let e = r.core.confirm_action(&q, true, None, agent).unwrap_err();
    assert!(format!("{e:#}").contains("only the owner"), "{e:#}");
    assert_eq!(r.core.publish_answer(&q).unwrap(), Answer::Waiting);
}
