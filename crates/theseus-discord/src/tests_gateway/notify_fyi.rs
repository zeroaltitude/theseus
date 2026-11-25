//! An FYI in the notices place, when that place fails and when it is pressed
//! (theseus-nuqut), through the stand-in's gateway. The DM never waits on the
//! notices place: a definite refusal there sends the FYI to the DM once, a
//! place that does not answer holds up nothing bound for the DM, and a send
//! that may have landed is never sent again. Only a kind that informs moves,
//! whatever another kind's words say. A press in the notices place is judged
//! as any press is: Undo is the owner's, from a private place; Halt is any
//! admitted person's.

use std::sync::Arc;

use serde_json::json;
use theseus_core::approval::{Answerer, Surface};
use theseus_core::config::discord::Category;
use theseus_core::config::self_improve::SelfMode;
use theseus_core::owner_note::{note, Kind};
use theseus_core::provider::{FakeProvider, Provider};
use theseus_kernel::ActionState;
use theseus_protocol::rsi::SelfResumeParams;
use theseus_sim::fake_discord::{FakeDiscord, Guild, Mode, DEFAULT_GUILD};

use super::{Rig, ANA, ANA_DM, BEN, CY, LAB};

/// The notices channel, an invented id.
const NOTICES: u64 = 900_000_000_000_000_040;

fn model(_: &std::path::Path, _: Arc<FakeDiscord>) -> Arc<dyn Provider> {
    Arc::new(FakeProvider::scripted(vec![]))
}

/// `#lab`, the notices channel (bound `notify = true` when `notify`, private
/// when `private`, ana and ben admitted) and ana's DM.
fn file(notify: bool, private: bool) -> String {
    format!(
        "guild_id = \"{DEFAULT_GUILD}\"\n\
         [[channel]]\nid = \"{LAB}\"\nname = \"lab\"\nusers = [\"{ANA}\"]\nmention_only = false\nprivate = true\n\
         [[channel]]\nid = \"{NOTICES}\"\nname = \"notices\"\nusers = [\"{ANA}\", \"{BEN}\"]\nmention_only = false\nprivate = {private}\nnotify = {notify}\n\
         [[dm]]\nuser = \"{ANA}\"\nname = \"ana\"\n"
    )
}

fn guild(private: bool) -> Guild {
    let g = Guild::new(DEFAULT_GUILD, (ANA, "ana"))
        .member(BEN, "ben")
        .member(CY, "cy")
        .private_channel(LAB, "lab", &[ANA]);
    match private {
        true => g.private_channel(NOTICES, "notices", &[ANA, BEN]),
        false => g.channel(NOTICES, "notices"),
    }
}

/// Ana is the owner; with `silent`, a notice posts without a ping.
async fn rig(notify: bool, private: bool, silent: bool) -> Rig {
    Rig::start_tweaked(
        model,
        guild(private),
        &file(notify, private),
        &[],
        move |c| {
            c.places.owner = Some(vec![format!("discord:{ANA}")]);
            c.self_improve.mode = SelfMode::Act;
            c.self_improve.backlog_starts = Some(0);
            if silent {
                c.discord.silent = vec![Category::Notices];
            }
        },
    )
    .await
}

/// One operator post, as the core stages it; its correlation id back.
fn stage(r: &Rig, body: serde_json::Value) -> String {
    let (a, frame) = r.core.outbox.stage_to_operator(None, body).unwrap();
    r.core.store.append(&frame).unwrap();
    r.core.outbox.posted(&a);
    a.correlation_id
}

/// The ontology's mint, as the core words it: an FYI with its Undo.
fn mint(text: &str) -> serde_json::Value {
    json!({"kind": "onto_card", "write": "jdg_mint", "undo": true,
           "text": note(Kind::Fyi, "Theseus started keeping track of something new",
                        "In \"pier\", this topic came up and Theseus had no record of it:",
                        &format!("{text}\nUndo: `theseus ontology undo jdg_mint`"))})
}

/// A live owner-card question: a card that waits on the owner.
fn question(text: &str) -> serde_json::Value {
    json!({"kind": "card_question", "card": {"id": "cdp_00000001", "ask": "add"},
           "text": note(Kind::Decision, "Keep this as a fact?", "a card waits on you", text)})
}

fn alert(text: &str) -> serde_json::Value {
    json!({"kind": "hands_alert",
           "text": note(Kind::Alert, "A hand ran away", "its spend", text)})
}

fn times(r: &Rig, channel: u64, text: &str) -> usize {
    r.posted(channel)
        .iter()
        .filter(|m| m.versions[0].contains(text))
        .count()
}

async fn landed(r: &Rig, channel: u64, text: &str) {
    r.until(&format!("{text:?} in {channel}"), || {
        times(r, channel, text) > 0
    })
    .await;
}

fn state(r: &Rig, id: &str) -> Option<ActionState> {
    let a = r.core.kernel.outbox_action(id).ok().flatten()?;
    Some(a.state)
}

/// Creates in the notices channel the fake answered with a failure.
fn failed_there(r: &Rig) -> usize {
    let path = format!("/channels/{NOTICES}/messages");
    r.fake
        .seen()
        .iter()
        .filter(|s| s.path == path && s.outcome == "failed")
        .count()
}

/// The notices channel is gone (404) or closed to the bot (403): Discord
/// refused the FYI for good, so it is written to the owner's DM, once, with
/// its Undo; the next FYI does the same, and nothing lands in the channel.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_definite_refusal_in_the_notices_place_sends_the_fyi_to_the_dm_once() {
    for status in [403, 404] {
        let r = rig(true, true, false).await;
        r.fake.fail_creates_in(NOTICES, Some(status));
        let first = stage(&r, mint("fyi-refused-one"));
        landed(&r, ANA_DM, "fyi-refused-one").await;
        // A later FYI behind it: by the time it lands, the first settled.
        stage(&r, mint("fyi-refused-two"));
        landed(&r, ANA_DM, "fyi-refused-two").await;
        assert_eq!(times(&r, ANA_DM, "fyi-refused-one"), 1, "{status}: once");
        assert_eq!(times(&r, NOTICES, "fyi-refused-one"), 0, "{status}");
        assert_eq!(state(&r, &first), Some(ActionState::Succeeded), "{status}");
        let m = r
            .posted(ANA_DM)
            .into_iter()
            .find(|m| m.versions[0].contains("fyi-refused-one"))
            .unwrap();
        assert_eq!(m.buttons[0].custom_id, "onto:undo:jdg_mint", "{status}");
        assert!(
            failed_there(&r) >= 2,
            "{status}: both were tried there first"
        );
        r.stop();
    }
}

/// The notices channel does not answer (503): the FYI waits aside, and a
/// card waiting on the owner and an alert queued behind it reach the DM while
/// the channel is still failing. Once it answers, the FYI lands there, once,
/// and never in the DM.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_notices_place_that_does_not_answer_holds_up_nothing_bound_for_the_dm() {
    let r = rig(true, true, false).await;
    r.fake.fail_creates_in(NOTICES, Some(503));
    stage(&r, mint("fyi-waits-aside"));
    r.until("the FYI was tried there", || failed_there(&r) > 0)
        .await;
    stage(&r, question("dm-card-behind"));
    stage(&r, alert("dm-alert-behind"));
    landed(&r, ANA_DM, "dm-card-behind").await;
    landed(&r, ANA_DM, "dm-alert-behind").await;
    assert_eq!(times(&r, NOTICES, "fyi-waits-aside"), 0);
    assert_eq!(times(&r, ANA_DM, "fyi-waits-aside"), 0);
    r.fake.fail_creates_in(NOTICES, None);
    landed(&r, NOTICES, "fyi-waits-aside").await;
    assert_eq!(times(&r, NOTICES, "fyi-waits-aside"), 1);
    assert_eq!(
        times(&r, ANA_DM, "fyi-waits-aside"),
        0,
        "never also in the DM"
    );
    r.stop();
}

/// A send to the notices place that may have landed (a 503) is never sent
/// anywhere else: when the place then refuses for good, the FYI settles as
/// failed and is not written to the DM, so it can never be there twice.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_fyi_that_may_have_landed_is_never_sent_to_the_dm_as_well() {
    let r = rig(true, true, false).await;
    r.fake.fail_creates_in(NOTICES, Some(503));
    let id = stage(&r, mint("fyi-maybe-landed"));
    r.until("the FYI was tried there", || failed_there(&r) > 0)
        .await;
    r.fake.fail_creates_in(NOTICES, Some(403));
    r.until("the FYI settled", || {
        state(&r, &id) == Some(ActionState::Failed)
    })
    .await;
    stage(&r, alert("dm-after"));
    landed(&r, ANA_DM, "dm-after").await;
    assert_eq!(times(&r, ANA_DM, "fyi-maybe-landed"), 0);
    assert_eq!(times(&r, NOTICES, "fyi-maybe-landed"), 0);
    r.stop();
}

/// Only a kind that informs moves: a card question, a retraction and a self
/// branch's keel hold whose words are headed FYI stay in the DM.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_kind_that_asks_stays_in_the_dm_whatever_its_words_say() {
    let r = rig(true, true, false).await;
    let fyi = |t: &str| note(Kind::Fyi, "Nothing to do", "just so you know", t);
    let kept = [
        ("card_question", "dm-fyi-headed-question"),
        ("retraction", "dm-fyi-headed-retraction"),
        ("self_keel", "dm-fyi-headed-keel"),
    ];
    for (kind, text) in kept {
        stage(
            &r,
            json!({"kind": kind, "text": fyi(text), "card": {"id": "cdp_00000002", "ask": "add"}}),
        );
    }
    stage(&r, mint("fyi-mint-moves"));
    landed(&r, NOTICES, "fyi-mint-moves").await;
    for (_, text) in kept {
        landed(&r, ANA_DM, text).await;
        assert_eq!(times(&r, NOTICES, text), 0, "{text}");
    }
    r.stop();
}

/// Back from an outage with four silent notices and a card waiting in the
/// operator lane: the lane has no place to fold them into, so it folds
/// nothing and delivers each, the card included, to the DM.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn after_an_outage_the_operator_lane_folds_nothing_and_delivers_what_waited() {
    let r = rig(false, true, true).await;
    r.fake.set_mode(Mode::Down);
    let plain = |t: &str| json!({"kind": "notice", "text": format!("plain notice {t}")});
    for t in ["n1", "n2", "n3", "n4"] {
        stage(&r, plain(t));
    }
    stage(&r, question("dm-card-after-outage"));
    r.until("the lane met the outage", || {
        r.fake.seen().iter().any(|s| s.outcome == "dropped")
    })
    .await;
    // Long enough for the operator lane to have tried its first post and
    // backed off (its first backoff is two seconds).
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    r.fake.set_mode(Mode::Up);
    landed(&r, ANA_DM, "dm-card-after-outage").await;
    for t in ["n1", "n2", "n3", "n4"] {
        landed(&r, ANA_DM, &format!("plain notice {t}")).await;
    }
    r.stop();
}

/// Who answers an interaction, and how its answer starts.
fn told(r: &Rig, pressed: &str, start: &str) -> impl Fn() -> bool {
    let fake = r.fake.clone();
    let (pressed, start) = (pressed.to_string(), start.to_string());
    move || {
        fake.replies().iter().any(|x| {
            x.interaction.as_deref() == Some(pressed.as_str())
                && x.content.as_deref().is_some_and(|c| c.starts_with(&start))
        })
    }
}

/// The self note of a join that conflicted: an FYI with Halt alone.
fn conflicted() -> serde_json::Value {
    json!({"kind": "self_join",
           "text": note(Kind::Fyi, "Self branch self/pier conflicted with main",
                        "its join found a conflict (tsk_pier)",
                        "fyi-join-conflict. Nothing waits for you.")})
}

/// Presses in the notices place, private and shared, by the owner and by an
/// admitted guest. Undo on the moved mint is the owner's from a private place
/// alone (the write is invented, so the owner's press that counts is answered
/// that the undo was not made: it passed the gate and found no such write).
/// Halt on a moved self note is anyone admitted's, wherever, as it is today.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn presses_in_the_notices_place_are_judged_as_any_press_is() {
    for private in [true, false] {
        let r = rig(true, private, false).await;
        stage(&r, mint("fyi-press-mint"));
        stage(&r, conflicted());
        landed(&r, NOTICES, "fyi-press-mint").await;
        landed(&r, NOTICES, "fyi-join-conflict").await;
        let find = |w: &str| {
            r.posted(NOTICES)
                .into_iter()
                .find(|m| m.versions[0].contains(w))
                .unwrap()
        };
        let (mint, join) = (find("fyi-press-mint"), find("fyi-join-conflict"));
        let labels: Vec<&str> = join.buttons.iter().map(|b| b.label.as_str()).collect();
        assert_eq!(labels, ["Halt self-improvement"], "{join:?}");

        // Undo: a guest never counts; the owner counts in a private place only.
        let p = r.press(&mint.id, "Undo", (BEN, "ben"));
        r.until("ben's undo", told(&r, &p, "🔐 Your undo did not count"))
            .await;
        let p = r.press(&mint.id, "Undo", (ANA, "ana"));
        match private {
            true => {
                r.until(
                    "ana's undo passed the gate",
                    told(&r, &p, "⚠️ The undo was not made"),
                )
                .await
            }
            false => {
                r.until(
                    "ana's undo in a shared place",
                    told(&r, &p, "🔐 Your undo did not count"),
                )
                .await
            }
        }

        // Halt: anyone admitted, here as in the DM. A store never resumed is
        // halted, so the owner resumes it first, from his own shell.
        r.core
            .self_resume(
                &SelfResumeParams::default(),
                &Answerer {
                    label: "the CLI".into(),
                    surface: Surface::Cli,
                    discord: None,
                    agent: None,
                },
            )
            .unwrap();
        assert!(!r.core.self_state().halted);
        let p = r.press(&join.id, "Halt self-improvement", (BEN, "ben"));
        r.until("ben halted", told(&r, &p, "🛑 Self-improvement halted"))
            .await;
        assert!(r.core.self_state().halted, "private = {private}");
        let p = r.press(&join.id, "Halt self-improvement", (ANA, "ana"));
        r.until(
            "already",
            told(&r, &p, "🛑 Self-improvement was already halted"),
        )
        .await;
        r.stop();
    }
}
