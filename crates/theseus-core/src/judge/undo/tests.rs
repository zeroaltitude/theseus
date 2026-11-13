//! The pure halves of the reversibility layer (theseus-p5kln): the mapping
//! from a planned call to `undo.v1`'s state, and the reading of the pack's
//! answer into a verdict the gate acts on.
//!
//! No model, no tree and no clock: the probe is stated and the judgment is
//! built by hand, so each of the fail-open arms is asserted by itself.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::json;
use theseus_judge::band::{Banded, Top};
use theseus_judge::builders::{CwdKind, Probe, Recovery, Sandbox, Tracked};
use theseus_judge::client::{Answer as WireAnswer, Timing};
use theseus_judge::judge::{AnswerRecord, StateRecord};
use theseus_judge::{Judgment, Mode, Outcome, Pack};

use super::state::{input_of, UndoCall};
use super::{Says, UndoVerdict, FINAL, RESHAPEABLE, REVERSIBLE, SHAPE, VERDICT};

/// A tree stated instead of made: every answer is a literal.
#[derive(Default)]
struct Stated {
    tracked: BTreeMap<String, Tracked>,
    absent: Vec<String>,
    root: Option<PathBuf>,
    dirty: Option<bool>,
    /// What the repository ignores (`p5kln-location-facts`). Nothing, here,
    /// unless a test states it: `undo_regen`'s table is tested against real
    /// repositories in its own module, and these tests are the MAPPING.
    ignored: Vec<String>,
}

impl Probe for Stated {
    fn exists(&mut self, path: &Path) -> bool {
        !self.absent.contains(&path.display().to_string())
    }
    fn tracked(&mut self, path: &Path) -> Tracked {
        self.tracked
            .get(&path.display().to_string())
            .copied()
            .unwrap_or(Tracked::Unknown)
    }
    fn ignored(&mut self, path: &Path) -> Option<bool> {
        Some(self.ignored.contains(&path.display().to_string()))
    }
    fn repo(&mut self, _dir: &Path) -> (Option<PathBuf>, Option<bool>) {
        (self.root.clone(), self.dirty)
    }
}

fn pack() -> std::sync::Arc<Pack> {
    theseus_judge::pack::embedded()
        .as_ref()
        .expect("the embedded packs parse")
        .iter()
        .find(|p| p.name() == "undo.v1")
        .expect("undo.v1 is embedded")
        .clone()
}

/// A judgment carrying one Choice answer per question, each at `confidence`.
fn judged(answers: &[(&str, &str, f64)], mode: Mode) -> Judgment {
    let p = pack();
    let records = answers
        .iter()
        .map(|(def, choice, c)| {
            let q = p.question(def).expect("the pack asks it");
            AnswerRecord {
                question: (*def).into(),
                def: (*def).into(),
                about: None,
                answer: WireAnswer::Choice {
                    choice: (*choice).into(),
                    confidence: *c,
                    probabilities: vec![((*choice).into(), *c)],
                },
                band: Banded {
                    band: theseus_judge::band::confidence_band(*c, q.thresholds),
                    top: Top::Choice((*choice).into()),
                    value: *c,
                },
                no_match: q.no_match.clone(),
            }
        })
        .collect();
    Judgment {
        id: "jdg_test".into(),
        pack: p.name(),
        version: p.version,
        pack_sha256: String::new(),
        point: p.point,
        mode,
        model: p.jev_model.clone(),
        answered_by: Some(p.jev_model.clone()),
        model_drift: false,
        state: StateRecord {
            sha256: String::new(),
            bytes: 0,
            tokens: 0,
            cap_tokens: p.state_cap_tokens,
            builder: "undo".into(),
            builder_version: 1,
            truncated: Vec::new(),
            dropped: Vec::new(),
        },
        questions: answers.len(),
        answers: records,
        call: None,
        timing: Timing::default(),
        usage: None,
        cost_micros: None,
        reserve_micros: None,
        outcome: Outcome::Answered,
        circuit: None,
        rate_limit: BTreeMap::new(),
        context: json!({}),
    }
}

/// The three verdicts, each read from an answer in its act band.
#[test]
fn each_of_the_three_verdicts_is_read_from_the_deciding_answer() {
    let p = pack();
    let says = |choice: &str, c: f64| {
        UndoVerdict::of(&p, &judged(&[(VERDICT, choice, c)], Mode::Live)).map(|v| v.says)
    };
    assert_eq!(says(REVERSIBLE, 0.97), Some(Says::Reversible));
    assert_eq!(says(RESHAPEABLE, 0.95), Some(Says::Reshapeable));
    assert_eq!(says(FINAL, 0.93), Some(Says::Final));

    // And only one of them holds, and only one raises: the gate reads these
    // two and nothing else.
    let v = |choice: &str| {
        UndoVerdict::of(&p, &judged(&[(VERDICT, choice, 0.95)], Mode::Live)).expect("a verdict")
    };
    assert_eq!(
        (v(REVERSIBLE).holds(), v(REVERSIBLE).raises()),
        (false, false)
    );
    assert_eq!(
        (v(RESHAPEABLE).holds(), v(RESHAPEABLE).raises()),
        (true, false)
    );
    assert_eq!((v(FINAL).holds(), v(FINAL).raises()), (false, true));
}

/// The pack's fail-open rule, in the pack's own words: "when the answer is
/// under the question's confirm bar, the call proceeds under the ordinary
/// order". Under the bar there is no verdict at all, so the gate holds
/// nothing and raises nothing; at the bar there is one.
#[test]
fn an_answer_under_the_confirm_bar_fails_open_and_one_at_it_does_not() {
    let p = pack();
    let q = p.question(VERDICT).expect("it asks `reversible`");
    assert_eq!(
        (q.thresholds.act, q.thresholds.confirm),
        (0.90, 0.60),
        "the pack's own bands; this test is written against them"
    );
    let at = |c: f64| UndoVerdict::of(&p, &judged(&[(VERDICT, FINAL, c)], Mode::Live));
    assert!(at(0.59).is_none(), "under the confirm bar: nothing acts");
    let on = at(0.60).expect("at the confirm bar: a verdict");
    assert_eq!(on.says, Says::Final);
    assert_eq!(on.p, 0.60);
    assert!(at(0.95).is_some(), "and in the act band, of course");
}

/// A shadow judgment acts on nothing, whatever it says: that is what
/// `[judge] reversibility = false` lowers the pack to.
#[test]
fn a_shadow_judgment_holds_nothing_and_raises_nothing() {
    let p = pack();
    let live = judged(&[(VERDICT, RESHAPEABLE, 0.99)], Mode::Live);
    assert!(UndoVerdict::of(&p, &live).is_some(), "live: a verdict");
    let shadow = judged(&[(VERDICT, RESHAPEABLE, 0.99)], Mode::Shadow);
    assert!(UndoVerdict::of(&p, &shadow).is_none(), "shadow: none");
}

/// The hold's words are the pack's own sentence for the shape it chose, and
/// `none` (or a shape under its own bar) names no shape at all.
#[test]
fn the_holds_shape_and_its_sentence_come_from_the_pack() {
    let p = pack();
    let v = |shape: &str, c: f64| {
        UndoVerdict::of(
            &p,
            &judged(
                &[(VERDICT, RESHAPEABLE, 0.95), (SHAPE, shape, c)],
                Mode::Live,
            ),
        )
        .expect("a verdict")
    };
    let moved = v("move_aside", 0.92);
    assert_eq!(moved.shape.as_deref(), Some("move_aside"));
    let means = moved.shape_means(&p).expect("the pack words it");
    assert!(
        means.starts_with("Move the targets somewhere out of the way"),
        "the pack's own sentence, not this module's: {means}"
    );
    assert_eq!(v("none", 0.95).shape, None, "`none` names no shape");
    assert_eq!(
        v("move_aside", 0.30).shape,
        None,
        "a shape under its own confirm bar names none"
    );
    // The shape is not the verdict: a held call with no shape is still held.
    assert!(v("none", 0.95).holds());
}

fn call() -> UndoCall {
    UndoCall {
        session_id: "ses_x".into(),
        turn_id: "trn_x".into(),
        tool: "fs.write".into(),
        class: "write".into(),
        argv: Vec::new(),
        steps: Vec::new(),
        targets: vec![("/w/one.rs".into(), "write".into())],
        url: None,
        other_args: Some(json!({"path": "/w/one.rs"})),
        cwd: None,
        is_program: false,
        l1: false,
        held: Default::default(),
    }
}

/// The mapping: each target gets the word the probe's answer resolves to,
/// and the two facts the gate alone knows reach the state.
#[test]
fn the_state_carries_each_targets_word_and_the_gates_own_facts() {
    let mut probe = Stated {
        tracked: [
            ("/w/one.rs".to_string(), Tracked::Clean),
            ("/w/two.rs".to_string(), Tracked::Untracked),
        ]
        .into(),
        absent: vec!["/w/new.rs".into()],
        root: Some(PathBuf::from("/w")),
        dirty: Some(true),
        ..Stated::default()
    };
    let mut c = call();
    c.targets = vec![
        ("/w/one.rs".into(), "write".into()),
        ("/w/two.rs".into(), "write".into()),
        ("/w/new.rs".into(), "write".into()),
    ];
    c.cwd = Some("/w".into());
    c.is_program = true;
    c.l1 = true;
    c.argv = vec!["rm".into(), "-rf".into(), "/w/two.rs".into()];
    let i = input_of(&c, &mut probe);
    assert_eq!(i.targets.len(), 3, "every target the plan named");
    let words: Vec<Recovery> = i.targets.iter().map(|t| t.recovery).collect();
    assert_eq!(
        words,
        [
            Recovery::TrackedClean,
            Recovery::Untracked,
            Recovery::Absent
        ]
    );
    assert_eq!(i.targets[0].access, "write");
    assert_eq!(i.cwd_kind, Some(CwdKind::RepoDirty));
    assert_eq!(
        i.sandbox,
        Some(Sandbox::L1),
        "the class the gate would bind"
    );
    assert_eq!(i.argv, c.argv);
    assert_eq!(i.tool, "fs.write");
    assert_eq!(i.class, "write");
    // What v1 does not resolve, stated rather than guessed.
    assert_eq!(i.remote_keeps, None, "an API call the gate cannot make");
    assert_eq!(i.terminal_tail, None);
    assert_eq!(i.posture, "", "0d runs before the order");
}

/// `read_this_session` reaches the state from the session's own record, and
/// it is the gate that answers it (`p5kln-location-facts`): the board is the
/// core's, so `UndoCall::of` is told, and `input_of` reads what it was told.
/// The precedence is `undo_resolve`'s; what is asserted here is the WIRING,
/// which is the half that was a hard `false` before this row.
#[test]
fn a_target_this_sessions_record_holds_reaches_the_state_as_read_this_session() {
    let mut probe = Stated {
        tracked: [
            ("/w/loose.txt".to_string(), Tracked::Untracked),
            ("/w/other.txt".to_string(), Tracked::Untracked),
        ]
        .into(),
        ..Stated::default()
    };
    let mut c = call();
    c.targets = vec![
        ("/w/loose.txt".into(), "write".into()),
        ("/w/other.txt".into(), "write".into()),
    ];

    // Told nothing: both are files nothing holds a copy of, which is what
    // every target read as before this row and is what an empty board after
    // a daemon restart still reads as.
    let i = input_of(&c, &mut probe);
    assert_eq!(
        i.targets.iter().map(|t| t.recovery).collect::<Vec<_>>(),
        [Recovery::Untracked, Recovery::Untracked],
        "an empty record errs toward holding"
    );

    // Told that the record holds the first one's bytes: only that one moves,
    // and it moves to the word the pack reads as a way back.
    c.held = ["/w/loose.txt".to_string()].into_iter().collect();
    let i = input_of(&c, &mut probe);
    assert_eq!(
        i.targets.iter().map(|t| t.recovery).collect::<Vec<_>>(),
        [Recovery::ReadThisSession, Recovery::Untracked],
        "the word is per target, not per call"
    );
}

/// `UndoCall::of` asks the board about each of the plan's own target paths,
/// and only those: the `exec` resource is the working directory, not a
/// target, and nothing is asked about it.
#[test]
fn the_gate_asks_the_record_about_each_target_path_and_no_other() {
    use std::cell::RefCell;
    use std::path::PathBuf as P;

    use theseus_tools::Tool as _;

    let asked: RefCell<Vec<P>> = RefCell::new(Vec::new());
    let holds = |p: &Path| {
        asked.borrow_mut().push(p.to_path_buf());
        p.ends_with("kept.txt")
    };

    let tool = theseus_tools::fs::WriteFile;
    let root = std::env::temp_dir().join(format!("theseus-undo-held-{}", std::process::id()));
    std::fs::create_dir_all(&root).expect("the scratch root");
    let ctx = theseus_tools::ToolCtx::for_tests(&root);
    let target = root.join("kept.txt");
    let input = json!({"path": target.display().to_string(), "content": "new bytes\n"});
    let plan = tool.plan(&input, &ctx).expect("the plan");
    let c = UndoCall::of(
        "ses_held",
        "trn_held",
        &tool,
        &plan,
        &input,
        super::state::Knows {
            is_program: false,
            l1: false,
            holds: &holds,
        },
    );

    let asked = asked.into_inner();
    assert_eq!(asked.len(), 1, "one question per target, and no more");
    assert!(asked[0].ends_with("kept.txt"), "{:?}", asked[0]);
    assert_eq!(
        c.held.len(),
        1,
        "the board said it holds it, so the call carries it"
    );
    assert!(
        c.held
            .iter()
            .next()
            .is_some_and(|p| p.ends_with("kept.txt")),
        "{:?}",
        c.held
    );
    // And the held set is keyed by the same string the state's target is, or
    // `input_of` would look the word up under a path that is not there.
    for (path, _) in &c.targets {
        assert!(
            c.held.contains(path),
            "the held set must use the target's own path string: {path}"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// A call that is not a program says nothing about a sandbox, rather than
/// saying it is not one; and a call with no directory asks the probe for no
/// repository.
#[test]
fn a_call_that_is_no_program_states_no_sandbox_and_no_directory() {
    let mut probe = Stated {
        tracked: [("/w/one.rs".to_string(), Tracked::Dirty)].into(),
        root: Some(PathBuf::from("/w")),
        dirty: Some(false),
        ..Stated::default()
    };
    let i = input_of(&call(), &mut probe);
    assert_eq!(i.sandbox, None);
    assert_eq!(i.cwd, None);
    assert_eq!(
        i.cwd_kind, None,
        "no directory: the repository is not asked about"
    );
    assert_eq!(i.targets[0].recovery, Recovery::TrackedDirty);
    assert_eq!(i.other_args, Some(json!({"path": "/w/one.rs"})));
}

/// A read that names nothing is not asked about: the money is not spent to
/// be told `reversible`. A read that names a target is asked, since a
/// call's class is not always its tool's.
#[test]
fn a_read_that_names_nothing_is_not_worth_asking_and_one_that_does_is() {
    let mut c = call();
    c.class = "read".into();
    c.targets.clear();
    c.other_args = None;
    assert!(!c.worth_asking(), "a read naming nothing");
    c.targets = vec![("/w/one.rs".into(), "read".into())];
    assert!(c.worth_asking(), "a read naming a target");
    c.targets.clear();
    c.url = Some("https://example.invalid/x".into());
    assert!(c.worth_asking(), "a read naming an address");
    c.url = None;
    c.class = "write".into();
    assert!(c.worth_asking(), "and every call that is not a read");
}
