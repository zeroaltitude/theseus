//! Voice in a Discord voice channel (rows 77 and 78: 44b's wire-in, with
//! 45a's Deepgram and 45b's spend). A child of `runtime`, apart from it for
//! the shape budget's file ceiling.
//!
//! - **A voice channel is a place**: a `[[channel]]` bound `voice = true`,
//!   with its own session, whose text chat is the place's text: the
//!   transcript, the replies, and the cards. Its class is the place rule's,
//!   as any channel's is: bound `private = true`, its viewers are read at the
//!   start (`check_private`), except in a trusted guild (theseus-rdqg).
//! - **Joining**: `/join` and `/leave`, from a private place, by one of its
//!   users, for a voice channel the bindings file binds whose users list
//!   them. Theseus never joins on its own. songbird's manager is the voice
//!   crate's `manager()`, so what it receives is decoded; it is built with
//!   the gateway's shard and spawns nothing, and fed only the gateway's voice
//!   state and voice server updates. Its driver starts at a join.
//! - **Utterances become turns** in the voice place's session, authored by
//!   their speaker, through the place's own line of turns: one at a time, a
//!   typed message's included. Only the place's users are heard.
//! - **Replies**: a voice turn's reply goes back to the engine, which speaks
//!   it a sentence at a time with barge-in, and plays the acknowledgment when
//!   the turn runs long. Any other turn's reply in the place (a wake's, a
//!   report's, a typed message's) is spoken at the next pause. The text goes
//!   to the place through the outbox as every reply does.
//! - **Spend** (45b): each speech call is checked against the session's limit
//!   before it is made, and booked after it, with its row.
//! - **A dropped connection** is a `voice.failed` row and a notice; the
//!   session goes on in text.

use std::collections::{HashMap, HashSet, VecDeque};
use std::num::NonZeroU64;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

use serde_json::json;
use theseus_core::voice::{SpeechCall, SpeechKind, VoiceConfig};
use theseus_core::Core;
use theseus_protocol::voice::VoiceStatus;
use theseus_protocol::Event as CoreEvent;
use theseus_protocol::{DiscordOrigin, LedgerKind, PlaceClass, TurnSubmitParams, TurnSubmitResult};
use theseus_voice::songbird::shards::TwilightMap;
use theseus_voice::songbird::Songbird;
use theseus_voice::{
    Command, Config as EngineConfig, CutWhy, DeepgramSettings, DeepgramSpeech, Engine, Event,
    Failure, HeardAs, Over, SongbirdIo, Speaker, Speech, SpeechError, SpeechFuture, Spoken,
    Synthesis, Transcript, TurnId, Utterance,
};
use tokio::sync::mpsc;
use twilight_gateway::{Event as Gateway, Intents, Shard};
use twilight_model::application::command::Command as SlashCommand;
use twilight_model::application::command::CommandType;
use twilight_model::application::interaction::application_command::{
    CommandDataOption, CommandOptionValue,
};
use twilight_model::channel::ChannelType;
use twilight_model::id::Id;
use twilight_util::builder::command::{ChannelBuilder, CommandBuilder};

use super::{Place, PlaceMsg, Shared};
use crate::bindings::Bindings;

mod notes;
#[cfg(test)]
mod tests_heard;

use notes::Notes;

/// Before every voice turn's input (theseus-rkvl): its reply is heard, not
/// read.
pub(crate) const FRAMING: &str = "[Voice call: they hear your reply, they don't read it. Answer \
     in one to three short sentences of plain speech: no lists, tables, code, markdown or long \
     numbers. If you were cut off, don't assume they heard the rest.]";

/// How long a join may take: songbird's own connect, then DAVE's handshake.
const JOIN_WAIT: Duration = Duration::from_secs(20);

/// A voice channel the bindings file binds (`voice = true`).
#[derive(Clone, Debug)]
struct VoicePlace {
    /// `channel:<id>`, the place's key.
    key: String,
    label: String,
    users: Vec<u64>,
    /// Its guild: a voice channel may be in any bound guild (step 38a).
    guild: u64,
}

/// The binding's voice: the places, songbird once the shard exists, who is
/// in which voice channel, and the one call while joined, in its place's
/// guild (songbird keeps one call per guild; the binding joins one at a time).
pub(crate) struct Voice {
    cfg: VoiceConfig,
    places: HashMap<u64, VoicePlace>,
    songbird: OnceLock<Arc<Songbird>>,
    /// From the gateway's voice states: who is in which voice channel now.
    states: Mutex<HashMap<u64, u64>>,
    /// Members' names, from voice states and lookups.
    names: Mutex<HashMap<u64, String>>,
    call: Mutex<Option<Call>>,
    /// Calls joined since the start, to tell an old call's end from the next.
    calls: std::sync::atomic::AtomicU64,
    status: Mutex<VoiceStatus>,
}

/// The call while joined.
struct Call {
    serial: u64,
    channel: u64,
    guild: u64,
    key: String,
    label: String,
    commands: mpsc::UnboundedSender<Command>,
    joined: Instant,
    /// A voice turn's submit is in flight.
    inflight: bool,
    /// Its turn's id, once the turn started: its end is answered by the
    /// submit, not spoken again.
    own: HashSet<String>,
    /// Voice turns waiting for the turn in flight.
    waiting: VecDeque<VoiceTurn>,
    /// Why it ended, when it did not end on a `/leave`.
    dropped: Option<String>,
    /// What the next voice turn is told about the last (theseus-qb8o).
    notes: Notes,
}

/// A turn of utterances from the call, for the voice place.
#[derive(Debug)]
pub(crate) struct VoiceTurn {
    serial: u64,
    turn: TurnId,
    utterances: Vec<Utterance>,
}

impl Voice {
    /// No voice place and voice off: a test's shared state.
    #[cfg(test)]
    pub(crate) fn none() -> Self {
        let none = Bindings {
            format: 2,
            guilds: Vec::new(),
            channel: Vec::new(),
            dm: Vec::new(),
            revision: String::new(),
        };
        Self::new(&VoiceConfig::default(), &none)
    }

    pub(crate) fn new(cfg: &VoiceConfig, bindings: &Bindings) -> Self {
        let places = bindings
            .channel
            .iter()
            .filter(|c| c.voice)
            .filter_map(|c| {
                let id = c.id.parse().ok()?;
                let users = c.users.iter().filter_map(|u| u.parse().ok()).collect();
                Some((
                    id,
                    VoicePlace {
                        key: format!("channel:{}", c.id),
                        label: c.label(),
                        users,
                        guild: c.guild.parse().unwrap_or(0),
                    },
                ))
            })
            .collect();
        let status = VoiceStatus {
            state: if cfg.enabled { "ready" } else { "off" }.into(),
            ..VoiceStatus::default()
        };
        Self {
            cfg: cfg.clone(),
            places,
            songbird: OnceLock::new(),
            states: Mutex::default(),
            names: Mutex::default(),
            call: Mutex::default(),
            calls: std::sync::atomic::AtomicU64::new(0),
            status: Mutex::new(status),
        }
    }

    /// The gateway intents voice needs: the guild's voice states, when it is on.
    pub(crate) fn intents(&self) -> Intents {
        match self.cfg.enabled {
            true => Intents::GUILD_VOICE_STATES,
            false => Intents::empty(),
        }
    }

    /// A gateway event: a voice state keeps who is where, and songbird sees
    /// its two events on a task apart from a join, which waits for them.
    pub(crate) fn gateway(&self, event: &Gateway) {
        let Some(songbird) = self.songbird.get() else {
            return;
        };
        match event {
            Gateway::VoiceStateUpdate(v) => {
                let user = v.0.user_id.get();
                if let Some(m) = &v.0.member {
                    self.names.lock().unwrap().insert(user, m.user.name.clone());
                }
                let mut states = self.states.lock().unwrap();
                match v.0.channel_id {
                    Some(c) => states.insert(user, c.get()),
                    None => states.remove(&user),
                };
            }
            Gateway::VoiceServerUpdate(_) => {}
            _ => return,
        }
        let (songbird, event) = (Arc::clone(songbird), event.clone());
        tokio::spawn(async move { songbird.process(&event).await });
    }

    /// Health's voice block, as last pushed.
    #[cfg(test)]
    pub(crate) fn status(&self) -> VoiceStatus {
        self.status.lock().unwrap().clone()
    }

    fn name(&self, user: u64) -> String {
        self.names
            .lock()
            .unwrap()
            .get(&user)
            .cloned()
            .unwrap_or_else(|| format!("user {user}"))
    }

    /// The voice channel `/join` means: the one named, the place it was typed
    /// in when that is one, the one its presser is in, or the only one bound.
    fn target(&self, named: Option<u64>, here: Option<u64>, presser: u64) -> Option<u64> {
        let bound = |c: &u64| self.places.contains_key(c);
        named
            .or_else(|| here.filter(bound))
            .or_else(|| {
                self.states
                    .lock()
                    .unwrap()
                    .get(&presser)
                    .copied()
                    .filter(bound)
            })
            .or_else(|| match self.places.len() {
                1 => self.places.keys().next().copied(),
                _ => None,
            })
    }

    /// The call's voice place, while joined.
    fn joined(&self) -> Option<(u64, String)> {
        self.call
            .lock()
            .unwrap()
            .as_ref()
            .map(|c| (c.channel, c.label.clone()))
    }

    /// The reply to a voice turn, to the call that heard it.
    fn reply(&self, serial: u64, turn: TurnId, text: String) {
        if let Some(c) = self.call.lock().unwrap().as_mut() {
            if c.serial == serial {
                c.inflight = false;
                let _ = c.commands.send(Command::Reply { turn, text });
            }
        }
    }
}

/// The slash commands voice adds: `/join`, with the voice channel as an
/// option for a place that is not one, and `/leave`.
pub(super) fn commands() -> Vec<SlashCommand> {
    vec![
        CommandBuilder::new(
            "join",
            "Join a voice channel this place's bindings name, and talk there (only when you invite me)",
            CommandType::ChatInput,
        )
        .option(
            ChannelBuilder::new("channel", "The voice channel; else the one you are in")
                .channel_types([ChannelType::GuildVoice])
                .required(false),
        )
        .build(),
        CommandBuilder::new(
            "leave",
            "Leave the voice channel; the conversation goes on in text",
            CommandType::ChatInput,
        )
        .build(),
    ]
}

/// `/join`'s channel option, when given.
pub(super) fn join_option(options: &[CommandDataOption]) -> Option<u64> {
    options
        .iter()
        .find_map(|o| match (o.name.as_str(), &o.value) {
            ("channel", CommandOptionValue::Channel(c)) => Some(c.get()),
            _ => None,
        })
}

impl Place {
    /// `/join` (44b): only when invited, from a private place, by one of its
    /// users, into a voice channel the bindings file binds that lists them.
    pub(super) async fn join(
        &self,
        named: Option<u64>,
        origin: Option<DiscordOrigin>,
        by: &str,
    ) -> String {
        match self.try_join(named, origin, by).await {
            Ok(said) | Err(said) => said,
        }
    }

    async fn try_join(
        &self,
        named: Option<u64>,
        origin: Option<DiscordOrigin>,
        by: &str,
    ) -> Result<String, String> {
        let shared = &self.shared;
        let voice = &shared.voice;
        if !voice.cfg.enabled {
            return Err("Voice is off in this daemon's config (`[voice] enabled`).".into());
        }
        let core = &shared.core;
        if core.runner.place_rule.class(&core.cfg, Some(&self.target)) != PlaceClass::Private {
            return Err(
                "I join a voice channel only when invited from a private place: a DM \
                        with you, or a channel bound private. This one is shared."
                    .into(),
            );
        }
        let presser = origin
            .as_ref()
            .and_then(|o| o.user_id.parse::<u64>().ok())
            .ok_or("Who asked is not known, so I won't join.")?;
        let here = self.channel.map(|c| c.get());
        let channel = voice.target(named, here, presser).ok_or(
            "Which voice channel? Name one with `/join channel:`, or join one the bindings file \
             binds first.",
        )?;
        let Some(place) = voice.places.get(&channel).cloned() else {
            return Err(format!(
                "<#{channel}> is not a voice channel the bindings file binds (`voice = true`)."
            ));
        };
        if !place.users.contains(&presser) {
            return Err(format!(
                "{} does not list you, so I won't join it.",
                place.label
            ));
        }
        if let Some((c, label)) = voice.joined() {
            return Err(match c == channel {
                true => format!("I'm already in {label}."),
                false => format!("I'm in {label}; `/leave` first."),
            });
        }
        let key = self.speech_key()?;
        let songbird = voice
            .songbird
            .get()
            .cloned()
            .ok_or("The gateway isn't up yet; try again in a moment.")?;
        let speech = DeepgramSpeech::new(&key, settings(&voice.cfg)).map_err(|e| e.to_string())?;
        drop(key);
        let (guild, chan) = (
            NonZeroU64::new(place.guild).ok_or("no guild")?,
            NonZeroU64::new(channel).ok_or("no channel")?,
        );
        let call = match tokio::time::timeout(JOIN_WAIT, songbird.join(guild, chan)).await {
            Ok(Ok(call)) => call,
            Ok(Err(e)) => return Err(self.join_failed(&songbird, &place, format!("{e}")).await),
            Err(_) => {
                let why = format!("no voice connection in {} s", JOIN_WAIT.as_secs());
                return Err(self.join_failed(&songbird, &place, why).await);
            }
        };
        let io = SongbirdIo::attach(call).await;
        Ok(self.start_call(io, speech, channel, place, by).await)
    }

    /// The voice key from the secret board: only Theseus reads it.
    fn speech_key(&self) -> Result<String, String> {
        use theseus_core::secrets::SecretState;
        let name = &self.shared.voice.cfg.key_secret;
        let state = self
            .shared
            .core
            .secrets
            .subscribe()
            .borrow()
            .get(name)
            .cloned();
        match state {
            Some(SecretState::Ready(key)) => Ok(key.expose().to_string()),
            Some(SecretState::Resolving) => {
                Err(format!("The voice key ({name}) is still resolving."))
            }
            Some(SecretState::Failed(why)) => {
                Err(format!("The voice key ({name}) did not resolve: {why}."))
            }
            None => Err(format!(
                "There is no `[secrets]` entry named {name} for the voice key."
            )),
        }
    }

    async fn join_failed(&self, songbird: &Songbird, place: &VoicePlace, why: String) -> String {
        let _ = songbird
            .remove(NonZeroU64::new(place.guild).unwrap_or(NonZeroU64::MIN))
            .await;
        self.shared.core.binding_ledger(
            LedgerKind::VoiceFailed,
            None,
            json!({"what": "join", "place": place.label, "error": why}),
        );
        format!("⚠️ Could not join {}: {why}.", place.label)
    }

    /// The call is up: the engine on it, its events read, and the place told.
    async fn start_call(
        &self,
        io: SongbirdIo,
        speech: DeepgramSpeech,
        channel: u64,
        place: VoicePlace,
        by: &str,
    ) -> String {
        let shared = &self.shared;
        let voice = &shared.voice;
        for &u in &place.users {
            let known = voice.names.lock().unwrap().contains_key(&u);
            if known {
                continue;
            }
            let asked = shared.http.guild_member(Id::new(place.guild), Id::new(u));
            if let Ok(r) = asked.await {
                if let Ok(m) = r.model().await {
                    voice.names.lock().unwrap().insert(u, m.user.name);
                }
            }
        }
        let metered = Metered {
            inner: speech,
            core: Arc::downgrade(&shared.core),
            key: place.key.clone(),
        };
        let config = EngineConfig::new(place.users.iter().map(|&u| Speaker(u)));
        let (engine, handle) = Engine::new(config, Box::new(io), Arc::new(metered));
        let serial = voice
            .calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;
        let hears: Vec<String> = place.users.iter().map(|&u| voice.name(u)).collect();
        *voice.call.lock().unwrap() = Some(Call {
            serial,
            channel,
            guild: place.guild,
            key: place.key.clone(),
            label: place.label.clone(),
            commands: handle.commands.clone(),
            joined: Instant::now(),
            inflight: false,
            own: HashSet::new(),
            waiting: VecDeque::new(),
            dropped: None,
            notes: Notes::default(),
        });
        update(shared, |s| {
            s.state = "joined".into();
            s.channel = Some(place.label.clone());
            s.hears = hears.clone();
            s.joins += 1;
            s.since_ms = Some(theseus_protocol::now_unix_ms());
        });
        shared.core.binding_ledger(
            LedgerKind::VoiceJoined,
            None,
            json!({"place": place.label, "channel": channel.to_string(), "by": by, "hears": hears}),
        );
        tokio::spawn(engine.run());
        tokio::spawn(pump(
            Arc::clone(shared),
            serial,
            place.clone(),
            handle.events,
        ));
        format!(
            "🎙️ Joined {}. I hear {}; anyone else is not heard. Talk over me to stop me; `/leave` \
             ends the call, and the conversation goes on here in text.",
            place.label,
            hears.join(", ")
        )
    }

    /// `/leave`: from a private place, by one of its users.
    pub(super) async fn leave(&self, by: &str) -> String {
        let shared = &self.shared;
        let core = &shared.core;
        if core.runner.place_rule.class(&core.cfg, Some(&self.target)) != PlaceClass::Private {
            return "`/leave` works from a private place, as `/join` does.".into();
        }
        let Some(call) = shared.voice.call.lock().unwrap().take() else {
            return "I'm not in a voice channel.".into();
        };
        let _ = call.commands.send(Command::Leave);
        if let Some(songbird) = shared.voice.songbird.get() {
            if let Some(guild) = NonZeroU64::new(call.guild) {
                if let Err(e) = songbird.leave(guild).await {
                    shared.board.error("voice leave", None, e);
                }
            }
        }
        ended(shared, &call, "left", by);
        format!(
            "👋 Left {}. The conversation goes on here in text.",
            call.label
        )
    }

    /// A turn of utterances from this place's call: now, or after the turn
    /// in flight, a typed message's included.
    pub(super) fn voice_turn(&mut self, t: VoiceTurn) {
        if self.inflight {
            if let Some(c) = self.shared.voice.call.lock().unwrap().as_mut() {
                c.waiting.push_back(t);
            }
            return;
        }
        self.submit_voice(t);
    }

    /// After a turn: a voice turn that waited goes now.
    pub(super) fn voice_next(&mut self) {
        if self.inflight {
            return;
        }
        let next = self
            .shared
            .voice
            .call
            .lock()
            .unwrap()
            .as_mut()
            .filter(|c| c.key == self.key)
            .and_then(|c| c.waiting.pop_front());
        if let Some(t) = next {
            self.submit_voice(t);
        }
    }

    fn submit_voice(&mut self, t: VoiceTurn) {
        let voice = &self.shared.voice;
        let names: Vec<String> = t
            .utterances
            .iter()
            .map(|u| voice.name(u.speaker.0))
            .collect();
        // The transcript, in the place's text: what was heard, by whom.
        for (u, name) in t.utterances.iter().zip(&names) {
            self.say(&format!("🎙️ **{name}**: {}", u.text), None);
        }
        let one = names.iter().all(|n| *n == names[0]);
        let said = t
            .utterances
            .iter()
            .zip(&names)
            .map(|(u, n)| match one {
                true => format!("🎙️ {}", u.text),
                false => format!("🎙️ [{n}] {}", u.text),
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        // What the last turn left unheard, before what was said now.
        let line = voice
            .call
            .lock()
            .unwrap()
            .as_mut()
            .filter(|c| c.serial == t.serial)
            .and_then(|c| c.notes.line(&t.utterances));
        let input = match line {
            Some(line) => format!("{FRAMING}\n{line}\n{said}"),
            None => format!("{FRAMING}\n{said}"),
        };
        let author = match one {
            true => format!("discord:{}", names[0]),
            false => "discord".into(),
        };
        self.inflight = true;
        self.saw_failure = false;
        if let Some(c) = voice.call.lock().unwrap().as_mut() {
            c.inflight = true;
        }
        let (rpc, tx, sid, shared) = (
            self.shared.rpc.clone(),
            self.tx.clone(),
            self.session_id.clone(),
            Arc::clone(&self.shared),
        );
        tokio::spawn(async move {
            let r = rpc
                .call::<_, TurnSubmitResult>(
                    theseus_protocol::method::TURN_SUBMIT,
                    TurnSubmitParams {
                        carried: false,
                        prompt: None,
                        session_id: Some(sid),
                        input,
                        profile: None,
                        provider: None,
                        model: None,
                        author: Some(author),
                        attachments: Vec::new(),
                        reply_to: None,
                        opened_from: None,
                    },
                )
                .await;
            // Spoken: the turn's reply, or silence when it failed, which the
            // place says in text.
            let text = r.as_ref().map(|r| r.output.clone()).unwrap_or_default();
            shared.voice.reply(t.serial, t.turn, text);
            let _ = tx.send(PlaceMsg::SubmitDone(r.map(|_| ())));
        });
    }

    /// A turn in this place began or ended while its call is up: a voice
    /// turn's start marks it as answered by its submit; any other turn's
    /// reply is spoken at the next pause.
    pub(super) fn voice_heard(&self, e: &CoreEvent) {
        let mut call = self.shared.voice.call.lock().unwrap();
        let Some(c) = call.as_mut().filter(|c| c.key == self.key) else {
            return;
        };
        match e {
            CoreEvent::TurnStarted(t) if c.inflight && !t.continuation => {
                c.own.insert(t.turn_id.clone());
            }
            CoreEvent::TurnEnded(r) if r.session_id == self.session_id => {
                if !c.own.remove(&r.turn_id) && !r.output.trim().is_empty() {
                    let _ = c.commands.send(Command::Report {
                        text: r.output.clone(),
                    });
                }
            }
            CoreEvent::TurnFailed(t) => {
                if let Some(id) = &t.turn_id {
                    c.own.remove(id);
                }
            }
            _ => {}
        }
    }
}

/// The voice settings as Deepgram takes them.
fn settings(cfg: &VoiceConfig) -> DeepgramSettings {
    DeepgramSettings {
        api_base: cfg.api_base.clone(),
        stt_model: cfg.stt_model.clone(),
        language: cfg.language.clone(),
        tts_voice: cfg.tts_voice.clone(),
        ..DeepgramSettings::default()
    }
}

/// Health's voice block, pushed with each change.
fn update(shared: &Shared, f: impl FnOnce(&mut VoiceStatus)) {
    let status = {
        let mut s = shared.voice.status.lock().unwrap();
        f(&mut s);
        s.clone()
    };
    shared.board.update(|b| b.voice = Some(status));
}

/// With the gateway's shard, when voice is on: songbird's manager over it,
/// decoding (the voice crate's `manager()`), which spawns nothing; and
/// health's voice block, ready.
pub(super) fn attach(
    shared: &Shared,
    shard: &Shard,
    me: Id<twilight_model::id::marker::UserMarker>,
) {
    if !shared.voice.cfg.enabled {
        return;
    }
    let senders = HashMap::from([(shard.id().number(), shard.sender())]);
    let songbird = theseus_voice::manager(Arc::new(TwilightMap::new(senders)), me);
    let _ = shared.voice.songbird.set(Arc::new(songbird));
    update(shared, |_| {});
}

/// A call ended: the board, its row, and, unless it was a `/leave`, a notice.
fn ended(shared: &Shared, call: &Call, why: &str, by: &str) {
    let seconds = call.joined.elapsed().as_secs();
    update(shared, |s| {
        s.state = "ready".into();
        s.channel = None;
        s.hears.clear();
        s.since_ms = None;
    });
    shared.core.binding_ledger(
        LedgerKind::VoiceLeft,
        None,
        json!({"place": call.label, "channel": call.channel.to_string(), "why": why, "by": by, "seconds": seconds}),
    );
}

/// Read one call's engine events until it ends: turns to the voice place,
/// speech to spend, the rest to the ledger and health.
async fn pump(
    shared: Arc<Shared>,
    serial: u64,
    place: VoicePlace,
    mut events: mpsc::UnboundedReceiver<Event>,
) {
    while let Some(e) = events.recv().await {
        match e {
            Event::Turn { id, utterances } => {
                with_call(&shared, serial, |c| c.notes.turn(id, &utterances));
                let tx = shared
                    .routes
                    .lock()
                    .unwrap()
                    .by_channel
                    .get(&channel_of(&place))
                    .cloned();
                if let Some(tx) = tx {
                    let _ = tx.send(PlaceMsg::Voice(VoiceTurn {
                        serial,
                        turn: id,
                        utterances,
                    }));
                }
            }
            Event::Utterance(u) => {
                with_call(&shared, serial, |c| c.notes.heard(&u));
                update(&shared, |s| {
                    s.utterances += 1;
                    s.heard_ms += u.length.as_millis() as u64;
                });
                let detail = json!({"speaker": u.speaker.0.to_string(),
                    "empty": u.text.trim().is_empty(), "heard_as": heard_as(u.heard_as),
                    "over": over(u.over.as_ref())});
                book(
                    &shared,
                    &place,
                    SpeechKind::Transcribed,
                    u.usage,
                    u.latency,
                    detail,
                );
            }
            Event::Synthesized {
                what,
                usage,
                latency,
            } => {
                update(&shared, |s| {
                    s.sentences += 1;
                    s.spoken_chars += usage.chars as u64;
                });
                let detail = json!({"what": spoken(what)});
                book(
                    &shared,
                    &place,
                    SpeechKind::Synthesized,
                    usage,
                    latency,
                    detail,
                );
            }
            Event::BargeIn {
                speaker,
                what,
                dropped,
            } => {
                update(&shared, |s| s.barge_ins += 1);
                shared.core.binding_ledger(
                    LedgerKind::VoiceBargeIn,
                    session(&shared, &place).as_deref(),
                    json!({"speaker": speaker.0.to_string(), "what": spoken(what), "dropped": dropped}),
                );
            }
            Event::Unlisted { speaker } => shared.core.binding_ledger(
                LedgerKind::VoiceUnlisted,
                session(&shared, &place).as_deref(),
                json!({"speaker": speaker.0.to_string(), "place": place.label}),
            ),
            Event::Failed { what, error } => failed(&shared, serial, &place, &what, &error),
            Event::Cut { .. } | Event::Resumed { .. } => held(&shared, serial, &place, &e),
            Event::Acknowledged { .. } | Event::Speaking { .. } | Event::Spoke { .. } => {}
        }
    }
    // The engine is done: a `/leave` took the call already; else it dropped.
    let call = {
        let mut slot = shared.voice.call.lock().unwrap();
        match slot.as_ref() {
            Some(c) if c.serial == serial => slot.take(),
            _ => None,
        }
    };
    if let Some(call) = call {
        if let (Some(songbird), Some(guild)) =
            (shared.voice.songbird.get(), NonZeroU64::new(call.guild))
        {
            let _ = songbird.remove(guild).await;
        }
        let why = call.dropped.clone().unwrap_or_else(|| "ended".into());
        ended(&shared, &call, &why, "the connection");
    }
}

/// A cut or a resumed stop (theseus-qb8o): a cut's note for the next voice
/// turn, and each one's row on the place's session, its metric, and a
/// resume's count in health.
fn held(shared: &Shared, serial: u64, place: &VoicePlace, e: &Event) {
    let sid = session(shared, place);
    match *e {
        Event::Cut {
            what,
            why,
            sentences,
            heard,
            into,
            ..
        } => {
            with_call(shared, serial, |c| c.notes.cut(e));
            let why = cut_why(why);
            shared.core.binding_ledger(
                LedgerKind::VoiceCut,
                sid.as_deref(),
                json!({"what": spoken(what), "why": why, "sentences": sentences,
                    "heard": heard, "into_ms": into.as_millis() as u64}),
            );
            shared.core.telemetry().record_voice_cut(why);
        }
        Event::Resumed { what, why, held } => {
            update(shared, |s| s.resumes += 1);
            let why = heard_as(why);
            shared.core.binding_ledger(
                LedgerKind::VoiceResumed,
                sid.as_deref(),
                json!({"what": spoken(what), "why": why, "held_ms": held.as_millis() as u64}),
            );
            shared.core.telemetry().record_voice_resumed(why);
        }
        _ => {}
    }
}

/// `f` on call `serial`, while it is the one joined.
fn with_call(shared: &Shared, serial: u64, f: impl FnOnce(&mut Call)) {
    if let Some(c) = shared
        .voice
        .call
        .lock()
        .unwrap()
        .as_mut()
        .filter(|c| c.serial == serial)
    {
        f(c);
    }
}

fn channel_of(place: &VoicePlace) -> u64 {
    place
        .key
        .strip_prefix("channel:")
        .and_then(|c| c.parse().ok())
        .unwrap_or(0)
}

fn spoken(what: Spoken) -> String {
    match what {
        Spoken::Reply(t) => format!("reply {}", t.0),
        Spoken::Report => "report".into(),
        Spoken::Acknowledgment => "acknowledgment".into(),
    }
}

fn cut_why(why: CutWhy) -> &'static str {
    match why {
        CutWhy::Words => "words",
        CutWhy::Superseded => "superseded",
        CutWhy::CallEnded => "call_ended",
    }
}

fn heard_as(h: HeardAs) -> &'static str {
    match h {
        HeardAs::Words => "words",
        HeardAs::Wordless => "wordless",
        HeardAs::Echo => "echo",
        HeardAs::Backchannel => "backchannel",
        HeardAs::Resume => "resume",
    }
}

/// What an utterance was said over: a sentence's index in what was being
/// said, or the turn whose reply was being prepared; null when neither.
fn over(o: Option<&Over>) -> serde_json::Value {
    match o {
        Some(Over::Saying { what, sentence, .. }) => {
            json!({"saying": spoken(*what), "sentence": sentence})
        }
        Some(Over::Preparing { turn }) => json!({"preparing": format!("reply {}", turn.0)}),
        None => serde_json::Value::Null,
    }
}

/// The voice place's session now (`/new` may have changed it).
fn session(shared: &Shared, place: &VoicePlace) -> Option<String> {
    shared.core.outbox.place_session(&place.key).ok().flatten()
}

/// A call that failed: a speech call's, or the connection's, which ends the
/// call and leaves the session in text.
fn failed(shared: &Shared, serial: u64, place: &VoicePlace, what: &Failure, error: &SpeechError) {
    let what_s = match what {
        Failure::Transcribe(s) => format!("transcribe {}", s.0),
        Failure::Synthesize(w) => format!("synthesize {}", spoken(*w)),
        Failure::Connection => "connection".into(),
    };
    update(shared, |s| {
        s.failures += 1;
        s.last_error = Some(error.0.clone());
    });
    let sid = session(shared, place);
    shared.core.binding_ledger(
        LedgerKind::VoiceFailed,
        sid.as_deref(),
        json!({"what": what_s, "place": place.label, "error": error.0}),
    );
    if matches!(what, Failure::Connection) {
        if let Some(c) = shared
            .voice
            .call
            .lock()
            .unwrap()
            .as_mut()
            .filter(|c| c.serial == serial)
        {
            c.dropped = Some(format!("dropped: {}", error.0));
        }
        if let Some(sid) = sid {
            let text = format!(
                "🔇 The voice connection dropped ({}). The conversation goes on here in text; `/join` \
                 to talk again.",
                error.0
            );
            let body = json!({"kind": "notice", "text": text});
            let target = format!("discord:{}", place.key);
            if let Err(e) = shared.core.outbox.post(&sid, "", &target, body) {
                shared
                    .board
                    .error("post notice", Some(&sid), format!("{e:#}"));
            }
            shared.wake_lanes();
        }
    }
}

/// Book a speech call to the voice place's session (45b), off the runtime's
/// workers: a booking is a frame, so an fsync.
fn book(
    shared: &Arc<Shared>,
    place: &VoicePlace,
    kind: SpeechKind,
    usage: theseus_voice::Usage,
    latency: Duration,
    detail: serde_json::Value,
) {
    let Some(sid) = session(shared, place) else {
        return;
    };
    let shared = Arc::clone(shared);
    tokio::task::spawn_blocking(move || {
        let call = SpeechCall {
            kind,
            provider: &usage.provider,
            model: &usage.model,
            audio: usage.audio,
            chars: usage.chars,
            latency,
            detail,
        };
        match shared.core.book_speech(&sid, &call) {
            Ok(b) => {
                if let Some(cost) = b.cost {
                    update(&shared, |s| s.spend_micros += cost);
                }
            }
            Err(e) => shared
                .board
                .error("book speech", Some(&sid), format!("{e:#}")),
        }
    });
}

/// Deepgram behind the session's spend limit (45b): a call whose estimate
/// does not fit what the session may still spend is not made.
struct Metered {
    inner: DeepgramSpeech,
    core: Weak<Core>,
    /// The voice place's key: its session is read at each call.
    key: String,
}

impl Metered {
    fn fits(
        &self,
        kind: SpeechKind,
        model: &str,
        audio: Duration,
        chars: usize,
    ) -> Result<(), SpeechError> {
        let Some(core) = self.core.upgrade() else {
            return Err(SpeechError("the daemon is stopping".into()));
        };
        let call = SpeechCall {
            kind,
            provider: theseus_voice::deepgram::PROVIDER,
            model,
            audio,
            chars,
            latency: Duration::ZERO,
            detail: serde_json::Value::Null,
        };
        let (Some(estimate), Ok(Some(sid))) = (call.cost(), core.outbox.place_session(&self.key))
        else {
            return Ok(());
        };
        core.speech_fits(&sid, estimate).map_err(SpeechError)
    }
}

impl Speech for Metered {
    fn transcribe<'a>(
        &'a self,
        speaker: Speaker,
        audio: &'a theseus_voice::Audio,
    ) -> SpeechFuture<'a, Transcript> {
        Box::pin(async move {
            let model = &self.inner.settings().stt_model;
            self.fits(SpeechKind::Transcribed, model, audio.duration(), 0)?;
            self.inner.transcribe(speaker, audio).await
        })
    }

    fn synthesize<'a>(&'a self, text: &'a str) -> SpeechFuture<'a, Synthesis> {
        Box::pin(async move {
            let model = &self.inner.settings().tts_voice;
            self.fits(
                SpeechKind::Synthesized,
                model,
                Duration::ZERO,
                text.chars().count(),
            )?;
            self.inner.synthesize(text).await
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use serde_json::json;
    use theseus_core::secrets::{Secret, SecretBoard};
    use theseus_core::Core;
    use theseus_protocol::{DiscordOrigin, LedgerTailParams, LedgerTailResult};
    use theseus_voice::{
        Command, Failure, HeardAs, Speaker, SpeechError, TurnId, Usage, Utterance,
    };
    use tokio::sync::mpsc;
    use twilight_model::id::Id;

    use super::super::tests::{core_scripted, core_with, place_for_tests};
    use super::*;
    use crate::bindings::Bindings;

    pub(super) const EDDIE: u64 = 100_000_000_000_000_042;
    pub(super) const LOUNGE: u64 = 123_456_789_012_345_678;

    /// The bindings: a private voice channel with Eddie, and a shared one.
    fn bindings() -> Bindings {
        Bindings::parse(&format!(
            "guild_id = \"100000000000000001\"\n\
             [[channel]]\nid = \"{LOUNGE}\"\nname = \"lounge\"\nusers = [\"{EDDIE}\"]\nprivate = true\nvoice = true\n\
             [[channel]]\nid = \"223456789012345678\"\nname = \"den\"\nusers = [\"100000000000000007\"]\nvoice = true\n"
        ))
        .unwrap()
    }

    fn eddie() -> Option<DiscordOrigin> {
        Some(DiscordOrigin {
            user_id: EDDIE.to_string(),
            channel_id: "43".into(),
            guild_id: None,
        })
    }

    /// The test place of `core`, its shared state's voice from `bindings`.
    pub(super) fn place(core: &Arc<Core>, sid: &str) -> (Place, mpsc::UnboundedReceiver<PlaceMsg>) {
        let (mut place, rx) = place_for_tests(core, sid);
        let mut shared = Arc::try_unwrap(place.shared).ok().expect("the test's own");
        shared.voice = Voice::new(&core.cfg.voice, &bindings());
        place.shared = Arc::new(shared);
        (place, rx)
    }

    /// `p` in the DM with Eddie, which the binding binds as the bindings
    /// file's `[[dm]]` would: with no owner named, its person is the owner
    /// (the place rule, theseus-zmgb).
    fn in_eddies_dm(core: &Core, p: &mut Place) {
        core.bind_places(vec![theseus_core::places::BoundPlace {
            target: format!("discord:dm:{EDDIE}"),
            name: "DM".into(),
            private: false,
            ..Default::default()
        }]);
        p.target = format!("discord:dm:{EDDIE}");
    }

    fn secrets(key: Option<&str>) -> Arc<SecretBoard> {
        let board = SecretBoard::new(["deepgram_api_key".to_string()], Instant::now());
        if let Some(k) = key {
            board.publish(
                BTreeMap::from([("deepgram_api_key".to_string(), Ok(Secret::new(k.into())))]),
                "test",
            );
        }
        board
    }

    /// Theseus joins only when invited, from a private place, by one of its
    /// users, into a voice channel the bindings file binds that lists them;
    /// and only with the key, and the gateway up. Each refusal says why, and
    /// none reaches Discord.
    #[tokio::test]
    async fn join_answers_only_an_invitation_from_a_private_place() {
        let d = tempfile::tempdir().unwrap();
        let off = core_with(d.path(), SecretBoard::empty(), |_| {});
        let (p, _rx) = place(&off, "ses_off");
        let said = p.join(None, eddie(), "discord:eddie").await;
        assert!(said.starts_with("Voice is off"), "{said}");

        let d = tempfile::tempdir().unwrap();
        let core = core_with(d.path(), secrets(None), |c| c.voice.enabled = true);
        let (mut p, _rx) = place(&core, "ses_dm");
        // A guild channel the file does not bind private is shared.
        p.target = "discord:channel:555".into();
        let said = p.join(None, eddie(), "discord:eddie").await;
        assert!(
            said.contains("only when invited from a private place"),
            "{said}"
        );
        // The DM with Eddie is private (no owner named: the bound DM's person
        // is).
        in_eddies_dm(&core, &mut p);
        let said = p.join(Some(999), eddie(), "discord:eddie").await;
        assert!(
            said.contains("not a voice channel the bindings file binds"),
            "{said}"
        );
        let said = p
            .join(Some(223_456_789_012_345_678), eddie(), "discord:eddie")
            .await;
        assert_eq!(said, "#den does not list you, so I won't join it.");
        // With no channel named, the only one Eddie's place could mean is
        // not chosen: there are two, and he is in neither.
        let said = p.join(None, eddie(), "discord:eddie").await;
        assert!(said.starts_with("Which voice channel?"), "{said}");
        let said = p.join(Some(LOUNGE), eddie(), "discord:eddie").await;
        assert!(said.contains("is still resolving"), "{said}");

        let d = tempfile::tempdir().unwrap();
        let core = core_with(d.path(), secrets(Some("tv-deepgram-7f3a9c")), |c| {
            c.voice.enabled = true
        });
        let (mut p, _rx) = place(&core, "ses_dm");
        in_eddies_dm(&core, &mut p);
        let said = p.join(Some(LOUNGE), eddie(), "discord:eddie").await;
        assert_eq!(said, "The gateway isn't up yet; try again in a moment.");
        assert!(p.shared.voice.joined().is_none(), "nothing joined");
    }

    /// A call, as a join leaves it, for a place whose engine is `commands`.
    pub(super) fn joined(p: &Place, commands: mpsc::UnboundedSender<Command>) {
        *p.shared.voice.call.lock().unwrap() = Some(Call {
            serial: 1,
            channel: LOUNGE,
            guild: 100_000_000_000_000_001,
            key: p.key.clone(),
            label: "#lounge".into(),
            commands,
            joined: std::time::Instant::now(),
            inflight: false,
            own: HashSet::new(),
            waiting: VecDeque::new(),
            dropped: None,
            notes: Notes::default(),
        });
    }

    pub(super) fn heard(text: &str) -> Utterance {
        Utterance {
            speaker: Speaker(EDDIE),
            started: Duration::ZERO,
            length: Duration::from_secs(1),
            closed: Duration::from_millis(1700),
            text: text.into(),
            usage: Usage {
                provider: "deepgram".into(),
                model: "nova-3".into(),
                audio: Duration::from_secs(1),
                chars: text.len(),
            },
            latency: Duration::from_millis(300),
            over: None,
            heard_as: HeardAs::Words,
        }
    }

    /// A turn's start, as the session's watch says it.
    fn turn_started(sid: &str, id: &str, continuation: bool) -> CoreEvent {
        CoreEvent::TurnStarted(theseus_protocol::TurnStarted {
            session_id: sid.into(),
            turn_id: id.into(),
            continuation,
            ..Default::default()
        })
    }

    /// A turn's end with its reply, as the session's watch says it.
    fn turn_ended(sid: &str, id: &str, output: &str) -> CoreEvent {
        let r: theseus_protocol::TurnSubmitResult = serde_json::from_value(json!({
            "session_id": sid, "turn_id": id, "loops": 1, "output": output,
            "stop_reason": "end_turn", "model": "m",
            "usage": {"input_tokens": 0, "output_tokens": 0}, "elapsed_ms": 0
        }))
        .unwrap();
        CoreEvent::TurnEnded(r)
    }

    /// The text of each post waiting for `target`.
    fn posts(core: &Arc<Core>, target: &str) -> Vec<String> {
        core.outbox
            .open_for(target)
            .iter()
            .map(|a| {
                theseus_core::outbox::body_of(a)["text"]
                    .as_str()
                    .unwrap_or("")
                    .to_string()
            })
            .collect()
    }

    /// An utterance's turn is a turn of the voice place's session, authored
    /// by its speaker; the place's text shows the transcript; the reply goes
    /// back to the engine to be spoken; and that turn's end is not spoken a
    /// second time, while any other turn's reply is, at the next pause.
    #[tokio::test]
    async fn a_voice_turn_is_a_turn_of_the_places_session_authored_by_its_speaker() {
        use theseus_core::provider::Scripted;
        let d = tempfile::tempdir().unwrap();
        let core = core_scripted(d.path(), vec![Scripted::text("Hello, Eddie.")]);
        let rec = theseus_core::session::SessionRecord::new(
            theseus_protocol::SessionKind::Conversation,
            None,
        );
        let sid = rec.session_id.clone();
        core.store.put_session(&sid, &rec).unwrap();
        let (mut p, mut rx) = place(&core, &sid);
        p.key = format!("channel:{LOUNGE}");
        p.target = format!("discord:channel:{LOUNGE}");
        p.channel = Some(Id::new(LOUNGE));
        p.shared
            .voice
            .names
            .lock()
            .unwrap()
            .insert(EDDIE, "eddie".into());
        let (commands, mut engine) = mpsc::unbounded_channel();
        joined(&p, commands);
        p.voice_turn(VoiceTurn {
            serial: 1,
            turn: TurnId(0),
            utterances: vec![heard("What changed today?")],
        });
        assert!(p.inflight);
        // Its turn starts, not a continuation, while its submit is in flight
        // (a test's place has no route for the session's events, so the test
        // hands them over; on this current-thread runtime the submit has not
        // run yet).
        p.voice_heard(&turn_started(&sid, "turn_voice", false));
        let done = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some(PlaceMsg::SubmitDone(r)) = rx.recv().await {
                    break r;
                }
            }
        })
        .await
        .unwrap();
        done.unwrap();
        assert_eq!(
            engine.try_recv().unwrap(),
            Command::Reply {
                turn: TurnId(0),
                text: "Hello, Eddie.".into()
            }
        );
        // Its end says nothing more: the submit answered it.
        p.voice_heard(&turn_ended(&sid, "turn_voice", "Hello, Eddie."));
        assert!(engine.try_recv().is_err(), "said once");
        // The session's input is the transcript, by Eddie.
        let rec: theseus_core::session::SessionRecord =
            core.store.get_session(&sid).unwrap().unwrap();
        assert_eq!(rec.turns, 1);
        let history: theseus_protocol::SessionHistoryResult = p
            .shared
            .rpc
            .call(
                theseus_protocol::method::SESSION_HISTORY,
                json!({"session_id": sid}),
            )
            .await
            .unwrap();
        let input = history
            .nodes
            .iter()
            .find(|n| n.kind == "user_message" || n.text.contains("What changed"))
            .expect("the input node");
        assert_eq!(input.text, format!("{FRAMING}\n🎙️ What changed today?"));
        assert_eq!(input.author.as_deref(), Some("discord:eddie"));
        // The place's text has the transcript.
        let posts = posts(&core, &p.target);
        assert!(
            posts
                .iter()
                .any(|t| t == "🎙️ **eddie**: What changed today?"),
            "{posts:?}"
        );
        // Another turn's reply in the place (a wake's: a continuation) is
        // spoken at the next pause.
        p.voice_heard(&turn_started(&sid, "turn_wake", true));
        p.voice_heard(&turn_ended(&sid, "turn_wake", "The deploy finished."));
        assert_eq!(
            engine.try_recv().unwrap(),
            Command::Report {
                text: "The deploy finished.".into()
            }
        );
    }

    /// A dropped connection is a `voice.failed` row with its reason, and a
    /// notice in the place's text that the conversation goes on there.
    #[tokio::test]
    async fn a_dropped_connection_is_a_failed_row_and_a_notice() {
        let d = tempfile::tempdir().unwrap();
        let core = core_with(d.path(), SecretBoard::empty(), |c| c.voice.enabled = true);
        let rec = theseus_core::session::SessionRecord::new(
            theseus_protocol::SessionKind::Conversation,
            None,
        );
        let sid = rec.session_id.clone();
        core.store.put_session(&sid, &rec).unwrap();
        let key = format!("channel:{LOUNGE}");
        core.outbox.bind_place(&key, &sid).unwrap();
        let (mut p, _rx) = place(&core, &sid);
        p.key = key.clone();
        let (commands, _engine) = mpsc::unbounded_channel();
        joined(&p, commands);
        let lounge = VoicePlace {
            key,
            label: "#lounge".into(),
            users: vec![EDDIE],
            guild: 100_000_000_000_000_001,
        };
        failed(
            &p.shared,
            1,
            &lounge,
            &Failure::Connection,
            &SpeechError("WsClosed(Some(SessionTimeout))".into()),
        );
        let rows: LedgerTailResult = p
            .shared
            .rpc
            .call(
                theseus_protocol::method::LEDGER_TAIL,
                LedgerTailParams {
                    n: Some(50),
                    kind: Some("voice.failed".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(rows.rows.len(), 1);
        assert_eq!(rows.rows[0].data["what"], "connection");
        assert_eq!(rows.rows[0].data["error"], "WsClosed(Some(SessionTimeout))");
        assert_eq!(rows.rows[0].session_id.as_deref(), Some(sid.as_str()));
        let posts = posts(&core, &format!("discord:channel:{LOUNGE}"));
        assert!(
            posts[0]
                .starts_with("🔇 The voice connection dropped (WsClosed(Some(SessionTimeout)))"),
            "{posts:?}"
        );
        let status = p.shared.voice.status();
        assert_eq!(
            (status.failures, status.last_error.as_deref()),
            (1, Some("WsClosed(Some(SessionTimeout))"))
        );
    }

    /// A core whose sessions may spend `limit_usd`, and a voice place on a
    /// session opened there, bound to the lounge.
    async fn spending(dir: &std::path::Path, limit_usd: f64) -> (Arc<Core>, Place, String) {
        let core = core_with(dir, SecretBoard::empty(), |c| {
            c.voice.enabled = true;
            c.kernel.spend_limit_usd = limit_usd;
        });
        let (p, _rx) = place(&core, "ses_unused");
        let info: theseus_protocol::SessionInfo = p
            .shared
            .rpc
            .call(theseus_protocol::method::SESSION_OPEN, json!({}))
            .await
            .unwrap();
        let sid = info.session_id;
        core.outbox
            .bind_place(&format!("channel:{LOUNGE}"), &sid)
            .unwrap();
        let (mut p, _rx) = place(&core, &sid);
        p.key = format!("channel:{LOUNGE}");
        (core, p, sid)
    }

    /// Speech is spend (45b): the engine's transcriptions and syntheses
    /// become `speech.transcribed` and `speech.synthesized` rows, each with
    /// its cost booked to the voice place's session, and health's voice
    /// block counts the spend.
    #[tokio::test]
    async fn each_speech_call_is_a_row_with_its_cost_on_the_places_session() {
        let d = tempfile::tempdir().unwrap();
        let (core, p, sid) = spending(d.path(), 100.0).await;
        let lounge = VoicePlace {
            key: p.key.clone(),
            label: "#lounge".into(),
            users: vec![EDDIE],
            guild: 100_000_000_000_000_001,
        };
        let (tx, events) = mpsc::unbounded_channel();
        let said = Usage {
            provider: "deepgram".into(),
            model: "aura-2-andromeda-en".into(),
            audio: Duration::from_millis(2_160),
            chars: 44,
        };
        // 3 s heard: 215 µ$; 44 characters said: 1,320 µ$.
        let mut utterance = heard("What changed today?");
        utterance.usage.audio = Duration::from_secs(3);
        tx.send(Event::Utterance(utterance)).unwrap();
        tx.send(Event::Synthesized {
            what: Spoken::Reply(TurnId(0)),
            usage: said,
            latency: Duration::from_millis(1_034),
        })
        .unwrap();
        drop(tx);
        pump(Arc::clone(&p.shared), 1, lounge, events).await;
        // The bookings run off the runtime's workers: wait for both rows.
        let rows = |kind: &str| {
            let tail: Vec<(u64, theseus_core::ledger::LedgerRow)> =
                core.store.ledger_tail(200).unwrap();
            tail.into_iter()
                .filter(|(_, r)| r.kind == kind && r.session_id.as_deref() == Some(sid.as_str()))
                .map(|(_, r)| r.data)
                .collect::<Vec<_>>()
        };
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while rows("speech.synthesized").is_empty() || rows("speech.transcribed").is_empty() {
            assert!(std::time::Instant::now() < deadline, "the bookings came");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let t = &rows("speech.transcribed")[0];
        assert_eq!(
            (t["model"].as_str(), t["cost_usd"].as_f64()),
            (Some("nova-3"), Some(0.000215))
        );
        assert_eq!(t["speaker"], EDDIE.to_string());
        let s = &rows("speech.synthesized")[0];
        assert_eq!(s["cost_usd"].as_f64(), Some(0.00132));
        assert_eq!(
            (s["chars"].as_u64(), s["what"].as_str()),
            (Some(44), Some("reply 0"))
        );
        let rec: theseus_core::session::SessionRecord =
            core.store.get_session(&sid).unwrap().unwrap();
        let e = core
            .kernel
            .execution(rec.execution_id.as_deref().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(e.budget.spent_micros, 1_535);
        // A booking counts in the status once its row is written, on the
        // same worker: wait for the count as for the rows (theseus-zuz9).
        let status = loop {
            let status = p.shared.voice.status();
            if status.spend_micros >= 1_535 || std::time::Instant::now() >= deadline {
                break status;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        assert_eq!(
            (status.utterances, status.sentences, status.spend_micros),
            (1, 1, 1_535)
        );
    }

    /// A speech call that would pass the session's limit is not made: the
    /// engine gets a `SpeechError` that says how to go on, and Deepgram is
    /// never asked (its address here is a closed port, bounded).
    #[tokio::test]
    async fn a_call_past_the_sessions_limit_is_not_made() {
        let d = tempfile::tempdir().unwrap();
        let (core, p, _sid) = spending(d.path(), 0.0).await;
        let settings = DeepgramSettings {
            api_base: "http://127.0.0.1:9".into(),
            connect_timeout: Duration::from_millis(500),
            timeout: Duration::from_secs(1),
            ..DeepgramSettings::default()
        };
        let metered = Metered {
            inner: DeepgramSpeech::new("tv-deepgram-7f3a9c", settings).unwrap(),
            core: Arc::downgrade(&core),
            key: p.key.clone(),
        };
        let t0 = std::time::Instant::now();
        let e = metered
            .synthesize("The deploy finished.")
            .await
            .unwrap_err();
        assert!(e.0.contains("at its spend limit"), "{e}");
        let audio = theseus_voice::Audio::silence(Duration::from_secs(2));
        let e = metered
            .transcribe(Speaker(EDDIE), &audio)
            .await
            .unwrap_err();
        assert!(e.0.contains("reset its spend to go on"), "{e}");
        assert!(t0.elapsed() < Duration::from_millis(400), "never sent");
    }

    #[test]
    fn join_names_a_voice_channel_and_leave_nothing() {
        let cmds = commands();
        let names: Vec<&str> = cmds.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["join", "leave"]);
        // The binding registers them after its own ten (`/extensions` is the tenth).
        let all = super::super::commands();
        let last: Vec<&str> = all[10..].iter().map(|c| c.name.as_str()).collect();
        assert_eq!((all.len(), last), (12, vec!["join", "leave"]));
        let join = &cmds[0];
        assert_eq!(join.options.len(), 1);
        assert_eq!(join.options[0].name, "channel");
        assert_eq!(join.options[0].required, Some(false));
        assert_eq!(
            join.options[0].channel_types.as_deref(),
            Some(&[ChannelType::GuildVoice][..])
        );
        assert!(cmds[1].options.is_empty());
        assert!(cmds.iter().all(|c| c.description.chars().count() <= 100));
    }
}
