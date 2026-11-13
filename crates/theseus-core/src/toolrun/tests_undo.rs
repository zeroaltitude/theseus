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
use crate::turn::TurnRequest;
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
    assert_eq!(status, "Error");
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
