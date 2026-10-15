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

use serde_json::{json, Value};
use theseus_protocol::task_start::{ReportTo, TaskStartParams, TaskStartResult};
use twilight_model::application::interaction::modal::{
    ModalInteractionComponent, ModalInteractionData,
};
use twilight_model::application::interaction::Interaction;

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
/// Discord's text input limit.
const INPUT_MAX: usize = 4000;

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

/// The new-task modal; `quote` is a message's text, offered as a quote.
pub(super) fn modal(quote: Option<&str>) -> Value {
    let mut components = vec![label(
        "What should the task do?",
        None,
        json!({"type": 4, "custom_id": BRIEF, "style": 2, "required": true,
               "max_length": INPUT_MAX}),
    )];
    if let Some(q) = quote {
        components.push(label(
            "The message (read as outside text)",
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
    json!({"type": 9, "data": {"custom_id": MODAL, "title": "New task", "components": components}})
}

/// A submitted modal's fields.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Submit {
    pub(super) brief: String,
    pub(super) quote: String,
    pub(super) budget: Option<f64>,
    pub(super) board: bool,
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
                ModalInteractionComponent::StringSelect(v) if v.custom_id == REPORT => {
                    s.board = v.values.first().is_some_and(|v| v == "board")
                }
                _ => {}
            }
        }
        let mut s = Submit::default();
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_modal_has_a_brief_a_budget_select_and_a_report_select() {
        let m = modal(None);
        let parts = m["data"]["components"].as_array().unwrap();
        let ids: Vec<&str> = parts
            .iter()
            .map(|p| p["component"]["custom_id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["brief", "budget", "report"]);
        let budget = &parts[1]["component"]["options"];
        assert_eq!(budget.as_array().unwrap().len(), 4);
        assert_eq!(budget[1]["default"], true, "$2 is the default");
    }

    #[test]
    fn a_message_made_a_task_brings_its_text_as_an_editable_quote() {
        let m = modal(Some("the log says the reef sensor died"));
        let parts = m["data"]["components"].as_array().unwrap();
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[1]["component"]["custom_id"], "quote");
        assert_eq!(
            parts[1]["component"]["value"],
            "the log says the reef sensor died"
        );
        assert_eq!(parts[1]["component"]["required"], false);
    }
}
