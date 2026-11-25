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
//! refuses a press is said, in one line, to the presser alone. A press not
//! taken within `WAIT` is cancelled: it can never apply after, and the
//! presser is told nothing changed.
//!
//! A fold update the lane could not write in an outage is dropped, as live
//! progress always was; once the lane is back and every post that waited has
//! gone, the place writes each fold it still holds once more, as it shows now
//! (`Place::lane_back`).
//!
//! A press only shows again what the place showed openly before, under what
//! it shows now: it changes nothing else and asks the core nothing. So anyone
//! the place answers may press it, as anyone who can see the conversation
//! could read the lines before (the brief's B3); no act gate is read or
//! touched.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use twilight_model::application::interaction::{Interaction, InteractionData};
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle};
use twilight_model::channel::message::Component;
use twilight_model::http::interaction::InteractionResponseType;

use super::show::Unshown;
use super::{LaneMsg, Place, PlaceMsg, Shared};
use crate::courier::{Press, Told};
use crate::render::fold::{FoldButtons, SUFFIX};
use crate::render::Op;

/// A fold's press ids begin with this.
const PREFIX: &str = "fold:";

/// What the presser is told when the turn cannot be read back.
pub(crate) const TOO_OLD: &str =
    "These steps are too old to show again: only a conversation's newest turns are kept.";

/// What the presser is told when the press was not taken in time.
pub(crate) const CANCELLED: &str =
    "That press waited too long for Discord and was dropped: nothing changed.";

/// What the presser is told while the place's lane waits out Discord.
pub(crate) const AWAY: &str =
    "Discord is not taking this conversation's messages just now: press it again in a moment.";

/// What the presser is told when the pressed message is not the turn's fold.
pub(crate) const NOT_HERE: &str = "This button is not this message's: nothing was changed.";

/// What the presser is told when the place shows no steps now.
pub(crate) const HIDDEN: &str = "This place shows no steps now.";

/// How long a press waits to be taken: past it, it is cancelled.
const WAIT: Duration = Duration::from_secs(10);

/// A press's state, shared by its presser's task and its place: the first to
/// move it from `WAITING` wins, so a press is either applied or cancelled,
/// never both.
const WAITING: u8 = 0;
const TAKEN: u8 = 1;
const DROPPED: u8 = 2;

/// A press on a fold, for its place: the turn, the page to open it at (None
/// closes it), where it was pressed, whether it is still wanted, and where
/// its presser is answered.
pub(crate) struct Ask {
    turn: String,
    page: Option<usize>,
    at: (u64, u64),
    state: Arc<AtomicU8>,
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
        Told::Cancelled => Some(CANCELLED),
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

/// Whether `i` is a press on a fold.
pub(super) fn is_press(i: &Interaction) -> bool {
    matches!(&i.data, Some(InteractionData::MessageComponent(c))
        if parse(&c.custom_id).is_some())
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
/// waits on the place. A press not taken within `WAIT` is cancelled. False
/// when the press is no fold's.
pub(super) fn pressed(
    shared: &Arc<Shared>,
    i: &Interaction,
    place: &mpsc::UnboundedSender<PlaceMsg>,
) -> bool {
    let Some(InteractionData::MessageComponent(c)) = &i.data else {
        return false;
    };
    let Some((turn, page)) = parse(&c.custom_id) else {
        return false;
    };
    let Some(at) = i
        .channel
        .as_ref()
        .zip(i.message.as_ref())
        .map(|(ch, m)| (ch.id.get(), m.id.get()))
    else {
        // A fold's press on no message: nothing to show it on.
        return true;
    };
    let (reply, answer) = oneshot::channel();
    let state = Arc::new(AtomicU8::new(WAITING));
    let ask = Ask {
        turn,
        page,
        at,
        state: state.clone(),
        reply,
    };
    let sent = place.send(PlaceMsg::Fold(Box::new(ask))).is_ok();
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
        let told = answer_of(&state, answer, WAIT).await;
        if let Some(line) = said(told) {
            shared.followup(&i, line).await;
        }
    });
    true
}

/// What a press's presser is told: its place's answer, or, when none came
/// within `wait`, that it was dropped, unless its place took it first, whose
/// answer then follows.
async fn answer_of(state: &AtomicU8, mut answer: oneshot::Receiver<Told>, wait: Duration) -> Told {
    match tokio::time::timeout(wait, &mut answer).await {
        Ok(Ok(t)) => t,
        _ if dropped(state) => Told::Cancelled,
        _ => answer.await.unwrap_or(Told::Cancelled),
    }
}

/// Cancel a press still waiting; false when its place took it first.
fn dropped(state: &AtomicU8) -> bool {
    state
        .compare_exchange(WAITING, DROPPED, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
}

impl Shared {
    /// The lane `target` is back from an outage and every post that waited
    /// has gone: its place writes its folds once more (`Place::lane_back`).
    pub(crate) fn lane_back(&self, target: &str) {
        let r = self.routes.lock().unwrap();
        let id = |p: &str| target.strip_prefix(p).and_then(|x| x.parse::<u64>().ok());
        let place = match (id("discord:channel:"), id("discord:dm:")) {
            (Some(c), _) => r.by_channel.get(&c),
            (_, Some(u)) => r.by_dm_user.get(&u),
            _ => None,
        };
        if let Some(tx) = place {
            let _ = tx.send(PlaceMsg::LaneBack);
        }
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
    /// hold the turn, sent to the lane as the fold's latest state. A press
    /// its presser has stopped waiting for changes nothing.
    pub(super) fn fold_show(&mut self, ask: Ask) {
        let taken = ask
            .state
            .compare_exchange(WAITING, TAKEN, Ordering::AcqRel, Ordering::Acquire)
            .is_ok();
        if !taken {
            return;
        }
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

    /// Its lane is back from an outage, every post that waited gone: each
    /// fold the place still holds is written once more, as it shows now. An
    /// update the outage dropped is not lost, and nothing was kept meanwhile.
    pub(super) fn lane_back(&mut self) {
        for op in self.view.resend(&self.renderer) {
            let _ = self.lane.send(LaneMsg::Live(op));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The press's wait, on a clock the test drives (tokio's paused time):
    /// no answer within it, and the press is dropped and told so, and its
    /// place can no longer take it; an answer within it is told as it is;
    /// a press its place took just before the wait ended is told its own
    /// answer when it comes, never "dropped".
    #[tokio::test(start_paused = true)]
    async fn a_press_not_taken_within_its_wait_is_dropped_and_never_taken() {
        let state = Arc::new(AtomicU8::new(WAITING));
        let (reply, answer) = oneshot::channel::<Told>();
        let told = tokio::spawn({
            let state = state.clone();
            async move { answer_of(&state, answer, WAIT).await }
        });
        tokio::time::advance(WAIT + Duration::from_millis(1)).await;
        assert_eq!(told.await.unwrap(), Told::Cancelled);
        assert_eq!(state.load(Ordering::Acquire), DROPPED);
        let taken = state.compare_exchange(WAITING, TAKEN, Ordering::AcqRel, Ordering::Acquire);
        assert!(taken.is_err(), "its place can no longer take it");
        drop(reply);

        let state = Arc::new(AtomicU8::new(WAITING));
        let (reply, answer) = oneshot::channel::<Told>();
        let told = tokio::spawn({
            let state = state.clone();
            async move { answer_of(&state, answer, WAIT).await }
        });
        tokio::time::advance(WAIT / 2).await;
        reply.send(Told::Shown).unwrap();
        assert_eq!(told.await.unwrap(), Told::Shown);

        let state = Arc::new(AtomicU8::new(WAITING));
        let (reply, answer) = oneshot::channel::<Told>();
        let told = tokio::spawn({
            let state = state.clone();
            async move { answer_of(&state, answer, WAIT).await }
        });
        assert!(state
            .compare_exchange(WAITING, TAKEN, Ordering::AcqRel, Ordering::Acquire)
            .is_ok());
        tokio::time::advance(WAIT * 2).await;
        reply.send(Told::Hidden).unwrap();
        assert_eq!(told.await.unwrap(), Told::Hidden, "its own answer");
    }

    /// At its place: a press its presser dropped changes nothing (nothing
    /// for the lane, no answer); a press still waiting is taken, and told.
    #[tokio::test]
    async fn a_dropped_press_changes_nothing_at_its_place() {
        use theseus_core::secrets::SecretBoard;
        use theseus_core::session::SessionRecord;
        use theseus_protocol::SessionKind;
        let d = tempfile::tempdir().unwrap();
        let core = super::super::tests::core_with(d.path(), SecretBoard::empty(), |_| {});
        let rec = SessionRecord::new(SessionKind::Conversation, None);
        let sid = rec.session_id.clone();
        core.store.put_session(&sid, &rec).unwrap();
        let (mut place, _mailbox) = super::super::tests::place_for_tests(&core, &sid);
        let (lane_tx, mut lane) = mpsc::unbounded_channel();
        place.lane = lane_tx;
        let ask = |state: &Arc<AtomicU8>| {
            let (reply, answer) = oneshot::channel();
            let a = Ask {
                turn: "turn_gone".into(),
                page: Some(0),
                at: (1, 2),
                state: state.clone(),
                reply,
            };
            (a, answer)
        };
        let state = Arc::new(AtomicU8::new(WAITING));
        assert!(dropped(&state));
        let (a, mut answer) = ask(&state);
        place.fold_show(a);
        assert!(lane.try_recv().is_err(), "nothing for the lane");
        assert!(answer.try_recv().is_err(), "no answer");
        assert_eq!(state.load(Ordering::Acquire), DROPPED);

        let state = Arc::new(AtomicU8::new(WAITING));
        let (a, mut answer) = ask(&state);
        place.fold_show(a);
        assert_eq!(state.load(Ordering::Acquire), TAKEN);
        assert_eq!(answer.try_recv(), Ok(Told::TooOld));
        assert!(!dropped(&state), "taken: no longer cancellable");
    }

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
