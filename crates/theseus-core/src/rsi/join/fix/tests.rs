//! The repair in place's parts (issue 36): which red is the change's own, the fixer's `FIX:` line,
//! each bound on a commit, the fixer's confinement, the brief's sentence, and the stand-in fixer
//! the loop's tests play a fix with ([`StandIn`]).

use std::path::Path;
use std::sync::{Arc, Mutex};

use super::bounds::{self, Change};
use super::{fix_of, own_work, ApproveAsk, FixAsk, FixFut, Fixed, Fixer};
use crate::rpc::Core;
use crate::rsi::join::review::{Fut, Review};
use crate::rsi::tree::{GateRun, Verdict};

/// A stand-in fixer: it writes `writes` into the tree (an empty text removes the file), runs
/// `git` there when asked, answers `done`, and approves with `approve`. The models that answer
/// are the ones each was asked for, as the review's stand-in's are. It keeps every ask.
pub(crate) struct StandIn {
    pub writes: Vec<(String, String)>,
    pub done: bool,
    pub git: Option<Vec<String>>,
    pub approve: bool,
    pub fixes: Mutex<Vec<FixAsk>>,
    pub approvals: Mutex<Vec<ApproveAsk>>,
}

impl StandIn {
    /// A fixer that writes `writes` and says done, and an approver that accepts.
    pub(crate) fn writing(writes: &[(&str, &str)]) -> Arc<Self> {
        Arc::new(Self {
            writes: writes
                .iter()
                .map(|(p, t)| ((*p).to_string(), (*t).to_string()))
                .collect(),
            done: true,
            git: None,
            approve: true,
            fixes: Mutex::default(),
            approvals: Mutex::default(),
        })
    }

    pub(crate) fn fixed(&self) -> usize {
        self.fixes.lock().unwrap().len()
    }

    pub(crate) fn approved(&self) -> usize {
        self.approvals.lock().unwrap().len()
    }
}

impl Fixer for StandIn {
    fn fix<'a>(&'a self, _core: &'a Arc<Core>, ask: &'a FixAsk) -> FixFut<'a> {
        self.fixes.lock().unwrap().push(ask.clone());
        for (path, text) in &self.writes {
            let p = Path::new(&ask.tree).join(path);
            if text.is_empty() {
                let _ = std::fs::remove_file(&p);
                continue;
            }
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, text).unwrap();
        }
        if let Some(args) = &self.git {
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            crate::rsi::tests_tree::git(Path::new(&ask.tree), &args);
        }
        let (profile, model) = (ask.target.profile.clone(), ask.target.model.clone());
        let done = self.done;
        Box::pin(async move {
            Ok(Fixed {
                done,
                why: match done {
                    true => "it repairs the fault and nothing else".into(),
                    false => "the fault is not a small one".into(),
                },
                cost_usd: 0.1,
                session: None,
                profile,
                model,
            })
        })
    }

    fn approve<'a>(&'a self, _core: &'a Arc<Core>, ask: &'a ApproveAsk) -> Fut<'a> {
        self.approvals.lock().unwrap().push(ask.clone());
        let (profile, model) = (ask.target.profile.clone(), ask.target.model.clone());
        let accept = self.approve;
        Box::pin(async move {
            Ok(Review {
                accept,
                fix: false,
                reasons: vec![match accept {
                    true => "it repairs the lint and nothing else".into(),
                    false => "it changes what the change is for".into(),
                }],
                cost_usd: 0.05,
                session: None,
                profile,
                model,
            })
        })
    }
}

fn run(tail: &str, exit: Option<i32>, secs: u64, verdict: Verdict) -> GateRun {
    GateRun {
        verdict,
        exit,
        blob: String::new(),
        log: "gate.log".into(),
        secs,
        tail: tail.into(),
        unrunnable: false,
    }
}

/// A red in the change's own work goes to a fixer; a keel stop, a machine fault, a check that
/// kept aborting, a phase that is not the change's, and a gate that named none never do.
#[test]
fn only_a_red_in_the_changes_own_work_is_a_fixers() {
    let red = |tail: &str| run(tail, Some(1), 300, Verdict::Red);
    for phase in ["fmt", "clippy", "test build", "suite", "shape"] {
        assert_eq!(
            own_work(&red(&format!("gate: FAILED in {phase}"))),
            Ok(phase.to_string())
        );
    }
    let not = |r: GateRun| own_work(&r).unwrap_err();
    assert!(not(run("gate: FAILED in keel", Some(1), 3, Verdict::Keel)).contains("keel"));
    assert!(not(run("gate: FAILED in suite", Some(127), 300, Verdict::Red)).contains("machine"));
    assert!(not(run("gate: FAILED in suite", Some(1), 0, Verdict::Red)).contains("machine"));
    assert!(not(run("gate: FAILED in suite", Some(75), 300, Verdict::Red)).contains("aborting"));
    assert!(not(red("gate: FAILED in disk floor")).contains("not the change's own work"));
    assert!(not(red("gate: FAILED in scrub")).contains("not the change's own work"));
    assert!(not(red("the build was still printing at its ceiling")).contains("no phase"));
}

#[test]
fn the_fix_line_is_read_as_the_reviews_verdict_is() {
    assert_eq!(
        fix_of("I renamed it.\nFIX: done: the unused variable is gone"),
        (true, "the unused variable is gone".to_string())
    );
    assert_eq!(
        fix_of("`FIX: none`\n- the fault is the design, not a line"),
        (false, "the fault is the design, not a line".to_string())
    );
    let (done, why) = fix_of("All better.");
    assert!(!done);
    assert_eq!(why, "the fixer gave no FIX line");
    let (done, why) = fix_of("FIX: maybe");
    assert!(!done && why.contains("neither done nor none"), "{why}");
}

fn change(files: &[(&str, u64, u64)], diff: &str) -> Change {
    Change {
        files: files
            .iter()
            .map(|(p, a, d)| ((*p).to_string(), *a, *d))
            .collect(),
        diff: diff.into(),
    }
}

#[test]
fn the_size_bound_counts_lines_both_ways_and_files() {
    let c = change(&[("a.rs", 30, 30)], "");
    assert_eq!(bounds::size(&c, 60, 6), None);
    let r = bounds::size(&change(&[("a.rs", 31, 30)], ""), 60, 6).unwrap();
    assert_eq!(r.bound, "size");
    assert!(r.why.contains("61 lines in 1 files"), "{}", r.why);
    let seven: Vec<(String, u64, u64)> = (0..7).map(|i| (format!("f{i}.rs"), 1, 0)).collect();
    let c = Change {
        files: seven,
        diff: String::new(),
    };
    assert_eq!(bounds::size(&c, 60, 6).unwrap().bound, "size");
}

/// The keel's own table and the loop's judge are out of a fix's reach.
#[test]
fn a_guarded_path_or_the_loops_own_judge_is_refused() {
    for path in [
        "scripts/gate.sh",
        "scripts/keel-guard.py",
        "crates/theseus-core/src/rsi/join/review.rs",
        "crates/theseus-core/src/rsi/join.rs",
        "crates/theseus-core/src/rsi/pipeline/queues.rs",
        "crates/theseus-core/src/policy.rs",
        "crates/theseus-sandbox/src/lib.rs",
    ] {
        let r = bounds::guarded(&change(&[(path, 1, 0)], "")).unwrap_or_else(|| panic!("{path}"));
        assert_eq!(r.bound, "guarded-path");
    }
    assert_eq!(
        bounds::guarded(&change(
            &[("crates/theseus-core/src/rsi/tree_fmt.rs", 1, 0)],
            ""
        )),
        None
    );
}

#[test]
fn a_test_file_is_known_by_its_name_or_folder() {
    for p in [
        "tests/fix.sh",
        "crates/a/src/tests.rs",
        "crates/a/src/rsi/tests_pr.rs",
        "crates/a/src/rsi/tests_pr/gated.rs",
        "scripts/test_keel_guard.py",
        "cockpit/src/board.test.ts",
    ] {
        assert!(bounds::is_test_file(p), "{p}");
    }
    for p in [
        "crates/a/src/turn.rs",
        "README.md",
        "crates/a/src/contest.rs",
    ] {
        assert!(!bounds::is_test_file(p), "{p}");
    }
}

/// The confinement the fixer's session gets is the builder's, read by the builder's own rule:
/// the item's tree while its turn runs, and nothing once it is over.
#[tokio::test]
async fn the_fixers_session_has_the_builders_tree_and_only_while_it_runs() {
    use theseus_protocol::rsi::SelfMode;
    let r = crate::rsi::tests_pr::rig_on(
        crate::rsi::tests_tree::fixture(),
        SelfMode::Act,
        None,
        |_| {},
    );
    let t = crate::rsi::tree::open(&r.f.place, "trim-the-sails", 1_792_000_000_000).unwrap();
    let sid = r.session("self");
    assert_eq!(crate::rsi::own_tree::of(&r.core.tools, &sid), None);
    super::turns::confine(&r.core, &sid, &t.slug);
    let own = crate::rsi::own_tree::of(&r.core.tools, &sid).unwrap();
    assert_eq!(own, t.dir);
    assert!(crate::rsi::own_tree::builder_path(&r.core.tools, &sid).is_some());
    super::turns::release(&r.core, &sid);
    assert_eq!(crate::rsi::own_tree::of(&r.core.tools, &sid), None);
    crate::rsi::tree::close(&r.f.place, &t).unwrap();
}

/// The builder is told, once, that a small fault may be repaired by another model, and that this
/// is no reason to skip its own tests.
#[test]
fn the_builder_is_told_a_fix_is_no_reason_to_skip_its_tests() {
    use crate::rsi::backlog::brief::{rules, rules_now};
    let mut cfg = crate::Config::example();
    let text = rules_now(&cfg, 3);
    assert!(
        text.ends_with(
            "A small fault your gate or your review finds may be repaired in place by another \
             model, which is no reason to skip running your own tests."
        ),
        "{text}"
    );
    // Off is what a builder was told before: the rules alone.
    cfg.self_fix.mode = crate::config::self_fix::FixMode::Off;
    assert_eq!(rules_now(&cfg, 3), rules(3));
    assert!(!rules(3).contains("repaired in place"));
}

/// The reviewer is offered `fix` with its bounds only while the setting is on and the head was not
/// fixed already; a fixed head is told what was fixed and offered none.
#[test]
fn the_review_offers_a_fix_only_when_one_may_be_made() {
    use crate::rsi::join::review::{prompt, Ask};
    let r = crate::rsi::join::tests::rig(theseus_protocol::rsi::SelfMode::Act);
    let live = r.core.live_profile().0;
    let target = r
        .core
        .runner
        .resolve_target(&live, Some("fable"), None, None)
        .unwrap();
    let ask = |offer: Option<(u64, u64)>, fixed: Option<&str>| Ask {
        branch: "self/20261014-trim".into(),
        head: "abc".into(),
        item: "trim".into(),
        diff: String::new(),
        report: String::new(),
        evidence: String::new(),
        target: target.clone(),
        built_ms: 0,
        scratch: "/tmp/s".into(),
        repo: "/tmp/r".into(),
        offer_fix: offer,
        fixed: fixed.map(str::to_string),
    };
    let off = prompt(&ask(None, None));
    assert!(off.contains("`VERDICT: accept` or `VERDICT: reject`"));
    assert!(!off.contains("VERDICT: fix"), "{off}");
    let on = prompt(&ask(Some((60, 6)), None));
    assert!(
        on.contains("`VERDICT: fix`") && on.contains("at most 60 lines"),
        "{on}"
    );
    let after = prompt(&ask(None, Some("commit abc by fable")));
    assert!(after.contains("commit abc by fable") && after.contains("not offered a second time"));
    assert!(!after.contains("`VERDICT: fix`"), "{after}");
}

/// A reviewer that answers the words it was given, in turn (`accept`, `reject` or `fix`), as the
/// model it was asked for, and keeps every ask.
pub(crate) struct ScriptedReview {
    words: Mutex<Vec<&'static str>>,
    pub asks: Mutex<Vec<crate::rsi::join::review::Ask>>,
}

pub(crate) fn scripted(words: &[&'static str]) -> Arc<ScriptedReview> {
    Arc::new(ScriptedReview {
        words: Mutex::new(words.iter().rev().copied().collect()),
        asks: Mutex::default(),
    })
}

impl crate::rsi::join::review::Reviewer for ScriptedReview {
    fn review<'a>(
        &'a self,
        _core: &'a Arc<Core>,
        ask: &'a crate::rsi::join::review::Ask,
    ) -> Fut<'a> {
        self.asks.lock().unwrap().push(ask.clone());
        let word = self.words.lock().unwrap().pop().unwrap_or("reject");
        let (profile, model) = (ask.target.profile.clone(), ask.target.model.clone());
        Box::pin(async move {
            Ok(Review {
                accept: word == "accept",
                fix: word == "fix",
                reasons: vec![format!(
                    "the stand-in says {word}: name the note's file plainly"
                )],
                cost_usd: 0.25,
                session: None,
                profile,
                model,
            })
        })
    }
}

/// A stand-in gpg for the throwaway repository `repo`, so a join's merge is signed without a key.
pub(crate) fn signing(repo: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let gpg = repo.parent().unwrap().join("fake-gpg.sh");
    std::fs::write(&gpg, crate::rsi::join::tests::FAKE_GPG).unwrap();
    std::fs::set_permissions(&gpg, std::fs::Permissions::from_mode(0o755)).unwrap();
    crate::rsi::tests_tree::git(repo, &["config", "gpg.program", gpg.to_str().unwrap()]);
    crate::rsi::tests_tree::git(repo, &["config", "user.signingkey", "FIXTUREKEY"]);
}

/// The week's digest counts fixes made, approved and refused, and says nothing of them in a week
/// with none.
#[test]
fn the_digest_counts_fixes_made_approved_and_refused() {
    use crate::rsi::digest::Outcomes;
    let row = |kind: &str, data: serde_json::Value| crate::ledger::LedgerRow {
        kind: kind.into(),
        data,
        at_unix_ms: 10,
        session_id: None,
        turn_id: None,
    };
    let none = Outcomes::of_rows(&[], 0, 100).lines(0.0, false);
    assert!(!none.contains("Fixed in place"), "{none}");
    let rows = [
        row("self.fix.made", serde_json::json!({})),
        row("self.fix.made", serde_json::json!({})),
        row("self.fix.verdict", serde_json::json!({"verdict": "accept"})),
        row("self.fix.verdict", serde_json::json!({"verdict": "reject"})),
        row("self.fix.refused", serde_json::json!({"bound": "size"})),
        row(
            "self.fix.refused",
            serde_json::json!({"bound": "not-independent"}),
        ),
    ];
    let text = Outcomes::of_rows(&rows, 0, 100).lines(0.0, false);
    assert!(
        text.contains(
            "- Fixed in place: 2 made, 1 approved by the model that built the branch, 1 refused \
             by a bound."
        ),
        "{text}"
    );
    assert!(text.contains("- Not fixed in place 1 times"), "{text}");
}
