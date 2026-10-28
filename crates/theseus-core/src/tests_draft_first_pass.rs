//! The first-pass bug review before Claude's (theseus-a1wvq.17; `draft/first_pass.rs`).
//! The rig is `tests_draft`'s with a second restricted profile, `dsflashmax`,
//! as `routine_coding`'s `first_pass`.

use crate::draft::first_pass::{BOUND, FIRST_PASS_HEADER};
use crate::draft::{self, meta_key};
use crate::tests_dm_rig::say;
use crate::tests_draft::{draft_edit, load, rig, Draft, ASK};

const FINDING: &str = "notes.txt:2: gamma may break the parser (FPFIND-4417)";
const SAID: &str = "A first-pass reviewer's findings (check each; it may be wrong):";

/// A rig whose class has the first pass, and a draft that edits `notes.txt`.
fn with_first_pass() -> Draft {
    let d = rig(|cfg| {
        cfg.copy_profile("dsflashmax", "glm");
        cfg.routing.classes.routine_coding.first_pass = Some("dsflashmax".into());
    });
    {
        let mut p = d.plan.lock().unwrap();
        p.draft_call = Some(draft_edit());
        p.first_pass_report = FINDING.into();
    }
    d
}

/// The key test: the first pass reads the draft's diff in a tree of its own,
/// offered no program; its report is in the Claude review's brief under the
/// header, and in no post of the place; its measure is on the row.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_first_pass_reviewers_findings_reach_the_claude_review() {
    let d = with_first_pass();
    let sid = d.dm(&d.repo);
    say(&d.r.core, &sid, ASK, None).await;
    d.drive();
    let row = d.settled().await;
    let saw = d
        .plan
        .lock()
        .unwrap()
        .review_saw
        .clone()
        .unwrap_or_default();
    assert!(saw.contains(SAID), "{saw}");
    assert!(saw.contains(FINDING), "{saw}");
    assert_eq!(row["applied"], true, "{row}");
    assert_eq!(row["first_pass_profile"], "dsflashmax", "{row}");
    assert_eq!(row["first_pass_outcome"], "complete", "{row}");
    assert!(row["first_pass_task"].is_string(), "{row}");
    assert!(row["first_pass_cost_usd"].is_number(), "{row}");
    let tasks = d.tasks(&sid);
    assert_eq!(tasks.len(), 3, "the draft, its first pass and its review");
    let (drafted, pass) = (&tasks[0], &tasks[1]);
    assert_eq!(row["first_pass_task"], pass.session_id.as_str());
    assert_eq!(pass.last_target.as_ref().unwrap().profile, "dsflashmax");
    // What the first pass was sent: the header and the diff, no program.
    let sent: Vec<_> = d
        .zai
        .sent()
        .into_iter()
        .filter(|q| crate::tests_tasks::first_user(q).contains(FIRST_PASS_HEADER))
        .collect();
    assert!(!sent.is_empty());
    let brief = crate::tests_tasks::first_user(&sent[0]);
    assert!(
        brief.contains("-beta") && brief.contains("+gamma"),
        "{brief}"
    );
    for q in &sent {
        let names: Vec<_> = q.tools.iter().filter_map(|t| t["name"].as_str()).collect();
        assert!(names.contains(&"fs_read"), "{names:?}");
        assert!(
            !names
                .iter()
                .any(|n| crate::draft_view::runs_tool(&n.replace('_', "."))),
            "{names:?}"
        );
    }
    // It reaches the place by no post: not its report, not a line.
    for kind in ["report", "notice"] {
        for (s, body) in d.texts(kind) {
            assert!(!body.contains("FPFIND-4417"), "{kind} of {s}: {body}");
            assert_ne!(s, pass.session_id);
            assert_ne!(s, drafted.session_id);
        }
    }
    // Its tree is gone with it; the draft's is gone after the review.
    assert!(!draft::tree_of(&d.r.core.cfg, &pass.session_id).exists());
    assert_eq!(d.file(), "alpha\ngamma\n");
}

/// A first pass that fails does not hold the review: it opens at once, and its
/// brief says in one line that the first pass gave no findings.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_first_pass_that_fails_does_not_block_the_review() {
    let d = with_first_pass();
    d.plan.lock().unwrap().first_pass_fails = true;
    let sid = d.dm(&d.repo);
    say(&d.r.core, &sid, ASK, None).await;
    d.drive();
    let row = d.settled().await;
    let saw = d
        .plan
        .lock()
        .unwrap()
        .review_saw
        .clone()
        .unwrap_or_default();
    assert!(!saw.contains(SAID), "{saw}");
    assert!(
        saw.contains("A first-pass reviewer was asked and gave no findings (failed)."),
        "{saw}"
    );
    assert_eq!(row["first_pass_outcome"], "failed", "{row}");
    assert_eq!(row["applied"], true, "the review still applied: {row}");
}

/// A first pass that fails once its tree is made (here: a tree an earlier
/// attempt left at its path, so `worktree add` refuses) does not wedge the
/// draft: the review opens without a first pass, and the tree is gone.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_first_pass_that_cannot_open_lets_the_review_open() {
    let d = with_first_pass();
    let sid = d.dm(&d.repo);
    say(&d.r.core, &sid, ASK, None).await;
    crate::test_support::until_within("the draft opens", 10, || !d.tasks(&sid).is_empty()).await;
    let drafted = d.tasks(&sid)[0].session_id.clone();
    let corr = crate::draft::first_pass::correlation(&drafted);
    let (_, fp_sid) = theseus_kernel::task_ids(&corr);
    let left = draft::tree_of(&d.r.core.cfg, &fp_sid);
    std::fs::create_dir_all(left.parent().unwrap()).unwrap();
    let added = std::process::Command::new("git")
        .arg("-C")
        .arg(&d.repo)
        .args(["worktree", "add", "--detach"])
        .arg(&left)
        .output()
        .unwrap();
    assert!(added.status.success(), "{added:?}");
    d.drive();
    let row = d.settled().await;
    assert_eq!(row["applied"], true, "the review still applied: {row}");
    assert!(row["first_pass_task"].is_null(), "{row}");
    assert_eq!(
        d.tasks(&sid).len(),
        2,
        "the draft and its review, no first pass"
    );
    assert!(!left.exists(), "the first pass's tree is left on disk");
    assert_eq!(d.file(), "alpha\ngamma\n");
}

/// A first pass that never answers is cancelled once it is past its bound, by
/// the sweep as by the timer, and the review opens without it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_first_pass_past_its_bound_is_cancelled_and_the_review_opens() {
    assert_eq!(BOUND.as_secs(), 600, "the bound the report names");
    let d = with_first_pass();
    d.zai.hang_on(FIRST_PASS_HEADER);
    let sid = d.dm(&d.repo);
    say(&d.r.core, &sid, ASK, None).await;
    d.drive();
    crate::test_support::until_within("the first pass opens", 60, || d.tasks(&sid).len() == 2)
        .await;
    let drafted = d.tasks(&sid)[0].session_id.clone();
    // Not yet past its bound: the sweep leaves it running.
    d.r.core.runner.sweep_drafts(None).await;
    assert!(d.drafts().is_empty());
    assert_eq!(d.tasks(&sid).len(), 2, "no review while it runs");
    // Past it.
    let core = &d.r.core;
    let mut rec: draft::Draft = core.store.get_meta(&meta_key(&drafted)).unwrap().unwrap();
    rec.first_pass.as_mut().unwrap().deadline_ms = 1;
    core.store.put_meta(&meta_key(&drafted), &rec).unwrap();
    core.runner.sweep_drafts(None).await;
    let row = d.settled().await;
    assert_eq!(row["first_pass_outcome"], "cancelled", "{row}");
    let saw = d
        .plan
        .lock()
        .unwrap()
        .review_saw
        .clone()
        .unwrap_or_default();
    assert!(saw.contains("gave no findings (cancelled)"), "{saw}");
    assert_eq!(row["applied"], true, "{row}");
}

#[test]
fn first_pass_without_cn_draft_is_refused() {
    let e = load("[routing.classes.routine_coding]\nfirst_pass = \"dsflash\"").unwrap_err();
    assert!(
        e.contains("routing.classes.routine_coding.first_pass: it reads a draft, and the class has no cn_draft"),
        "{e}"
    );
    let ok = load(
        "[routing.classes.routine_coding]\ncn_draft = \"dsflash\"\nfinal = [\"sonnet\"]\n\
         first_pass = \"glm\"",
    )
    .unwrap();
    assert_eq!(
        ok.routing.classes.routine_coding.first_pass.as_deref(),
        Some("glm")
    );
    let plain = toml::to_string(&load("").unwrap().routing).unwrap();
    assert!(!plain.contains("first_pass"), "unset by default: {plain}");
}

#[test]
fn a_claude_first_pass_is_refused() {
    for bad in ["sonnet", "nope"] {
        let e = load(&format!(
            "[routing.classes.routine_coding]\ncn_draft = \"dsflash\"\nfinal = [\"sonnet\"]\n\
             first_pass = \"{bad}\""
        ))
        .unwrap_err();
        assert!(
            e.contains(&format!(
                "routing.classes.routine_coding.first_pass: \"{bad}\" is not a configured profile of restricted origin"
            )),
            "{bad}: {e}"
        );
    }
}

/// A first pass on the profile that drafted would be the drafting model
/// reviewing its own draft (the owner, 2026-10-17: a Claude model always does
/// the final review; theseus-ms3hd).
#[test]
fn a_first_pass_on_its_drafts_own_profile_is_refused() {
    let e = load(
        "[routing.classes.routine_coding]\ncn_draft = \"dsflash\"\nfinal = [\"sonnet\"]\n\
         first_pass = \"dsflash\"",
    )
    .unwrap_err();
    assert!(
        e.contains(
            "routing.classes.routine_coding.first_pass: \"dsflash\" is also the class's cn_draft"
        ) && e.contains("review its own draft"),
        "{e}"
    );
}

/// The timer path of the bound, on real short durations: a timer that wakes
/// before the deadline re-arms for the time left and cancels nothing, and the
/// one that finds the deadline passed cancels the first pass, whose end opens
/// the review (the sweep path is the test above).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_early_timer_rearms_and_does_not_cancel() {
    let d = with_first_pass();
    d.zai.hang_on(FIRST_PASS_HEADER);
    let sid = d.dm(&d.repo);
    say(&d.r.core, &sid, ASK, None).await;
    d.drive();
    crate::test_support::until_within("the first pass opens", 60, || d.tasks(&sid).len() == 2)
        .await;
    let drafted = d.tasks(&sid)[0].session_id.clone();
    let core = &d.r.core;
    let mut rec: draft::Draft = core.store.get_meta(&meta_key(&drafted)).unwrap().unwrap();
    rec.first_pass.as_mut().unwrap().deadline_ms = theseus_protocol::now_unix_ms() + 1_500;
    core.store.put_meta(&meta_key(&drafted), &rec).unwrap();
    core.runner
        .arm_first_pass(&drafted, std::time::Duration::from_millis(20));
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert!(d.drafts().is_empty(), "the early timer cancelled it");
    assert_eq!(d.tasks(&sid).len(), 2, "no review while it runs");
    let row = d.settled().await;
    assert_eq!(row["first_pass_outcome"], "cancelled", "{row}");
    assert_eq!(row["applied"], true, "{row}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_first_pass_past_its_bound_is_cancelled_by_its_timer() {
    let d = with_first_pass();
    d.zai.hang_on(FIRST_PASS_HEADER);
    let sid = d.dm(&d.repo);
    say(&d.r.core, &sid, ASK, None).await;
    d.drive();
    crate::test_support::until_within("the first pass opens", 60, || d.tasks(&sid).len() == 2)
        .await;
    let drafted = d.tasks(&sid)[0].session_id.clone();
    let core = &d.r.core;
    let mut rec: draft::Draft = core.store.get_meta(&meta_key(&drafted)).unwrap().unwrap();
    rec.first_pass.as_mut().unwrap().deadline_ms = 1;
    core.store.put_meta(&meta_key(&drafted), &rec).unwrap();
    // No sweep: the timer alone.
    core.runner
        .arm_first_pass(&drafted, std::time::Duration::from_millis(20));
    let row = d.settled().await;
    assert_eq!(row["first_pass_outcome"], "cancelled", "{row}");
    let exec = d.tasks(&sid)[1].execution_id.clone().unwrap();
    let ended = core.kernel.execution(&exec).unwrap().unwrap().ended_reason;
    assert_eq!(
        ended.as_deref(),
        Some("cancelled by first pass: past its bound")
    );
    assert_eq!(row["applied"], true, "{row}");
}
