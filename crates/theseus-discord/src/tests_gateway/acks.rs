//! How a handler acknowledges an interaction, and who sees what (issue 29).
//!
//! The service wants an interaction's first response within three seconds, so a handler that waits for the daemon
//! acknowledges first and completes the same response after. **What a deferral's flag says is what completes it**:
//! an answer that was private before the sweep must still be private, and a card that was updated for everyone must
//! still be updated for everyone. So each handler's acknowledgement is held to what its final answer was before:
//!
//! - **`Private`**: a deferred reply only the presser sees (callback type 5 with the ephemeral flag), where the answer
//!   was an ephemeral reply;
//! - **`Card`**: a deferred UPDATE of the pressed message (callback type 6), where the answer updated the card for
//!   everyone (a type-7 update) and any refusal was an ephemeral message.
//!
//! The table fires each handler through the real gateway path (the binding, a real core, the stand-in service) with
//! ids the daemon has never heard of: every handler acknowledges BEFORE its daemon call, so the first response is
//! the same whatever the daemon then says. When in doubt, the acknowledgement is private.

use std::sync::Arc;

use serde_json::json;
use theseus_core::provider::{FakeProvider, Provider, Scripted};
use theseus_protocol::method;
use theseus_sim::fake_discord::commands::Sender;
use theseus_sim::fake_discord::Reply;

use super::{bindings, guild, Rig, ANA, ANA_DM, BEN, LAB};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ack {
    Private,
    Card,
}

enum Fire {
    Slash(&'static str, &'static [(&'static str, &'static str)]),
    Sub(
        &'static str,
        &'static str,
        &'static [(&'static str, &'static str)],
    ),
    Press(&'static str),
    Select(&'static str, &'static [&'static str]),
    Modal(&'static str, &'static [(&'static str, &'static str)]),
    ModalFromMessage(&'static str, &'static [(&'static str, &'static str)]),
}

struct Row {
    handler: &'static str,
    fire: Fire,
    ack: Ack,
    /// What its final answer was before the sweep.
    was: &'static str,
}

const ROUTINE_FORM: &[(&str, &str)] = &[
    ("title", "Tide check"),
    ("schedule", "weekdays 08:00"),
    ("brief", "Read the tide table and report it."),
    ("budget", "0.50 / 5"),
    ("where", "dm"),
];
const WATCH_FORM: &[(&str, &str)] = &[
    ("title", "Is the gate open"),
    ("target", "https://example.invalid/gate"),
    ("until", "it says open"),
];

use Ack::{Card, Private};
use Fire::{Modal, ModalFromMessage, Press, Select, Slash, Sub};

/// Every handler the sweep changed that a person can reach by an interaction with the stand-ins.
const TABLE: &[Row] = &[
    Row {
        handler: "keep form's submit",
        fire: Modal("kf:act_0000feed", &[("value", "x")]),
        ack: Private,
        was: "an ephemeral reply",
    },
    Row {
        handler: "proof form's submit, from a command",
        fire: Modal("pf:act_0000feed", &[("code", "123456")]),
        ack: Private,
        was: "an ephemeral reply",
    },
    Row {
        handler: "proof form's submit, from a card",
        fire: ModalFromMessage("pf:act_0000feed", &[("code", "123456")]),
        ack: Card,
        was: "an update of the card (type 7); a refusal an ephemeral message",
    },
    Row {
        handler: "an ask card's option",
        fire: Press("ask:1:act_0000feed"),
        ack: Card,
        was: "an update of the card; a refusal ephemeral",
    },
    Row {
        handler: "an expired card's Ask again",
        fire: Press("again:act_0000feed"),
        ack: Card,
        was: "an update of the card; a refusal ephemeral",
    },
    Row {
        handler: "a thread ask's answer",
        fire: Press("thr:ask_0001:new"),
        ack: Card,
        was: "an update of the card; a refusal ephemeral",
    },
    Row {
        handler: "a new persona's soul",
        fire: Modal("psoul:m:pilot", &[("soul", "words")]),
        ack: Private,
        was: "an ephemeral reply",
    },
    Row {
        handler: "the answer modal, from a command",
        fire: Modal("wm:00feed:decline", &[("note", "no")]),
        ack: Private,
        was: "an ephemeral reply",
    },
    Row {
        handler: "the answer modal, from a card",
        fire: ModalFromMessage("wm:00feed:decline", &[("note", "no")]),
        ack: Card,
        was: "an update of the card; a refusal ephemeral",
    },
    Row {
        handler: "a routine post's Details",
        fire: Press("hz:details:wak_0000feed"),
        ack: Private,
        was: "an ephemeral reply",
    },
    Row {
        handler: "a routine post's answer",
        fire: Press("hz:done:wak_0000feed"),
        ack: Card,
        was: "an update of the post; a refusal ephemeral",
    },
    Row {
        handler: "the board's reminder select",
        fire: Select("hq:pick", &["hq:cancel:wak_0000feed"]),
        ack: Private,
        was: "an ephemeral reply",
    },
    Row {
        handler: "/agenda",
        fire: Slash("agenda", &[]),
        ack: Private,
        was: "an ephemeral reply",
    },
    Row {
        handler: "/routine list",
        fire: Sub("routine", "list", &[]),
        ack: Private,
        was: "an ephemeral reply",
    },
    Row {
        handler: "/routine pause",
        fire: Sub("routine", "pause", &[("name", "nothing")]),
        ack: Private,
        was: "an ephemeral reply",
    },
    Row {
        handler: "/snooze",
        fire: Slash("snooze", &[("wake", "wak_0000feed"), ("when", "in 1h")]),
        ack: Private,
        was: "an ephemeral reply",
    },
    Row {
        handler: "/watch list",
        fire: Sub("watch", "list", &[]),
        ack: Private,
        was: "an ephemeral reply",
    },
    Row {
        handler: "a routine's button, not on the list",
        fire: Press("hr:pause:rtn_0000feed"),
        ack: Private,
        was: "an ephemeral reply",
    },
    Row {
        handler: "the routine form's submit",
        fire: Modal("hzm:routine", ROUTINE_FORM),
        ack: Private,
        was: "an ephemeral reply",
    },
    Row {
        handler: "the watch form's submit",
        fire: Modal("hzm:watch:fetch", WATCH_FORM),
        ack: Private,
        was: "an ephemeral reply",
    },
    Row {
        handler: "a command watch's approval",
        fire: Press("hw:no:0000feed"),
        ack: Card,
        was: "an update of the approval message",
    },
    Row {
        handler: "/orders",
        fire: Slash("orders", &[]),
        ack: Private,
        was: "an ephemeral list",
    },
    Row {
        handler: "an order's Revoke",
        fire: Press("orders-revoke:nothing"),
        ack: Card,
        was: "an update of the list message",
    },
    Row {
        handler: "/extensions",
        fire: Slash("extensions", &[]),
        ack: Private,
        was: "an ephemeral list",
    },
    Row {
        handler: "an extension's Revoke",
        fire: Press("ext-revoke:nothing"),
        ack: Card,
        was: "an update of the list message",
    },
    Row {
        handler: "/project",
        fire: Sub("project", "trusted", &[]),
        ack: Private,
        was: "an ephemeral reply",
    },
    Row {
        handler: "/thread",
        fire: Sub("thread", "list", &[]),
        ack: Private,
        was: "an ephemeral reply",
    },
    Row {
        handler: "/remind",
        fire: Slash("remind", &[("when", "in 1h"), ("what", "stretch")]),
        ack: Private,
        was: "an ephemeral reply",
    },
    Row {
        handler: "a reminder's button",
        fire: Press("wake:done:wak_0000feed"),
        ack: Card,
        was: "an update of the reminder; a refusal ephemeral",
    },
    Row {
        handler: "a cited source's Show source",
        fire: Press("src:ses_0000feed:m:0000feed"),
        ack: Private,
        was: "an ephemeral reply",
    },
    Row {
        handler: "/tasks done",
        fire: Slash("tasks", &[("show", "done")]),
        ack: Private,
        was: "an ephemeral view",
    },
    Row {
        handler: "the tasks view's refresh (done)",
        fire: Press("tb:refresh:done"),
        ack: Card,
        was: "an update of the view",
    },
    Row {
        handler: "a failed task's Retry",
        fire: Press("tb:retry:exe_0000feed"),
        ack: Private,
        was: "an ephemeral reply",
    },
    Row {
        handler: "a failed task's Dismiss",
        fire: Press("tb:dismiss:exe_0000feed"),
        ack: Private,
        was: "an ephemeral reply",
    },
    Row {
        handler: "a task's confirmed Stop",
        fire: Press("tb:yes:exe_0000feed"),
        ack: Card,
        was: "an update of the view",
    },
    Row {
        handler: "/prompt's argument form",
        fire: Modal("prompt:srv/greet", &[]),
        ack: Private,
        was: "an ephemeral reply for every refusal (the run's line public)",
    },
];

fn ana() -> Sender<'static> {
    Sender {
        user: ANA,
        name: "ana",
        channel: None,
    }
}

async fn rig() -> Rig {
    let model = |_: &std::path::Path, _| -> Arc<dyn Provider> {
        Arc::new(FakeProvider::scripted(vec![Scripted::text("hello")]))
    };
    Rig::start_tweaked(model, guild(true), &bindings(true), &[], |_| {}).await
}

/// The first response the binding made to `iid`: its callback.
fn first(r: &Rig, iid: &str) -> Option<Reply> {
    r.fake
        .replies()
        .into_iter()
        .find(|x| x.interaction.as_deref() == Some(iid) && x.kind == "callback")
}

async fn first_of(r: &Rig, what: &str, iid: &str) -> Reply {
    r.until(what, || first(r, iid).is_some()).await;
    first(r, iid).unwrap()
}

/// Every handler the sweep changed acknowledges as what its final answer was before it: private stays private, and a
/// card updated for everyone is updated for everyone.
#[tokio::test]
async fn every_handler_the_sweep_changed_acknowledges_as_its_answer_was_before() {
    let r = rig().await;
    // A message of the bot's, for a form opened from a card.
    r.say((ANA, "ana"), None, "hello");
    r.until("the bot's reply in the DM", || {
        r.posted(ANA_DM)
            .iter()
            .any(|m| m.content.starts_with("hello"))
    })
    .await;
    let card = r.posted(ANA_DM).remove(0).id;
    let mut wrong = Vec::new();
    for row in TABLE {
        let fired = match &row.fire {
            Slash(name, opts) => r.fake.slash(&ana(), name, opts),
            Sub(name, sub, opts) => r.fake.slash_sub(&ana(), name, sub, opts),
            Press(id) => r.fake.press_custom(&ana(), None, (id, 2), &[]),
            Select(id, values) => r.fake.press_custom(&ana(), None, (id, 3), values),
            Modal(id, fields) => r.fake.submit_modal(&ana(), id, fields, &[], None),
            ModalFromMessage(id, fields) => {
                r.fake.submit_modal_from(&ana(), &card, id, fields, &[])
            }
        };
        let iid = fired.unwrap_or_else(|e| panic!("{}: {e}", row.handler));
        let got = first_of(&r, row.handler, &iid).await;
        let seen = match (got.response_type, got.ephemeral) {
            (Some(5), true) => Ack::Private,
            (Some(6), _) => Ack::Card,
            _ => {
                wrong.push(format!("{}: a first response of {got:?}", row.handler));
                continue;
            }
        };
        if seen != row.ack {
            wrong.push(format!(
                "{}: acknowledged as {seen:?}, was {:?} ({})",
                row.handler, row.ack, row.was
            ));
        }
        assert_eq!(
            got.content, None,
            "{}: the acknowledgement says nothing",
            row.handler
        );
    }
    assert!(
        wrong.is_empty(),
        "{} of {} handlers:\n{}",
        wrong.len(),
        TABLE.len(),
        wrong.join("\n")
    );
    // `/task share` is one more: a command that goes through the place's own loop.
    let shared = r
        .fake
        .slash_with(&ana(), "task", &[("name", "nothing")], &[("share", BEN)])
        .unwrap();
    let got = first_of(&r, "/task share", &shared).await;
    assert_eq!(
        (got.response_type, got.ephemeral),
        (Some(5), true),
        "{got:?}"
    );
    // And a new task's submit, the other handler of that loop: private.
    let started = r
        .fake
        .submit_modal(&ana(), "nt:", &[("brief", "Count the gulls.")], &[], None)
        .unwrap();
    let got = first_of(&r, "a new task's submit", &started).await;
    assert_eq!(
        (got.response_type, got.ephemeral),
        (Some(5), true),
        "{got:?}"
    );
}

/// A routine's button on the routine LIST, which is an ephemeral view, edits that view: a deferred update, and the
/// view stays what it was (private).
#[tokio::test]
async fn a_routines_button_on_the_list_updates_the_private_list() {
    let r = rig().await;
    let list = r.fake.slash_sub(&ana(), "routine", "list", &[]).unwrap();
    r.until("the list", || {
        r.fake
            .reply_message(&list)
            .is_some_and(|m| m.flags & 64 != 0)
    })
    .await;
    let view = r.fake.reply_message(&list).unwrap().id;
    let pressed = r
        .fake
        .press_custom(&ana(), Some(&view), ("hr:pause:rtn_0000feed", 2), &[])
        .unwrap();
    let got = first_of(&r, "the list's button", &pressed).await;
    assert_eq!(got.response_type, Some(6), "{got:?}");
}

fn big_prompt() -> serde_json::Value {
    let arguments: Vec<_> = (0..6).map(|n| json!({"name": format!("a{n}")})).collect();
    json!({"prompts": [{"server": "srv", "prompt": "big", "name": "srv/big", "arguments": arguments, "digest": "d"}]})
}

fn small_prompt() -> serde_json::Value {
    json!({"prompts": [{"server": "srv", "prompt": "greet", "name": "srv/greet",
                        "arguments": [{"name": "who"}], "digest": "d"}]})
}

fn lab() -> Sender<'static> {
    Sender {
        user: ANA,
        name: "ana",
        channel: Some(LAB),
    }
}

/// Every non-ephemeral thing the service was shown: the replies that everyone sees, and every message in a channel.
fn public_text(r: &Rig) -> String {
    let replies: Vec<String> = r
        .fake
        .replies()
        .into_iter()
        .filter(|x| !x.ephemeral)
        .filter_map(|x| x.content)
        .collect();
    let messages: Vec<String> = r
        .fake
        .all_messages()
        .into_iter()
        .flat_map(|m| m.versions)
        .collect();
    format!("{replies:?}{messages:?}")
}

/// A prompt form submitted in a GUILD place with a line that is not `name=value`: the refusal quotes what was typed, so
/// it is for the presser alone. The acknowledgement is private (a deferral's flag is what completes it), and what was
/// typed is in nothing the place can see.
#[tokio::test]
async fn a_prompt_forms_refusal_in_a_guild_place_is_private_and_the_typed_line_is_nowhere_public() {
    crate::rpc_client::stub(method::MCP_PROMPT_LIST, big_prompt());
    let r = rig().await;
    let typed = "INVENTED-TYPED-LINE-WITHOUT-AN-EQUALS-SIGN";
    let iid = r
        .fake
        .submit_modal(&lab(), "prompt:srv/big", &[("args", typed)], &[], None)
        .unwrap();
    let got = first_of(&r, "the acknowledgement", &iid).await;
    assert_eq!(
        (got.response_type, got.ephemeral),
        (Some(5), true),
        "{got:?}"
    );
    r.until("the refusal", || {
        r.fake
            .replies()
            .iter()
            .any(|x| x.interaction.as_deref() == Some(iid.as_str()) && x.kind == "original")
    })
    .await;
    let done = r
        .fake
        .replies()
        .into_iter()
        .find(|x| x.interaction.as_deref() == Some(iid.as_str()) && x.kind == "original")
        .unwrap();
    assert!(
        done.ephemeral,
        "the refusal completes the private response: {done:?}"
    );
    assert!(
        done.content
            .as_deref()
            .unwrap_or("")
            .contains("not name=value"),
        "{done:?}"
    );
    // The refusal quotes the typed line, privately: no one else is shown it.
    assert!(
        done.content.as_deref().unwrap_or("").contains(typed),
        "{done:?}"
    );
    assert!(
        !public_text(&r).contains(typed),
        "in what the place can see"
    );
}

/// A prompt that runs from the form: the presser gets a short private line, the place gets ONE public line of its
/// own (the one `prompt_run` meant for the place), and nothing of the arguments is in either.
#[tokio::test]
async fn a_prompt_that_runs_from_the_form_posts_one_public_line_and_none_of_its_arguments() {
    crate::rpc_client::stub(method::MCP_PROMPT_LIST, small_prompt());
    let r = rig().await;
    let typed = "INVENTED-ARGUMENT-VALUE";
    let iid = r
        .fake
        .submit_modal(&lab(), "prompt:srv/greet", &[("who", typed)], &[], None)
        .unwrap();
    let got = first_of(&r, "the acknowledgement", &iid).await;
    assert_eq!(
        (got.response_type, got.ephemeral),
        (Some(5), true),
        "{got:?}"
    );
    r.until("the public line", || {
        r.fake.replies().iter().any(|x| {
            x.interaction.as_deref() == Some(iid.as_str()) && x.kind == "followup" && !x.ephemeral
        })
    })
    .await;
    let mine: Vec<Reply> = r
        .fake
        .replies()
        .into_iter()
        .filter(|x| x.interaction.as_deref() == Some(iid.as_str()))
        .collect();
    let public: Vec<&Reply> = mine.iter().filter(|x| !x.ephemeral).collect();
    assert_eq!(public.len(), 1, "one public line: {mine:?}");
    assert!(
        public[0]
            .content
            .as_deref()
            .unwrap_or("")
            .contains("Running the prompt `srv/greet`"),
        "{public:?}"
    );
    let private = mine.iter().find(|x| x.kind == "original").unwrap();
    assert!(private.ephemeral, "{private:?}");
    assert!(
        !format!("{mine:?}").contains(typed),
        "the arguments are in neither line"
    );
}

/// The press was acknowledged as a deferred UPDATE of the card (callback type 6: the card itself is updated for
/// everyone), and the card message the person pressed now says `said` with its buttons gone.
pub(super) fn assert_card_updated_for_everyone(
    r: &super::Rig,
    pressed: &str,
    card: &str,
    said: &str,
) {
    let ack = r
        .fake
        .replies()
        .into_iter()
        .find(|x| x.interaction.as_deref() == Some(pressed) && x.kind == "callback")
        .expect("the press was acknowledged");
    assert_eq!(ack.response_type, Some(6), "{ack:?}");
    let m = r
        .fake
        .all_messages()
        .into_iter()
        .find(|m| m.id == card)
        .expect("the card");
    assert!(m.content.contains(said), "the card itself says it: {m:?}");
    assert!(m.buttons.is_empty(), "its buttons went: {m:?}");
}
