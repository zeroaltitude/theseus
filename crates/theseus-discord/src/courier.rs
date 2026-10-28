//! Durable delivery, the binding's side (theseus-q4v): the lanes.
//!
//! One lane per place (a text channel or a DM), and one for the operator's
//! notices, each the only writer of its messages. A lane:
//! - sends its target's outbox posts in order, each dispatched before its
//!   first call and settled with the message ids Discord gave;
//! - then serves the live progress its place's renderer sends: the streamed
//!   text and tool messages of a running turn, and typing, only the latest
//!   state of each message, never replayed. While Discord is away, live
//!   progress is dropped and the posts wait.
//!
//! A lane needs only REST, so posts go out while the gateway is down. A create
//! carries Discord's nonce, with `enforce_nonce`, derived from its message's
//! key: a retry after a crash between the send and the settle returns the
//! first message instead of posting again.
//!
//! A card keeps its turn's order (theseus-50p). Its call's tool line is live
//! progress, and the card is a post written the moment the call asks, so the
//! card would go first, and the turn's reply after it, and the tool line last.
//! A card written while this lane runs, for a session a place here renders,
//! waits until the place has shown its call (`LaneMsg::Asked`), at most
//! `CARD_WAIT`; what the place showed before that goes first.

mod ask;
mod board;
mod catchup;
mod held;
mod offer;
mod persona;
mod thread;
pub(crate) mod v2;

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use theseus_protocol::LedgerKind;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use theseus_core::outbox::{body_of, kind_of, Closed};
use theseus_core::places::PlaceClass;
use theseus_kernel::{Action, ActionState, Outcome};
use theseus_protocol::TurnSubmitResult;
use tokio::sync::mpsc;
use twilight_http::request::Request;
use twilight_http::routing::Route as HttpRoute;
use twilight_model::channel::message::{AllowedMentions, Message, MessageFlags};
use twilight_model::id::Id;

use crate::policy::{self, Event};
use crate::render::{self, Buttons, NoticeCard, Op, Route};
use crate::runtime::{asked_button, asked_menu, confirm_buttons, Shared};

/// How long Discord is taken to honor a nonce. Past it, a create sent again
/// after a send that may have landed says it may be a copy (`render::RESENT`);
/// within it, Discord returns the first message and nothing new is posted.
pub const NONCE_WINDOW_MS: u64 = 120_000;
/// The longest a lane waits before it tries Discord again.
const BACKOFF_MAX: Duration = Duration::from_secs(60);
/// The longest a card waits for its place to show its call (theseus-50p).
/// The place says so within milliseconds; past this, something kept it from
/// seeing the question, and the card goes without it.
const CARD_WAIT: Duration = Duration::from_secs(5);
/// How many questions a lane remembers its place has shown.
const ASKED_KEPT: usize = 64;
/// How many message keys a lane remembers (`msgs`, `sent`, `sealed`), the
/// task board's and its place's held turns' aside: past it, the quarter named
/// longest ago is forgotten (theseus-celu.37, theseus-6809).
const KEYS_KEPT: usize = 256;

/// What a lane is told.
pub(crate) enum LaneMsg {
    /// Live progress: the latest state of one message, or typing.
    Live(Op),
    /// The outbox changed: look for posts.
    Wake,
    /// The DM's channel, once a message came from it.
    Channel(u64),
    /// Who wrote the place's latest message: the DM a card prefers.
    Author(u64),
    /// The message a turn's first streamed message replies to.
    Anchor(u64),
    /// The place has shown a question's call, its tool line sent on before
    /// this: the question's card may follow (theseus-50p).
    Asked(String),
    /// The place's label, changed by the bindings file (theseus-ocwt).
    Label(String),
    /// The turns the place's renderer holds now, oldest first, sent whenever
    /// they change (theseus-6809): their keys are kept whole, and a turn
    /// that leaves the list is forgotten.
    Held(Vec<String>),
    /// The task board with nothing to show (theseus-2j2ka): written only
    /// where a board is pinned already.
    BoardIdle(String),
    /// The place's board is drawn elsewhere now (the task home stands,
    /// theseus-ysp2k): the one this lane made is removed.
    BoardMoved,
}

/// Why a call to Discord did not go through.
#[derive(Debug)]
pub(crate) struct SendErr {
    /// Discord is away (or busy): try again later. Otherwise it refused for
    /// good, and no retry changes that.
    pub away: bool,
    /// The call may have reached Discord anyway: a timeout, a 5xx, a
    /// connection that closed after the request went out.
    pub unsure: bool,
    /// The message is gone (404 on an edit).
    pub gone: bool,
    pub message: String,
}

impl SendErr {
    fn refused(message: impl Into<String>) -> Self {
        Self {
            away: false,
            unsure: false,
            gone: false,
            message: message.into(),
        }
    }

    pub(crate) fn of(e: &twilight_http::Error) -> Self {
        use twilight_http::error::ErrorType as E;
        let message = e.to_string();
        let (away, unsure, gone) = match e.kind() {
            E::Response { status, .. } => match status.get() {
                429 => (true, false, false),
                404 => (false, false, true),
                s if s >= 500 => (true, true, false),
                _ => (false, false, false),
            },
            E::RequestError => (true, !refused_connection(e), false),
            E::RequestTimedOut | E::RequestCanceled | E::Parsing { .. } => (true, true, false),
            E::BuildingRequest | E::Json | E::Validation => (false, false, false),
            // A token that is not one, or that Discord revoked: nothing is sent,
            // and the posts wait for a restart with a good one.
            _ => (true, false, false),
        };
        Self {
            away,
            unsure,
            gone,
            message,
        }
    }
}

/// A connection Discord (or the proxy) refused: the request never went out.
fn refused_connection(e: &twilight_http::Error) -> bool {
    let mut src: Option<&(dyn std::error::Error + 'static)> = std::error::Error::source(e);
    while let Some(s) = src {
        if let Some(io) = s.downcast_ref::<std::io::Error>() {
            if io.kind() == std::io::ErrorKind::ConnectionRefused {
                return true;
            }
        }
        src = s.source();
    }
    format!("{e:?}").contains("ConnectionRefused")
}

/// What a card does now (theseus-50p).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CardHold {
    /// It goes.
    Go,
    /// Its place has not shown its call yet: it waits until then.
    Until(tokio::time::Instant),
    /// Its wait is over, and its place never showed the call: it goes all the
    /// same, since a card is never held hostage to live progress.
    GaveUp,
}

/// The card's rule, over what its lane knows: whether the place has shown the
/// call (`asked`), whether the card was written while this lane runs
/// (`fresh`) for a session a place here renders (`rendered_here`), and the wait
/// it began, if any.
fn card_hold(
    asked: bool,
    fresh: bool,
    rendered_here: bool,
    held: Option<tokio::time::Instant>,
    now: tokio::time::Instant,
) -> CardHold {
    if asked || !fresh || !rendered_here {
        return CardHold::Go;
    }
    match held {
        None => CardHold::Until(now + CARD_WAIT),
        Some(until) if now < until => CardHold::Until(until),
        Some(_) => CardHold::GaveUp,
    }
}

/// A create's nonce: from its message's key, which names the turn or the
/// post it belongs to, so every send of one message carries the same one.
/// A decimal u64, under Discord's 25 characters.
pub fn nonce(key: &str) -> String {
    let d = Sha256::digest(format!("theseus/{key}").as_bytes());
    let mut b = [0u8; 8];
    b.copy_from_slice(&d[..8]);
    u64::from_be_bytes(b).to_string()
}

/// One message a post writes: created, or edited when its message is known.
#[derive(Clone)]
struct Write {
    key: String,
    channel: u64,
    content: String,
    buttons: Buttons,
    reply_to: Option<u64>,
    /// The message to edit, when the post knows it (a card's settle).
    message: Option<u64>,
    /// Who its create notifies (theseus-9j9): a card's answerers, named at
    /// the start of its content; nobody, for every other message.
    mentions: Vec<u64>,
    /// Whether its create notifies at all (theseus-l1y1, `policy`): without
    /// it the message is silent, mentions included. An edit never notifies.
    ping: bool,
}

/// Whether `w` is a question waiting on the owner: a card with an answer's
/// buttons, the needs-you post, a V2 question card, or the burst summary that
/// says how many wait. The ping window does not hold these back.
fn asks(w: &Write) -> bool {
    matches!(
        w.buttons,
        Buttons::Confirm(_)
            | Buttons::Card(..)
            | Buttons::Ask(..)
            | Buttons::ConfirmGrant(..)
            | Buttons::Accept(_)
    ) || w.key.starts_with("needs-you:")
        || v2::question_of(&w.key).is_some()
}

/// `content`, after a mention of each of `users`: Discord notifies them,
/// and only them, when `allowed_mentions.users` names the same (theseus-9j9).
fn mentioning(users: &[u64], content: String) -> String {
    if users.is_empty() {
        return content;
    }
    let at: Vec<String> = users.iter().map(|u| format!("<@{u}>")).collect();
    format!("{} {content}", at.join(" "))
}

/// Who can answer a card in a guild channel (theseus-9j9): the place's
/// users, whom the binding lets press its buttons, and of them only the
/// owners, since the core refuses anyone else's answer (the place rule,
/// `places::owner_in_private`). A shared channel's cards go to a DM.
pub(crate) fn answerers(place_users: &[u64], owners: &BTreeSet<String>) -> Vec<u64> {
    place_users
        .iter()
        .copied()
        .filter(|u| owners.contains(&format!("discord:{u}")))
        .collect()
}

/// What a post comes to: its writes, and what its completion keeps besides
/// the messages.
struct Plan {
    writes: Vec<Write>,
    extra: Value,
}

impl Plan {
    fn nothing(why: &str) -> Self {
        Self {
            writes: vec![],
            extra: json!({"skipped": why}),
        }
    }
}

pub(crate) struct Lane {
    pub shared: Arc<Shared>,
    /// `discord:dm:<user>`, `discord:channel:<id>`, or `discord:operator`.
    pub target: String,
    /// "dm", "channel", or "operator".
    pub kind: &'static str,
    /// As a card names the place: `#general`, `DM @zeroaltitude`.
    pub label: String,
    pub channel: Option<u64>,
    pub dm_user: Option<u64>,
    pub last_author: Option<u64>,
    /// key → (channel, message) of the messages this lane wrote: the held
    /// turns' and the board's, and the newest `KEYS_KEPT` others by when
    /// their key was last named (`touched`).
    pub msgs: HashMap<String, (u64, u64)>,
    /// key → what Discord last got for it.
    pub sent: HashMap<String, String>,
    /// Keys a post made final: the stream's later states of them are dropped.
    pub sealed: HashSet<String>,
    /// key → when it was last named, by `clock`: written, sealed, or a live
    /// state of it taken, dropped or not. Every key of `msgs`, `sent`, and
    /// `sealed` but the board's and the held turns' is here, so those others
    /// number `KEYS_KEPT` at most.
    pub touched: HashMap<String, u64>,
    /// The turns its place's renderer holds (`LaneMsg::Held`): their keys
    /// stay out of the recency bound (theseus-6809).
    pub held_turns: Vec<String>,
    /// Turns the renderer dropped whose keys a waiting live op still names:
    /// forgotten once that op is written (`held.rs`).
    pub released: Vec<String>,
    pub clock: u64,
    /// Live ops waiting: the latest per message, in the order they first came.
    pub live: Vec<Op>,
    pub anchor: Option<u64>,
    /// The owner's message whose turn's first text part has not been written
    /// yet: that part is the answer (theseus-l1y1, `[discord] silent`'s
    /// `answer`, apart from the reply's other parts). Set with the
    /// anchor, when its author is an owner; taken by the part's create, or
    /// by the reply's post that answers it.
    pub owed: Option<u64>,
    /// Discord is away until then: nothing is tried, and live ops drop.
    pub retry_at: Option<tokio::time::Instant>,
    pub attempt: u32,
    /// Posts whose earlier sends may have landed in this process.
    pub unsure: HashSet<String>,
    /// When this process started, in unix ms: a post dispatched before it may
    /// have landed.
    pub started_ms: u64,
    /// Questions whose calls the place has shown, the newest last.
    pub asked: VecDeque<String>,
    /// A card waiting for its place to show its call, and until when.
    pub held: Option<(String, tokio::time::Instant)>,
    /// The place showed a call after live progress came: that progress goes
    /// before the next post.
    pub stream_first: bool,
    /// The task board was looked for among the pins in this process (39b).
    pub board_sought: bool,
    /// A pin of the board was refused, and said so once.
    pub pin_refused: bool,
    /// A board this lane made that is to be removed (`LaneMsg::BoardMoved`).
    pub stale_board: Option<(u64, u64)>,
    /// An earlier run's board is to be looked for among the pins.
    pub seek_stale: bool,
    /// A persona's signature: its webhook and the messages it wrote
    /// (theseus-sjzm).
    signing: persona::Signing,
    /// Components V2: the board's pace and the needs-you cards' pings (theseus-2j2ka).
    pub v2: v2::State,
    /// The outage this lane is coming back from (theseus-ss36w).
    catchup: catchup::State,
}

impl Lane {
    pub(crate) fn new(
        shared: Arc<Shared>,
        target: String,
        kind: &'static str,
        label: String,
        channel: Option<u64>,
        dm_user: Option<u64>,
    ) -> Self {
        Self {
            shared,
            target,
            kind,
            label,
            channel,
            dm_user,
            last_author: None,
            msgs: HashMap::new(),
            sent: HashMap::new(),
            sealed: HashSet::new(),
            touched: HashMap::new(),
            held_turns: Vec::new(),
            released: Vec::new(),
            clock: 0,
            live: Vec::new(),
            anchor: None,
            owed: None,
            retry_at: None,
            attempt: 0,
            unsure: HashSet::new(),
            started_ms: theseus_protocol::now_unix_ms(),
            asked: VecDeque::new(),
            held: None,
            stream_first: false,
            board_sought: false,
            pin_refused: false,
            stale_board: None,
            seek_stale: false,
            signing: Default::default(),
            v2: v2::State::default(),
            catchup: catchup::State::default(),
        }
    }

    pub(crate) async fn run(mut self, mut rx: mpsc::UnboundedReceiver<LaneMsg>) {
        loop {
            // Woken by a message, by the end of Discord's absence, or by the
            // end of a held card's wait.
            let wake = self
                .retry_at
                .into_iter()
                .chain(self.held.as_ref().map(|h| h.1))
                .chain(self.v2.due())
                .min();
            let msg = match wake {
                Some(at) => tokio::select! {
                    m = rx.recv() => m.map(Some),
                    _ = tokio::time::sleep_until(at) => Some(None),
                },
                None => rx.recv().await.map(Some),
            };
            match msg {
                None => break,
                Some(Some(m)) => self.take(m),
                Some(None) => {}
            }
            // Everything queued meanwhile: live ops collapse to their latest.
            while let Ok(m) = rx.try_recv() {
                self.take(m);
            }
            if self.retires() {
                break;
            }
            if self
                .retry_at
                .is_some_and(|at| tokio::time::Instant::now() < at)
            {
                continue;
            }
            self.retry_at = None;
            self.v2_tick().await;
            self.drop_stale_board().await;
            // What the place showed before it showed a call goes before the
            // call's card (theseus-50p).
            if std::mem::take(&mut self.stream_first) {
                self.apply_live().await;
            }
            // The posts first, in order; then the stream.
            if self.deliver_posts().await {
                self.apply_live().await;
            }
            if self.retires() {
                break;
            }
        }
    }

    /// The bindings file no longer names this lane's place (theseus-ocwt):
    /// it ends here, between posts, so a post it sent settled as sent, and
    /// what it had not sent is refused, as a start refuses it.
    fn retires(&self) -> bool {
        if !self.shared.lane_retires(&self.target) {
            return false;
        }
        tracing::info!(target = %self.target, "discord: a lane whose place is no longer bound ends");
        self.shared.refuse_unbound();
        true
    }

    fn take(&mut self, m: LaneMsg) {
        match m {
            LaneMsg::Live(op) => {
                let board = op.key() == Some(render::BOARD_KEY);
                if board {
                    self.v2.only_if_pinned = false;
                }
                if self.retry_at.is_none() || board {
                    // A loop's thinking goes before the reply that follows it
                    // (theseus-l1y1), as a call's progress goes before its card.
                    if let Op::Upsert { key, content, .. } = &op {
                        self.stream_first |= !self.msgs.contains_key(key)
                            && policy::of_live(key, false, content) == Event::Thinking;
                    }
                    self.queue_live(op);
                }
            }
            LaneMsg::Wake => {}
            LaneMsg::BoardIdle(content) => {
                self.take(LaneMsg::Live(Op::Upsert {
                    key: render::BOARD_KEY.into(),
                    content,
                    buttons: Buttons::V2,
                }));
                self.v2.only_if_pinned = true;
            }
            LaneMsg::BoardMoved => self.board_moved(),
            LaneMsg::Channel(c) => self.channel = Some(c),
            LaneMsg::Author(a) => self.last_author = Some(a),
            LaneMsg::Anchor(a) => {
                self.anchor = Some(a);
                self.owed = self.by_an_owner().then_some(a);
            }
            LaneMsg::Label(l) => self.label = l,
            LaneMsg::Held(turns) => self.hold(turns),
            LaneMsg::Asked(q) => {
                self.stream_first |= !self.live.is_empty();
                self.asked.push_back(q);
                if self.asked.len() > ASKED_KEPT {
                    self.asked.pop_front();
                }
            }
        }
    }

    /// Keep the latest state of each message, where it first came.
    fn queue_live(&mut self, op: Op) {
        let Some(key) = op.key().map(str::to_string) else {
            if !self.live.iter().any(|o| matches!(o, Op::Typing)) {
                self.live.push(op);
            }
            return;
        };
        self.touch(&key);
        if self.sealed.contains(&key) {
            return;
        }
        match self.live.iter_mut().find(|o| o.key() == Some(key.as_str())) {
            Some(slot) => *slot = op,
            None => self.live.push(op),
        }
    }

    /// `key` was named now. Past `KEYS_KEPT`, the quarter of the keys named
    /// longest ago is forgotten from every map, the board's and the held
    /// turns' never (theseus-celu.37, theseus-6809). A key is forgotten only after `KEYS_KEPT * 3 / 4`
    /// others were named since it, and every late state of a key names it
    /// again: so a sealed reply part is forgotten only once its place's
    /// actor, which sends a turn's states in its events' order, has shown
    /// hundreds of messages of later turns, long after the turn's last state.
    fn touch(&mut self, key: &str) {
        self.clock += 1;
        if key == render::BOARD_KEY || self.held_key(key) {
            return;
        }
        self.touched.insert(key.to_string(), self.clock);
        if self.touched.len() <= KEYS_KEPT {
            return;
        }
        let mut by_age: Vec<(u64, String)> = self.touched.drain().map(|(k, at)| (at, k)).collect();
        by_age.sort_unstable();
        let forget = by_age.len() - KEYS_KEPT * 3 / 4;
        for (i, (at, k)) in by_age.into_iter().enumerate() {
            if i < forget {
                self.msgs.remove(&k);
                self.sent.remove(&k);
                self.sealed.remove(&k);
            } else {
                self.touched.insert(k, at);
            }
        }
    }

    /// A post made `key` final: the stream's later states of it are dropped.
    fn seal(&mut self, key: &str) {
        self.touch(key);
        self.sealed.insert(key.to_string());
    }

    /// Discord is away: wait, and drop the stream meanwhile.
    fn away(&mut self, e: &SendErr) {
        self.attempt += 1;
        let delay = Duration::from_secs(1 << self.attempt.min(6)).min(BACKOFF_MAX);
        self.retry_at = Some(tokio::time::Instant::now() + delay);
        // The board is a state, not a stream: its latest waits out the
        // outage, so a place does not keep a stale one until its next change.
        let board = self
            .live
            .iter()
            .find(|o| o.key() == Some(render::BOARD_KEY))
            .cloned();
        self.live.clear();
        self.live.extend(board);
        self.release();
        self.shared.core.outbox.error("discord", e.message.clone());
        if self.attempt == 1 {
            // Once per outage in the ledger; health keeps the latest.
            self.shared
                .board
                .error("deliver", None, format!("{}: {}", self.target, e.message));
        } else {
            tracing::debug!(target = %self.target, error = %e.message, attempt = self.attempt, "discord still away");
        }
    }

    // ------------------------------------------------------------ posts

    /// Deliver this lane's posts in order. False when Discord is away, or
    /// the daemon is stopping: then no post is dispatched, and each waits for
    /// the next start (theseus-pfv).
    async fn deliver_posts(&mut self) -> bool {
        // Back from an outage: what was written in it folds into one message.
        if self.catchup.pending() {
            if let Err(e) = self.catch_up().await {
                if !self.shared.core.outbox.stopping() {
                    self.away(&e);
                }
                return false;
            }
        }
        while !self.shared.core.outbox.stopping() {
            // Its place is no longer bound: no post is begun (theseus-ocwt).
            if self.shared.is_retired(&self.target) {
                return false;
            }
            let Some(a) = self.shared.core.outbox.next_for(&self.target) else {
                return true;
            };
            // It waits, and the posts after it wait behind it; the stream
            // goes on.
            if self.waits_for_its_call(&a) {
                return true;
            }
            match self.deliver(&a).await {
                Ok(()) => self.attempt = 0,
                // The stop came while this post was planned or sent: it is
                // the stop's, not Discord's absence.
                Err(_) if self.shared.core.outbox.stopping() => return false,
                Err(e) => {
                    if e.unsure {
                        self.unsure.insert(a.correlation_id.clone());
                    }
                    self.catchup.begin(a.planned_at_ms);
                    self.away(&e);
                    return false;
                }
            }
        }
        false
    }

    /// Does this post wait for its place (theseus-50p)? A card written while
    /// this lane runs, for a session a place here renders, waits until the
    /// place has shown its call (the call's tool line, sent on to this lane
    /// first), or `CARD_WAIT` is over. A card from before this process, or
    /// for a session no place here renders (a task's), has no tool line here
    /// to wait for.
    fn waits_for_its_call(&mut self, a: &Action) -> bool {
        if kind_of(a) != "card" {
            return false;
        }
        let q = body_of(a)["question"].as_str().unwrap_or("").to_string();
        let asked = match self.asked.iter().position(|x| *x == q) {
            Some(i) => self.asked.remove(i).is_some(),
            None => false,
        };
        let fresh = a.planned_at_ms >= self.started_ms;
        // Read only when it decides.
        let rendered_here = !asked
            && fresh
            && matches!(self.shared.core.kernel.action(&q), Ok(Some(question))
                if self.shared.renders_on(&question.session_id, &self.target));
        let held = self.held.as_ref().filter(|(h, _)| *h == q).map(|h| h.1);
        match card_hold(
            asked,
            fresh,
            rendered_here,
            held,
            tokio::time::Instant::now(),
        ) {
            CardHold::Until(until) => {
                self.held = Some((q, until));
                true
            }
            CardHold::GaveUp => {
                tracing::warn!(target = %self.target, question = %q, "a card went without its call's tool line: its place never showed the call");
                self.held = None;
                false
            }
            CardHold::Go => {
                self.held = None;
                false
            }
        }
    }

    /// One post: planned into writes, dispatched, written, settled. An error
    /// back means Discord is away (or the store would not take the settle)
    /// and the post waits, dispatched; a refusal settles it as failed, and the
    /// lane goes on.
    async fn deliver(&mut self, a: &Action) -> Result<(), SendErr> {
        let t0 = std::time::Instant::now();
        let plan = match self.plan(a).await {
            Ok(p) => p,
            Err(e) if e.away => return Err(e),
            Err(e) => {
                return self.settle(a, Outcome::Failed, vec![], json!({"error": e.message}));
            }
        };
        if plan.writes.is_empty() {
            return self.settle(a, Outcome::Succeeded, vec![], plan.extra);
        }
        // In flight from just before its dispatch until it settles, or its
        // delivery gives up: a clean stop waits for it, within its grace, and
        // a stopping daemon dispatches nothing new (theseus-pfv).
        let Some(_sending) = self.shared.core.outbox.sending() else {
            return Err(SendErr {
                away: true,
                unsure: false,
                gone: false,
                message: "the daemon is stopping".into(),
            });
        };
        let before = a.dispatched_at_ms;
        let dispatched = match self.shared.core.outbox.dispatch(&a.correlation_id) {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), post = %a.correlation_id, "outbox dispatch failed");
                return Err(SendErr {
                    away: true,
                    unsure: false,
                    gone: false,
                    message: format!("dispatch: {e:#}"),
                });
            }
        };
        // Sent before and maybe landed, longer ago than Discord keeps a
        // nonce: a new create says it may be a copy.
        let first = dispatched.dispatched_at_ms.unwrap_or(0);
        let maybe_landed =
            self.unsure.contains(&a.correlation_id) || before.is_some_and(|d| d < self.started_ms);
        let resend =
            maybe_landed && theseus_protocol::now_unix_ms().saturating_sub(first) > NONCE_WINDOW_MS;
        let mut messages = Vec::new();
        for mut w in plan.writes {
            if resend
                && w.buttons != Buttons::V2
                && w.message.is_none()
                && !self.msgs.contains_key(&w.key)
            {
                w.content = format!("{}\n{}", w.content, render::RESENT);
            }
            match self.write(&w).await {
                Ok(Some((c, m))) => messages.push((w.key.clone(), c, m)),
                Ok(None) => {}
                Err(e) if e.away => return Err(e),
                Err(e) if e.gone && w.message.is_some() => {
                    // A settle whose card was deleted: nothing left to edit.
                    tracing::info!(post = %a.correlation_id, key = %w.key, "the message to edit is gone");
                }
                Err(e) => {
                    return self.settle(a, Outcome::Failed, messages, json!({"error": e.message}));
                }
            }
        }
        let mut extra = plan.extra;
        extra["ms"] = json!(t0.elapsed().as_millis() as u64);
        if resend {
            extra["resent"] = json!(true);
        }
        // A card's question closed: a burst's summary has one fewer to count.
        if let (true, Some(q)) = (kind_of(a) == "settle", extra["question"].as_str()) {
            let q = q.to_string();
            self.v2_settled(&q).await;
        }
        self.settle(a, Outcome::Succeeded, messages, extra)
    }

    /// Record how a post went. When the store will not take it, the post is
    /// still open: the lane backs off and sends it again later, under the same
    /// nonce, rather than taking it up again at once.
    fn settle(
        &mut self,
        a: &Action,
        outcome: Outcome,
        messages: Vec<(String, u64, u64)>,
        mut detail: Value,
    ) -> Result<(), SendErr> {
        let first = messages.first().map(|(_, _, m)| m.to_string());
        detail["messages"] = messages
            .iter()
            .map(|(k, c, m)| json!({"key": k, "channel": c.to_string(), "id": m.to_string()}))
            .collect();
        match self
            .shared
            .core
            .outbox
            .settle(&a.correlation_id, outcome, first, detail, "discord")
        {
            Ok(_) => {
                self.unsure.remove(&a.correlation_id);
                Ok(())
            }
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), post = %a.correlation_id, "outbox settle failed");
                Err(SendErr {
                    away: true,
                    unsure: true,
                    gone: false,
                    message: format!("settle: {e:#}"),
                })
            }
        }
    }

    /// A content label's message (theseus-9ymz0): its words as the core wrote
    /// them, in the session's place, with the one Remove button.
    async fn content_label_post(&mut self, body: &Value, corr: &str) -> Result<Plan, SendErr> {
        let channel = self.place_channel().await?;
        let session = body["session_id"].as_str().unwrap_or("").to_string();
        let flag = body["flag"].as_str().unwrap_or("").to_string();
        Ok(Plan {
            writes: vec![Write {
                key: format!("content:{corr}"),
                channel,
                content: body["text"].as_str().unwrap_or("").to_string(),
                buttons: Buttons::ContentRemove(session, flag),
                reply_to: None,
                message: None,
                mentions: vec![],
                ping: self.pings(Event::Note),
            }],
            extra: json!({}),
        })
    }

    /// A post's writes, by its kind.
    #[expect(
        clippy::too_many_lines,
        reason = "shape budget: a post kind is one arm; split it"
    )]
    async fn plan(&mut self, a: &Action) -> Result<Plan, SendErr> {
        let body = body_of(a).clone();
        let corr = &a.correlation_id;
        let silent = self.silent();
        let text =
            |t: String, key: String, channel: u64, reply_to: Option<u64>, event: Event| Write {
                key,
                channel,
                content: t,
                buttons: Buttons::Keep,
                reply_to,
                message: None,
                mentions: vec![],
                ping: policy::pings(event, &silent),
            };
        let reply_to = body["reply_to"].as_str().and_then(|s| s.parse().ok());
        match kind_of(a) {
            "reply" => self.reply_in_place(&body).await,
            "card" => self.card(&body).await,
            "settle" => Ok(self.settled(&body)),
            "notice" => {
                // The operator lane has no place of its own: its notices go
                // where approvals go, as `restarted`'s do (theseus-haw2b).
                let (channel, extra) = match self.kind {
                    "operator" => {
                        let (channel, place) = self.operator_channel(&body).await?;
                        (channel, json!({"place": place}))
                    }
                    _ => (self.place_channel().await?, json!({})),
                };
                let t = body["text"].as_str().unwrap_or("").to_string();
                // A watch that is met speaks (theseus-j90w2): it pings as a
                // reminder does and offers its answers.
                let met = body["purpose"].as_str() == Some("watch.met");
                let event = if met { Event::Reminder } else { Event::Note };
                let mut w = text(t, format!("note:{corr}"), channel, reply_to, event);
                if met {
                    let answers = crate::runtime::horizon_answers(&body);
                    if !answers.is_empty() {
                        let wake = body["wake_id"].as_str().unwrap_or(corr).to_string();
                        w.buttons = Buttons::Horizon(wake, answers);
                    }
                    self.shared.tasks_future_dirty();
                }
                // A pointer (theseus-sjzm) is history, not news: no ping.
                w.ping &= !body["quiet"].as_bool().unwrap_or(false);
                Ok(Plan {
                    writes: vec![w],
                    extra,
                })
            }
            "failed" => self.failed_post(&body, corr).await,
            // A content label (theseus-9ymz0): the one message the locked DM flow gains.
            "content_label" => self.content_label_post(&body, corr).await,
            "report" => self.report_post(&body, corr).await,
            "offer" => Ok(self.offer_post(&body)),
            // A hands group's one line (step 40 part 2): under its group's
            // key, so each new state of it edits the one message.
            "hands" => {
                let channel = self.place_channel().await?;
                let t = body["text"].as_str().unwrap_or("").to_string();
                let key = format!("hands:{}", body["group"].as_str().unwrap_or(corr));
                Ok(Plan {
                    writes: vec![text(t, key, channel, None, Event::Hands)],
                    extra: json!({}),
                })
            }
            // A glide's post (38b): another session's words, once, here.
            "glide" => {
                let ping = self.pings(Event::Glide);
                Ok(glide(&body, corr, self.place_channel().await?, ping))
            }
            // A job's refusal notice, from a build before the trace was
            // retired (theseus-zmgb): nothing to post.
            "refusal" => Ok(Plan::nothing("a job's refusal notice, retired")),
            "restarted" => {
                let tables: Vec<String> = body["tables"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|t| t.as_str().map(str::to_string))
                    .collect();
                let t = render::restarted(body["at_unix_ms"].as_u64().unwrap_or(0), &tables);
                let (channel, place) = self.operator_channel(&body).await?;
                Ok(Plan {
                    writes: vec![text(
                        t,
                        format!("note:{corr}"),
                        channel,
                        None,
                        Event::Restarted,
                    )],
                    extra: json!({"place": place}),
                })
            }
            // Free space under the state dir crossed a line, or came back
            // (theseus-f337): the owner hears it.
            "disk" => self.disk_post(&body, corr).await,
            // An MCP server's tools, or a prompt's definition, changed (M7
            // 36b, 36c): the owner hears it.
            "mcp_changed" | "mcp_prompt_changed" => {
                let t = mcp_note(&body);
                let (channel, place) = self.operator_channel(&body).await?;
                Ok(Plan {
                    writes: vec![text(t, format!("note:{corr}"), channel, None, Event::Mcp)],
                    extra: json!({"place": place}),
                })
            }
            // Jev's notices (step 24's notices), the day ceiling's, the week's
            // self digest (theseus-pw1q.4), and a retraction's card of the tasks
            // to re-run (theseus-2ced), and a self branch the keel stopped
            // (theseus-pw1q.8), a self-join's card or notice (theseus-pw1q.9), a narrower policy for a
            // hand role and the red after its apply (theseus-pw1q.15), and Jev's privacy warning
            // (theseus-gasov), and a home past its size (theseus-jg1eg): the owner's DM alone.
            "jev_notice" | "jev_paused" | "spend_ceiling" | "self_digest" | "retraction"
            | "self_keel" | "seal_card" | "self_join" | "self_narrow" | "self_narrow_red"
            | "initiative" | "standing_card" | "digest" | "privacy_warning" | "onto_card"
            | "home_size" | "link_card" | "persona_soul" | "card_question" => {
                self.jev_post(a, &body).await
            }
            // The self-build halt's note, to the owner's DM alone, pinging (theseus-6s7ll).
            "self_halt" => self.jev_post(a, &body).await,
            "jev_labeled" => Ok(self.jev_labeled(&body)),
            // An open question pinged the owner's DM (theseus-j7cje).
            "needs_you" => self.needs_you_post(&body).await,
            // A reminder fired (theseus-8ux6x): its note as it is, in the
            // place the core chose, with Done and Snooze.
            "reminder" => self.reminder_post(&body, corr).await,
            other => Err(SendErr::refused(format!(
                "a post of kind {other:?} is not one this binding knows"
            ))),
        }
    }

    async fn reply_in_place(&mut self, body: &Value) -> Result<Plan, SendErr> {
        let channel = self.place_channel().await?;
        Ok(self.reply(body, channel))
    }

    /// A reply's post answers its anchor, whether its own first part used it
    /// or the stream took it: a live key of the turn written after the post
    /// must not reply again (theseus-nxnqc). A newer anchor, the owner's next
    /// message, stays: compared by value.
    fn answered(&mut self, to: Option<u64>) {
        if to.is_some() && self.anchor == to {
            self.anchor = None;
        }
    }

    /// A turn's reply: its loops' final text under the stream's keys, which
    /// edits what the stream posted and creates what it did not.
    fn reply(&mut self, body: &Value, channel: u64) -> Plan {
        let turn_id = body["turn_id"].as_str().unwrap_or("").to_string();
        let texts = self.shared.core.outbox.reply_texts(body);
        let result: Option<TurnSubmitResult> = serde_json::from_value(body["result"].clone()).ok();
        let mut parts = render::reply_parts(&turn_id, &texts, result.as_ref());
        // A wake's turn says which wake woke it (DD8), and a turn a task's
        // report started says which report did (W1).
        let wakes: Vec<String> = ["wakes", "reports"]
            .iter()
            .flat_map(|k| body[*k].as_array().into_iter().flatten())
            .filter_map(|w| w["text"].as_str().map(str::to_string))
            .collect();
        render::wake_header(&mut parts, &wakes);
        if let Some((tag, persona)) = self.thread_tag(body, &turn_id) {
            if let Some(p) = persona {
                persona::sign_parts(&mut parts, &p);
            }
            render::thread_tag(&mut parts, &tag);
        }
        // Replying to the turn's message once: only when the stream posted
        // nothing of it.
        let prefix = format!("{turn_id}:");
        let streamed = self.msgs.keys().any(|k| k.starts_with(&prefix));
        let to: Option<u64> = body["reply_to"].as_str().and_then(|s| s.parse().ok());
        let mut anchor = (!streamed).then_some(to).flatten();
        self.answered(to);
        // Its first part is the answer when it answers the owner's own
        // message and the stream did not write that part already: a live
        // create of it takes `owed`, so `owed` says so alone. The stream may
        // have written another key of the turn, a loop's thinking or tool
        // line, before the post came (theseus-l1y1).
        let owed = to.is_some() && self.owed == to;
        if owed {
            self.owed = None;
        }
        let event = policy::of_reply(body);
        let mut answers = event == Event::Answer && owed;
        let mut writes: Vec<Write> = parts
            .into_iter()
            .map(|(key, content)| {
                self.seal(&key);
                // A footer alone (a turn that only asked) is no answer.
                let part = if std::mem::take(&mut answers) && policy::is_text_part(&key) {
                    Event::Answer
                } else if event == Event::Woken {
                    Event::Woken
                } else {
                    Event::ReplyPart
                };
                let ping = self.pings(part);
                Write {
                    key,
                    channel,
                    content,
                    buttons: Buttons::Keep,
                    reply_to: anchor.take(),
                    message: None,
                    mentions: vec![],
                    ping,
                }
            })
            .collect();
        // The sources the reply cites open from a row under its last message
        // (theseus-ejdk).
        let mut srcs = Vec::new();
        if let Some(r) = &result {
            let (_, order) = render::cite::marked(&texts, &r.cites);
            srcs = render::cite::sources(&order, |d| self.source_link(d));
        }
        // A reply to a message that described work for later offers to make it a
        // task, in a private place (theseus-bh3t.5).
        let offer = body["offer"]["brief"]
            .as_str()
            .filter(|_| self.shared.is_private(&self.target));
        if let Some(brief) = offer {
            self.shared
                .tasks
                .offers
                .keep(&self.shared.core.store, &turn_id, brief);
        }
        if let (Some(last), false) = (writes.last_mut(), srcs.is_empty() && offer.is_none()) {
            let session = result.as_ref().map(|r| r.session_id.clone());
            last.buttons = Buttons::Sources(
                session.unwrap_or_default(),
                srcs,
                offer.map(|_| turn_id.clone()),
            );
        }
        // Where a late offer's button would join (theseus-ypnxt.5): a private
        // place's reply only, as `offer_post` acts there alone, so a shared
        // place's replies never push one out of the short list.
        let private = self.shared.is_private(&self.target);
        if let (Some(last), true) = (writes.last(), offer.is_none() && private) {
            let (session, sources) = match &last.buttons {
                Buttons::Sources(s, srcs, _) => (s.clone(), srcs.clone()),
                _ => (
                    result
                        .as_ref()
                        .map(|r| r.session_id.clone())
                        .unwrap_or_default(),
                    vec![],
                ),
            };
            self.shared
                .tasks
                .offers
                .replied(crate::runtime::start_task::Replied {
                    turn: turn_id.clone(),
                    key: last.key.clone(),
                    session,
                    sources,
                });
        }
        Plan {
            writes,
            extra: json!({"turn_id": turn_id}),
        }
    }

    /// A cited Discord message's link, when this binding can name its place:
    /// a bound channel's guild, or this lane's own DM.
    fn source_link(&self, d: &theseus_protocol::cite::DiscordSource) -> Option<String> {
        if let Some(id) = d.target.strip_prefix("discord:channel:") {
            let guild = self.shared.place_bits.get(&format!("channel:{id}")).guild?;
            return Some(render::cite::jump_link(
                Some(&guild),
                id.parse().ok()?,
                &d.message_id,
            ));
        }
        let own = d.target == self.target && self.target.starts_with("discord:dm:");
        own.then_some(self.channel)
            .flatten()
            .map(|c| render::cite::jump_link(None, c, &d.message_id))
    }

    /// A confirm card, where its route says: here, in a trusted DM with a
    /// note here, or only a note here. A question that closed while the card
    /// waited (Discord away, say) is written once, already settled, without
    /// buttons or a mention (theseus-l1y1), and its settle then has nothing
    /// left to edit.
    async fn card(&mut self, body: &Value) -> Result<Plan, SendErr> {
        let q = body["question"].as_str().unwrap_or("").to_string();
        let core = self.shared.core.clone();
        let question = match core.kernel.action(&q) {
            Ok(Some(a)) => a,
            Ok(None) => return Ok(Plan::nothing("its question is not in the store")),
            Err(e) => return Err(SendErr::refused(format!("{e:#}"))),
        };
        let req = match core.question_request(&question, body["node"].as_str()) {
            Ok(Some(r)) => r,
            Ok(None) => return Ok(Plan::nothing("its question could not be read")),
            Err(e) => return Err(SendErr::refused(format!("{e:#}"))),
        };
        let channel = self.place_channel().await?;
        let route = self.route().await?;
        let elsewhere = theseus_core::approval::elsewhere(core.cfg.web.enabled);
        let mut card = render::card(&req, &route, elsewhere);
        let mut note = render::card_note(&route, &card.line, elsewhere);
        let key = key_of_card(&q);
        let note_key = format!("approval:{q}");
        let mut buttons = ask::card_buttons(&req, &q);
        // The card as one V2 message (theseus-2j2ka) when the bot may send one.
        let v2 = self.shared.v2_cards() && !req.options.is_empty();
        let closed = Closed::of(&question).map(|c| self.waiting_settle(&q).unwrap_or(c));
        if let Some(c) = &closed {
            // Written as its settle would leave it: no buttons, no mention.
            card.content = render::settled(c, &card.line, card.budget);
            let dm = match &route {
                Route::Dm { dm, .. } => Some(dm.as_str()),
                _ => None,
            };
            note = note.map(|_| render::settled_note(&card.content, dm));
            buttons = if v2 { Buttons::V2 } else { Buttons::Clear };
        }
        let event = if closed.is_some() {
            Event::CardClosed
        } else {
            Event::Card
        };
        let mut writes = Vec::new();
        // A card in a guild channel names the people who can answer it, and
        // Discord notifies them and nobody else (theseus-9j9): a card is the
        // one message that waits on a human. A card in a DM needs no mention,
        // and the note beside a card sent elsewhere names nobody.
        let mut named = Vec::new();
        let (how, dm) = match &route {
            Route::Here => {
                if self.kind == "channel" && closed.is_none() {
                    let owners = core.runner.place_rule.owners(&core.cfg);
                    named = answerers(&self.shared.place_users(channel), &owners);
                }
                writes.push(Write {
                    key,
                    channel,
                    content: mentioning(&named, card.content),
                    buttons,
                    reply_to: None,
                    message: None,
                    mentions: named.clone(),
                    ping: self.pings(event),
                });
                ("here", None)
            }
            Route::Dm { user, dm, .. } => {
                let dm_channel = self.shared.dm_channel(*user).await?;
                writes.push(Write {
                    key,
                    channel: dm_channel,
                    content: card.content,
                    buttons,
                    reply_to: None,
                    message: None,
                    mentions: vec![],
                    ping: self.pings(event),
                });
                ("dm", Some(dm.clone()))
            }
            Route::Elsewhere { .. } => ("elsewhere", None),
        };
        // The card itself, as V2 when it goes as one: its words and answers.
        for w in writes.iter_mut().filter(|w| v2 && w.key == key_of_card(&q)) {
            let text = std::mem::take(&mut w.content);
            (w.content, w.buttons) = card_message(&q, text, &req.options, closed.is_some());
        }
        if let Some(n) = note {
            let ping = self.pings(Event::CardNote);
            writes.push(Write {
                ping,
                ..lone(note_key, channel, n, false)
            });
        }
        let named: Vec<String> = named.iter().map(u64::to_string).collect();
        let mut extra = json!({"question": q, "route": how, "dm": dm, "line": card.line, "budget": card.budget, "mentions": named, "v2": v2});
        if let Some(c) = closed {
            extra["settled"] = json!(c.how);
        }
        Ok(Plan { writes, extra })
    }

    /// How the settle waiting behind a card says its question closed: the
    /// close's own words (who answered, and from where), which the question's
    /// action alone does not keep. Read only for a card whose question closed.
    fn waiting_settle(&self, question: &str) -> Option<Closed> {
        self.shared
            .core
            .outbox
            .open_for(&self.target)
            .iter()
            .find(|a| kind_of(a) == "settle" && body_of(a)["question"] == question)
            .and_then(|a| serde_json::from_value(body_of(a)["closed"].clone()).ok())
    }

    /// How a card's question closed: each message its card posted, edited,
    /// the card's buttons gone.
    fn settled(&mut self, body: &Value) -> Plan {
        let closed: Closed = serde_json::from_value(body["closed"].clone()).unwrap_or_default();
        let card_id = body["card"].as_str().unwrap_or("");
        let card = match self.shared.core.kernel.outbox_action(card_id) {
            Ok(Some(c)) if c.state == ActionState::Succeeded => c,
            _ => return Plan::nothing("its card was never posted"),
        };
        let d = card.detail.unwrap_or_default();
        // Its question had closed when it was written: it was written settled.
        if !d["settled"].is_null() {
            return Plan::nothing("its card was written settled");
        }
        let line = d["line"].as_str().unwrap_or("");
        let content = render::settled(&closed, line, d["budget"].as_bool().unwrap_or(false));
        let dm = (d["route"] == "dm").then(|| d["dm"].as_str().unwrap_or("the DM"));
        let mut writes = Vec::new();
        for m in d["messages"].as_array().into_iter().flatten() {
            let key = m["key"].as_str().unwrap_or("").to_string();
            let (Some(channel), Some(message)) = (
                m["channel"].as_str().and_then(|s| s.parse().ok()),
                m["id"].as_str().and_then(|s| s.parse().ok()),
            ) else {
                continue;
            };
            let (content, buttons) = if key.starts_with("confirm:") && d["v2"] == true {
                // A V2 card closes as a V2 message: its box, its line, no buttons.
                let c = render::card_v2::closed(&content);
                (serde_json::to_string(&c).unwrap_or_default(), Buttons::V2)
            } else if key.starts_with("confirm:") && body["again"] == true {
                // An expired card offers one more try (theseus-27p6w).
                (
                    content.clone(),
                    Buttons::Again(d["question"].as_str().unwrap_or("").to_string()),
                )
            } else if key.starts_with("confirm:") {
                (content.clone(), Buttons::Clear)
            } else {
                (render::settled_note(&content, dm), Buttons::Keep)
            };
            writes.push(Write {
                key,
                channel,
                content,
                buttons,
                reply_to: None,
                message: Some(message),
                mentions: vec![],
                ping: self.pings(Event::CardClosed),
            });
        }
        if writes.is_empty() {
            return Plan::nothing("its card posted no message");
        }
        Plan {
            writes,
            extra: json!({"card": card_id, "closed": closed.how, "question": d["question"]}),
        }
    }

    /// Where a card goes (the place rule, theseus-zmgb): here when the place
    /// is private (a DM with the owner, or a channel bound `private = true`),
    /// where the owner's answer counts; else the owner's DM, with a note
    /// here; else the note alone.
    async fn route(&mut self) -> Result<Route, SendErr> {
        let core = &self.shared.core;
        if core.runner.place_rule.class(&core.cfg, Some(&self.target)) == PlaceClass::Private {
            return Ok(Route::Here);
        }
        let why = match self.kind {
            "dm" => "this DM is not with an owner, and only an owner's answer counts",
            _ => "this channel is shared, and an answer counts only from a private place",
        }
        .to_string();
        self.shared.open_dm_channels().await?;
        Ok(match self.shared.approval_dm(self.last_author) {
            Some((user, dm)) => Route::Dm {
                user,
                dm,
                place: self.label.clone(),
                why,
            },
            None => Route::Elsewhere { why },
        })
    }

    /// Whether a write of `e` pings: unless its place's `silent` list names
    /// its category (theseus-l1y1).
    fn pings(&self, e: Event) -> bool {
        policy::pings(e, &self.silent())
    }

    /// How long this place's window holds it to one ping: its own, else
    /// `[discord] ping_window_secs`; zero, the default, is none.
    fn window(&self) -> Duration {
        let own = self.shared.pings.of(&self.target).window_secs;
        Duration::from_secs(own.unwrap_or(self.shared.core.cfg.discord.ping_window_secs))
    }

    /// The categories that post silent here: the place's own list, else
    /// `[discord] silent`.
    fn silent(&self) -> Vec<theseus_core::config::discord::Category> {
        let own = self.shared.pings.of(&self.target).silent;
        own.unwrap_or_else(|| self.shared.core.cfg.discord.silent.clone())
    }

    /// Whether the place's latest message came from an owner, whose answer
    /// is the `answer` (theseus-l1y1): the owners the place rule names.
    fn by_an_owner(&self) -> bool {
        let core = &self.shared.core;
        self.last_author.is_some_and(|a| {
            core.runner
                .place_rule
                .owners(&core.cfg)
                .contains(&format!("discord:{a}"))
        })
    }

    /// This place's channel; a DM's opens on first use.
    async fn place_channel(&mut self) -> Result<u64, SendErr> {
        if let Some(c) = self.channel {
            return Ok(c);
        }
        match self.dm_user {
            Some(u) => {
                let c = self.shared.dm_channel(u).await?;
                self.channel = Some(c);
                Ok(c)
            }
            None => Err(SendErr::refused(format!(
                "{} is no place with a channel",
                self.target
            ))),
        }
    }

    /// A failed turn's post.
    async fn failed_post(&mut self, body: &Value, corr: &str) -> Result<Plan, SendErr> {
        let channel = self.place_channel().await?;
        let key = match body["turn_id"].as_str() {
            Some(t) => format!("{t}:failed"),
            None => format!("failed:{corr}"),
        };
        let content = render::failed(
            body["class"].as_str().unwrap_or("error"),
            body["error"].as_str().unwrap_or(""),
            body["then"].as_str(),
            body["turns"].as_u64().unwrap_or(1),
        );
        Ok(Plan {
            writes: vec![lone(key, channel, content, self.pings(Event::TurnFailed))],
            extra: json!({}),
        })
    }

    /// A reminder's post: its note as it is, with its four answers. It always
    /// pings (`Event::Reminder` is in no category), and posts once under its
    /// wake's key, so a retry never makes a second.
    async fn reminder_post(&mut self, body: &Value, corr: &str) -> Result<Plan, SendErr> {
        let channel = self.place_channel().await?;
        let wake = body["wake_id"].as_str().unwrap_or(corr).to_string();
        let ping = self.pings(Event::Reminder);
        // A routine's failure speaks for the routine, with its own answers
        // (theseus-j90w2): its line as the core wrote it, its buttons through
        // `work.answer`.
        let routine = body["purpose"]
            .as_str()
            .is_some_and(|p| theseus_protocol::notices::routine_urgency(p).is_some());
        self.shared.tasks_future_dirty();
        Ok(Plan {
            writes: vec![Write {
                key: format!("reminder:{corr}"),
                channel,
                content: if routine {
                    body["text"].as_str().unwrap_or("").to_string()
                } else {
                    crate::runtime::remind_text(body)
                },
                buttons: if routine {
                    Buttons::Horizon(wake.clone(), crate::runtime::horizon_answers(body))
                } else {
                    Buttons::Reminder(wake.clone())
                },
                reply_to: None,
                message: None,
                mentions: vec![],
                ping,
            }],
            extra: json!({"wake_id": wake}),
        })
    }

    /// A task's report (DD7): one message, in the place the task reports to,
    /// under a key of its own, so it posts once.
    async fn report_post(&mut self, body: &Value, corr: &str) -> Result<Plan, SendErr> {
        let channel = self.place_channel().await?;
        // A task that wrote nothing of its own carries its words in the post.
        let said = body["node"]
            .as_str()
            .and_then(|n| self.shared.core.outbox.said(n))
            .or_else(|| body["text"].as_str().map(str::to_string));
        // A routine's run speaks as the routine (theseus-j90w2).
        let routine = self.shared.routine_report(body, said.as_deref());
        if routine.is_some() {
            self.shared.tasks_future_dirty();
        }
        let guests = body["task"]
            .as_str()
            .map_or(0, |s| self.shared.comments_on_session(s));
        let place = self.shared.place_of_task(
            body["task"].as_str().unwrap_or(""),
            body["execution_id"].as_str().unwrap_or(""),
        );
        let content =
            routine.unwrap_or_else(|| render::report(body, said.as_deref(), guests, place));
        let key = format!("report:{}", body["task"].as_str().unwrap_or(corr));
        let ping = self.pings(policy::of_report(body));
        Ok(Plan {
            writes: vec![lone(key, channel, content, ping)],
            extra: json!({"task": body["task"]}),
        })
    }

    /// A disk crossing's post (theseus-f337), where approvals go.
    async fn disk_post(&mut self, body: &Value, corr: &str) -> Result<Plan, SendErr> {
        let t = crate::diskwords::disk_note(body);
        let ping = self.pings(policy::of_disk(body));
        let (channel, place) = self.operator_channel(body).await?;
        Ok(Plan {
            writes: vec![Write {
                key: format!("note:{corr}"),
                channel,
                content: t,
                buttons: Buttons::Keep,
                reply_to: None,
                message: None,
                mentions: vec![],
                ping,
            }],
            extra: json!({"place": place}),
        })
    }

    /// A Jev notice, or the notices' pause, in the owner's DM: a notice
    /// under its judgment's key, with its right / wrong / noise.
    async fn jev_post(&mut self, a: &Action, body: &Value) -> Result<Plan, SendErr> {
        let (channel, place) = self.owner_dm().await?;
        let (content, key, buttons) = match kind_of(a) {
            "jev_notice" => {
                let judgment = body["judgment"].as_str().unwrap_or(&a.correlation_id);
                let t = crate::runtime::jev_notice_text(body);
                (
                    t,
                    format!("jev:{judgment}"),
                    Buttons::JevLabel(judgment.to_string()),
                )
            }
            // A proposed seal (theseus-f2g0): Seal this thread, Not needed.
            "seal_card" => (
                body["text"].as_str().unwrap_or("").to_string(),
                format!("seal:{}", a.correlation_id),
                Buttons::Seal(crate::runtime::seal_slug(body)),
            ),
            // Jev's offer to link two threads (theseus-98bz.12): Link these threads, Not needed.
            "link_card" => {
                let (here, other) = crate::runtime::seal::link::pair_of(body);
                (
                    body["text"].as_str().unwrap_or("").to_string(),
                    format!("link:{}", a.correlation_id),
                    Buttons::Link(here, other),
                )
            }
            // What Theseus said first, in its persona's voice (theseus-slvl.9):
            // its key names the message, so a 👍 or 👎 on it finds it.
            "initiative" => (
                body["text"].as_str().unwrap_or("").to_string(),
                format!("ini:{}", body["id"].as_str().unwrap_or(&a.correlation_id)),
                Buttons::Keep,
            ),
            // Where Jev filed a session, or asks to (theseus-xzxqv): a mint has its undo.
            "onto_card" => (
                body["text"].as_str().unwrap_or("").to_string(),
                format!("onto:{}", a.correlation_id),
                match crate::runtime::onto_write(body) {
                    Some(write) => Buttons::OntoUndo(write),
                    None => Buttons::Keep,
                },
            ),
            // A persona added without a soul asks for it (theseus-utfnl).
            "persona_soul" => (
                body["text"].as_str().unwrap_or("").to_string(),
                format!("psoul:{}", a.correlation_id),
                Buttons::PersonaSoul(crate::runtime::persona_soul::key_of(body)),
            ),
            // The self notes: Drop for each started item, and Halt (theseus-o1h3f.7).
            k @ ("self_digest" | "self_join") => (
                body["text"].as_str().unwrap_or("").to_string(),
                format!("self:{}", a.correlation_id),
                Buttons::SelfNote(crate::runtime::self_note::items_of(k, body)),
            ),
            // The self-build halt: pings, and carries Resume (theseus-6s7ll).
            "self_halt" => (
                body["text"].as_str().unwrap_or("").to_string(),
                format!("self:{}", a.correlation_id),
                Buttons::SelfResume,
            ),
            // A live owner-card question (theseus-6f8t0).
            "card_question" => {
                let (ask, id) = crate::runtime::card_question::card_of(body);
                (
                    body["text"].as_str().unwrap_or("").to_string(),
                    format!("cardq:{}", a.correlation_id),
                    Buttons::CardQuestion(ask, id),
                )
            }
            // What `directive.v1` kept, or asks about (theseus-slvl.5).
            "standing_card" => {
                let (mode, id, everywhere) = crate::runtime::capture_card(body);
                (
                    body["text"].as_str().unwrap_or("").to_string(),
                    format!("standing:{}", a.correlation_id),
                    Buttons::Standing(mode, id, everywhere),
                )
            }
            _ => {
                let t = body["text"].as_str().unwrap_or("").to_string();
                (t, format!("note:{}", a.correlation_id), Buttons::Keep)
            }
        };
        Ok(Plan {
            writes: vec![Write {
                key,
                channel,
                content: content.clone(),
                buttons,
                reply_to: None,
                message: None,
                mentions: vec![],
                ping: self.pings(Event::Jev),
            }],
            extra: json!({"place": place, "text": content}),
        })
    }

    /// Where a Jev notice goes: the owner's DM, the one approvals go to,
    /// and nowhere else. It never falls back to the session's place, which
    /// may be shared: with no such DM it is refused, saying why.
    async fn owner_dm(&mut self) -> Result<(u64, String), SendErr> {
        self.shared.open_dm_channels().await?;
        match self.shared.approval_dm(None) {
            Some((user, dm)) => Ok((self.shared.dm_channel(user).await?, dm)),
            None => Err(SendErr::refused(
                "no DM with the owner takes approvals, and a Jev notice goes nowhere else",
            )),
        }
    }

    /// A noticed judgment was labeled: its notice, edited to say so, its
    /// buttons gone.
    fn jev_labeled(&mut self, body: &Value) -> Plan {
        let notice = body["notice"].as_str().unwrap_or("");
        let posted = match self.shared.core.kernel.outbox_action(notice) {
            Ok(Some(c)) if c.state == ActionState::Succeeded => c,
            _ => return Plan::nothing("its notice was never posted"),
        };
        let d = posted.detail.unwrap_or_default();
        let content = crate::runtime::jev_labeled_text(d["text"].as_str().unwrap_or(""), body);
        let writes: Vec<Write> = d["messages"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| {
                Some(Write {
                    key: m["key"].as_str()?.to_string(),
                    channel: m["channel"].as_str()?.parse().ok()?,
                    content: content.clone(),
                    buttons: Buttons::Clear,
                    reply_to: None,
                    message: Some(m["id"].as_str()?.parse().ok()?),
                    mentions: vec![],
                    ping: self.pings(Event::Jev),
                })
            })
            .collect();
        if writes.is_empty() {
            return Plan::nothing("its notice posted no message");
        }
        Plan {
            writes,
            extra: json!({"notice": notice}),
        }
    }

    /// Where an operator's notice goes: the DM approvals go to, else the
    /// place of the session it concerns, when this daemon binds it
    /// (theseus-c3e). A place the bindings file does not name, a session's
    /// in a store copied from another daemon's or a place since removed, is
    /// never posted to and no DM is opened for it: two daemons may share a
    /// bot when their bindings name disjoint places (theseus-e89). The
    /// notice is refused with the reason, as one with no place is. Returns
    /// the channel and its label.
    async fn operator_channel(&mut self, body: &Value) -> Result<(u64, String), SendErr> {
        self.shared.open_dm_channels().await?;
        if let Some((user, dm)) = self.shared.approval_dm(None) {
            return Ok((self.shared.dm_channel(user).await?, dm));
        }
        let fallback = body["fallback"].as_str().unwrap_or("");
        let channel: Option<u64> = fallback
            .strip_prefix("discord:channel:")
            .and_then(|s| s.parse().ok());
        let dm: Option<u64> = fallback
            .strip_prefix("discord:dm:")
            .and_then(|s| s.parse().ok());
        if (channel.is_some() || dm.is_some()) && !self.shared.binds(fallback) {
            return Err(SendErr::refused(format!(
                "no DM takes approvals, and the notice's place, {fallback}, is not one this \
                 daemon's bindings file names"
            )));
        }
        if let Some(id) = channel {
            return Ok((id, fallback.to_string()));
        }
        if let Some(u) = dm {
            return Ok((self.shared.dm_channel(u).await?, fallback.to_string()));
        }
        Err(SendErr::refused(
            "no DM takes approvals, and the notice names no place to fall back to",
        ))
    }

    // ------------------------------------------------------------ writing

    /// Create or edit one message. None when there is nothing to send (empty
    /// text, or what Discord already has).
    async fn write(&mut self, w: &Write) -> Result<Option<(u64, u64)>, SendErr> {
        if w.content.trim().is_empty() {
            return Ok(None);
        }
        // A persona's text is signed (theseus-sjzm).
        let signed = self.sign(w).await;
        let w = signed.as_ref().map_or(w, |s| &s.write);
        let known = w
            .message
            .map(|m| (w.channel, m))
            .or_else(|| self.msgs.get(&w.key).copied());
        if let Some((c, m)) = known {
            if self.sent.get(&w.key) == Some(&w.content)
                && matches!(w.buttons, Buttons::Keep | Buttons::V2)
            {
                return Ok(Some((c, m)));
            }
            let edited = match self.edit_hooked(m, &w.content).await {
                Some(r) => r,
                None => self.edit(c, m, &w.content, &w.buttons).await,
            };
            match edited {
                Ok(()) => {
                    self.touch(&w.key);
                    self.sent.insert(w.key.clone(), w.content.clone());
                    self.msgs.insert(w.key.clone(), (c, m));
                    return Ok(Some((c, m)));
                }
                // A message someone deleted: a post's own part is posted again.
                Err(e) if e.gone && w.message.is_none() => {
                    self.msgs.remove(&w.key);
                }
                Err(e) => return Err(e),
            }
        }
        // At most one ping per place per window, when it has one
        // (theseus-l1y1): taken only once the create lands, so a create
        // Discord never took leaves it open.
        let (now, window) = (tokio::time::Instant::now(), self.window());
        // A needs-you card pings at most once per `CARD_EVERY` a place, and not
        // in the place's quiet hour (theseus-2j2ka, `courier/v2.rs`).
        let card = (w.buttons == Buttons::V2)
            .then(|| v2::question_of(&w.key))
            .flatten();
        let paced = match card {
            Some(_) if self.shared.tasks.quiet_until(&self.target).is_some() => false,
            Some(q) => {
                let core = self.shared.core.clone();
                self.v2.card_may_ping(q, now, |q| {
                    core.kernel
                        .action(q)
                        .ok()
                        .flatten()
                        .is_some_and(|a| theseus_core::outbox::Closed::of(&a).is_none())
                })
            }
            None => true,
        };
        // A question that waits on the owner is no chat message: the window
        // never holds it silent (a reply that used the window first would
        // otherwise leave the question unheard, theseus-31pgz). A ping it
        // makes still counts in the window, for the chat that follows.
        let ping = w.ping && paced && (asks(w) || self.shared.pings.open(w.channel, now, window));
        let (m, landed) = match signed
            .as_ref()
            .and_then(|s| Some((s.hook.as_ref()?, &s.name)))
        {
            Some((hook, name)) => self.create_hooked(hook, name, w, ping).await?,
            None => {
                self.create(
                    w.channel,
                    &w.key,
                    &w.content,
                    &w.buttons,
                    w.reply_to,
                    (&w.mentions, w.ping, ping),
                )
                .await?
            }
        };
        if ping {
            self.shared.pings.mark(w.channel, now, window);
            if let Some(q) = card {
                self.v2.card_pinged(q, now);
            }
        }
        self.touch(&w.key);
        self.msgs.insert(w.key.clone(), (w.channel, m));
        self.shared.noted(m, w.channel, &w.key);
        self.shared.replies.asked().landed(m, &w.key);
        if landed != w.content {
            // The nonce returned an earlier send of this message (the stream's,
            // or one before a restart): bring it to this state.
            self.edit(w.channel, m, &w.content, &w.buttons).await?;
        }
        self.sent.insert(w.key.clone(), w.content.clone());
        Ok(Some((w.channel, m)))
    }

    /// A new message, with its key's nonce and `enforce_nonce`: a second send
    /// of it returns the first message. Returns its id and the content Discord
    /// has for it, which is an earlier send's when the nonce matched one.
    /// Discord notifies exactly `mentions` (theseus-9j9): nobody for any
    /// message but a card, whatever its text says; and only when it `ping`s
    /// (theseus-l1y1): else it carries `SUPPRESS_NOTIFICATIONS`, and no
    /// device notifies. `wants` is the table's word, before the window: its
    /// row says when the window held a ping back.
    async fn create(
        &mut self,
        channel: u64,
        key: &str,
        content: &str,
        buttons: &Buttons,
        reply_to: Option<u64>,
        (mentions, wants, ping): (&[u64], bool, bool),
    ) -> Result<(u64, String), SendErr> {
        if matches!(buttons, Buttons::V2) {
            return self
                .create_v2((channel, key, content), mentions, (wants, ping))
                .await;
        }
        let mut allowed = json!({"parse": [], "replied_user": false});
        if !mentions.is_empty() {
            allowed["users"] = json!(mentions.iter().map(u64::to_string).collect::<Vec<_>>());
        }
        let mut body = json!({
            "content": content,
            "nonce": nonce(key),
            "enforce_nonce": true,
            "allowed_mentions": allowed,
        });
        if !ping {
            body["flags"] = json!(policy::SUPPRESS_NOTIFICATIONS);
        }
        let comps = match buttons {
            Buttons::Confirm(corr) => confirm_buttons(corr),
            Buttons::Ask(corr, choices) => crate::runtime::ask_buttons(corr, choices),
            Buttons::Card(corr, choices) => crate::runtime::card_buttons(corr, choices),
            Buttons::ConfirmGrant(corr, o) => crate::runtime::grant::grant_buttons(corr, o),
            Buttons::Accept(corr) => crate::runtime::accept_buttons(corr),
            Buttons::ShouldHaveAsked(options) => asked_menu(options),
            Buttons::JevLabel(judgment) => crate::runtime::jev_buttons(judgment),
            Buttons::Reminder(wake) => crate::runtime::remind_buttons(wake),
            Buttons::Horizon(wake, answers) => crate::runtime::horizon_buttons(wake, answers),
            Buttons::Seal(slug) => crate::runtime::seal_buttons(slug),
            Buttons::Link(h, o) => crate::runtime::seal::link::buttons(h, o),
            Buttons::OntoUndo(write) => crate::runtime::onto_buttons(write),
            Buttons::PersonaSoul(key) => crate::runtime::persona_soul::buttons(key),
            Buttons::SelfNote(items) => crate::runtime::self_note::buttons(items),
            Buttons::SelfResume => crate::runtime::self_note::resume_buttons(),
            Buttons::ContentRemove(s, f) => crate::runtime::content::buttons(s, f),
            Buttons::Again(q) => crate::runtime::again_buttons(q),
            Buttons::Standing(mode, id, wide) => crate::runtime::capture_buttons(mode, id, *wide),
            Buttons::CardQuestion(ask, id) => crate::runtime::card_question::buttons(ask, id),
            Buttons::Sources(session, srcs, offer) => {
                crate::runtime::source_buttons(session, srcs, offer.as_deref())
            }
            Buttons::Clear | Buttons::Keep | Buttons::V2 => vec![],
        };
        if !comps.is_empty() {
            body["components"] = serde_json::to_value(&comps).unwrap_or_default();
        }
        if let Some(a) = reply_to {
            body["message_reference"] =
                json!({"message_id": a.to_string(), "fail_if_not_exists": false});
        }
        let req = Request::builder(&HttpRoute::CreateMessage {
            channel_id: channel,
        })
        .json(&body)
        .build()
        .map_err(|e| SendErr::of(&e))?;
        let resp = self
            .shared
            .http
            .request::<Message>(req)
            .await
            .map_err(|e| SendErr::of(&e))?;
        let m = resp.model().await.map_err(|e| SendErr {
            away: true,
            unsure: true,
            gone: false,
            message: format!("reading the sent message: {e}"),
        })?;
        self.shared.board.update(|s| s.messages_out += 1);
        // Whom Discord says the message mentions: what it notified. The row
        // is written off the lane's path, so the next write never waits on
        // its sync (theseus-ck0n).
        let mentioned: Vec<String> = m.mentions.iter().map(|u| u.id.to_string()).collect();
        self.shared.core.binding_ledger_soon(
            LedgerKind::DiscordMessageOut,
            None,
            json!({"place": self.label, "message_id": m.id.to_string(), "part": key, "ping": ping, "held": wants && !ping,
                   "chars": content.chars().count(), "buttons": matches!(buttons, Buttons::Confirm(_) | Buttons::Accept(_)),
                   "menu": matches!(buttons, Buttons::ShouldHaveAsked(_)), "mentions": mentioned}),
        );
        Ok((m.id.get(), m.content))
    }

    async fn edit(
        &self,
        channel: u64,
        message: u64,
        content: &str,
        buttons: &Buttons,
    ) -> Result<(), SendErr> {
        if matches!(buttons, Buttons::V2) {
            return self.edit_v2(channel, message, content).await;
        }
        let none = AllowedMentions::default();
        let comps = match buttons {
            Buttons::Confirm(corr) => Some(confirm_buttons(corr)),
            Buttons::Ask(corr, choices) => Some(crate::runtime::ask_buttons(corr, choices)),
            Buttons::Card(corr, choices) => Some(crate::runtime::card_buttons(corr, choices)),
            Buttons::ConfirmGrant(corr, o) => Some(crate::runtime::grant::grant_buttons(corr, o)),
            Buttons::Accept(corr) => Some(crate::runtime::accept_buttons(corr)),
            Buttons::ShouldHaveAsked(options) => Some(asked_menu(options)),
            Buttons::JevLabel(judgment) => Some(crate::runtime::jev_buttons(judgment)),
            Buttons::Reminder(wake) => Some(crate::runtime::remind_buttons(wake)),
            Buttons::Horizon(wake, answers) => Some(crate::runtime::horizon_buttons(wake, answers)),
            Buttons::Seal(slug) => Some(crate::runtime::seal_buttons(slug)),
            Buttons::Link(h, o) => Some(crate::runtime::seal::link::buttons(h, o)),
            Buttons::OntoUndo(write) => Some(crate::runtime::onto_buttons(write)),
            Buttons::PersonaSoul(key) => Some(crate::runtime::persona_soul::buttons(key)),
            Buttons::SelfNote(items) => Some(crate::runtime::self_note::buttons(items)),
            Buttons::SelfResume => Some(crate::runtime::self_note::resume_buttons()),
            Buttons::ContentRemove(s, f) => Some(crate::runtime::content::buttons(s, f)),
            Buttons::Again(q) => Some(crate::runtime::again_buttons(q)),
            Buttons::Standing(mode, id, wide) => {
                Some(crate::runtime::capture_buttons(mode, id, *wide))
            }
            Buttons::CardQuestion(ask, id) => Some(crate::runtime::card_question::buttons(ask, id)),
            Buttons::Sources(session, srcs, offer) => Some(crate::runtime::source_buttons(
                session,
                srcs,
                offer.as_deref(),
            )),
            Buttons::Clear => Some(vec![]),
            Buttons::Keep | Buttons::V2 => None,
        };
        let mut req = self
            .shared
            .http
            .update_message(Id::new(channel), Id::new(message))
            .content(Some(content))
            .allowed_mentions(Some(&none));
        if let Some(c) = &comps {
            req = req.components(Some(c));
        }
        req.await.map_err(|e| SendErr::of(&e))?;
        self.shared.board.update(|s| s.edits += 1);
        Ok(())
    }

    // ------------------------------------------------------------ live

    /// The stream's waiting ops, best-effort: the first failure drops the rest.
    async fn apply_live(&mut self) {
        let ops = std::mem::take(&mut self.live);
        for op in ops {
            if self.retry_at.is_some() {
                break;
            }
            let r = match op {
                Op::Typing => self.typing().await,
                Op::Upsert {
                    key,
                    content,
                    buttons,
                } if key == render::BOARD_KEY => self.board_op(content, buttons).await,
                Op::Upsert {
                    key,
                    content,
                    buttons,
                } => {
                    if self.sealed.contains(&key) {
                        continue;
                    }
                    let Ok(channel) = self.place_channel().await else {
                        continue;
                    };
                    let new = !self.msgs.contains_key(&key);
                    let reply_to = new.then(|| self.anchor.take()).flatten();
                    // The first text part of the turn that answers the
                    // owner's own message is the answer (theseus-l1y1).
                    let part = policy::is_text_part(&key);
                    let event = policy::of_live(&key, self.owed.is_some(), &content);
                    let ping = self.pings(event);
                    let r = self
                        .write(&Write {
                            key,
                            channel,
                            content,
                            buttons,
                            reply_to,
                            message: None,
                            mentions: vec![],
                            ping,
                        })
                        .await;
                    if new && part && matches!(r, Ok(Some(_))) {
                        self.owed = None;
                    }
                    r.map(|_| ())
                }
                Op::Notice { key, card } => self.notice(&key, &card).await,
            };
            match r {
                Ok(()) => {}
                Err(e) if e.away => self.away(&e),
                Err(e) => self.shared.board.error("live message", None, e.message),
            }
        }
        self.release();
    }

    async fn typing(&mut self) -> Result<(), SendErr> {
        let Some(c) = self.channel else {
            return Ok(());
        };
        self.shared
            .http
            .create_typing_trigger(Id::new(c))
            .await
            .map(|_| ())
            .map_err(|e| SendErr::of(&e))
    }

    /// A structured notice (a Discord embed, no mentions): live, when
    /// `[discord] notice_embeds` is on.
    async fn notice(&mut self, key: &str, card: &NoticeCard) -> Result<(), SendErr> {
        let channel = self.place_channel().await?;
        let mut b = twilight_util::builder::embed::EmbedBuilder::new()
            .title(card.title.clone())
            .color(card.color)
            .description(card.description.clone());
        for (name, value) in &card.fields {
            if !value.trim().is_empty() {
                b = b.field(twilight_util::builder::embed::EmbedFieldBuilder::new(
                    name.clone(),
                    value.clone(),
                ));
            }
        }
        let embeds = [b.build()];
        let none = AllowedMentions::default();
        // Its "Should have asked" button, or none once the tool asks first.
        let comps = asked_button(card.ask.as_ref());
        let http = &self.shared.http;
        match self.msgs.get(key).copied() {
            Some((c, m)) => {
                http.update_message(Id::new(c), Id::new(m))
                    .embeds(Some(&embeds))
                    .components(Some(&comps))
                    .allowed_mentions(Some(&none))
                    .await
                    .map_err(|e| SendErr::of(&e))?;
                self.shared.board.update(|s| s.edits += 1);
            }
            None => {
                // A call's notice is a tool line (theseus-l1y1), held to the
                // place's window as any create is.
                let wants = self.pings(Event::ToolLine);
                let (now, window) = (tokio::time::Instant::now(), self.window());
                let ping = wants && self.shared.pings.open(channel, now, window);
                let mut req = http
                    .create_message(Id::new(channel))
                    .embeds(&embeds)
                    .allowed_mentions(Some(&none));
                if !ping {
                    req = req.flags(MessageFlags::SUPPRESS_NOTIFICATIONS);
                }
                if !comps.is_empty() {
                    req = req.components(&comps);
                }
                let m = req
                    .await
                    .map_err(|e| SendErr::of(&e))?
                    .model()
                    .await
                    .map_err(|e| SendErr {
                        away: true,
                        unsure: true,
                        gone: false,
                        message: e.to_string(),
                    })?;
                if ping {
                    self.shared.pings.mark(channel, now, window);
                }
                self.touch(key);
                self.msgs.insert(key.to_string(), (channel, m.id.get()));
                self.shared.board.update(|s| s.messages_out += 1);
                self.shared.core.binding_ledger_soon(
                    LedgerKind::DiscordMessageOut,
                    None,
                    json!({"place": self.label, "message_id": m.id.to_string(), "part": key, "notice": card.title,
                           "ping": ping, "held": wants && !ping}),
                );
            }
        }
        Ok(())
    }
}

/// Wake every lane when the outbox changes, and every half minute besides;
/// first refuse what was written for a place no longer bound (theseus-l3m),
/// which no lane would ever take.
pub(crate) async fn courier(shared: Arc<Shared>) {
    let mut rx = shared.core.outbox.subscribe();
    let mut tick = tokio::time::interval(Duration::from_secs(30));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            r = rx.changed() => if r.is_err() { break },
            _ = tick.tick() => {}
        }
        shared.refuse_unbound();
        shared.wake_lanes();
    }
}

/// A card's message key, which its settle finds it by.
fn key_of_card(question: &str) -> String {
    format!("confirm:{question}")
}

/// A card's `content` and buttons as one V2 message: its words and the
/// question's `answer_options` (waiting), or its settled line (closed).
fn card_message(
    question: &str,
    text: String,
    options: &[theseus_protocol::work::AnswerOption],
    closed: bool,
) -> (String, Buttons) {
    let comps = if closed {
        render::card_v2::closed(&text)
    } else {
        render::card_v2::waiting(question, &text, options)
    };
    (
        serde_json::to_string(&comps).unwrap_or_default(),
        Buttons::V2,
    )
}

/// One plain message a post writes, pinging or not.
fn lone(key: String, channel: u64, content: String, ping: bool) -> Write {
    Write {
        key,
        channel,
        content,
        buttons: Buttons::Keep,
        reply_to: None,
        message: None,
        mentions: vec![],
        ping,
    }
}

/// A glide's post (38b): another session's words, posted in `channel` once,
/// under its call's keys (`glide:<call>`, then `glide:<call>:<part>`), in
/// this place's order.
fn glide(body: &Value, corr: &str, channel: u64, ping: bool) -> Plan {
    let call = body["call"].as_str().unwrap_or(corr);
    let writes = render::glide(body)
        .into_iter()
        .enumerate()
        .map(|(i, content)| Write {
            key: match i {
                0 => format!("glide:{call}"),
                i => format!("glide:{call}:{i}"),
            },
            channel,
            content,
            buttons: Buttons::Keep,
            reply_to: None,
            message: None,
            mentions: vec![],
            ping,
        })
        .collect();
    Plan {
        writes,
        extra: json!({"glide": call}),
    }
}

/// What the owner is told when an MCP server's tools (`mcp_changed`) or a
/// prompt's definition (`mcp_prompt_changed`) changed.
pub(crate) fn mcp_note(body: &Value) -> String {
    use theseus_core::owner_note::{note, Kind};
    let server = body["server"].as_str().unwrap_or("?");
    let summary = body["summary"].as_str().unwrap_or("");
    match body["prompt"].as_str() {
        Some(prompt) => note(
            Kind::Fyi,
            &format!("MCP server {server}'s prompt {prompt} changed"),
            &format!("it changed since you last used it ({summary})"),
            "This use went ahead.",
        ),
        None => note(
            Kind::Fyi,
            &format!("MCP server {server} changed its tools"),
            &format!("the server sent a new list ({summary})"),
            "The new list is offered from the next turn.",
        ),
    }
}

#[cfg(test)]
mod tests_bound;
#[cfg(test)]
mod tests_catchup;
#[cfg(test)]
mod tests_operator;
#[cfg(test)]
mod tests_self_note;
#[cfg(test)]
mod tests_v2;

#[cfg(test)]
mod tests_order;

#[cfg(test)]
mod tests {
    use super::*;

    /// A card's rule (theseus-50p): a card written while its lane runs, for a
    /// session a place here renders, waits for its place to show its call,
    /// for `CARD_WAIT` at most, and then goes all the same; once the place
    /// has shown it, it goes at once; a card from before this process, or for
    /// a session no place here renders, never waits.
    #[test]
    fn a_card_waits_for_its_call_at_most_card_wait() {
        let now = tokio::time::Instant::from_std(std::time::Instant::now());
        let later = now + CARD_WAIT / 2;
        // A fresh card for a session rendered here starts its wait.
        assert_eq!(
            card_hold(false, true, true, None, now),
            CardHold::Until(now + CARD_WAIT)
        );
        // It waits on, until its wait is over, never longer.
        let until = now + CARD_WAIT;
        assert_eq!(
            card_hold(false, true, true, Some(until), later),
            CardHold::Until(until)
        );
        assert_eq!(
            card_hold(false, true, true, Some(until), until),
            CardHold::GaveUp
        );
        assert_eq!(
            card_hold(false, true, true, Some(until), until + CARD_WAIT),
            CardHold::GaveUp
        );
        // The place showed the call: it goes, waiting or not.
        assert_eq!(card_hold(true, true, true, None, now), CardHold::Go);
        assert_eq!(
            card_hold(true, true, true, Some(until), later),
            CardHold::Go
        );
        // A card from before this process, or one no place here shows.
        assert_eq!(card_hold(false, false, true, None, now), CardHold::Go);
        assert_eq!(card_hold(false, true, false, None, now), CardHold::Go);
        assert_eq!(
            card_hold(false, true, false, Some(until), later),
            CardHold::Go
        );
    }

    #[test]
    fn a_nonce_is_stable_per_message_and_fits_discord() {
        let a = nonce("turn_1:L0:p0");
        assert_eq!(a, nonce("turn_1:L0:p0"));
        assert_ne!(a, nonce("turn_1:L0:p1"));
        assert!(
            a.len() <= 25 && a.chars().all(|c| c.is_ascii_digit()),
            "{a}"
        );
    }

    /// A card's answerers (theseus-9j9): the place's users who are owners,
    /// whose answers alone count (the place rule, theseus-zmgb); nobody when
    /// none of them is.
    #[test]
    fn a_cards_answerers_are_the_places_users_who_are_owners() {
        // Invented ids, of a Discord id's length.
        let (a, b, c) = (
            100_000_000_000_000_101,
            200_000_000_000_000_202,
            300_000_000_000_000_303,
        );
        let place = [a, b, c];
        assert!(answerers(&place, &BTreeSet::new()).is_empty());
        let owners: BTreeSet<String> =
            [format!("discord:{b}"), "discord:400000000000000404".into()].into();
        assert_eq!(answerers(&place, &owners), [b]);
        assert!(answerers(&[], &owners).is_empty());
        assert_eq!(
            mentioning(&[202, 101], "**Approve?** `fs.write` a.txt".into()),
            "<@202> <@101> **Approve?** `fs.write` a.txt"
        );
        assert_eq!(mentioning(&[], "plain".into()), "plain");
    }
}
