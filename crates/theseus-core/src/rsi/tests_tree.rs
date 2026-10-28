//! `rsi::tree` (theseus-pw1q.8) over a throwaway repository: an upstream
//! (bare, the remote) with a tiny stand-in `scripts/gate.sh` on main, and a
//! clone of it as the repo. The stand-in fails `broken` trees, prints the
//! keel's words for `keel` trees, and records the tree and build tree it ran
//! for. Every git call is hermetic: no global or system config is read.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::tree::{self, Place, Refused, Verdict};
use crate::scrub::Scrubber;

/// main's gate: judges the tree it is given, never its own directory.
const GATE: &str = r#"#!/bin/bash
cd "${THESEUS_GATE_ROOT:-$(dirname "$0")/..}"
echo "stand-in gate: root $PWD target $CARGO_TARGET_DIR nobench $THESEUS_GATE_NO_BENCH session ${THESEUS_SESSION:-none}"
if [ -f keel.txt ]; then echo "gate: FAILED in keel"; exit 1; fi
if [ -f broken.txt ]; then echo "gate: FAILED in suite"; exit 1; fi
echo "gate: ok"
"#;

/// A planted weakened gate: what a tree could write over its own copy.
const WEAK: &str = "#!/bin/bash\necho 'gate: ok'\nexit 0\n";

pub(crate) fn hermetic() -> Vec<(OsString, OsString)> {
    [
        ("GIT_CONFIG_GLOBAL", "/dev/null"),
        ("GIT_CONFIG_NOSYSTEM", "1"),
        ("GIT_AUTHOR_NAME", "Fixture"),
        ("GIT_AUTHOR_EMAIL", "fixture@example.invalid"),
        ("GIT_COMMITTER_NAME", "Fixture"),
        ("GIT_COMMITTER_EMAIL", "fixture@example.invalid"),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), v.into()))
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

/// The upstream, the repo cloned from it, and a state dir apart from both.
pub(crate) struct Fixture {
    pub(crate) place: Place,
    pub(crate) upstream: PathBuf,
    _dir: tempfile::TempDir,
}

pub(crate) fn fixture() -> Fixture {
    fixture_with(GATE)
}

pub(super) fn fixture_with(gate: &str) -> Fixture {
    fixture_full(gate, None)
}

/// `fixture_with`, and main's `scripts/may-build.sh` when one is given.
pub(super) fn fixture_full(gate: &str, may: Option<&str>) -> Fixture {
    let dir = crate::scratch::tempdir();
    let root = dir.path().canonicalize().unwrap();
    let seed = root.join("seed");
    std::fs::create_dir_all(seed.join("scripts")).unwrap();
    std::fs::write(seed.join("scripts/gate.sh"), gate).unwrap();
    if let Some(m) = may {
        std::fs::write(seed.join("scripts/may-build.sh"), m).unwrap();
    }
    std::fs::write(seed.join("README.md"), "the ship\n").unwrap();
    std::fs::write(seed.join(".gitignore"), "target\n").unwrap();
    git(&seed, &["init", "-q", "-b", "main"]);
    git(&seed, &["add", "-A"]);
    git(&seed, &["commit", "-q", "-m", "seed"]);
    let upstream = root.join("upstream.git");
    git(
        &root,
        &["clone", "-q", "--bare", "seed", upstream.to_str().unwrap()],
    );
    git(&root, &["clone", "-q", "upstream.git", "repo"]);
    let state = root.join("state");
    std::fs::create_dir_all(&state).unwrap();
    Fixture {
        place: Place {
            repo: root.join("repo"),
            state,
            env: hermetic(),
            jail: None,
            notify: None,
            stop: None,
        },
        upstream,
        _dir: dir,
    }
}

const NOW: u64 = 1_792_000_000_000;

/// The tree is outside the repo's own checkout, on a new self branch off
/// origin/main, with a build tree of its own under the state dir; and the
/// repo's own work tree is untouched.
#[test]
fn a_tree_is_apart_from_the_repo_with_its_own_target() {
    let f = fixture();
    let t = tree::open(&f.place, "Fix the mast!", NOW).unwrap();
    let repo = f.place.repo.canonicalize().unwrap();
    assert_eq!(t.slug, "fix-the-mast");
    assert!(t.branch.starts_with("self/") && t.branch.ends_with("-fix-the-mast"));
    assert!(!t.dir.starts_with(&repo), "{t:?} is inside the repo");
    assert_eq!(
        t.dir,
        f.place.trees().canonicalize().unwrap().join("fix-the-mast")
    );
    assert_eq!(t.target, f.place.targets().join("fix-the-mast"));
    assert!(t.target.is_dir() && !t.target.starts_with(&repo) && !t.target.starts_with(&t.dir));
    assert_eq!(
        git(&t.dir, &["rev-parse", "HEAD"]),
        git(&repo, &["rev-parse", "origin/main"])
    );
    assert_eq!(git(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]), "main");
    assert_eq!(git(&repo, &["status", "--porcelain"]), "");
    assert_eq!(tree::find(&f.place, "Fix the mast!"), Some(t.clone()));
    assert!(
        tree::open(&f.place, "fix the mast", NOW).is_err(),
        "one tree an item"
    );
    // A second tree for the same name a day takes the next branch name.
    tree::close(&f.place, &t).unwrap();
    let t2 = tree::open(&f.place, "fix the mast", NOW).unwrap();
    assert_eq!(t2.branch, format!("{}-2", t.branch));
}

/// Trees opened at once on one repo, as the self pipeline's builds start
/// (theseus-pw1q.16): each opens, on its own branch. Unserialized, two `git
/// worktree add`s raced for the repo's config lock and one failed half done.
#[test]
fn trees_opened_at_once_each_open() {
    let f = fixture();
    for round in 0..4 {
        let items: Vec<String> = (0..6).map(|i| format!("mast {round} {i}")).collect();
        let opened: Vec<_> = std::thread::scope(|s| {
            let hs: Vec<_> = items
                .iter()
                .map(|i| s.spawn(|| tree::open(&f.place, i, NOW)))
                .collect();
            hs.into_iter().map(|h| h.join().unwrap()).collect()
        });
        for (item, t) in items.iter().zip(opened) {
            let t = t.unwrap_or_else(|e| panic!("{item}: {e:#}"));
            assert_eq!(
                git(&t.dir, &["rev-parse", "--abbrev-ref", "HEAD"]),
                t.branch
            );
        }
    }
}

/// A state dir inside the repo's checkout would put the trees inside it:
/// refused before a worktree is added.
#[test]
fn trees_inside_the_repo_are_refused() {
    let mut f = fixture();
    f.place.state = f.place.repo.join("state");
    let e = tree::open(&f.place, "fix the mast", NOW).unwrap_err();
    assert!(format!("{e:#}").contains("inside the repo"), "{e:#}");
    let repo = f.place.repo.canonicalize().unwrap();
    assert_eq!(git(&repo, &["worktree", "list"]).lines().count(), 1);
}

/// The gate that judges is main's copy, run on the tree: a tree that writes
/// a weakened gate.sh over its own is still judged red by main's.
#[tokio::test]
async fn the_gate_is_mains_copy_even_when_the_tree_edits_its_own() {
    let f = fixture();
    let t = tree::open(&f.place, "weak gate", NOW).unwrap();
    std::fs::write(t.dir.join("scripts/gate.sh"), WEAK).unwrap();
    std::fs::write(t.dir.join("broken.txt"), "a failing change\n").unwrap();
    tree::commit(&f.place, &t, "self: weaken the gate (weak-gate)", "body")
        .unwrap()
        .unwrap();
    let run = tree::gate(&f.place, &t).await.unwrap();
    assert_eq!(run.verdict, Verdict::Red, "{}", run.tail);
    let log = std::fs::read_to_string(&run.log).unwrap();
    assert!(log.contains("gate: FAILED in suite"), "{log}");
    let blob = git(&f.place.repo, &["rev-parse", "origin/main:scripts/gate.sh"]);
    assert_eq!(run.blob, blob);
    assert!(!run.log.starts_with(&t.dir), "the log outlives the tree");
    // It ran on the tree, with the tree's own target, no bench, and none of
    // the daemon's own variables.
    assert!(
        log.contains(&format!(
            "root {} target {} nobench 1 session none",
            t.dir.display(),
            t.target.display()
        )),
        "{log}"
    );
    // Its build tree is reached at target/, as the benches look for it.
    assert_eq!(std::fs::read_link(t.dir.join("target")).unwrap(), t.target);
    // Fixed, the same tree is green.
    std::fs::remove_file(t.dir.join("broken.txt")).unwrap();
    tree::commit(&f.place, &t, "self: fix (weak-gate)", "body")
        .unwrap()
        .unwrap();
    assert_eq!(
        tree::gate(&f.place, &t).await.unwrap().verdict,
        Verdict::Green
    );
}

/// The keel's failure is its own verdict.
#[tokio::test]
async fn a_keel_failure_is_the_keels_verdict() {
    let f = fixture();
    let t = tree::open(&f.place, "loosen", NOW).unwrap();
    std::fs::write(t.dir.join("keel.txt"), "a loosened test\n").unwrap();
    tree::commit(&f.place, &t, "self: loosen (loosen)", "body").unwrap();
    assert_eq!(
        tree::gate(&f.place, &t).await.unwrap().verdict,
        Verdict::Keel
    );
}

/// A green branch pushes to refs/heads/self/* on the remote, and nothing
/// else moves there; main is refused by name, and so is any name outside
/// self/.
#[test]
fn a_push_goes_to_self_only_and_main_is_refused() {
    let f = fixture();
    let t = tree::open(&f.place, "trim the sails", NOW).unwrap();
    std::fs::write(t.dir.join("sails.txt"), "trimmed\n").unwrap();
    let head = tree::commit(&f.place, &t, "self: trim (trim)", "body")
        .unwrap()
        .unwrap();
    let clean = Scrubber::default();
    let scrub = |s: &str| clean.scrub(s).1;
    let main_before = git(&f.upstream, &["rev-parse", "refs/heads/main"]);
    for bad in [
        "main",
        "refs/heads/main",
        "self/",
        "self/../main",
        "+self/x",
        "self/x:main",
    ] {
        assert!(
            matches!(
                tree::push(&f.place, &t, "origin", bad, &scrub),
                Err(Refused::NotSelf(_))
            ),
            "{bad}"
        );
    }
    assert_eq!(
        git(&f.upstream, &["rev-parse", "refs/heads/main"]),
        main_before
    );
    let spec = tree::push(&f.place, &t, "origin", &t.branch, &scrub).unwrap();
    assert_eq!(spec, format!("refs/heads/{0}:refs/heads/{0}", t.branch));
    assert_eq!(
        git(
            &f.upstream,
            &["rev-parse", &format!("refs/heads/{}", t.branch)]
        ),
        head
    );
    assert_eq!(
        git(&f.upstream, &["rev-parse", "refs/heads/main"]),
        main_before
    );
    assert_eq!(
        git(&f.upstream, &["for-each-ref", "--format=%(refname)"]),
        format!("refs/heads/main\nrefs/heads/{}", t.branch)
    );
}

/// A scrubber that holds `token` as the vault's resolved secret `deploy_key`,
/// fed the way the daemon's is: through the secret board.
fn knowing(token: &str) -> Scrubber {
    use crate::secrets::{Secret, SecretBoard};
    let board = SecretBoard::new(["deploy_key".to_string()], std::time::Instant::now());
    board.publish(
        [("deploy_key".to_string(), Ok(Secret::new(token.into())))].into(),
        "fake",
    );
    Scrubber::from_board(board)
}

/// A value the vault holds, in what the branch adds or in a commit message,
/// stops the push; nothing reaches the remote. (The daemon's scrubber guesses
/// no shapes now: a token it was never given is `scripts/scrub.sh`'s to find,
/// which main runs after it.)
#[test]
fn a_vault_value_in_the_tree_stops_the_push() {
    let f = fixture();
    let token = format!("ghp_{}", "Zq7RkV2mW9xLpT4sNd8YbHc3FgJe6UaQo1Ki");
    let known = knowing(&token);
    let scrub = |s: &str| known.scrub(s).1;
    for (slug, file, msg) in [
        (
            "in a file",
            format!("token = \"{token}\"\n"),
            "self: add (x)".to_string(),
        ),
        (
            "in a message",
            "fine\n".to_string(),
            format!("self: add {token} (x)"),
        ),
    ] {
        let t = tree::open(&f.place, slug, NOW).unwrap();
        std::fs::write(t.dir.join("cfg.txt"), file).unwrap();
        tree::commit(&f.place, &t, &msg, "body").unwrap().unwrap();
        let r = tree::push(&f.place, &t, "origin", &t.branch, &scrub);
        assert!(matches!(r, Err(Refused::Scrub { .. })), "{slug}: {r:?}");
        let up = git(&f.upstream, &["for-each-ref", "--format=%(refname)"]);
        assert_eq!(up, "refs/heads/main", "{slug}");
    }
}

/// A token the vault never gave the scrubber is not guessed at: the same
/// tree, with a scrubber that does not know the value, pushes.
#[test]
fn a_token_the_scrubber_was_never_given_is_not_guessed() {
    let f = fixture();
    let token = format!("ghp_{}", "Zq7RkV2mW9xLpT4sNd8YbHc3FgJe6UaQo1Ki");
    let known = knowing("some-other-secret-value");
    let scrub = |s: &str| known.scrub(s).1;
    let t = tree::open(&f.place, "unknown token", NOW).unwrap();
    std::fs::write(t.dir.join("cfg.txt"), format!("token = \"{token}\"\n")).unwrap();
    tree::commit(&f.place, &t, "self: add (x)", "body")
        .unwrap()
        .unwrap();
    assert!(tree::push(&f.place, &t, "origin", &t.branch, &scrub).is_ok());
}

/// Main's own scrub check, when main has one, stops a push too.
#[test]
fn mains_scrub_script_stops_the_push() {
    let f = fixture();
    let seed = f.place.repo.parent().unwrap().join("seed");
    std::fs::write(
        seed.join("scripts/scrub.sh"),
        "#!/bin/bash\n! grep -rq 'Captain Invented' \"$1\" --exclude-dir=.git\n",
    )
    .unwrap();
    git(&seed, &["add", "-A"]);
    git(&seed, &["commit", "-q", "-m", "scrub"]);
    git(&seed, &["push", "-q", f.upstream.to_str().unwrap(), "main"]);
    git(&f.place.repo, &["fetch", "-q", "origin"]);
    let t = tree::open(&f.place, "a name", NOW).unwrap();
    std::fs::write(t.dir.join("crew.txt"), "Captain Invented\n").unwrap();
    tree::commit(&f.place, &t, "self: crew (a-name)", "body").unwrap();
    let none = |_: &str| 0;
    let r = tree::push(&f.place, &t, "origin", &t.branch, &none);
    assert!(
        matches!(&r, Err(Refused::Scrub { by, .. }) if by.contains("scrub.sh")),
        "{r:?}"
    );
}

/// Close removes the tree and its build tree; the branch stays.
#[tokio::test]
async fn close_removes_the_tree_and_its_target_and_keeps_the_branch() {
    let f = fixture();
    let t = tree::open(&f.place, "close me", NOW).unwrap();
    std::fs::write(t.dir.join("a.txt"), "a\n").unwrap();
    let head = tree::commit(&f.place, &t, "self: a (close-me)", "b")
        .unwrap()
        .unwrap();
    tree::gate(&f.place, &t).await.unwrap();
    std::fs::write(t.target.join("built"), "x").unwrap();
    tree::close(&f.place, &t).unwrap();
    assert!(!t.dir.exists() && !t.target.exists());
    assert_eq!(tree::find(&f.place, "close me"), None);
    assert_eq!(
        tree::branches(&f.place).unwrap(),
        vec![(t.branch.clone(), head)]
    );
}

/// A branch that changes an exam's file is never pushed: an exam item or
/// the exam crate's data (the proposer rule of theseus-pw1q.11, a join fix).
#[test]
fn a_push_that_touches_the_exams_is_refused() {
    let f = fixture();
    let clean = Scrubber::default();
    let scrub = |s: &str| clean.scrub(s).1;
    let t = tree::open(&f.place, "loosen the exam", NOW).unwrap();
    std::fs::create_dir_all(t.dir.join("crates/theseus-exam/exam")).unwrap();
    std::fs::write(
        t.dir.join("crates/theseus-exam/exam/exam-v2.toml"),
        "easier\n",
    )
    .unwrap();
    tree::commit(&f.place, &t, "self: loosen (x)", "body")
        .unwrap()
        .unwrap();
    let r = tree::push(&f.place, &t, "origin", &t.branch, &scrub);
    assert!(matches!(r, Err(Refused::Exams(_))), "{r:?}");
    let up = git(&f.upstream, &["for-each-ref", "--format=%(refname)"]);
    assert_eq!(up, "refs/heads/main");
}

/// A stand-in may-build rule that says no `no` times, with a reason on its
/// last line, then yes. Its counter lives beside the fixture, not in the tree.
fn may_script(counter: &Path, no: u32) -> String {
    format!(
        "#!/bin/bash\nn=$(cat {c} 2>/dev/null || echo 0); n=$((n+1)); echo $n > {c}\n\
         if [ $n -le {no} ]; then echo 'checking'; echo 'three trees compiling (round '$n')'; exit 1; fi\n\
         exit 0\n",
        c = counter.display()
    )
}

type Told = std::sync::Arc<std::sync::Mutex<Vec<String>>>;

fn listening(p: &Place) -> (Place, Told) {
    let told: Told = Told::default();
    let sink = told.clone();
    let mut p = p.clone();
    p.notify = Some(tree::Notify::new(move |t| sink.lock().unwrap().push(t)));
    (p, told)
}

/// Run `may_build_paced` for `no` refusals on a simulated clock.
async fn waited(no: u32, sink: bool) -> (anyhow::Result<()>, Vec<String>) {
    let f = fixture_full(GATE, Some("#!/bin/bash\n"));
    let counter = f.place.state.join("count");
    let (mut place, told) = listening(&f.place);
    if !sink {
        place.notify = None;
    }
    let t = tree::open(&place, "wait item", NOW).unwrap();
    let script = f.place.state.join("may.sh");
    std::fs::write(&script, may_script(&counter, no)).unwrap();
    let r = tree::may_build_paced(
        &place,
        &t,
        &script,
        std::time::Duration::from_secs(60),
        tree::MAY_BUILD_WAIT,
        tree::Clock::simulated(),
    )
    .await
    .map(|w| assert_eq!(w, tree::Waited::Yes));
    let told = told.lock().unwrap().clone();
    (r, told)
}

/// A no from the rule for 90 minutes is a wait, not a failure: the item is
/// told twice (at 30 and 60 minutes, never once a minute) and the gate then
/// starts (the owner, 2026-10-25 13:58: "i want there to be precious few hard
/// gates /anywhere/ in theseus").
#[tokio::test]
async fn a_long_no_from_may_build_is_a_wait_with_one_notice_a_window() {
    let (r, told) = waited(90, true).await;
    r.expect("the wait must not fail the item");
    assert_eq!(told.len(), 2, "{told:#?}");
}

/// The notice carries the rule's last words, the item and the wait.
#[tokio::test]
async fn the_wait_notice_carries_the_rules_reason() {
    let (r, told) = waited(40, true).await;
    r.unwrap();
    assert_eq!(told.len(), 1, "{told:#?}");
    assert!(
        told[0].contains("three trees compiling (round"),
        "{}",
        told[0]
    );
    assert!(told[0].contains("wait-item"), "{}", told[0]);
    assert!(told[0].contains("waited 30 minutes"), "{}", told[0]);
    assert!(
        !told[0].contains("checking"),
        "only the last line: {}",
        told[0]
    );
    let none = super::tree::wait_notice_for_test("t", "");
    assert!(none.contains("gave no reason"), "{none}");
}

/// A place with no sink still waits and still gates.
#[tokio::test]
async fn a_place_with_no_sink_still_waits_without_error() {
    let (r, told) = waited(40, false).await;
    r.unwrap();
    assert!(told.is_empty());
}

/// A gate stopped at its limit names the phase its log was last in; a log
/// with no phase line says so.
#[tokio::test]
async fn a_stopped_gate_names_its_phase() {
    let f = fixture();
    let t = tree::open(&f.place, "slow", NOW).unwrap();
    let log = f.place.state.join("slow.log");
    for (body, want) in [
        (
            "echo 'gate: phase suite'; exec sleep 30",
            "gate: phase suite",
        ),
        ("exec sleep 30", "no `gate: ` line"),
    ] {
        let script = f.place.state.join("slow.sh");
        std::fs::write(&script, format!("#!/bin/bash\n{body}\n")).unwrap();
        let e = tree::run_niced(
            &f.place,
            &t,
            &script,
            &log,
            tree::Bound::Silence(std::time::Duration::from_millis(700)),
            false,
        )
        .await
        .unwrap_err();
        assert!(format!("{e:#}").contains(want), "{e:#}");
    }
}

/// A gate that keeps printing is not stopped at its limit: the limit is on
/// silence, and the ceiling is far past it (theseus-x1zz8).
#[tokio::test]
async fn a_gate_still_printing_past_its_limit_runs_to_its_end() {
    let f = fixture();
    let t = tree::open(&f.place, "busy", NOW).unwrap();
    let log = f.place.state.join("busy.log");
    let script = f.place.state.join("busy.sh");
    std::fs::write(
        &script,
        "#!/bin/bash\nfor i in 1 2 3 4 5 6 7 8 9 10 11 12; do echo \"gate: phase $i\"; sleep 0.1; done\n",
    )
    .unwrap();
    let code = tree::run_niced(
        &f.place,
        &t,
        &script,
        &log,
        tree::Bound::Silence(std::time::Duration::from_millis(700)),
        false,
    )
    .await
    .unwrap();
    assert_eq!(code, Some(0));
}

/// The same script under `Bound::Clock` IS stopped at its limit, and says the
/// clock rather than silence. This is the may-build rule's bound: a predicate
/// that prints must not be able to hold its caller's loop - and so a stop
/// aimed at the build - open for 24 times its window (theseus-x1zz8, join).
#[tokio::test]
async fn a_printing_predicate_under_a_clock_bound_is_still_stopped_at_its_limit() {
    let f = fixture();
    let t = tree::open(&f.place, "rule", NOW).unwrap();
    let log = f.place.state.join("rule.log");
    let script = f.place.state.join("rule.sh");
    std::fs::write(
        &script,
        "#!/bin/bash\nfor i in 1 2 3 4 5 6 7 8 9 10 11 12; do echo \"gate: phase $i\"; sleep 0.1; done\n",
    )
    .unwrap();
    let e = tree::run_niced(
        &f.place,
        &t,
        &script,
        &log,
        tree::Bound::Clock(std::time::Duration::from_millis(400)),
        false,
    )
    .await
    .unwrap_err();
    let said = format!("{e:#}");
    assert!(said.contains("ran for its whole"), "{said}");
    assert!(!said.contains("printed nothing"), "{said}");
}

/// The two-hour backstop is generous and stays where it is.
#[test]
fn the_gate_limit_is_still_two_hours() {
    assert_eq!(tree::GATE_SILENCE, std::time::Duration::from_secs(2 * 3600));
}

/// Advance the upstream's main behind the repo's back: a commit that rewrites
/// `scripts/gate.sh` to `gate`, pushed from a scratch clone. The commit.
fn move_main(f: &Fixture, gate: &str) -> String {
    let root = f.upstream.parent().unwrap();
    let work = root.join(format!("mover-{}", gate.len()));
    git(
        root,
        &[
            "clone",
            "-q",
            f.upstream.to_str().unwrap(),
            work.to_str().unwrap(),
        ],
    );
    std::fs::write(work.join("scripts/gate.sh"), gate).unwrap();
    git(&work, &["add", "-A"]);
    git(&work, &["commit", "-q", "-m", "main moves"]);
    git(&work, &["push", "-q", "origin", "HEAD:main"]);
    git(&work, &["rev-parse", "HEAD"])
}

const NEWER_GATE: &str = "#!/bin/bash\necho 'gate: ok (newer)'\nexit 0\n";

/// The door sees main move: a tree cut after the upstream advanced is cut
/// from the moved main, not from the frozen ref (theseus-g5wxc).
#[test]
fn a_moved_main_is_visible_to_the_next_cut() {
    let f = fixture();
    let moved = move_main(&f, NEWER_GATE);
    let t = tree::open(&f.place, "after the move", NOW).unwrap();
    assert_eq!(git(&t.dir, &["rev-parse", "HEAD"]), moved);
    assert_eq!(
        git(&f.place.repo, &["rev-parse", "origin/main"]),
        moved,
        "the repo's view of main advanced"
    );
}

/// The gate blob follows main: the script the door writes out for the next
/// cut is main's new one, by blob and by content (theseus-g5wxc).
#[tokio::test]
async fn the_gate_blob_follows_main() {
    let f = fixture();
    let old_blob = git(&f.place.repo, &["rev-parse", "origin/main:scripts/gate.sh"]);
    move_main(&f, NEWER_GATE);
    let want = git(&f.upstream, &["rev-parse", "main:scripts/gate.sh"]);
    assert_ne!(want, old_blob);
    let t = tree::open(&f.place, "new gate", NOW).unwrap();
    let run = tree::gate(&f.place, &t).await.unwrap();
    assert_eq!(run.blob, want, "judged by the old gate blob");
    let log = std::fs::read_to_string(&run.log).unwrap();
    assert!(log.contains("gate: ok (newer)"), "{log}");
}

/// A remote that cannot be reached does not fail the door: the tree is cut
/// from the base the repo has (theseus-g5wxc).
#[test]
fn a_failed_fetch_does_not_fail_the_door() {
    let f = fixture();
    let before = git(&f.place.repo, &["rev-parse", "origin/main"]);
    let gone = f.upstream.with_extension("gone");
    std::fs::rename(&f.upstream, &gone).unwrap();
    let t = tree::open(&f.place, "no network", NOW).expect("the door goes on");
    assert_eq!(git(&t.dir, &["rev-parse", "HEAD"]), before);
}

/// A fetch that fails is told to the owner, who would otherwise never learn the
/// tree was cut from a stale base (the second theseus-51sfw shape).
#[test]
fn a_failed_fetch_tells_the_owner_and_a_good_one_says_nothing() {
    let f = fixture();
    let (place, told) = listening(&f.place);
    tree::open(&place, "reachable", NOW).unwrap();
    assert!(
        told.lock().unwrap().is_empty(),
        "a fetch that works is not news"
    );
    let g = fixture();
    let (place, told) = listening(&g.place);
    std::fs::rename(&g.upstream, g.upstream.with_extension("gone")).unwrap();
    tree::open(&place, "unreachable", NOW).expect("the door goes on");
    let said = told.lock().unwrap().clone();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].contains("could not reach origin") && said[0].contains("may be behind main"),
        "{said:?}"
    );
}

/// One fetch a pass, not one an item: a second cut straight after the first
/// does not fetch again (the self backlog's night opens several in a loop).
#[test]
fn a_pass_fetches_once_not_once_an_item() {
    let f = fixture();
    let first = tree::open(&f.place, "first", NOW).unwrap();
    move_main(&f, NEWER_GATE);
    let second = tree::open(&f.place, "second", NOW).unwrap();
    assert_eq!(
        git(&second.dir, &["rev-parse", "HEAD"]),
        git(&first.dir, &["rev-parse", "HEAD"]),
        "the second cut of a pass reused the pass's fetch"
    );
}

/// `may_build_paced` against a rule that never says yes, a stop check that
/// answers from the number of its reads, and a simulated clock. The result,
/// the rule's runs and the stop's reads.
async fn waited_with_stop(
    stop_at: Option<u32>,
    no: u32,
) -> (anyhow::Result<tree::Waited>, u32, u32) {
    let f = fixture_full(GATE, Some("#!/bin/bash\n"));
    let counter = f.place.state.join("count");
    let reads = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let mut place = f.place.clone();
    if let Some(at) = stop_at {
        let seen = reads.clone();
        place.stop = Some(tree::Stop::new(move || {
            let n = seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            (n >= at).then(|| "the owner's stop".to_string())
        }));
    }
    let t = tree::open(&place, "stop item", NOW).unwrap();
    let script = f.place.state.join("may.sh");
    std::fs::write(&script, may_script(&counter, no)).unwrap();
    let r = tree::may_build_paced(
        &place,
        &t,
        &script,
        std::time::Duration::from_secs(60),
        tree::MAY_BUILD_WAIT,
        tree::Clock::simulated(),
    )
    .await;
    let runs = std::fs::read_to_string(&counter)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    (r, runs, reads.load(std::sync::atomic::Ordering::SeqCst))
}

/// A stop that lands in the middle of a wait ends it, as a stop and not as
/// the rule's yes, though the rule would have said no for another 1,000 asks.
#[tokio::test]
async fn a_stop_ends_a_wait_and_is_not_the_rules_yes() {
    let (r, runs, _) = waited_with_stop(Some(4), 1000).await;
    assert_eq!(r.unwrap(), tree::Waited::Stopped("the owner's stop".into()));
    assert_eq!(runs, 3, "the rule was asked on the ticks before the stop");
}

/// Noticed within one tick: the stop is read before each ask, so a stop that
/// lands after the third ask ends the wait before a fourth.
#[tokio::test]
async fn a_stop_is_noticed_within_one_tick() {
    let (r, runs, reads) = waited_with_stop(Some(3), 1000).await;
    assert!(matches!(r.unwrap(), tree::Waited::Stopped(_)));
    assert_eq!((runs, reads), (2, 3));
}

/// A wait with no stop check is as it was: it waits out the rule's no and
/// ends on its yes.
#[tokio::test]
async fn a_wait_with_no_stop_check_waits_for_the_rule() {
    let (r, runs, reads) = waited_with_stop(None, 90).await;
    assert_eq!(r.unwrap(), tree::Waited::Yes);
    assert_eq!((runs, reads), (91, 0));
}
