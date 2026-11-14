//! The reversibility layer through a whole core (theseus-p5kln), against the
//! fake Jev: a call `undo.v1` calls `irreversible_reshapeable` is HELD before
//! the order runs and no card is made; one it calls `irreversible_final` has
//! the decision the order RETURNED raised to `approve`, in `auto` mode, where
//! `Layer::Mode` would have lowered a raise made inside the ladder; a
//! `reversible` call runs exactly as it did before; and a call the sealed
//! holdout refuses is never sent to Jev at all.
//!
//! The negative ("no `undo.v1` request") is asserted beside the positive
//! ("exactly one"), read off `FakeJev::seen`, so neither can pass against a
//! tree where 0d was never wired.
//!
//! No clock is read: nothing here sleeps on wall time but the one 200 ms
//! settle in the refusal test, which bounds a thing that must never happen.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, Scripted as Jev};
use theseus_protocol::modes::SessionMode;
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::judge::undo::{FINAL, RESHAPEABLE, REVERSIBLE};
use crate::ledger::LedgerRow;
use crate::node::Body;
use crate::policy::Posture;
use crate::provider::{FakeProvider, Scripted};
use crate::rpc::{Core, Parts};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::tests_judge::{board, off, warm};
use crate::toolrun::reversible::HELD_REASON;
use crate::turn::{repeat_step, TurnRequest};
use crate::Config;

/// What the call would overwrite, and what it would write.
const WAS: &str = "old bytes\n";
const NOW: &str = "new bytes\n";

struct Rig {
    core: Arc<Core>,
    /// The file the scripted call overwrites.
    target: PathBuf,
    jev: FakeJev,
    _dir: tempfile::TempDir,
}

/// A Jev that answers `verdict` at `confidence`, and names `move_aside` as
/// the shape.
fn says(verdict: &str, confidence: f64) -> FakeJev {
    let jev = FakeJev::start().unwrap();
    jev.script(
        "reversible",
        Jev::Choice {
            option: verdict.into(),
            confidence,
        },
    );
    jev.script(
        "shape",
        Jev::Choice {
            option: "move_aside".into(),
            confidence: 0.95,
        },
    );
    jev
}

/// A core with the judge on against a Jev that says `verdict`, `undo.v1` the
/// only pack judging, the reversibility switch on (the default), and every
/// posture open so nothing but this layer can raise one. The file the
/// scripted `fs.write` overwrites exists before the core is built, because
/// the probe reads the real tree.
///
/// `sealed` puts the target inside `[self.sealed] dir`, which refuses the
/// call outright: that is the egress test's arrangement.
fn rig(verdict: &str, confidence: f64, sealed: bool, tweak: impl FnOnce(&mut Config)) -> Rig {
    rig_scripted(verdict, confidence, sealed, tweak, script)
}

/// The rig with a script of its own: the override's round trip needs two
/// turns, and the second proposes the original call beside a re-planned one.
fn rig_scripted(
    verdict: &str,
    confidence: f64,
    sealed: bool,
    tweak: impl FnOnce(&mut Config),
    script: impl FnOnce(&Path) -> Vec<Scripted>,
) -> Rig {
    let jev = says(verdict, confidence);
    let dir = crate::scratch::tempdir();
    let base = dir.path().canonicalize().unwrap();
    let (state, work, holdout) = (base.join("state"), base.join("work"), base.join("sealed"));
    for d in [&state, &work, &holdout] {
        std::fs::create_dir_all(d).unwrap();
    }
    let target = match sealed {
        true => holdout.join("holdout.txt"),
        false => work.join("notes.txt"),
    };
    std::fs::write(&target, WAS).unwrap();

    let mut cfg = Config::example();
    cfg.server.state_dir = state.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(work.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = Posture::Open;
    // The sealed set sits outside every root, which the loader requires.
    cfg.self_improve.sealed.dir = Some(holdout.to_string_lossy().into_owned());
    cfg.judge.enabled = true;
    cfg.judge.api_base = jev.base();
    cfg.judge.connect_secs = 1;
    cfg.judge.total_secs = 3;
    // EVERY other pack off, so `seen()` counts `undo.v1`'s requests alone.
    // A new pack added to `WIRED` later is off here too, so this rig does
    // not go quietly wrong when one lands.
    for p in crate::judge::inbound::PACKS {
        cfg.judge.packs.insert(p.into(), off());
    }
    for (p, _) in crate::judge::WIRED {
        if *p != crate::judge::undo::PACK_NAME {
            cfg.judge.packs.insert((*p).into(), off());
        }
    }
    assert!(cfg.judge.reversibility, "on by default: the owner's word");
    tweak(&mut cfg);
    cfg.validate().unwrap();

    let store = Store::open(&state.join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script(&target)));
    let mut p = Parts::for_tests(cfg, fake, store);
    p.secrets = board();
    let core = Core::build(p).unwrap();
    warm(&core);
    Rig {
        core,
        target,
        jev,
        _dir: dir,
    }
}

/// One `fs.write` over a file that exists, then an answer.
fn script(path: &Path) -> Vec<Scripted> {
    vec![
        Scripted::tools(
            "I'll replace it.",
            &[(
                "c1",
                "fs_write",
                json!({"path": path.to_string_lossy(), "content": NOW}),
            )],
        ),
        Scripted::text("Done."),
    ]
}

impl Rig {
    fn session(&self) -> String {
        let r = SessionRecord::new(SessionKind::Conversation, None);
        self.core.store.put_session(&r.session_id, &r).unwrap();
        r.session_id
    }

    async fn turn(&self, sid: &str) -> TurnSubmitResult {
        let rec = self
            .core
            .store
            .get_session::<SessionRecord>(sid)
            .unwrap()
            .unwrap();
        let (live, _) = self.core.live_profile();
        let target = self
            .core
            .runner
            .resolve_target(&live, None, None, None)
            .unwrap();
        let sink = EventSink::new(self.core.bus.clone(), sid, None);
        self.core
            .runner
            .run(TurnRequest {
                prompt: None,
                surface: None,
                session: rec,
                input: Some("Replace the notes.".into()),
                target,
                sink,
                author: "test".into(),
                recompile: None,
                attachments: vec![],
                arrived: None,
                reply_to: None,
                answer_at: None,
            })
            .await
            .unwrap()
    }

    fn rows(&self, kind: &str) -> Vec<Value> {
        self.core
            .store
            .ledger_tail::<LedgerRow>(10_000)
            .unwrap()
            .into_iter()
            .map(|(_, r)| r)
            .filter(|r| r.kind == kind)
            .map(|r| r.data)
            .collect()
    }

    /// The session's tool results: `(status, text, the meta's reason)`.
    fn results(&self, sid: &str) -> Vec<(String, String, String)> {
        self.core
            .store
            .session_nodes(sid)
            .unwrap()
            .into_iter()
            .filter_map(|(_, n)| match &n.body {
                Body::ToolResult {
                    status,
                    content,
                    meta,
                    ..
                } => Some((
                    format!("{status:?}"),
                    content.clone(),
                    meta.get("reason")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                )),
                _ => None,
            })
            .collect()
    }

    fn bytes(&self) -> String {
        std::fs::read_to_string(&self.target).unwrap()
    }
}

/// Point one, whole: a `reshapeable` verdict holds the call before `order`
/// runs, the model is told the pack's own shape sentence, the file is not
/// written, no card is made, and the `tool.held` row says why.
#[tokio::test(flavor = "multi_thread")]
async fn a_reshapeable_verdict_holds_the_call_and_makes_no_card() {
    let r = rig(RESHAPEABLE, 0.96, false, |_| {});
    let sid = r.session();
    r.turn(&sid).await;

    assert_eq!(r.bytes(), WAS, "the call did not run");

    let results = r.results(&sid);
    assert_eq!(results.len(), 1, "one result: {results:?}");
    let (status, text, reason) = &results[0];
    // `Declined` is "the call never ran", which is what a hold is; `Error`,
    // which this asserted until `p0b-held-loop-repeat-cap`, means "the tool
    // ran and failed, or its input was invalid" and is true of neither half.
    // The change is deliberate and it is the P0 fix: the repeat cap reads
    // this status to tell a loop that ran nothing from one that ran and
    // failed, so an `Error` here left a model re-planning the same held call
    // unbounded (`turn::repeat_step`). Nothing the model reads moved with it
    // -- `is_error` is `status.is_error()`, true of both -- and the words,
    // the reason, the absent card and the `tool.held` row below are unchanged.
    assert_eq!(status, "Declined");
    assert!(text.starts_with("Not run: "), "{text}");
    assert!(
        text.contains("Move the targets somewhere out of the way"),
        "the pack's own sentence for `move_aside`, not this layer's: {text}"
    );
    assert!(
        reason.starts_with(&format!("{HELD_REASON}: move_aside")),
        "the reason names the shape: {reason}"
    );

    assert!(
        r.rows("tool.confirm_requested").is_empty(),
        "a hold makes no card: it costs the owner no attention"
    );
    let held = r.rows("tool.held");
    assert_eq!(held.len(), 1, "one tool.held row: {held:?}");
    assert_eq!(held[0]["tool"], "fs.write");
    assert_eq!(held[0]["shape"], "move_aside");
    assert_eq!(held[0]["verdict"], RESHAPEABLE);
    assert!(held[0]["judgment"].as_str().is_some_and(|s| !s.is_empty()));
    assert!(r.rows("tool.raised").is_empty(), "a hold is not a raise");

    assert_eq!(r.jev.seen().len(), 1, "asked exactly once for the one call");
}

/// How many times the scripted model re-plans the same held call. Well above
/// `MAX_REPEATS`, so a turn that is NOT ended by the cap runs on into double
/// digits and the test fails on the loop count rather than on a script that
/// happened to run out.
const REPLANS: usize = 14;

/// A model that answers a hold by proposing the same call again, for ever:
/// `REPLANS` byte-identical `fs.write` calls, each with its own `tool_use`
/// id as a real response would have, and an answer at the end that is only
/// reached if nothing bounds the loop.
fn script_replanning_the_same_call(path: &Path) -> Vec<Scripted> {
    let mut s: Vec<Scripted> = (0..REPLANS)
        .map(|i| {
            Scripted::tools(
                "I'll replace it.",
                &[(
                    &format!("c{}", i + 1),
                    "fs_write",
                    json!({"path": path.to_string_lossy(), "content": NOW}),
                )],
            )
        })
        .collect();
    s.push(Scripted::text("Done."));
    s
}

/// **The P0 this card exists for, through a whole core**
/// (`p0b-held-loop-repeat-cap`, row B6 `theseus-2yowd`): a model that answers
/// a reversibility hold by re-planning the same call ends the turn at the
/// repeat cap, in single-digit loops, with `end_turn:repeating` — and not at
/// the kernel's hard multiple of the spend limit, some thousands of loops
/// later, which was the only terminus until this.
///
/// It is a whole-core test on purpose. The unit tests in
/// `turn::repeat_step` and `advancer` hand `Repeats` and the Advancer their
/// inputs directly, and every one of them passed while this hazard was live:
/// the chain broke between them, at the status a held call writes. So this
/// drives the real thing — the classifier on, Jev answering
/// `irreversible_reshapeable`, the gate holding the call, the turn loop
/// counting it — and it is the test that goes red if the hold's status is
/// taken back to `Error`.
///
/// The counts are positive as well as negative: `REPLANS` is 14, so a turn
/// that was not ended by the cap has loops to spare; `tool.held` is asserted
/// once per loop, so this cannot pass on a tree that stopped holding; and the
/// statuses are read off the stored results, which is the field the cap reads.
#[tokio::test(flavor = "multi_thread")]
async fn a_model_that_keeps_re_planning_the_same_held_call_ends_the_turn() {
    let r = rig_scripted(
        RESHAPEABLE,
        0.96,
        false,
        |_| {},
        script_replanning_the_same_call,
    );
    let sid = r.session();
    let res = r.turn(&sid).await;

    assert_eq!(res.stop_reason, repeat_step::REPEATING, "{res:?}");
    assert_eq!(res.loops, repeat_step::MAX_REPEATS, "{res:?}");
    assert!(
        res.loops < 10 && (REPLANS as u32) > res.loops + 2,
        "single-digit loops, with script left over: {} of {REPLANS}",
        res.loops
    );
    assert_eq!(r.bytes(), WAS, "the call never ran, in any loop");

    // Every loop held, and not one of them asked the owner: the hold's whole
    // point is that it costs him no attention, so the cap is the only thing
    // that can end this turn.
    let held = r.rows("tool.held");
    assert_eq!(held.len() as u32, repeat_step::MAX_REPEATS, "{held:?}");
    assert!(
        r.rows("tool.confirm_requested").is_empty(),
        "a held loop asks nobody, which is why it could run to the spend stop"
    );

    // The field the cap reads: one result a loop, every one of them "the call
    // never ran". An `Error` here is the defect this card closed.
    let results = r.results(&sid);
    assert_eq!(
        results.len() as u32,
        repeat_step::MAX_REPEATS,
        "{results:?}"
    );
    assert!(
        results.iter().all(|(status, _, _)| status == "Declined"),
        "every held result reads as a call that never ran: {results:?}"
    );

    // The durable record of the end, on the last loop's own row.
    let loops = r.rows("loop.ended");
    assert_eq!(loops.len() as u32, repeat_step::MAX_REPEATS, "{loops:?}");
    let last = loops.last().unwrap();
    assert_eq!(
        last["decision"],
        json!({"decision": "end_turn", "reason": repeat_step::REPEATING}),
        "{last}"
    );
    assert_eq!(
        last["outcome"]["repeats"],
        json!(repeat_step::MAX_REPEATS),
        "{last}"
    );
    assert_eq!(
        r.jev.seen().len() as u32,
        repeat_step::MAX_REPEATS,
        "0d asked once for each loop's one call"
    );
}

/// A `reversible` verdict changes nothing: the call runs and nothing is
/// held or raised. The positive count beside the negatives: Jev WAS asked,
/// so this does not pass on a tree where 0d was never wired.
#[tokio::test(flavor = "multi_thread")]
async fn a_reversible_verdict_runs_the_call_unchanged() {
    let r = rig(REVERSIBLE, 0.97, false, |_| {});
    let sid = r.session();
    r.turn(&sid).await;
    assert_eq!(r.bytes(), NOW, "the call ran");
    assert!(r.rows("tool.held").is_empty());
    assert!(r.rows("tool.raised").is_empty());
    assert!(r.rows("tool.confirm_requested").is_empty());
    assert_eq!(
        r.jev.seen().len(),
        1,
        "asked once, and the answer let it run"
    );
}

/// Point two, and the error `docs/design/undo-v1.md` corrects: in `auto`
/// mode, in a private place, on a tool nothing tightened, with `floor`
/// unset, a raise made INSIDE the ladder is exactly the shape
/// `policy/modes.rs`'s `gate` exists to lower. This raise lands on what the
/// order RETURNED, so the owner is still asked.
#[tokio::test(flavor = "multi_thread")]
async fn a_final_verdict_survives_auto_mode() {
    let r = rig(FINAL, 0.95, false, |_| {});
    let sid = r.session();
    r.core.tools.modes.set(&sid, SessionMode::Auto);
    r.turn(&sid).await;

    let cards = r.rows("tool.confirm_requested");
    assert_eq!(cards.len(), 1, "the owner is asked: {cards:?}");
    assert_eq!(r.bytes(), WAS, "and nothing is written until they answer");
    let raised = r.rows("tool.raised");
    assert_eq!(raised.len(), 1, "one tool.raised row: {raised:?}");
    assert_eq!(raised[0]["verdict"], FINAL);
    assert_eq!(raised[0]["tool"], "fs.write");
    assert!(
        r.rows("tool.held").is_empty(),
        "a final verdict is not a hold: the model is not asked to re-plan it"
    );
    assert_eq!(r.jev.seen().len(), 1);
}

/// 0d sits AFTER the outright refusals (`p5kln-p0-judge-egress`): a call the
/// sealed holdout refuses has its content, its paths and the bytes it was
/// about to write refused before anything reaches a model. The positive
/// count is in the tests above; without one an empty set would pass this.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_the_sealed_holdout_refuses_is_never_sent_to_jev() {
    let r = rig(RESHAPEABLE, 0.96, true, |_| {});
    let sid = r.session();
    r.turn(&sid).await;

    let results = r.results(&sid);
    assert_eq!(results.len(), 1, "{results:?}");
    assert!(results[0].1.starts_with("Not run: "), "{:?}", results[0]);
    assert!(
        results[0]
            .1
            .contains("the sealed holdout set is out of reach"),
        "the sealed holdout refused it, not some other rule: {:?}",
        results[0].1
    );
    assert!(
        results[0].2.starts_with("place: "),
        "a place refusal, not a hold: {:?}",
        results[0].2
    );
    assert_eq!(r.bytes(), WAS);
    // A judgment is spawned, so give one a window in which to arrive.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        r.jev.seen().len(),
        0,
        "a refused call's content was sent to a model"
    );
    assert!(r.rows("tool.held").is_empty());
}

/// `[judge] reversibility = false` holds the pack in shadow: the judgment is
/// still made and recorded, and the call runs. That is the switch's whole
/// contract, and it is what makes this revertible without a build.
#[tokio::test(flavor = "multi_thread")]
async fn the_switch_off_judges_and_holds_nothing() {
    let r = rig(RESHAPEABLE, 0.99, false, |c| c.judge.reversibility = false);
    let sid = r.session();
    r.turn(&sid).await;
    assert_eq!(r.bytes(), NOW, "in shadow the call runs");
    assert!(r.rows("tool.held").is_empty());
    assert!(r.rows("tool.raised").is_empty());
    assert_eq!(r.jev.seen().len(), 1, "and it was still judged");
}

// ---------------------------------------------------------------------------
// The owner's override of a reshapeable hold (theseus-p5kln).
//
// The first half of his decision of 2026-11-12 is the base's: the classifier
// is wired and ON. This is the second half, the power he asked for in those
// words -- *"i have the power to say it's OK to run what was originally
// suggested"* -- and the three things it does not touch are each asserted
// here: the switch's default, the no-card property of a hold nobody
// overrides, and that what runs is the ORIGINAL call and never a re-planned
// one.
// ---------------------------------------------------------------------------

/// What the re-planned call would write to: the shape `move_aside` asks for,
/// beside the original rather than over it.
const BESIDE: &str = "notes.txt.new";

/// Turn one proposes the call and it is held; turn two proposes the ORIGINAL
/// call and a RE-PLANNED one in the same response, so one gate pass judges
/// both with the owner's allowance standing.
fn script_original_and_replanned(path: &Path) -> Vec<Scripted> {
    let beside = path.with_file_name(BESIDE);
    let original = json!({"path": path.to_string_lossy(), "content": NOW});
    vec![
        Scripted::tools("I'll replace it.", &[("c1", "fs_write", original.clone())]),
        Scripted::text("Held; I'll re-plan it."),
        Scripted::tools(
            "The original, and the reshaped one.",
            &[
                ("c2", "fs_write", original),
                (
                    "c3",
                    "fs_write",
                    json!({"path": beside.to_string_lossy(), "content": NOW}),
                ),
            ],
        ),
        Scripted::text("Done."),
    ]
}

impl Rig {
    /// The holds the owner can still allow, newest first.
    fn held(&self) -> Vec<theseus_protocol::work::undo::HeldCall> {
        self.core
            .undo_list(&theseus_protocol::work::undo::UndoListParams {
                all: true,
                ..Default::default()
            })
            .unwrap()
    }

    /// What the re-planned call would have written, if anything did.
    fn beside(&self) -> Option<String> {
        std::fs::read_to_string(self.target.with_file_name(BESIDE)).ok()
    }

    /// The owner's own terminal: `places::owner_in_person` passes it, since
    /// no agent's process is behind it.
    fn owner() -> crate::approval::Client {
        use crate::approval::Surface;
        crate::tests_m3::surface("sock#1", Surface::Cli).behind(crate::peer_proc::Behind::Owner)
    }

    /// A turn over the wire through `turn.submit`, from the owner's own
    /// terminal: the dispatch arm and everything `turn.submit` reads out of
    /// a message BEFORE the turn runs, which is where the override in words
    /// is read (`rpc::allow_in_words`). `Rig::turn` calls the runner
    /// directly and reaches none of it.
    async fn submit(&self, sid: &str, input: &str) -> Value {
        crate::tests_m3::rpc_as(
            &self.core,
            Rig::owner(),
            theseus_protocol::method::TURN_SUBMIT,
            json!({"session_id": sid, "input": input}),
        )
        .await
        .expect("turn.submit")
    }

    /// `undo.allow` over the wire, as `client` (theseus-me9fc's own rig):
    /// the dispatch arm, `route`, and `judge_act`, not just the core call.
    async fn allow_as(
        &self,
        client: crate::approval::Client,
        hold: &str,
    ) -> Result<Value, theseus_protocol::RpcError> {
        crate::tests_m3::rpc_as(
            &self.core,
            client,
            theseus_protocol::method::UNDO_ALLOW,
            json!({"hold": hold}),
        )
        .await
    }
}

/// **The bar of `p5kln-override-route`, whole.** A reshapeable verdict holds
/// the call; the owner takes the override by id; the ORIGINAL call runs, as
/// it was first proposed; and the re-planned call does not.
///
/// The two file reads at the end are the whole proof and they are both
/// positive: the original's bytes moved, and the reshaped call's file was
/// never created. A test that asserted only the second would pass on a tree
/// where the override ran nothing at all.
#[tokio::test(flavor = "multi_thread")]
async fn the_override_runs_the_original_call_and_not_the_replanned_one() {
    let r = rig_scripted(
        RESHAPEABLE,
        0.96,
        false,
        |_| {},
        script_original_and_replanned,
    );
    let sid = r.session();

    // Turn one: held, as the base holds it, and no card.
    r.turn(&sid).await;
    assert_eq!(r.bytes(), WAS, "the call did not run");
    assert_eq!(r.rows("tool.held").len(), 1);
    assert!(r.rows("tool.confirm_requested").is_empty(), "no card");

    // The hold left one offer, naming the call as it was first proposed.
    let held = r.held();
    assert_eq!(held.len(), 1, "one held call to override: {held:?}");
    assert_eq!(held[0].tool, "fs.write");
    assert_eq!(held[0].input["content"], NOW, "the original's own bytes");
    assert!(!held[0].waiting(), "nothing has allowed it yet");

    // The owner's word, by id, from his own terminal.
    r.allow_as(Rig::owner(), &held[0].id)
        .await
        .expect("the owner may override his own hold");
    assert!(
        r.held().iter().any(|h| h.id == held[0].id && h.waiting()),
        "allowed, and not yet spent"
    );
    let rows = r.rows("tool.hold_allowed");
    assert_eq!(rows.len(), 1, "one tool.hold_allowed row: {rows:?}");
    assert_eq!(rows[0]["hold"], held[0].id);
    assert_eq!(rows[0]["tool"], "fs.write");

    // Turn two: the original call and a re-planned one, judged together.
    r.turn(&sid).await;
    assert_eq!(
        r.bytes(),
        NOW,
        "the ORIGINAL call ran, as it was first proposed"
    );
    assert_eq!(
        r.beside(),
        None,
        "and the re-planned call was still held: the owner allowed the \
         original and nothing else"
    );

    // The spend: one allowance, one call, recorded.
    let spent = r.rows("tool.hold_overridden");
    assert_eq!(spent.len(), 1, "one override spent: {spent:?}");
    assert_eq!(spent[0]["hold"], held[0].id);
    let after = r.held();
    assert!(
        after
            .iter()
            .any(|h| h.id == held[0].id && h.ran_ms.is_some()),
        "the allowance is spent: {after:?}"
    );
    // The re-planned call left its own hold, so it is still held and not run.
    assert_eq!(
        r.rows("tool.held").len(),
        2,
        "the re-planned call was held too"
    );
    assert!(
        r.rows("tool.confirm_requested").is_empty(),
        "and still no card: an override is not a question"
    );
}

/// **The quiet path is still quiet.** The same two turns with no override:
/// both calls stay held, nothing is written, and no card is made, so a
/// reshapeable hold costs the owner no attention — which is the property the
/// owner's decision puts out of reach. The positive count beside it: the
/// holds ARE recorded, so the offer exists and nobody is asked about it.
#[tokio::test(flavor = "multi_thread")]
async fn a_hold_nobody_overrode_runs_nothing_and_still_makes_no_card() {
    let r = rig_scripted(
        RESHAPEABLE,
        0.96,
        false,
        |_| {},
        script_original_and_replanned,
    );
    let sid = r.session();
    r.turn(&sid).await;
    r.turn(&sid).await;

    assert_eq!(r.bytes(), WAS, "nothing ran");
    assert_eq!(r.beside(), None, "nothing ran beside it either");
    assert_eq!(r.rows("tool.held").len(), 3, "three calls, three holds");
    assert!(
        r.rows("tool.hold_overridden").is_empty(),
        "nothing was overridden"
    );
    assert!(
        r.rows("tool.confirm_requested").is_empty(),
        "a hold makes no card: it costs the owner no attention"
    );
    let held = r.held();
    assert_eq!(
        held.len(),
        3,
        "and each is offered for an override: {held:?}"
    );
    assert!(held.iter().all(|h| !h.waiting()));
}

/// **The override is the owner's and nobody else's** (this card's keel
/// class). It is a route PAST a safety hold, so it takes the bar
/// `theseus-me9fc` set for granting a repository's trust:
/// `places::owner_in_person`. An agent's own CLI connection — the thing the
/// place rule alone would have let through, since the socket is the owner's
/// user — is refused by name, with an `approval.refused` row and no
/// allowance; the owner's own terminal is the positive count.
#[tokio::test(flavor = "multi_thread")]
async fn a_caller_that_is_not_the_owner_in_person_cannot_override_a_hold() {
    use crate::approval::Surface;
    use crate::peer_proc::Behind;
    use crate::tests_m3::surface;

    let r = rig(RESHAPEABLE, 0.96, false, |_| {});
    let sid = r.session();
    r.turn(&sid).await;
    let held = r.held();
    assert_eq!(held.len(), 1, "one held call: {held:?}");

    let agents = [
        (
            surface("sock#9", Surface::Cli).behind(Behind::Agent("no terminal".into())),
            "no terminal",
        ),
        (
            surface("sock#9", Surface::Cli).behind(Behind::Agent("claude".into())),
            "claude",
        ),
        (
            surface("web#9", Surface::Web).behind(Behind::Agent("untraced".into())),
            "untraced",
        ),
    ];
    for (client, name) in agents {
        let e = r
            .allow_as(client, &held[0].id)
            .await
            .expect_err("an agent's process cannot override a hold");
        assert_eq!(
            e.code,
            theseus_protocol::error_code::REFUSED,
            "{name}: {e:?}"
        );
        let why = format!("{e:?}");
        assert!(
            why.contains(name),
            "the refusal names the agent's process ({name}): {why}"
        );
        assert!(
            why.contains("held call"),
            "and the refusal is about the hold, not a repository's trust: {why}"
        );
    }
    let refused = r.rows("approval.refused");
    assert_eq!(refused.len(), 3, "each refused act is one row: {refused:?}");
    assert!(
        refused.iter().all(|row| row["act"] == "undo.allow"),
        "{refused:?}"
    );
    assert!(
        r.rows("tool.hold_allowed").is_empty(),
        "no allowance was written"
    );
    assert!(r.held().iter().all(|h| !h.waiting()), "none is allowed");

    // The positive count: the owner's own terminal takes it.
    r.allow_as(Rig::owner(), &held[0].id)
        .await
        .expect("the owner");
    assert_eq!(r.rows("tool.hold_allowed").len(), 1);
    assert!(r.held().iter().any(|h| h.waiting()));
}

/// Turn one is held; turn two proposes ONLY the re-planned call, with the
/// owner's allowance standing; turn three proposes the original.
fn script_replanned_then_original(path: &Path) -> Vec<Scripted> {
    let beside = path.with_file_name(BESIDE);
    let original = json!({"path": path.to_string_lossy(), "content": NOW});
    let replanned = json!({"path": beside.to_string_lossy(), "content": NOW});
    vec![
        Scripted::tools("I'll replace it.", &[("c1", "fs_write", original.clone())]),
        Scripted::text("Held; I'll re-plan it."),
        Scripted::tools("Beside it instead.", &[("c2", "fs_write", replanned)]),
        Scripted::text("Reshaped."),
        Scripted::tools("The original, then.", &[("c3", "fs_write", original)]),
        Scripted::text("Done."),
    ]
}

/// **The allowance names the CALL, not the tool.** The previous test proves
/// the round trip, but in it the original and the re-planned call are
/// proposed together, so the original spends the one allowance first and the
/// single-use rule alone would hold the other — a planted revert of
/// `Holds::allowance_for`'s input match passed that test. This one isolates
/// it: the re-planned call is proposed **alone**, with the allowance
/// standing and unspent, and it must still be held. Then the original runs,
/// which is the positive count without which an allowance that admitted
/// nothing would pass.
#[tokio::test(flavor = "multi_thread")]
async fn an_allowance_does_not_admit_a_replanned_call_proposed_alone() {
    let r = rig_scripted(
        RESHAPEABLE,
        0.96,
        false,
        |_| {},
        script_replanned_then_original,
    );
    let sid = r.session();
    r.turn(&sid).await;
    let held = r.held();
    assert_eq!(held.len(), 1, "one held call: {held:?}");
    r.allow_as(Rig::owner(), &held[0].id)
        .await
        .expect("the owner");

    // Turn two: the re-planned call alone, with the allowance waiting.
    r.turn(&sid).await;
    assert_eq!(
        r.beside(),
        None,
        "the re-planned call is not what the owner allowed, so it is held"
    );
    assert!(
        r.rows("tool.hold_overridden").is_empty(),
        "and nothing spent the allowance"
    );
    assert!(
        r.held().iter().any(|h| h.id == held[0].id && h.waiting()),
        "the allowance is still waiting for the call it names"
    );

    // Turn three: the original, which the allowance does name.
    r.turn(&sid).await;
    assert_eq!(r.bytes(), NOW, "the original ran");
    assert_eq!(r.rows("tool.hold_overridden").len(), 1);
}

/// Turn one is held; turn two proposes the ORIGINAL call TWICE, byte for
/// byte, with one allowance standing. Two calls of one turn reaching for one
/// allowance is the shape the owner's word of 2026-11-13 is about.
fn script_the_original_twice(path: &Path) -> Vec<Scripted> {
    let original = json!({"path": path.to_string_lossy(), "content": NOW});
    vec![
        Scripted::tools("I'll replace it.", &[("c1", "fs_write", original.clone())]),
        Scripted::text("Held; I'll re-plan it."),
        Scripted::tools(
            "The original, twice.",
            &[
                ("c2", "fs_write", original.clone()),
                ("c3", "fs_write", original),
            ],
        ),
        Scripted::text("Done."),
    ]
}

/// **Two calls of one turn reach for one allowance, and EXACTLY ONE RUNS**
/// (the owner, 2026-11-13 17:48, S34-2). One allowance admits one call; the
/// other is held again, as any other reversibility hold — the same
/// `Declined` status the repeat cap reads, the same `undo:` reason, no card,
/// and an offer to override it again.
///
/// Both counts are positive, so neither half can pass against a tree that
/// ran nothing: exactly one `tool.hold_overridden` row, and exactly one of
/// the turn's two results NOT declined.
#[tokio::test(flavor = "multi_thread")]
async fn two_calls_of_one_turn_reach_for_one_allowance_and_exactly_one_runs() {
    let r = rig_scripted(RESHAPEABLE, 0.96, false, |_| {}, script_the_original_twice);
    let sid = r.session();

    r.turn(&sid).await;
    let held = r.held();
    assert_eq!(held.len(), 1, "one held call: {held:?}");
    r.allow_as(Rig::owner(), &held[0].id)
        .await
        .expect("the owner");

    // Turn two: the same call twice, with one allowance standing.
    r.turn(&sid).await;

    let spent = r.rows("tool.hold_overridden");
    assert_eq!(spent.len(), 1, "ONE allowance was spent: {spent:?}");
    assert_eq!(spent[0]["hold"], held[0].id);

    let results = r.results(&sid);
    assert_eq!(results.len(), 3, "one call, then two: {results:?}");
    let ran: Vec<&(String, String, String)> =
        results.iter().filter(|(s, _, _)| s != "Declined").collect();
    assert_eq!(
        ran.len(),
        1,
        "EXACTLY ONE of the three calls ran: {results:?}"
    );
    assert_eq!(ran[0].0, "Ok", "and it ran cleanly: {ran:?}");
    assert_eq!(
        r.bytes(),
        NOW,
        "the one that ran wrote the original's bytes"
    );

    // The one that did not is an ORDINARY hold: the status the repeat cap
    // reads, the shape's own reason, no card, and offered again.
    let (_, text, reason) = results
        .last()
        .filter(|(s, _, _)| s == "Declined")
        .or_else(|| results.get(1).filter(|(s, _, _)| s == "Declined"))
        .expect("one of turn two's two calls was declined");
    assert!(text.starts_with("Not run: "), "{text}");
    assert!(
        reason.starts_with(&format!("{HELD_REASON}: move_aside")),
        "the same reason any hold carries: {reason}"
    );
    assert_eq!(
        r.rows("tool.held").len(),
        2,
        "the call that found the allowance spent was HELD, and said so"
    );
    assert!(
        r.rows("tool.confirm_requested").is_empty(),
        "and still no card: a hold costs the owner no attention"
    );
    assert_eq!(
        r.held().iter().filter(|h| h.allowed.is_none()).count(),
        1,
        "and it is offered for an override again: {:?}",
        r.held()
    );
}

/// **A spend the store refused holds the call too** — the second arm, and
/// this builder's call rather than the owner's (his set leaves it to the
/// builder). The argument is `spend_allowance`'s own: the spend lands BEFORE
/// the call so that nothing between them can let one allowance run the call
/// twice. A refused frame leaves the allowance reading as waiting, which is
/// exactly the state that ordering exists to forbid; and it is the frame
/// that carries `tool.hold_overridden`, so running on it would perform the
/// one class of call the gate judged unrecoverable with no record that it
/// was ever authorised.
///
/// The fault is the store's own (`fail_turn_frame`, theseus-l6y), armed on
/// the holds record alone and only after the hold has been written, so the
/// one frame it refuses is the spend's. Nothing here is `#[cfg(test)]` in
/// the gate: the arm is driven as a bad disk drives it.
#[tokio::test(flavor = "multi_thread")]
async fn a_spend_the_store_refused_holds_the_call_again() {
    let r = rig_scripted(
        RESHAPEABLE,
        0.96,
        false,
        |_| {},
        script_original_and_replanned,
    );
    let sid = r.session();

    r.turn(&sid).await;
    let held = r.held();
    assert_eq!(held.len(), 1, "one held call: {held:?}");
    r.allow_as(Rig::owner(), &held[0].id)
        .await
        .expect("the owner");

    // The next turn frame carrying the holds record is the spend's own.
    r.core.store.fail_turn_frame(|rows| {
        rows.iter()
            .any(|row| row.key.as_deref() == Some(crate::toolrun::holds::RECORD))
    });
    r.turn(&sid).await;

    assert_eq!(r.bytes(), WAS, "the call did NOT run on an unwritten spend");
    assert_eq!(r.beside(), None, "and the re-planned call was held as ever");
    assert!(
        r.rows("tool.hold_overridden").is_empty(),
        "nothing was announced as overridden, because nothing was recorded"
    );
    assert!(
        r.held().iter().any(|h| h.id == held[0].id && h.waiting()),
        "the allowance is untouched and still waiting: {:?}",
        r.held()
    );
    let results = r.results(&sid);
    assert!(
        results.iter().all(|(s, _, _)| s == "Declined"),
        "every call of both turns was held: {results:?}"
    );
    assert!(
        results
            .iter()
            .all(|(_, _, reason)| reason.starts_with(HELD_REASON)),
        "each by an ordinary reversibility hold: {results:?}"
    );
    assert!(
        r.rows("tool.confirm_requested").is_empty(),
        "a hold makes no card, however it was reached"
    );
}

/// Turn one holds the original; turn two is an ordinary message that says
/// nothing about it and proposes no call; turn three is the owner's override
/// in words, and the response it draws proposes the original again.
fn script_held_then_asked_then_the_original(path: &Path) -> Vec<Scripted> {
    let original = json!({"path": path.to_string_lossy(), "content": NOW});
    vec![
        Scripted::tools("I'll replace it.", &[("c1", "fs_write", original.clone())]),
        Scripted::text("Held; I'll re-plan it."),
        Scripted::text("It would lose the old bytes."),
        Scripted::tools("The original, then.", &[("c2", "fs_write", original)]),
        Scripted::text("Done."),
    ]
}

/// **The chat override route, end to end through `turn.submit`.** The owner
/// says the original was right in the conversation it was held in, and the
/// call runs — no `undo.allow`, no id typed at a command line. That is the
/// half of `p5kln-override-route` the owner's principle of 2026-11-12 asked
/// for (minimise the cases where a person must drop to a command line), and
/// until now only its WORDS had tests (`allow_in_words::tests`, pure) while
/// the whole-core tests all took the override over the wire by id.
///
/// It goes through `rpc_as`, so the dispatch arm, `allow_in_words` at
/// `turn.submit`, `Core::undo_allow` and `judge_act(Act::UndoAllow)` all run
/// — and the turn the same message starts is what spends the allowance.
///
/// The ordinary message in the middle is the negative that makes the rest
/// mean something: a turn whose words are not an override allows nothing, so
/// the run at the end cannot be the mere passage of time.
#[tokio::test(flavor = "multi_thread")]
async fn the_owners_override_in_words_runs_the_original_through_turn_submit() {
    let r = rig_scripted(
        RESHAPEABLE,
        0.96,
        false,
        |_| {},
        script_held_then_asked_then_the_original,
    );
    let sid = r.session();

    r.turn(&sid).await;
    let held = r.held();
    assert_eq!(held.len(), 1, "one held call: {held:?}");
    assert!(held[0].allowed.is_none(), "nothing has allowed it yet");

    // An ordinary message is no override.
    r.submit(&sid, "What would it lose?").await;
    assert!(
        r.rows("tool.hold_allowed").is_empty(),
        "an ordinary message allows nothing"
    );
    assert!(r.held().iter().all(|h| h.allowed.is_none()));

    // The override, said in the conversation, by the owner in person.
    r.submit(&sid, "run the original").await;

    let allowed = r.rows("tool.hold_allowed");
    assert_eq!(
        allowed.len(),
        1,
        "the words wrote the allowance: {allowed:?}"
    );
    assert_eq!(allowed[0]["hold"], held[0].id);
    assert_eq!(allowed[0]["tool"], "fs.write");

    assert_eq!(
        r.bytes(),
        NOW,
        "the ORIGINAL call ran, on words alone: no id was typed anywhere"
    );
    let spent = r.rows("tool.hold_overridden");
    assert_eq!(spent.len(), 1, "one allowance spent: {spent:?}");
    assert_eq!(spent[0]["hold"], held[0].id);
    assert!(
        r.rows("tool.confirm_requested").is_empty(),
        "an override is not a question"
    );
}

/// The three outcomes of a spend, and the one of them that admits the call.
/// Pure, so the arm no ordinary run can stage is still stated somewhere a
/// reader can check it against the owner's word.
#[test]
fn only_a_spend_this_frame_recorded_admits_the_call() {
    use crate::toolrun::reversible::Spend;
    assert!(Spend::Recorded.admits(), "the allowance was this call's");
    assert!(
        !Spend::AlreadySpent.admits(),
        "one allowance admits ONE call (S34-2)"
    );
    assert!(
        !Spend::Unwritten.admits(),
        "a spend nothing recorded admits nothing"
    );
}

// ---- the two facts the resolver now states, on the LIVE gate path ---------
//
// `p5kln-location-facts` (row B6 `theseus-2yowd`; the owner, 2026-11-13
// 11:28). What these assert is the FACT that reached the request, not the
// answer that came back: the fake Jev answers a scripted verdict whatever the
// state says, so a test that read the verdict would be reading its own
// script. The word is what this row built, and the word is what the labelled
// set states by hand -- so proving the daemon now computes the same word is
// exactly the gap the 1.0.5 review named. Whether Jev then uses it well is
// the eval's measurement, on the eval's own set.

impl Rig {
    /// Every target word in the one `undo.v1` request this turn sent, as
    /// `(path, recovery)`, read out of the fake's recorded body.
    fn words_sent(&self) -> Vec<(String, String)> {
        let seen = self.jev.seen();
        assert_eq!(seen.len(), 1, "asked exactly once: {} times", seen.len());
        // The body is `{"state", ["assets",] "model", "questions"}`
        // (`client.rs`), and the state is the object the builder laid out.
        let state = &seen[0].body["state"];
        state["targets"]
            .as_array()
            .unwrap_or_else(|| panic!("the state carries targets: {state}"))
            .iter()
            .map(|t| {
                (
                    t["path"].as_str().unwrap_or_default().to_string(),
                    t["recovery"].as_str().unwrap_or_default().to_string(),
                )
            })
            .collect()
    }
}

/// A session that WROTE the file it is now rewriting: its own record holds
/// those bytes, so the word is `read_this_session` and not "nothing holds a
/// copy". The card's first bar, on the live path.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_this_session_wrote_reaches_the_request_as_one_the_record_holds() {
    let r = rig(REVERSIBLE, 0.95, false, |_| {});
    let sid = r.session();
    // The session wrote it: exactly what `fs_stale::wrote` records after an
    // `fs.write`, through the board the runtime binds per call.
    let mark = theseus_tools::reads::Mark::at(&r.target).expect("the target's mark");
    r.core.tools.reads.wrote(&sid, &r.target, mark);

    r.turn(&sid).await;
    let words = r.words_sent();
    assert_eq!(words.len(), 1, "one target: {words:?}");
    assert_eq!(
        words[0].1, "read_this_session",
        "a file this session wrote is one its own record holds: {words:?}"
    );
}

/// The same file, changed underneath the session since. The record holds
/// bytes that are no longer the ones the call would destroy, so the honest
/// word is the one that holds the call. The card's second bar.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_changed_underneath_the_session_reaches_the_request_as_one_nothing_holds() {
    let r = rig(REVERSIBLE, 0.95, false, |_| {});
    let sid = r.session();
    let mark = theseus_tools::reads::Mark::at(&r.target).expect("the target's mark");
    r.core.tools.reads.wrote(&sid, &r.target, mark);
    // Somebody else writes it: a different length, so the mark differs
    // whatever the filesystem's mtime resolution is.
    std::fs::write(&r.target, "a third party wrote this, and it is longer\n").unwrap();

    r.turn(&sid).await;
    let words = r.words_sent();
    assert_eq!(words.len(), 1, "one target: {words:?}");
    assert_eq!(
        words[0].1, "outside_repo",
        "the record's bytes are stale, so it claims no copy: {words:?}"
    );
}

/// `rm -rf target` beside a `Cargo.toml`, in a repository that ignores it:
/// `regenerable`, from the closed table. The card's third bar.
#[tokio::test(flavor = "multi_thread")]
async fn rm_rf_a_build_directory_beside_a_manifest_reaches_the_request_as_regenerable() {
    let r = rig_in_a_repo(&["target/\n.env\ndata/\n"], "target", |work| {
        std::fs::write(work.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        std::fs::create_dir_all(work.join("target/debug")).unwrap();
        std::fs::write(work.join("target/debug/app"), "a build output\n").unwrap();
    })
    .await;
    assert_eq!(
        r, "regenerable",
        "the project's own build makes it again, and the table says so"
    );
}

/// An ignored `.env` in the same repository, beside the same manifest. The
/// card's fourth bar, and the one that matters most: nothing recreates it,
/// so it must NOT be `regenerable` however much git ignores it.
#[tokio::test(flavor = "multi_thread")]
async fn an_ignored_env_file_reaches_the_request_as_untracked_and_never_regenerable() {
    let r = rig_in_a_repo(&["target/\n.env\ndata/\n"], ".env", |work| {
        std::fs::write(work.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        std::fs::write(work.join(".env"), "API_BASE=\n").unwrap();
    })
    .await;
    assert_eq!(
        r, "untracked",
        "git holds no copy of it and nothing rebuilds it"
    );
}

/// An ignored data directory beside the same manifest: the dangerous one, and
/// also not `regenerable`.
#[tokio::test(flavor = "multi_thread")]
async fn an_ignored_data_directory_reaches_the_request_as_untracked() {
    let r = rig_in_a_repo(&["target/\n.env\ndata/\n"], "data", |work| {
        std::fs::write(work.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        std::fs::create_dir_all(work.join("data")).unwrap();
        std::fs::write(work.join("data/measurements.csv"), "1,2,3\n").unwrap();
    })
    .await;
    assert_eq!(r, "untracked", "a data directory is nobody's build output");
}

/// A rig whose working root is a real git repository, with `ignore` as its
/// `.gitignore` and `arrange` run inside it; the scripted call is an
/// `fs.write` to `rel` under it. Returns the one recovery word the request
/// carried for that target.
async fn rig_in_a_repo(ignore: &[&str], rel: &str, arrange: impl FnOnce(&Path)) -> String {
    let jev = says(REVERSIBLE, 0.95);
    let dir = crate::scratch::tempdir();
    let base = dir.path().canonicalize().unwrap();
    let (state, work, holdout) = (base.join("state"), base.join("work"), base.join("sealed"));
    for d in [&state, &work, &holdout] {
        std::fs::create_dir_all(d).unwrap();
    }
    // A hermetic repository: none of the operator's git configuration, and
    // signing off, so a locked agent cannot hang it (AGENTS.md, Item 7).
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
            .env("GIT_COMMITTER_NAME", "Fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .current_dir(&work)
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .output()
            .expect("git runs");
        assert!(out.status.success(), "git {args:?}");
    };
    git(&["init", "-q", "-b", "main"]);
    std::fs::write(work.join(".gitignore"), ignore.concat()).unwrap();
    std::fs::write(work.join("kept.txt"), WAS).unwrap();
    git(&["add", "kept.txt", ".gitignore"]);
    git(&["commit", "-q", "-m", "the first commit"]);
    arrange(&work);

    let target = work.join(rel);
    let mut cfg = Config::example();
    cfg.server.state_dir = state.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(work.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = Posture::Open;
    cfg.self_improve.sealed.dir = Some(holdout.to_string_lossy().into_owned());
    cfg.judge.enabled = true;
    cfg.judge.api_base = jev.base();
    cfg.judge.connect_secs = 1;
    cfg.judge.total_secs = 3;
    for p in crate::judge::inbound::PACKS {
        cfg.judge.packs.insert(p.into(), off());
    }
    for (p, _) in crate::judge::WIRED {
        if *p != crate::judge::undo::PACK_NAME {
            cfg.judge.packs.insert((*p).into(), off());
        }
    }
    cfg.validate().unwrap();

    let store = Store::open(&state.join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script(&target)));
    let mut p = Parts::for_tests(cfg, fake, store);
    p.secrets = board();
    let core = Core::build(p).unwrap();
    warm(&core);
    let r = Rig {
        core,
        target,
        jev,
        _dir: dir,
    };
    let sid = r.session();
    r.turn(&sid).await;
    let words = r.words_sent();
    assert_eq!(words.len(), 1, "one target: {words:?}");
    words[0].1.clone()
}
