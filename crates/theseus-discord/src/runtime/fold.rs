//! A turn's folded steps, pressed (issue 56): **Show steps** opens the fold
//! (`render/fold.rs`), the page buttons move through a long one, and **Hide
//! steps** closes it. The press's id carries all it needs, the turn and the
//! page, `fold:o:<page>:<turn>` or `fold:c:<turn>`: nothing is kept for it.
//! The place builds what the fold shows from its own renderer, reading the
//! turn back from the session's nodes when it has not seen it since it
//! started (a restart), and the pressed message is edited by its id, as a
//! confirm's settle is, so a fold made before a restart opens after it. A
//! turn too old to read back is said, in one line, to the presser alone.
//!
//! A press only shows again what the place showed openly before: it changes
//! nothing and asks the core nothing. So anyone the place answers may press
//! it, as anyone who can see the conversation could read the lines before
//! (the brief's B3); no act gate is read or touched.

use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use twilight_model::application::interaction::{Interaction, InteractionData};
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle};
use twilight_model::channel::message::{AllowedMentions, Component};
use twilight_model::http::interaction::InteractionResponseType;

use super::{command_reply, LaneMsg, Place, PlaceMsg, Shared};
use crate::render::fold::{FoldButtons, SUFFIX};
use crate::render::{Buttons, Op};

/// A fold's press ids begin with this.
const PREFIX: &str = "fold:";

/// What the presser is told when the turn cannot be read back.
pub(crate) const TOO_OLD: &str =
    "These steps are too old to show again: only a conversation's newest turns are kept.";

/// What the presser is told when the place did not answer in time.
const BUSY: &str = "Theseus is busy here: press it again in a moment.";

/// How long a press waits for its place.
const WAIT: Duration = Duration::from_secs(10);

/// A press on a fold, for its place: the turn, the page to open it at (None
/// closes it), and where the place answers what it shows now.
pub(crate) struct Ask {
    turn: String,
    page: Option<usize>,
    reply: oneshot::Sender<Option<(String, Buttons)>>,
}

/// The id of a press that opens `turn`'s fold at `page`, or closes it.
fn id(turn: &str, page: Option<usize>) -> String {
    match page {
        Some(p) => format!("{PREFIX}o:{p}:{turn}"),
        None => format!("{PREFIX}c:{turn}"),
    }
}

/// A fold's press id as its turn and page (None: close).
pub(crate) fn parse(custom_id: &str) -> Option<(String, Option<usize>)> {
    let rest = custom_id.strip_prefix(PREFIX)?;
    let (turn, page) = match rest.split_once(':')? {
        ("c", turn) => (turn, None),
        ("o", rest) => {
            let (page, turn) = rest.split_once(':')?;
            (turn, Some(page.parse().ok()?))
        }
        _ => return None,
    };
    (!turn.is_empty()).then(|| (turn.to_string(), page))
}

/// Whether `i` is a press on a fold.
pub(super) fn is_press(i: &Interaction) -> bool {
    matches!(&i.data, Some(InteractionData::MessageComponent(c)) if parse(&c.custom_id).is_some())
}

fn button(label: &str, custom_id: String) -> Component {
    Component::Button(Button {
        id: None,
        custom_id: Some(custom_id),
        disabled: false,
        emoji: None,
        label: Some(label.to_string()),
        style: ButtonStyle::Secondary,
        url: None,
        sku_id: None,
    })
}

/// A fold's components: its turn's "Should I have asked?" choices, as on a
/// process message, and a row with Show steps, or the pages' buttons and
/// Hide steps.
pub(crate) fn buttons(f: &FoldButtons) -> Vec<Component> {
    let mut rows = super::asked::asked_controls(&f.menu);
    let mut row = Vec::new();
    match f.page {
        None => row.push(button("Show steps", id(&f.turn, Some(0)))),
        Some(p) => {
            if p > 0 {
                row.push(button("◀ Earlier", id(&f.turn, Some(p - 1))));
            }
            if p + 1 < f.pages {
                row.push(button("Later ▶", id(&f.turn, Some(p + 1))));
            }
            row.push(button("Hide steps", id(&f.turn, None)));
        }
    }
    rows.push(Component::ActionRow(ActionRow {
        id: None,
        components: row,
    }));
    rows
}

impl Shared {
    /// A press on a fold, in the place `place`: acknowledged at once, then
    /// the pressed message shows what the place says the fold shows now.
    /// False when the press is no fold's.
    pub(super) async fn fold_press(
        &self,
        i: &Interaction,
        place: &mpsc::UnboundedSender<PlaceMsg>,
    ) -> bool {
        let Some(InteractionData::MessageComponent(c)) = &i.data else {
            return false;
        };
        let Some((turn, page)) = parse(&c.custom_id) else {
            return false;
        };
        self.respond(
            i,
            InteractionResponseType::DeferredUpdateMessage,
            None,
            false,
        )
        .await;
        let (reply, answer) = oneshot::channel();
        if place
            .send(PlaceMsg::Fold(Box::new(Ask { turn, page, reply })))
            .is_err()
        {
            return true;
        }
        let shown = match tokio::time::timeout(WAIT, answer).await {
            Ok(Ok(shown)) => shown,
            _ => {
                self.followup(i, BUSY).await;
                return true;
            }
        };
        let (Some((content, Buttons::Fold(f))), Some(ch), Some(m)) =
            (shown, i.channel.as_ref(), i.message.as_ref())
        else {
            self.followup(i, TOO_OLD).await;
            return true;
        };
        let content = command_reply::leaving(&self.core.exits, &content);
        let comps = buttons(&f);
        if let Err(e) = self
            .http
            .update_message(ch.id, m.id)
            .content(Some(&content))
            .components(Some(&comps))
            .allowed_mentions(Some(&AllowedMentions::default()))
            .await
        {
            self.board.error("fold press", None, e);
        }
        true
    }
}

impl Place {
    /// A press on one of this place's folds: what it shows now, from the
    /// renderer, which reads the session's turns back once when it does not
    /// hold the turn. While the turn runs, the same goes to the lane too,
    /// after any edit of the fold it holds, so a live edit never closes it.
    pub(super) fn fold_asked(&mut self, ask: Ask) {
        if !self.renderer.knows(&ask.turn) && !self.renderer.was_rebuilt() {
            self.read_back();
        }
        let shown = self.view.press(&ask.turn, ask.page, &self.renderer);
        if let Some((content, buttons)) = &shown {
            if self.renderer.running_turn().as_deref() == Some(ask.turn.as_str()) {
                let _ = self.lane.send(LaneMsg::Live(Op::Upsert {
                    key: format!("{}{SUFFIX}", ask.turn),
                    content: content.clone(),
                    buttons: buttons.clone(),
                }));
            }
        }
        let _ = ask.reply.send(shown);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A press id says its turn and page, and nothing else reads as one.
    #[test]
    fn a_press_id_carries_the_turn_and_the_page() {
        let t = "turn_01JABCDEF";
        assert_eq!(parse(&id(t, Some(3))), Some((t.to_string(), Some(3))));
        assert_eq!(parse(&id(t, None)), Some((t.to_string(), None)));
        for bad in [
            "fold:o:x:turn_a",
            "fold:c:",
            "fold:z:turn_a",
            "src:a:b",
            "fold:o:1",
        ] {
            assert_eq!(parse(bad), None, "{bad}");
        }
        assert!(id(&"t".repeat(80), Some(99)).len() <= 100, "Discord's 100");
    }

    /// Closed: Show steps alone. Open: the pages' buttons where there is a
    /// page that way, and Hide steps.
    #[test]
    fn the_buttons_open_page_and_close() {
        let labels = |f: &FoldButtons| -> Vec<String> {
            buttons(f)
                .into_iter()
                .flat_map(|r| match r {
                    Component::ActionRow(r) => r.components,
                    c => vec![c],
                })
                .filter_map(|c| match c {
                    Component::Button(b) => b.label,
                    _ => None,
                })
                .collect()
        };
        let f = |page, pages| FoldButtons {
            turn: "turn_a".into(),
            page,
            pages,
            menu: vec![],
        };
        assert_eq!(labels(&f(None, 1)), ["Show steps"]);
        assert_eq!(labels(&f(Some(0), 1)), ["Hide steps"]);
        assert_eq!(labels(&f(Some(0), 3)), ["Later ▶", "Hide steps"]);
        assert_eq!(
            labels(&f(Some(1), 3)),
            ["◀ Earlier", "Later ▶", "Hide steps"]
        );
        assert_eq!(labels(&f(Some(2), 3)), ["◀ Earlier", "Hide steps"]);
    }
}
