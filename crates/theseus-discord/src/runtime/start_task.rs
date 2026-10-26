//! Starting a task from Discord (theseus-a5uds). A child of `runtime`.
//!
//! The board's `[ ➕ New task ]`, `/task new` and the message command "Make
//! this a task" open one modal: what the task does, a budget ($1, $2, $5 or
//! $10) and where its report goes (here, or silently to the board). A message
//! made a task brings its text as a quote, a field the owner can see and edit:
//! `task.start` takes it as `quotes`, which hold the task's session as outside
//! text, as web text does. The submit is `task.start` as the presser, homed in
//! this place's session; the core judges it, so a shared place is refused and
//! told where to start one instead.
//!
//! A reply to a message that described work for later offers it too
//! (theseus-bh3t.5): `➕ Make this a task` (`tb:offer:<turn>`) opens the same
//! modal with the message as the brief; the briefs wait in `Offers`, the
//! newest 64, kept in the store's META (`discord.offers`, theseus-ypnxt.5), so
//! a press after a restart opens the modal with its brief. The submit drops
//! the button from the reply. When the judge is slow the reply goes out first
//! and the core's `offer` post adds the button by one edit (`Offers::replied`
//! remembers the reply's last part).

use std::collections::VecDeque;
use std::sync::Mutex;

use serde_json::{json, Value};
use theseus_core::store::Store;
use theseus_protocol::task_start::{Expect, ReportTo, TaskStartParams, TaskStartResult};
use twilight_model::application::interaction::modal::{
    ModalInteractionComponent, ModalInteractionData,
};
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::message::component::{Button, ButtonStyle};
use twilight_model::channel::message::Component;

use super::{origin_of, Place};

/// The modal's `custom_id`.
pub(super) const MODAL: &str = "nt:";
/// The board's button and the budget choices.
pub const NEW_BUTTON: &str = crate::render::board_v2::NEW;
const BUDGETS: [&str; 4] = ["1", "2", "5", "10"];
const DEFAULT_BUDGET: &str = "2";
const BRIEF: &str = "brief";
const QUOTE: &str = "quote";
const BUDGET: &str = "budget";
const REPORT: &str = "report";
const EXPECT: &str = "expect";
/// Discord's text input limit.
const INPUT_MAX: usize = 4000;
/// The modal id of a message by someone else: its quote is outside text
/// (theseus-nfhaf), where `nt:` alone carries the presser's own words.
fn outside_id() -> String {
    format!("{MODAL}{OUTSIDE}")
}
const OUTSIDE: &str = "!";

/// A reply's offer button: this, then the turn.
pub(super) const OFFER: &str = "tb:offer:";
/// How many replies' briefs wait for their button's press.
const OFFERS_KEPT: usize = 64;
/// The META key holding them, newest last.
const META_KEY: &str = "discord.offers";
/// How many replies' last parts are remembered for a late offer.
const REPLIED_KEPT: usize = 16;

/// A reply's last message, as its offer button joins it.
pub(crate) struct Replied {
    pub turn: String,
    pub key: String,
    pub session: String,
    pub sources: Vec<crate::render::cite::Source>,
}

/// The briefs of the replies that offered a task, by turn: read from META at
/// the first use, written back whole at each change.
#[derive(Default)]
pub(crate) struct Offers {
    briefs: Mutex<Option<VecDeque<(String, String)>>>,
    replied: Mutex<VecDeque<Replied>>,
}

impl Offers {
    fn with<R>(&self, store: &Store, f: impl FnOnce(&mut VecDeque<(String, String)>) -> R) -> R {
        let mut g = self.briefs.lock().unwrap_or_else(|e| e.into_inner());
        let q = g.get_or_insert_with(|| {
            store
                .get_meta::<VecDeque<(String, String)>>(META_KEY)
                .ok()
                .flatten()
                .unwrap_or_default()
        });
        f(q)
    }

    pub(crate) fn keep(&self, store: &Store, turn: &str, brief: &str) {
        self.with(store, |q| {
            q.retain(|(t, _)| t != turn);
            while q.len() >= OFFERS_KEPT {
                q.pop_front();
            }
            q.push_back((turn.to_string(), brief.to_string()));
            if let Err(e) = store.put_meta(META_KEY, q) {
                tracing::warn!(error = %format!("{e:#}"), "the task offers were not kept");
            }
        });
    }

    pub(crate) fn brief(&self, store: &Store, turn: &str) -> Option<String> {
        self.with(store, |q| {
            q.iter().find(|(t, _)| t == turn).map(|(_, b)| b.clone())
        })
    }

    /// A reply's last message, the one its button would join.
    pub(crate) fn replied(&self, r: Replied) {
        let mut q = self.replied.lock().unwrap_or_else(|e| e.into_inner());
        q.retain(|x| x.turn != r.turn);
        if q.len() == REPLIED_KEPT {
            q.pop_front();
        }
        q.push_back(r);
    }

    pub(crate) fn reply_of(
        &self,
        turn: &str,
    ) -> Option<(String, String, Vec<crate::render::cite::Source>)> {
        let q = self.replied.lock().unwrap_or_else(|e| e.into_inner());
        q.iter()
            .find(|x| x.turn == turn)
            .map(|x| (x.key.clone(), x.session.clone(), x.sources.clone()))
    }
}

/// The button under a reply that offers to make its message a task.
pub(super) fn offer_button(turn: &str) -> Component {
    Component::Button(Button {
        id: None,
        custom_id: Some(format!("{OFFER}{turn}")),
        disabled: false,
        emoji: None,
        label: Some("➕ Make this a task".into()),
        style: ButtonStyle::Primary,
        url: None,
        sku_id: None,
    })
}

fn label(label: &str, description: Option<&str>, component: Value) -> Value {
    let mut v = json!({"type": 18, "label": label, "component": component});
    if let Some(d) = description {
        v["description"] = json!(d);
    }
    v
}

fn pick(id: &str, options: &[(&str, &str)], default: &str) -> Value {
    let options: Vec<Value> = options
        .iter()
        .map(|(l, v)| json!({"label": l, "value": v, "default": *v == default}))
        .collect();
    json!({"type": 3, "custom_id": id, "options": options, "min_values": 1,
           "max_values": 1, "required": true})
}

/// The new-task modal; `quote` is a message's text, offered as a quote, with
/// whether the message is the presser's own (`true`) or someone else's, which
/// words its label and sets the modal's id (theseus-tbm1q); `brief` fills the
/// first field (the core titles the task from its first line).
pub(super) fn modal(quote: Option<(&str, bool)>, brief: Option<&str>) -> Value {
    let mut input = json!({"type": 4, "custom_id": BRIEF, "style": 2, "required": true,
                           "max_length": INPUT_MAX});
    if let Some(b) = brief {
        input["value"] = json!(crate::render::clip(b, INPUT_MAX));
    }
    let mut components = vec![label("What should the task do?", None, input)];
    if let Some((q, own)) = quote {
        components.push(label(
            if own {
                "The message"
            } else {
                "The message (read as outside text)"
            },
            Some("The task reads it verbatim; clear it to start without"),
            json!({"type": 4, "custom_id": QUOTE, "style": 2, "required": false,
                   "max_length": INPUT_MAX, "value": crate::render::clip(q, INPUT_MAX)}),
        ));
    }
    components.push(label(
        "Budget",
        None,
        pick(
            BUDGET,
            &[("$1", "1"), ("$2", "2"), ("$5", "5"), ("$10", "10")],
            DEFAULT_BUDGET,
        ),
    ));
    components.push(label(
        "Report",
        None,
        pick(
            REPORT,
            &[
                ("Here, when it ends", "home"),
                ("Silently to the board", "board"),
            ],
            "home",
        ),
    ));
    components.push(label(
        "Expected",
        None,
        pick(
            EXPECT,
            &[("short", "short"), ("long (over 10 min)", "long")],
            "short",
        ),
    ));
    let id = match quote {
        Some((_, false)) => outside_id(),
        _ => MODAL.to_string(),
    };
    json!({"type": 9, "data": {"custom_id": id, "title": "New task", "components": components}})
}

/// A submitted modal's fields.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Submit {
    pub(super) brief: String,
    pub(super) quote: String,
    pub(super) budget: Option<f64>,
    pub(super) board: bool,
    /// The task is expected to take over ten minutes (theseus-bh3t.1).
    pub(super) long: bool,
    /// The reply whose offer opened the modal (its turn).
    pub(super) offer: Option<String>,
    /// The quote is another person's message (`nt:!`), not the presser's words.
    pub(super) outside: bool,
}

impl Submit {
    pub(super) fn of(m: &ModalInteractionData) -> Self {
        fn walk(c: &ModalInteractionComponent, s: &mut Submit) {
            match c {
                ModalInteractionComponent::Label(l) => walk(&l.component, s),
                ModalInteractionComponent::ActionRow(r) => {
                    r.components.iter().for_each(|c| walk(c, s))
                }
                ModalInteractionComponent::TextInput(t) if t.custom_id == BRIEF => {
                    s.brief = t.value.clone()
                }
                ModalInteractionComponent::TextInput(t) if t.custom_id == QUOTE => {
                    s.quote = t.value.clone()
                }
                ModalInteractionComponent::StringSelect(v) if v.custom_id == BUDGET => {
                    s.budget = v.values.first().and_then(|v| v.parse().ok())
                }
                ModalInteractionComponent::StringSelect(v) if v.custom_id == EXPECT => {
                    s.long = v.values.first().is_some_and(|v| v == "long")
                }
                ModalInteractionComponent::StringSelect(v) if v.custom_id == REPORT => {
                    s.board = v.values.first().is_some_and(|v| v == "board")
                }
                _ => {}
            }
        }
        let mut s = Submit {
            offer: m
                .custom_id
                .strip_prefix(MODAL)
                .filter(|t| !t.is_empty() && *t != OUTSIDE)
                .map(str::to_string),
            outside: m.custom_id.strip_prefix(MODAL) == Some(OUTSIDE),
            ..Default::default()
        };
        m.components.iter().for_each(|c| walk(c, &mut s));
        s
    }

    /// What `task.start` is asked, homed in `session`.
    pub(super) fn params(&self, session: &str, who: &str, i: &Interaction) -> TaskStartParams {
        let quote = self.quote.trim();
        TaskStartParams {
            brief: self.brief.trim().to_string(),
            home: Some(session.to_string()),
            budget_usd: self
                .budget
                .filter(|b| BUDGETS.contains(&b.to_string().as_str())),
            outside_quotes: self.outside,
            quotes: if quote.is_empty() {
                vec![]
            } else {
                vec![quote.to_string()]
            },
            report_to: Some(if self.board {
                ReportTo::Board
            } else {
                ReportTo::Home
            }),
            expect: self.long.then_some(Expect::Long),
            author: Some(who.to_string()),
            discord: origin_of(
                i.author().map(|u| u.id.get()),
                i.channel.as_ref().map(|c| c.id.get()),
                i.guild_id,
            ),
            ..Default::default()
        }
    }
}

impl Place {
    /// A submitted new-task modal: `task.start` as the presser.
    pub(super) async fn start_task(&self, i: &Interaction, who: &str, sub: Submit) {
        let s = &self.shared;
        if sub.brief.trim().is_empty() {
            s.tell(i, "A task needs words: say what it should do.".into())
                .await;
            return;
        }
        let class = s
            .core
            .runner
            .place_rule
            .class(&s.core.cfg, Some(&self.target));
        if class == theseus_core::places::PlaceClass::Shared {
            s.tell(
                i,
                "🔐 A task starts from a private place: your DM with me, or a channel bound \
                 private. Start it there, or with `theseus task start`."
                    .into(),
            )
            .await;
            return;
        }
        let r = s
            .rpc
            .call::<_, TaskStartResult>(
                theseus_protocol::method::TASK_START,
                sub.params(&self.session_id, who, i),
            )
            .await;
        let line = match r {
            Ok(t) => format!(
                "▶️ Started `…{}` with up to ${:.2}. {}",
                t.short,
                t.limit_usd,
                if sub.board {
                    "Its report waits on the board."
                } else {
                    "It shows on the board, and its report comes here."
                }
            ),
            Err(e) if e.code == theseus_protocol::error_code::REFUSED => {
                let why = e.data.get("why").and_then(Value::as_str);
                format!("🔐 It did not start: {}.", why.unwrap_or(&e.message))
            }
            Err(e) => format!("⚠️ It did not start: {e}"),
        };
        s.tell(i, line).await;
        if sub.offer.is_some() {
            self.drop_offer(i).await;
        }
    }

    /// The offer's button was pressed: the modal, with the message as the brief.
    pub(super) async fn offer_press(&self, i: &Interaction, turn: &str) {
        let s = &self.shared;
        let me = i.author().map(|u| format!("discord:{}", u.id.get()));
        let owners = s.core.runner.place_rule.owners(&s.core.cfg);
        if !me.is_some_and(|me| owners.contains(&me)) {
            s.tell(i, "Only the owner can start a task here.".into())
                .await;
            return;
        }
        let mut m = modal(None, s.tasks.offers.brief(&s.core.store, turn).as_deref());
        m["data"]["custom_id"] = json!(format!("{MODAL}{turn}"));
        s.respond_modal(i, &m).await;
    }

    /// A task started from an offer: its reply loses the button (one edit).
    async fn drop_offer(&self, i: &Interaction) {
        let Some(msg) = &i.message else { return };
        let rows: Vec<Component> = msg
            .components
            .iter()
            .filter_map(|c| match c {
                Component::ActionRow(r) => {
                    let mut r = r.clone();
                    r.components.retain(|b| {
                        !matches!(b, Component::Button(b)
                            if b.custom_id.as_deref().is_some_and(|id| id.starts_with(OFFER)))
                    });
                    (!r.components.is_empty()).then_some(Component::ActionRow(r))
                }
                other => Some(other.clone()),
            })
            .collect();
        let edit = self.shared.http.update_message(msg.channel_id, msg.id);
        if let Err(e) = edit.components(Some(&rows)).await {
            self.shared.board.error("offer edit", None, e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The briefs outlive the process: a fresh `Offers` over the same store (a
    /// restart) finds them, the newest 64 only (theseus-ypnxt.5).
    #[test]
    fn the_briefs_are_kept_in_the_store_so_a_restart_finds_them() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("store")).unwrap();
        let before = Offers::default();
        for n in 0..(OFFERS_KEPT + 3) {
            before.keep(&store, &format!("turn_{n}"), &format!("brief {n}"));
        }
        let after = Offers::default();
        assert_eq!(after.brief(&store, "turn_0"), None, "the oldest went");
        assert_eq!(after.brief(&store, "turn_3").as_deref(), Some("brief 3"));
        let last = format!("turn_{}", OFFERS_KEPT + 2);
        assert_eq!(
            after.brief(&store, &last).as_deref(),
            Some(&*format!("brief {}", OFFERS_KEPT + 2))
        );
        // Kept again for the same turn: one entry, the newer brief.
        after.keep(&store, &last, "brief again");
        assert_eq!(
            Offers::default().brief(&store, &last).as_deref(),
            Some("brief again")
        );
    }

    #[test]
    fn the_modal_has_a_brief_a_budget_select_a_report_select_and_an_expected_select() {
        let m = modal(None, None);
        let parts = m["data"]["components"].as_array().unwrap();
        let ids: Vec<&str> = parts
            .iter()
            .map(|p| p["component"]["custom_id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["brief", "budget", "report", "expect"]);
        let budget = &parts[1]["component"]["options"];
        assert_eq!(budget.as_array().unwrap().len(), 4);
        assert_eq!(budget[1]["default"], true, "$2 is the default");
    }

    #[test]
    fn a_message_made_a_task_brings_its_text_as_an_editable_quote() {
        let m = modal(Some(("the log says the reef sensor died", true)), None);
        let parts = m["data"]["components"].as_array().unwrap();
        assert_eq!(parts.len(), 5, "a modal holds five");
        assert_eq!(parts[1]["component"]["custom_id"], "quote");
        assert_eq!(
            parts[1]["component"]["value"],
            "the log says the reef sensor died"
        );
        assert_eq!(parts[1]["component"]["required"], false);
    }

    const DESCRIPTION: &str = "The task reads it verbatim; clear it to start without";

    fn quote_label(m: &Value) -> &Value {
        &m["data"]["components"][1]
    }

    /// The presser's own message is not outside text, so its label does not say so.
    #[test]
    fn the_owners_own_message_is_labelled_the_message() {
        let m = modal(Some(("the reef log", true)), None);
        assert_eq!(quote_label(&m)["label"], "The message");
        assert_eq!(quote_label(&m)["description"], DESCRIPTION);
        assert_eq!(m["data"]["custom_id"], "nt:");
    }

    #[test]
    fn another_persons_message_is_labelled_outside_text_and_carries_the_bang() {
        let m = modal(Some(("the reef log", false)), None);
        assert_eq!(
            quote_label(&m)["label"],
            "The message (read as outside text)"
        );
        assert_eq!(quote_label(&m)["description"], DESCRIPTION);
        assert_eq!(m["data"]["custom_id"], "nt:!");
    }

    /// The modal with no quote is what it was before the label learned whose
    /// message it is: the whole JSON, field for field.
    #[test]
    fn the_modal_with_no_quote_is_unchanged() {
        let want: Value = serde_json::from_str(
            r#"{"data":{"components":[{"component":{"custom_id":"brief","max_length":4000,"required":true,"style":2,"type":4},"label":"What should the task do?","type":18},{"component":{"custom_id":"budget","max_values":1,"min_values":1,"options":[{"default":false,"label":"$1","value":"1"},{"default":true,"label":"$2","value":"2"},{"default":false,"label":"$5","value":"5"},{"default":false,"label":"$10","value":"10"}],"required":true,"type":3},"label":"Budget","type":18},{"component":{"custom_id":"report","max_values":1,"min_values":1,"options":[{"default":true,"label":"Here, when it ends","value":"home"},{"default":false,"label":"Silently to the board","value":"board"}],"required":true,"type":3},"label":"Report","type":18},{"component":{"custom_id":"expect","max_values":1,"min_values":1,"options":[{"default":true,"label":"short","value":"short"},{"default":false,"label":"long (over 10 min)","value":"long"}],"required":true,"type":3},"label":"Expected","type":18}],"custom_id":"nt:","title":"New task"},"type":9}"#,
        )
        .unwrap();
        assert_eq!(modal(None, None), want);
    }

    /// Whichever label it wears, the quote field is the same input.
    #[test]
    fn the_two_modals_differ_only_in_the_label_and_the_id() {
        let mut own = modal(Some(("x", true)), None);
        let mut other = modal(Some(("x", false)), None);
        own["data"]["custom_id"] = Value::Null;
        other["data"]["custom_id"] = Value::Null;
        own["data"]["components"][1]["label"] = Value::Null;
        other["data"]["components"][1]["label"] = Value::Null;
        assert_eq!(own, other);
    }
}
