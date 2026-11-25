//! A turn's folded steps, pressed (issue 56): **Show steps** opens the fold
//! (`render/fold.rs`), the page buttons move through a long one, and **Hide
//! steps** closes it. The press's id carries all it needs, the turn and the
//! page, `fold:o:<page>:<turn>` or `fold:c:<turn>`: nothing is kept for it.
//!
//! The place's lane is the one writer of a fold's message (`courier/press.rs`).
//! A press is acknowledged at once; then the place asks its lane, which
//! honours it only on the message it wrote that turn's fold to, and not while
//! it waits out Discord. Only then does the place make the fold's new state,
//! from its own renderer (reading the turn back from the session's nodes when
//! it has not seen it since it started: a restart), and send it to the lane
//! as any live state, after whatever the lane holds of that fold. So a fold
//! made before a restart opens after it, and a press and the turn's own
//! updates are written in the place's order, running or finished. What
//! refuses a press is said, in one line, to the presser alone.
//!
//! A folded FYI note (`courier/fold_note.rs`) is pressed the same way, by
//! `fold:no:<post>` and `fold:nc:<post>`: its lane reads its words back from
//! the post itself, only when the pressed message is the one the post was
//! written to, so a button forged elsewhere shows nothing.
//!
//! A press only shows again what the place showed openly before, under what
//! it shows now: it changes nothing else and asks the core nothing. So anyone
//! the place answers may press it, as anyone who can see the conversation
//! could read the lines before (the brief's B3); no act gate is read or
//! touched.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use twilight_model::application::interaction::{Interaction, InteractionData};
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle};
use twilight_model::channel::message::Component;
use twilight_model::http::interaction::InteractionResponseType;

use super::show::Unshown;
use super::{LaneMsg, Place, PlaceMsg, Shared};
use crate::bindings::Steps;
use crate::courier::{NotePress, Press, Told};
use crate::render::fold::{FoldButtons, SUFFIX};
use crate::render::Op;

/// A fold's press ids begin with this.
const PREFIX: &str = "fold:";

/// What the presser is told when the turn cannot be read back.
pub(crate) const TOO_OLD: &str =
    "These steps are too old to show again: only a conversation's newest turns are kept.";

/// What the presser is told when a note's post cannot be read back.
pub(crate) const NOTE_GONE: &str = "This note is too old to show again.";

/// What the presser is told when the place did not answer in time.
const BUSY: &str = "Theseus is busy here: press it again in a moment.";

/// What the presser is told while the place's lane waits out Discord.
pub(crate) const AWAY: &str =
    "Discord is not taking this conversation's messages just now: press it again in a moment.";

/// What the presser is told when the pressed message is not the turn's fold.
pub(crate) const NOT_HERE: &str = "This button is not this message's: nothing was changed.";

/// What the presser is told when the place shows no steps now.
pub(crate) const HIDDEN: &str = "This place shows no steps now.";

/// How long a press waits for its place.
const WAIT: Duration = Duration::from_secs(10);

/// A press on a fold, for its place: the turn, the page to open it at (None
/// closes it), where it was pressed, and where its presser is answered.
pub(crate) struct Ask {
    turn: String,
    page: Option<usize>,
    at: (u64, u64),
    reply: oneshot::Sender<Told>,
}

/// What the presser is told of `t`, if anything.
fn said(t: Told) -> Option<&'static str> {
    match t {
        Told::Shown => None,
        Told::Away => Some(AWAY),
        Told::NotHere => Some(NOT_HERE),
        Told::TooOld => Some(TOO_OLD),
        Told::Hidden => Some(HIDDEN),
        Told::NoteGone => Some(NOTE_GONE),
        Told::Busy => Some(BUSY),
    }
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

/// A folded note's press id as its post and whether it opens it.
pub(crate) fn parse_note(custom_id: &str) -> Option<(String, bool)> {
    let rest = custom_id.strip_prefix(PREFIX)?;
    let (open, post) = match rest.split_once(':')? {
        ("no", post) => (true, post),
        ("nc", post) => (false, post),
        _ => return None,
    };
    (!post.is_empty()).then(|| (post.to_string(), open))
}

/// A folded note's button: Show note, or, open, Hide note.
pub(crate) fn note_buttons(post: &str, open: bool) -> Vec<Component> {
    let b = if open {
        button("Hide note", format!("{PREFIX}nc:{post}"))
    } else {
        button("Show note", format!("{PREFIX}no:{post}"))
    };
    vec![Component::ActionRow(ActionRow {
        id: None,
        components: vec![b],
    })]
}

/// Whether `i` is a press on a fold or a folded note.
pub(super) fn is_press(i: &Interaction) -> bool {
    matches!(&i.data, Some(InteractionData::MessageComponent(c))
        if parse(&c.custom_id).is_some() || parse_note(&c.custom_id).is_some())
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

/// A press on a fold, in the place `place`: sent to the place at once, so
/// presses reach it in the order they came, then acknowledged, and its
/// presser answered, in a task of its own, so the gateway's handler never
/// waits on the place. False when the press is no fold's.
pub(super) fn pressed(
    shared: &Arc<Shared>,
    i: &Interaction,
    place: &mpsc::UnboundedSender<PlaceMsg>,
) -> bool {
    let Some(InteractionData::MessageComponent(c)) = &i.data else {
        return false;
    };
    let at = i
        .channel
        .as_ref()
        .zip(i.message.as_ref())
        .map(|(ch, m)| (ch.id.get(), m.id.get()));
    let (reply, answer) = oneshot::channel();
    let msg = match (parse(&c.custom_id), parse_note(&c.custom_id), at) {
        (Some((turn, page)), _, Some(at)) => PlaceMsg::Fold(Box::new(Ask {
            turn,
            page,
            at,
            reply,
        })),
        (None, Some((post, open)), Some(at)) => PlaceMsg::Note(Box::new(NotePress {
            post,
            open,
            at,
            reply,
        })),
        (None, None, _) => return false,
        // A fold's press on no message: nothing to show it on.
        _ => return true,
    };
    let sent = place.send(msg).is_ok();
    let (shared, i) = (shared.clone(), i.clone());
    tokio::spawn(async move {
        shared
            .respond(
                &i,
                InteractionResponseType::DeferredUpdateMessage,
                None,
                false,
            )
            .await;
        if !sent {
            return;
        }
        let told = match tokio::time::timeout(WAIT, answer).await {
            Ok(Ok(t)) => t,
            _ => Told::Busy,
        };
        if let Some(line) = said(told) {
            shared.followup(&i, line).await;
        }
    });
    true
}

impl Shared {
    /// How the place whose channel is `channel` shows its steps and notes: a
    /// bound channel's word, or a DM's by its user. A channel bound nowhere
    /// is folded, as a place that says nothing is.
    pub(crate) fn steps_in(&self, channel: u64) -> Steps {
        let user = {
            let r = self.routes.lock().unwrap();
            r.dm_channel
                .iter()
                .find(|(_, c)| **c == channel)
                .map(|(u, _)| *u)
        };
        let key = user.map_or_else(|| format!("channel:{channel}"), |u| format!("dm:{u}"));
        self.place_bits.get(&key).steps
    }
}

impl Place {
    /// A press on one of this place's folds: its lane checks it first, and
    /// only when the lane will write the fold does the place show it
    /// (`fold_show`), in this actor's order with every other update of it.
    pub(super) fn fold_asked(&mut self, ask: Ask) {
        let tx = self.tx.clone();
        let press = Press {
            key: format!("{}{SUFFIX}", ask.turn),
            at: ask.at,
            then: Box::new(move |checked| match checked {
                Ok(()) => {
                    let _ = tx.send(PlaceMsg::FoldShow(Box::new(ask)));
                }
                Err(why) => {
                    let _ = ask.reply.send(why);
                }
            }),
        };
        let _ = self.lane.send(LaneMsg::Press(Box::new(press)));
    }

    /// A press its lane will write: the fold as it shows now, from the
    /// renderer, which reads the session's turns back once when it does not
    /// hold the turn, sent to the lane as the fold's latest state.
    pub(super) fn fold_show(&mut self, ask: Ask) {
        if !self.renderer.knows(&ask.turn) && !self.renderer.was_rebuilt() {
            self.read_back();
        }
        let told = match self.view.press(&ask.turn, ask.page, &self.renderer) {
            Ok((content, buttons)) => {
                let _ = self.lane.send(LaneMsg::Live(Op::Upsert {
                    key: format!("{}{SUFFIX}", ask.turn),
                    content,
                    buttons,
                }));
                Told::Shown
            }
            Err(Unshown::Hidden) => Told::Hidden,
            Err(Unshown::TooOld) => Told::TooOld,
        };
        let _ = ask.reply.send(told);
    }

    /// A press on a folded note: its lane checks and writes it.
    pub(super) fn note_asked(&self, n: Box<NotePress>) {
        let _ = self.lane.send(LaneMsg::NotePress(n));
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

    /// A note's press id says its post and whether it opens; a turn's id is
    /// no note's, and a note's no turn's.
    #[test]
    fn a_note_press_id_carries_its_post() {
        assert_eq!(parse_note("fold:no:act_1"), Some(("act_1".into(), true)));
        assert_eq!(parse_note("fold:nc:act_1"), Some(("act_1".into(), false)));
        for bad in ["fold:no:", "fold:o:0:turn_a", "fold:c:turn_a", "src:no:x"] {
            assert_eq!(parse_note(bad), None, "{bad}");
        }
        assert_eq!(parse("fold:no:act_1"), None);
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
