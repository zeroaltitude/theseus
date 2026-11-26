//! A standalone note that only informs, folded (issue 56), end to end through
//! the stand-in's gateway: in a conversation that folds its steps (the
//! default), an FYI of a kind the notices rule names, with no button of its
//! own, is one closed line with Show note, which opens it to the words an
//! open place shows and closes it again. A press is honoured only on the
//! exact message its post was written to folded, by channel and message:
//! never on a note written open, never on a note with a button, never from
//! another channel. Folding is decided where the note actually lands: the
//! notices place never folds, and a note it refuses for good is folded in
//! the DM as any DM note is. A note that comes while a turn runs is the
//! same: its own closed line beside the turn's fold, never inside it. A
//! card, a question, an alert, a refusal, an error, a reminder, a decision,
//! an action note and a met watch are never folded.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_core::owner_note::told::{self, Undo};
use theseus_core::owner_note::{note, Kind};
use theseus_core::provider::{FakeProvider, ProviderError, Scripted};
use theseus_kernel::ActionState;
use theseus_sim::fake_discord::{Forged, Guild, Msg, DEFAULT_GUILD};

use crate::tests_gateway::{Rig, ANA, ANA_DM, CY, LAB};

/// The notices channel, an invented id.
const NOTICES: u64 = 900_000_000_000_000_040;

const OPEN: &str = "steps = \"open\"\n";

/// A plain notice's closed line.
const LINE: &str = "-# ℹ️ FYI: a note";

const NOT_HERE: &str = "This button is not this message's";
const TOO_OLD: &str = "This note is too old to show again";
const AWAY: &str = "Discord is not taking this conversation's messages just now";

/// The rig's vault value (`tests_gateway`): masked at every exit.
const VALUE: &str = "jev-test-key-0123456789";

/// `#lab` and ana's DM with `dm`'s words; with `notices`, the notices
/// channel bound `notify = true` with those words.
fn file(dm: &str, notices: Option<&str>) -> String {
    let more = notices.map_or_else(String::new, |w| {
        format!(
            "[[channel]]\nid = \"{NOTICES}\"\nname = \"notices\"\nusers = [\"{ANA}\"]\nmention_only = false\nprivate = true\nnotify = true\n{w}"
        )
    });
    format!(
        "guild_id = \"{DEFAULT_GUILD}\"\n\
         [[channel]]\nid = \"{LAB}\"\nname = \"lab\"\nusers = [\"{ANA}\"]\nmention_only = false\nprivate = true\n\
         {more}[[dm]]\nuser = \"{ANA}\"\nname = \"ana\"\n{dm}"
    )
}

fn guild() -> Guild {
    Guild::new(DEFAULT_GUILD, (ANA, "ana"))
        .member(CY, "cy")
        .private_channel(LAB, "lab", &[ANA])
        .private_channel(NOTICES, "notices", &[ANA])
}

/// Ana alone owns the rig.
async fn rig(dm: &str, notices: Option<&str>, script: Vec<Scripted>) -> Rig {
    Rig::start_tweaked(
        |_, _| Arc::new(FakeProvider::scripted(script)),
        guild(),
        &file(dm, notices),
        &[],
        |c| c.places.owner = Some(vec![format!("discord:{ANA}")]),
    )
    .await
}

/// An operator post, as the core stages one: its correlation id.
fn stage(r: &Rig, body: Value) -> String {
    let (a, frame) = r.core.outbox.stage_to_operator(None, body).unwrap();
    r.core.store.append(&frame).unwrap();
    r.core.outbox.posted(&a);
    a.correlation_id
}

/// A plain FYI notice's words, as the core writes one.
fn fyi_text(words: &str) -> String {
    note(Kind::Fyi, "Personas moved", "a start", words)
}

fn fyi(words: &str) -> Value {
    json!({"kind": "notice", "text": fyi_text(words)})
}

fn labels(m: &Msg) -> Vec<String> {
    m.buttons.iter().map(|b| b.label.clone()).collect()
}

/// Whether `m` ever carried a folded note's button.
fn note_fold(m: &Msg) -> bool {
    m.buttons
        .iter()
        .any(|b| b.custom_id.starts_with("fold:no:") || b.custom_id.starts_with("fold:nc:"))
}

/// The bot's message in `channel` whose first version holds `s`.
fn first_with(r: &Rig, channel: u64, s: &str) -> Option<Msg> {
    r.posted(channel)
        .into_iter()
        .find(|m| m.versions[0].contains(s))
}

fn message(r: &Rig, channel: u64, id: &str) -> Msg {
    r.fake
        .messages(channel)
        .into_iter()
        .find(|m| m.id == id)
        .unwrap()
}

/// Whether the presser of `pressed` was told, alone, a line starting `s`.
fn told(r: &Rig, pressed: &str, s: &str) -> bool {
    r.fake.replies().iter().any(|x| {
        x.interaction.as_deref() == Some(pressed)
            && x.ephemeral
            && x.content.as_deref().is_some_and(|c| c.starts_with(s))
    })
}

/// A press of `custom_id` on message `id`, as a crafted client could send
/// it, by `user`, from `channel` (the message's own when None).
fn forge(r: &Rig, id: &str, custom_id: &str, user: (u64, &str), channel: Option<u64>) -> String {
    r.fake
        .press_forged(&Forged {
            message: id,
            custom_id,
            user,
            channel,
        })
        .unwrap()
}

/// The folded note whose post is `post`, in `channel`, once written.
async fn folded(r: &Rig, channel: u64, post: &str) -> Msg {
    let id = format!("fold:no:{post}");
    let has = |m: &Msg| m.buttons.iter().any(|b| b.custom_id == id);
    r.until("the folded note", || r.posted(channel).iter().any(has))
        .await;
    r.posted(channel).into_iter().find(has).unwrap()
}

/// A press of `button` on message `id` by ana, sent once the stand-in's
/// gateway has a client to take it (a new process connects after its start
/// returns): the interaction's id.
async fn press(r: &Rig, id: &str, button: &str) -> String {
    let sent = std::sync::Mutex::new(None);
    r.until("the press taken by the gateway", || {
        let p = r.fake.press(&theseus_sim::fake_discord::Pressed {
            message: id,
            button,
            user: ANA,
            name: "ana",
        });
        p.map(|i| *sent.lock().unwrap() = Some(i)).is_ok()
    })
    .await;
    sent.into_inner().unwrap().unwrap()
}

/// Show note, then Hide note, on `m` in `channel`: open, its words are
/// `open`; closed, its line is `line` again.
async fn open_and_close(r: &Rig, channel: u64, m: &Msg, open: &str, line: &str) {
    press(r, &m.id, "Show note").await;
    r.until("the note open", || {
        message(r, channel, &m.id).content == open
    })
    .await;
    assert_eq!(labels(&message(r, channel, &m.id)), ["Hide note"]);
    press(r, &m.id, "Hide note").await;
    r.until("the note closed", || {
        message(r, channel, &m.id).content == line
    })
    .await;
    assert_eq!(labels(&message(r, channel, &m.id)), ["Show note"]);
}

/// N1 and N5: a DM bound open gets the note as before, its words, masked,
/// no button. The same note in a DM that folds is one closed line said from
/// its kind, with Show note, pinging as the open note did, its words in no
/// version; open, it is exactly the open DM's message; Hide note closes it.
#[tokio::test]
async fn a_standalone_fyi_in_a_folded_dm_is_one_line_that_opens_to_the_open_dms_words() {
    let words = format!("The persona files moved; the key {VALUE} stays home.");
    let r = rig(OPEN, None, vec![]).await;
    stage(&r, fyi(&words));
    r.until("the open note", || {
        first_with(&r, ANA_DM, "persona files moved").is_some()
    })
    .await;
    let open = first_with(&r, ANA_DM, "persona files moved").unwrap();
    assert!(open.buttons.is_empty(), "{open:#?}");
    assert!(!open.content.contains(VALUE), "masked: {open:#?}");
    let masked = r.core.exits.text(&fyi_text(&words)).0.into_owned();
    assert_eq!(open.content, masked, "today's words, the value masked");
    r.stop();

    let r = rig("", None, vec![]).await;
    let post = stage(&r, fyi(&words));
    let m = folded(&r, ANA_DM, &post).await;
    assert_eq!(m.content, LINE);
    assert_eq!(labels(&m), ["Show note"]);
    assert_eq!(m.silent(), open.silent(), "it pings as the open note did");
    assert!(
        r.posted(ANA_DM)
            .iter()
            .all(|x| x.versions.iter().all(|v| !v.contains("persona files"))),
        "the words were never shown closed"
    );
    open_and_close(&r, ANA_DM, &m, &open.content, LINE).await;
    for v in &message(&r, ANA_DM, &m.id).versions {
        assert!(!v.contains(VALUE), "the value left in {v:?}");
    }
}

/// N2: a forged Show or Hide note on a note with a button of its own (the
/// ontology's mint with its Undo, a told note with its Undo), by the owner
/// on the same message, and by a non-member from another channel, changes
/// nothing: the presser is told it is not this message's, and the words, the
/// buttons and the edit count are as they were. So does a forged press on a
/// note written open. The folded note beside them still opens.
#[tokio::test]
async fn a_forged_note_press_never_folds_a_note_with_a_button_or_one_written_open() {
    let r = rig("", None, vec![]).await;
    let mint = stage(
        &r,
        json!({"kind": "onto_card", "write": "jdg_mint", "undo": true,
               "text": note(Kind::Fyi, "Theseus started keeping track of something new",
                            "In pier", "mint-with-undo")}),
    );
    let (a, frame) = told::stage(
        &r.core.outbox,
        None,
        "Theseus kept a standing order",
        "In “Tides”, without asking you",
        "told-with-undo",
        &Undo::StandingRevoke("std_forged".into()),
    )
    .unwrap();
    r.core.store.append(&frame).unwrap();
    r.core.outbox.posted(&a);
    let told_post = a.correlation_id;
    let plain = stage(&r, fyi("plain-folded"));
    let f = folded(&r, ANA_DM, &plain).await;
    for (post, words) in [(&mint, "mint-with-undo"), (&told_post, "told-with-undo")] {
        r.until(words, || first_with(&r, ANA_DM, words).is_some())
            .await;
        let before = first_with(&r, ANA_DM, words).unwrap();
        assert!(
            !before.buttons.is_empty() && !note_fold(&before),
            "{before:#?}"
        );
        let presses = [
            (format!("fold:no:{post}"), (ANA, "ana"), None),
            (format!("fold:nc:{post}"), (ANA, "ana"), None),
            (format!("fold:no:{post}"), (CY, "cy"), Some(LAB)),
        ];
        for (id, who, from) in presses {
            let pressed = forge(&r, &before.id, &id, who, from);
            r.until("the presser told", || told(&r, &pressed, NOT_HERE))
                .await;
            let m = message(&r, ANA_DM, &before.id);
            assert_eq!(
                (&m.content, &m.buttons, m.edits),
                (&before.content, &before.buttons, before.edits),
                "{id} by {who:?}: {m:#?}"
            );
        }
    }
    assert_eq!(
        first_with(&r, ANA_DM, "mint-with-undo").unwrap().buttons[0].custom_id,
        "onto:undo:jdg_mint",
        "the Undo is still there"
    );
    // After the forged presses, in the same lane: the real one is honoured.
    open_and_close(&r, ANA_DM, &f, &fyi_text("plain-folded"), LINE).await;
    r.stop();

    // A note written open (the DM says open): a forged press changes nothing.
    let r = rig(OPEN, None, vec![]).await;
    let post = stage(&r, fyi("written-open"));
    r.until("the open note", || {
        first_with(&r, ANA_DM, "written-open").is_some()
    })
    .await;
    let before = first_with(&r, ANA_DM, "written-open").unwrap();
    for id in [format!("fold:nc:{post}"), format!("fold:no:{post}")] {
        let pressed = forge(&r, &before.id, &id, (ANA, "ana"), None);
        r.until("the presser told", || told(&r, &pressed, NOT_HERE))
            .await;
        let m = message(&r, ANA_DM, &before.id);
        assert_eq!(
            (&m.content, &m.buttons, m.edits),
            (&before.content, &before.buttons, before.edits),
            "{m:#?}"
        );
    }
}

/// N2: a press on a folded note that says it comes from another channel
/// than the one the note was written to is refused, by the owner as by a
/// non-member, and the note is unchanged; a note's id forged onto another
/// message shows nothing there.
#[tokio::test]
async fn a_note_press_from_another_channel_or_on_another_message_changes_nothing() {
    let r = rig("", None, vec![Scripted::text("An answer.")]).await;
    let post = stage(&r, fyi("only-its-own-message"));
    let f = folded(&r, ANA_DM, &post).await;
    for who in [(ANA, "ana"), (CY, "cy")] {
        let pressed = forge(&r, &f.id, &format!("fold:no:{post}"), who, Some(LAB));
        r.until("the presser told", || told(&r, &pressed, NOT_HERE))
            .await;
        let m = message(&r, ANA_DM, &f.id);
        assert_eq!((m.content.as_str(), m.edits), (LINE, f.edits), "{m:#?}");
    }
    r.say((ANA, "ana"), None, "Hello?");
    r.until("the answer", || {
        first_with(&r, ANA_DM, "An answer.").is_some()
    })
    .await;
    let other = first_with(&r, ANA_DM, "An answer.").unwrap();
    let pressed = forge(
        &r,
        &other.id,
        &format!("fold:no:{post}"),
        (ANA, "ana"),
        None,
    );
    r.until("the presser told", || told(&r, &pressed, NOT_HERE))
        .await;
    let m = message(&r, ANA_DM, &other.id);
    assert_eq!((&m.content, m.edits), (&other.content, other.edits));
    assert!(!m.content.contains("only-its-own-message"));
    open_and_close(&r, ANA_DM, &f, &fyi_text("only-its-own-message"), LINE).await;
}

/// N3: a note the notices place takes is never folded there, whatever that
/// place's steps word says and whatever `announce` says: a plain FYI and an
/// FYI that says `announce`, both its words, no button.
#[tokio::test]
async fn the_notices_place_never_folds_a_note_whatever_it_or_announce_says() {
    for words in ["steps = \"folded\"\n", ""] {
        let r = rig("", Some(words), vec![]).await;
        stage(&r, fyi("notices-plain"));
        let mut announced = fyi("notices-announced");
        announced["announce"] = json!(true);
        stage(&r, announced);
        for w in ["notices-plain", "notices-announced"] {
            r.until(w, || first_with(&r, NOTICES, w).is_some()).await;
            let m = first_with(&r, NOTICES, w).unwrap();
            assert_eq!(m.content, fyi_text(w), "{words:?}");
            assert!(m.buttons.is_empty(), "{m:#?}");
            assert!(first_with(&r, ANA_DM, w).is_none(), "{w} is not in the DM");
        }
        r.stop();
    }
}

/// N3: the notices place refuses a note for good (403): it goes to the DM,
/// once, and there it is folded as any DM note is, opening to its words; a
/// DM bound open gets its words, as before. The notices place's own
/// routing is unchanged: it was tried there first, and nothing landed there.
#[tokio::test]
async fn a_note_the_notices_place_refuses_is_folded_in_the_dm_by_the_dms_own_word() {
    let tried = |r: &Rig| {
        let path = format!("/channels/{NOTICES}/messages");
        r.fake
            .seen()
            .iter()
            .filter(|s| s.path == path && s.outcome == "failed")
            .count()
    };
    let r = rig("", Some(""), vec![]).await;
    r.fake.fail_creates_in(NOTICES, Some(403));
    let post = stage(&r, fyi("refused-to-the-dm"));
    let f = folded(&r, ANA_DM, &post).await;
    assert_eq!(f.content, LINE);
    assert!(tried(&r) >= 1, "tried in the notices place first");
    assert!(r
        .posted(NOTICES)
        .iter()
        .all(|m| !m.content.contains("refused")));
    r.until("settled", || {
        r.core
            .kernel
            .outbox_action(&post)
            .ok()
            .flatten()
            .map(|a| a.state)
            == Some(ActionState::Succeeded)
    })
    .await;
    open_and_close(&r, ANA_DM, &f, &fyi_text("refused-to-the-dm"), LINE).await;
    r.stop();

    let r = rig(OPEN, Some(""), vec![]).await;
    r.fake.fail_creates_in(NOTICES, Some(403));
    stage(&r, fyi("refused-open"));
    r.until("the open note", || {
        first_with(&r, ANA_DM, "refused-open").is_some()
    })
    .await;
    let m = first_with(&r, ANA_DM, "refused-open").unwrap();
    assert_eq!(m.content, fyi_text("refused-open"));
    assert!(m.buttons.is_empty(), "{m:#?}");
    assert!(tried(&r) >= 1);
}

/// Every message in the DM that `ours` names was written open: no folded
/// note's button, no folded line, in any version.
fn never_folded(r: &Rig, ours: impl Fn(&Msg) -> bool) {
    let mut seen = 0;
    for m in r.posted(ANA_DM) {
        if !ours(&m) {
            continue;
        }
        assert!(!note_fold(&m), "never folded: {m:#?}");
        assert!(m.versions.iter().all(|v| !v.starts_with("-# ℹ️")), "{m:#?}");
        seen += 1;
    }
    assert!(seen > 0, "nothing checked");
}

/// N4: in a DM that folds, none of these is folded, each written as ever:
/// a question with its buttons, an alert, a reminder, a decision and an
/// action note, a met watch, the mint and a told note with their Undo, a
/// self note with its Halt. A plain FYI beside them is folded, so the rule
/// is live in this rig.
#[tokio::test]
async fn what_needs_the_owner_and_what_has_a_button_is_never_folded() {
    let r = rig("", None, vec![]).await;
    let n = |k: Kind, w: &str| note(k, "Something", "a reason", w);
    let staged = [
        json!({"kind": "card_question", "card": {"id": "cdp_00000001", "ask": "add"},
               "text": n(Kind::Fyi, "never-question")}),
        json!({"kind": "hands_alert", "text": n(Kind::Alert, "never-alert")}),
        json!({"kind": "notice", "text": n(Kind::Reminder, "never-reminder")}),
        json!({"kind": "notice", "text": n(Kind::Decision, "never-decision")}),
        json!({"kind": "notice", "text": n(Kind::Action, "never-action")}),
        json!({"kind": "notice", "purpose": "watch.met", "wake_id": "wk_1",
               "text": n(Kind::Fyi, "never-met-watch")}),
        json!({"kind": "onto_card", "write": "jdg_mint", "undo": true,
               "text": n(Kind::Fyi, "never-mint")}),
        json!({"kind": "self_join", "text": n(Kind::Fyi, "never-self-join")}),
    ];
    for b in staged {
        stage(&r, b);
    }
    let (a, frame) = told::stage(
        &r.core.outbox,
        None,
        "Theseus kept a standing order",
        "without asking you",
        "never-told",
        &Undo::StandingRevoke("std_never".into()),
    )
    .unwrap();
    r.core.store.append(&frame).unwrap();
    r.core.outbox.posted(&a);
    let plain = stage(&r, fyi("folded-beside"));
    folded(&r, ANA_DM, &plain).await;
    let open_words = [
        "never-question",
        "never-alert",
        "never-reminder",
        "never-decision",
        "never-action",
        "never-met-watch",
        "never-mint",
        "never-self-join",
        "never-told",
    ];
    for w in open_words {
        r.until(w, || first_with(&r, ANA_DM, w).is_some()).await;
    }
    for w in [
        "never-question",
        "never-mint",
        "never-self-join",
        "never-told",
    ] {
        let m = first_with(&r, ANA_DM, w).unwrap();
        assert!(!m.buttons.is_empty(), "{w} keeps its buttons: {m:#?}");
    }
    never_folded(&r, |m| open_words.iter().any(|w| m.versions[0].contains(w)));
}

/// N4: in a DM that folds, a turn's approval card, its refusal once
/// declined, and the error that ends the turn are written as ever, never as
/// a folded note.
#[tokio::test]
async fn a_card_its_refusal_and_a_turns_error_are_never_folded_notes() {
    let r = Rig::start_tweaked(
        |dir, _| {
            let out = dir.join("outside").join("proof.txt");
            Arc::new(FakeProvider::scripted(vec![
                Scripted::tools(
                    "Writing it.",
                    &[(
                        "w1",
                        "fs_write",
                        json!({"path": out.to_string_lossy(), "content": "x"}),
                    )],
                ),
                Scripted::Fail(ProviderError::InvalidRequest {
                    status: 400,
                    message: "the request was refused".into(),
                }),
            ]))
        },
        guild(),
        &file("", None),
        &[],
        |c| c.places.owner = Some(vec![format!("discord:{ANA}")]),
    )
    .await;
    r.say((ANA, "ana"), None, "Write the proof.");
    r.until("the card", || {
        first_with(&r, ANA_DM, "**Approve?**").is_some()
    })
    .await;
    let card = first_with(&r, ANA_DM, "**Approve?**").unwrap();
    assert!(labels(&card).contains(&"Approve".to_string()), "{card:#?}");
    r.press(&card.id, "Decline", (ANA, "ana"));
    r.until("the card settled and the turn failed", || {
        message(&r, ANA_DM, &card.id).edits > card.edits
            && r.posted(ANA_DM)
                .iter()
                .any(|m| m.content.contains("Turn failed"))
    })
    .await;
    never_folded(&r, |m| m.id == card.id || m.content.contains("Turn failed"));
}

/// The turn's fold in `channel`: the one message with a turn fold's button
/// (`fold:o:` or `fold:c:`), a note's being `fold:no:` and `fold:nc:`.
fn turn_fold(r: &Rig, channel: u64) -> Option<Msg> {
    let turns: Vec<Msg> = r
        .posted(channel)
        .into_iter()
        .filter(|m| {
            m.buttons
                .iter()
                .any(|b| b.custom_id.starts_with("fold:o:") || b.custom_id.starts_with("fold:c:"))
        })
        .collect();
    assert!(turns.len() <= 1, "one turn, one fold: {turns:#?}");
    turns.into_iter().next()
}

/// N1, inside a turn: while a turn runs (its command held at a gate, so it
/// cannot end), an informing note from the operator lane and one posted to
/// the conversation's own lane are each one closed line of their own beside
/// the turn's fold, never inside it, and each opens and closes while the
/// turn still runs; a told note posted then keeps its Undo, open. Once the
/// turn ends, the notes are still closed lines and still open.
#[tokio::test]
async fn an_informing_note_during_a_turn_is_its_own_closed_line_beside_the_turns_fold() {
    let r = Rig::start_tweaked(
        |dir, _| {
            std::fs::create_dir_all(dir.join("work")).unwrap();
            let gate = dir.join("work").join("gate");
            let wait = format!("while [ ! -f {} ]; do sleep 0.1; done", gate.to_string_lossy());
            Arc::new(FakeProvider::scripted(vec![
                Scripted::tools("", &[("c1", "proc_run", json!({"argv": ["sh", "-c", wait]}))]),
                Scripted::text("Done waiting."),
            ]))
        },
        guild(),
        &file("", None),
        &[],
        |c| c.places.owner = Some(vec![format!("discord:{ANA}")]),
    )
    .await;
    r.say((ANA, "ana"), None, "Wait for the gate.");
    r.until("the turn's fold, its command running", || {
        turn_fold(&r, ANA_DM).is_some_and(|f| f.content.contains("1 command"))
    })
    .await;
    let session = r.core.outbox.place_session(&format!("dm:{ANA}")).unwrap().unwrap();
    let operator = stage(&r, fyi("operator-during-the-turn"));
    let target = format!("discord:dm:{ANA}");
    let own = r.core.outbox.post(&session, "", &target, fyi("own-lane-during-the-turn"));
    let own = own.unwrap().correlation_id;
    let (a, frame) = told::stage(
        &r.core.outbox,
        None,
        "Theseus kept a standing order",
        "without asking you",
        "told-during-the-turn",
        &Undo::StandingRevoke("std_during".into()),
    )
    .unwrap();
    r.core.store.append(&frame).unwrap();
    r.core.outbox.posted(&a);
    let notes = [
        (folded(&r, ANA_DM, &operator).await, "operator-during-the-turn"),
        (folded(&r, ANA_DM, &own).await, "own-lane-during-the-turn"),
    ];
    r.until("the told note", || first_with(&r, ANA_DM, "told-during-the-turn").is_some())
        .await;
    for (m, words) in &notes {
        assert_eq!(m.content, LINE);
        open_and_close(&r, ANA_DM, m, &fyi_text(words), LINE).await;
    }
    // Still within the turn: the gate is shut, so its answer cannot exist.
    assert!(first_with(&r, ANA_DM, "Done waiting.").is_none(), "the turn still runs");
    let f = turn_fold(&r, ANA_DM).unwrap();
    assert!(notes.iter().all(|(m, _)| m.id != f.id), "each its own message");
    never_folded(&r, |m| m.versions[0].contains("told-during-the-turn"));
    std::fs::write(r.dir.path().join("work").join("gate"), "").unwrap();
    r.until("the answer", || first_with(&r, ANA_DM, "Done waiting.").is_some())
        .await;
    let f = turn_fold(&r, ANA_DM).unwrap();
    for v in &f.versions {
        assert!(!v.contains("during-the-turn") && !v.contains("ℹ️"), "not in the fold: {v:?}");
    }
    assert!(!note_fold(&f), "the turn's fold carries no note's button: {f:#?}");
    for (m, words) in &notes {
        assert_eq!(message(&r, ANA_DM, &m.id).content, LINE);
        open_and_close(&r, ANA_DM, m, &fyi_text(words), LINE).await;
    }
}

/// N1: a note's press goes through its place's lane: while that lane waits
/// out Discord, the press is refused and the note is unchanged; once Discord
/// is back the same press opens it.
#[tokio::test]
async fn a_note_press_waits_on_its_places_lane() {
    let r = rig(
        "",
        None,
        vec![
            Scripted::text("First answer."),
            Scripted::text("Second answer."),
        ],
    )
    .await;
    let post = stage(&r, fyi("lane-waits"));
    let f = folded(&r, ANA_DM, &post).await;
    let failed = || {
        r.fake
            .seen()
            .iter()
            .filter(|s| s.outcome == "failed")
            .count()
    };
    r.fake.fail_creates_in(ANA_DM, Some(503));
    r.say((ANA, "ana"), None, "Hello?");
    r.until("the DM's lane refused by Discord", || failed() > 0)
        .await;
    let pressed = r.press(&f.id, "Show note", (ANA, "ana"));
    r.until("the presser told", || told(&r, &pressed, AWAY))
        .await;
    let m = message(&r, ANA_DM, &f.id);
    assert_eq!((m.content.as_str(), m.edits), (LINE, f.edits), "{m:#?}");
    r.fake.fail_creates_in(ANA_DM, None);
    r.until("the answer, once Discord is back", || {
        first_with(&r, ANA_DM, "First answer.").is_some()
    })
    .await;
    open_and_close(&r, ANA_DM, &f, &fyi_text("lane-waits"), LINE).await;
}

/// N6: a folded note's place starts again (the file drops the DM and takes
/// it back: a new place, a new lane, nothing in memory): its press still
/// opens it, from the post's own record. A new process over a store that
/// no longer holds the post: the press says in one line that the note is
/// too old, and shows nothing.
#[tokio::test]
async fn a_folded_note_opens_after_its_place_starts_again_or_says_it_is_too_old() {
    let r = rig("", None, vec![]).await;
    let post = stage(&r, fyi("before-the-restart"));
    let f = folded(&r, ANA_DM, &post).await;
    let file_path = r.dir.path().join("bindings.toml");
    let places = || r.core.bindings.all().first().map_or(0, |b| b.places.len());
    let without_dm = format!(
        "guild_id = \"{DEFAULT_GUILD}\"\n\
         [[channel]]\nid = \"{LAB}\"\nname = \"lab\"\nusers = [\"{ANA}\"]\nmention_only = false\nprivate = true\n"
    );
    let dm_bound = || places() == 2;
    std::fs::write(&file_path, without_dm).unwrap();
    r.until("the DM dropped", || places() == 1).await;
    std::fs::write(&file_path, file("", None)).unwrap();
    r.until("the DM bound again", dm_bound).await;
    open_and_close(&r, ANA_DM, &f, &fyi_text("before-the-restart"), LINE).await;
    let fake = r.fake.clone();
    r.stop();

    let r = Rig::start_over(
        fake,
        |_, _| Arc::new(FakeProvider::scripted(vec![])),
        &file("", None),
        &[],
        |c| c.places.owner = Some(vec![format!("discord:{ANA}")]),
    )
    .await;
    let before = message(&r, ANA_DM, &f.id);
    let pressed = press(&r, &f.id, "Show note").await;
    r.until("the presser told", || told(&r, &pressed, TOO_OLD))
        .await;
    let m = message(&r, ANA_DM, &f.id);
    assert_eq!((&m.content, m.edits), (&before.content, before.edits));
    assert_eq!(m.content, LINE);
}
