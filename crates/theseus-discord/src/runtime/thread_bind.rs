//! A thread on Discord (theseus-sjzm): a channel or a Discord thread bound as
//! a thread's place, the owner's DM that addresses a thread, and the pointer
//! that keeps a thread's own history whole. A child of `runtime`, apart from
//! it for the shape budget's file ceiling.
//!
//! - **A bound place** runs as the thread: its messages go to the thread's
//!   session (`Core::thread_place_session`, judged by the place: private
//!   only), and that session's posts go to the place (`Outbox::place_bound`,
//!   in memory, asserted again at each start). The place's own plain session
//!   is left as it was, not superseded, so `/thread unbind` gives it back.
//!   The binding is the thread's `surfaces` (`discord:channel:<id>`), set by
//!   `/thread bind` (the core judges: the owner, from a private place) and
//!   by `[[discord.thread]]`; one Discord surface to a thread.
//! - **A DM that addresses a thread** (a reply to a message that carries the
//!   thread's tag, or a message that begins `thread:<slug>`) runs in the
//!   thread's session and is answered in the DM where it was spoken
//!   (`TurnSubmitParams.speaking.answer_at`); the thread's own surface gets a
//!   one-line pointer at it, so its history is whole.
//! - A thread's reply in the DM carries the thread's title as its first line
//!   (`render::thread::tag`); in a bound place it carries none.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::json;
use theseus_core::places::BoundPlace;
use theseus_protocol::threads::{
    channel_surface, surface_channel, Speaking, ThreadChanged, ThreadEditParams, ThreadNameParams,
    ThreadRow, ThreadState,
};
use theseus_protocol::{method, DiscordOrigin, TurnSubmitParams, TurnSubmitResult};
use tokio::sync::mpsc;
use twilight_model::channel::Message;
use twilight_model::id::Id;

use super::{Inbound, Pending, Place, PlaceMsg, Shared};
use crate::bindings::ChannelBinding;

/// How much of the DM's message the pointer quotes.
pub(super) const POINTER_CHARS: usize = 80;

/// What the binding knows of threads: which channel runs which thread, the
/// pointers waiting for a thread that has no surface yet, and the sender of
/// the Discord-thread syncer.
#[derive(Default)]
pub(crate) struct State {
    inner: Mutex<Inner>,
    pub(super) sync: OnceLock<mpsc::UnboundedSender<ThreadChanged>>,
}

#[derive(Default)]
struct Inner {
    by_channel: HashMap<u64, String>,
    pointers: HashMap<String, Vec<String>>,
}

impl State {
    /// The thread a channel runs, if it runs one.
    pub(crate) fn slug_of(&self, channel: u64) -> Option<String> {
        self.inner.lock().unwrap().by_channel.get(&channel).cloned()
    }

    /// The channel that runs `slug`, if one does.
    pub(super) fn channel_of(&self, slug: &str) -> Option<u64> {
        let i = self.inner.lock().unwrap();
        i.by_channel
            .iter()
            .find(|(_, s)| *s == slug)
            .map(|(c, _)| *c)
    }

    pub(super) fn bind(&self, channel: u64, slug: &str) {
        let mut i = self.inner.lock().unwrap();
        i.by_channel.retain(|_, s| s != slug);
        i.by_channel.insert(channel, slug.to_string());
    }

    pub(super) fn unbind(&self, channel: u64) -> Option<String> {
        self.inner.lock().unwrap().by_channel.remove(&channel)
    }

    fn keep_pointer(&self, slug: &str, text: String) {
        let mut i = self.inner.lock().unwrap();
        let p = i.pointers.entry(slug.to_string()).or_default();
        p.push(text);
        if p.len() > 8 {
            p.remove(0);
        }
    }

    /// The pointers that waited for `slug`'s surface.
    pub(super) fn take_pointers(&self, slug: &str) -> Vec<String> {
        self.inner
            .lock()
            .unwrap()
            .pointers
            .remove(slug)
            .unwrap_or_default()
    }
}

/// What a thread-bound place is told besides messages.
pub(super) enum ThreadMsg {
    /// Run as `slug` from now on: take its session.
    Adopt(String),
    /// Run as itself again: give the thread's session back.
    Release,
    /// A thread turn spoken here failed: say so.
    Failed(String),
}

/// A message's first line, at most `POINTER_CHARS` characters, ending in an
/// ellipsis when cut.
pub(super) fn quoted(text: &str) -> String {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
    match line.chars().count() > POINTER_CHARS {
        true => format!("{}…", line.chars().take(POINTER_CHARS).collect::<String>()),
        false => line,
    }
}

/// The pointer's words: `-# from the DM: <quoted> · <link>`.
pub(super) fn pointer(text: &str, dm_channel: Option<u64>, message: u64) -> String {
    let said = quoted(text);
    let said = if said.is_empty() {
        "(a file)".to_string()
    } else {
        // A pointer is a footnote: no mention, no markup of the owner's.
        said.replace('`', "'").replace("@", "@\u{200b}")
    };
    match dm_channel {
        Some(c) => {
            format!("-# from the DM: {said} · https://discord.com/channels/@me/{c}/{message}")
        }
        None => format!("-# from the DM: {said}"),
    }
}

/// `thread:<slug>` at the head of a message, then the rest of it.
pub(super) fn explicit(text: &str) -> Option<(String, String)> {
    let rest = text.trim_start().strip_prefix("thread:")?;
    let (slug, tail) = match rest.find(char::is_whitespace) {
        Some(i) => (&rest[..i], rest[i..].trim_start()),
        None => (rest, ""),
    };
    (theseus_protocol::threads::slug_ok(slug) && !tail.is_empty())
        .then(|| (slug.to_string(), tail.to_string()))
}

impl Shared {
    /// The thread a reply to `message` addresses (the joiner of
    /// `thread-route` calls this for its rule (c)): the thread the turn that
    /// message answered ran in, when that turn ran in a thread and the
    /// message carried its tag.
    pub(crate) fn thread_of_reply(&self, message: u64) -> Option<String> {
        self.replies.thread_of(message)
    }

    /// The thread a message in the owner's DM addresses, and its text without
    /// the address: a reply to a message that carries a thread's tag, or
    /// `thread:<slug>` first. Never in a guild channel, and only a thread that
    /// exists: a paused one is addressed too, and the core's refusal, said in
    /// the DM, tells how to resume it.
    pub(super) fn addressed_thread(&self, m: &Message, text: String) -> (Option<String>, String) {
        if m.guild_id.is_some() {
            return (None, text);
        }
        if let Some((slug, rest)) = explicit(&text) {
            if self.core.threads().get(&slug).is_some() {
                return (Some(slug), rest);
            }
        }
        let replied = m
            .reference
            .as_ref()
            .and_then(|r| r.message_id)
            .or_else(|| m.referenced_message.as_ref().map(|r| r.id));
        let slug = replied
            .and_then(|r| self.thread_of_reply(r.get()))
            .filter(|s| self.core.threads().get(s).is_some());
        (slug, text)
    }

    /// A channel binding for a place the binding makes while it runs (a
    /// Discord thread): private by the owner's word, driven by the owners.
    fn dynamic_binding(&self, guild: Option<u64>, channel: u64, label: &str) -> ChannelBinding {
        let owners = self.core.runner.place_rule.owners(&self.core.cfg);
        let users = owners
            .iter()
            .filter_map(|o| o.strip_prefix("discord:"))
            .map(str::to_string)
            .collect();
        ChannelBinding {
            guild: guild.map(|g| g.to_string()).unwrap_or_default(),
            id: channel.to_string(),
            name: Some(label.to_string()),
            users,
            mention_only: false,
            private: Some(true),
            voice: false,
            ceiling: None,
            show_tools: true,
            show_thinking: true,
            silent: None,
            ping_window_secs: None,
        }
    }

    /// A lane for a place made while the binding runs, and the core told it
    /// is private. Nothing waits on Discord.
    pub(super) fn dynamic_lane(
        self: &Arc<Self>,
        guild: Option<u64>,
        channel: u64,
        label: &str,
    ) -> anyhow::Result<ChannelBinding> {
        let b = self.dynamic_binding(guild, channel, label);
        let target = format!("discord:channel:{channel}");
        if self.lane(&target).is_none() {
            self.start_channel_lane(&b)?;
        }
        self.core.runner.place_rule.bind_more(BoundPlace {
            target,
            name: b.label(),
            private: true,
            guild: guild.map(|g| g.to_string()),
            ceiling: None,
        });
        Ok(b)
    }

    /// Lanes for the channels threads run that the bindings file does not
    /// name, before the start refuses the posts of places it has no lane for:
    /// a reply to a Discord thread that was in flight at a restart still goes.
    pub(super) fn revive_thread_lanes(self: &Arc<Self>) {
        for (channel, slug) in self.stored_channels() {
            self.threads.bind(channel, &slug);
            let known = self
                .routes
                .lock()
                .unwrap()
                .by_channel
                .contains_key(&channel);
            if !known && self.lane(&format!("discord:channel:{channel}")).is_none() {
                let label = self.thread_label(&slug);
                if let Err(e) = self.dynamic_lane(None, channel, &label) {
                    self.board.error("thread lane", None, format!("{e:#}"));
                }
            }
        }
    }

    /// Each live thread's Discord channel, from its surfaces.
    fn stored_channels(&self) -> Vec<(u64, String)> {
        let board = self.core.threads();
        board
            .all()
            .filter(|t| t.state != ThreadState::Done)
            .filter_map(|t| {
                let c = t.surfaces.iter().find_map(|s| surface_channel(s))?;
                Some((c, t.slug.clone()))
            })
            .collect()
    }

    fn thread_label(&self, slug: &str) -> String {
        let board = self.core.threads();
        let title = board.get(slug).map_or(slug, |t| t.title.as_str());
        format!("#{}", title.chars().take(60).collect::<String>())
    }

    /// After the places start: the Discord thread syncer, each channel that
    /// runs a thread with its place told to take the thread's session, and
    /// the config's bindings applied for the threads that exist.
    pub(super) fn start_threads(self: &Arc<Self>) {
        let (tx, rx) = mpsc::unbounded_channel();
        if self.threads.sync.set(tx).is_ok() {
            tokio::spawn(super::guild_thread::run(self.clone(), rx));
        }
        let s = self.clone();
        tokio::spawn(async move {
            for (channel, slug) in s.stored_channels() {
                let listed = s.routes.lock().unwrap().by_channel.contains_key(&channel);
                if !listed {
                    let label = s.thread_label(&slug);
                    let started = match s.dynamic_lane(None, channel, &label) {
                        Ok(b) => s.start_channel(&b).await,
                        Err(e) => Err(e),
                    };
                    if let Err(e) = started {
                        s.board.error("thread place", None, format!("{e:#}"));
                        continue;
                    }
                }
                s.tell_place(channel, ThreadMsg::Adopt(slug));
            }
            for b in &s.core.cfg.discord.thread {
                s.apply_config(&b.thread);
            }
        });
    }

    /// `[[discord.thread]]` for `slug`, once the thread exists: its channel
    /// (a private place the bindings file names) runs it, and the thread's
    /// surfaces say so. A binding that cannot be applied is said on the board.
    pub(super) fn apply_config(self: &Arc<Self>, slug: &str) {
        let Some(b) = self
            .core
            .cfg
            .discord
            .thread
            .iter()
            .find(|b| b.thread == slug)
        else {
            return;
        };
        let Ok(channel) = b.channel.parse::<u64>() else {
            return;
        };
        if self.threads.channel_of(slug) == Some(channel) {
            return;
        }
        let Some(t) = self.core.threads().get(slug).cloned() else {
            return;
        };
        let target = format!("discord:channel:{channel}");
        let surface = channel_surface(channel);
        let listed = self
            .routes
            .lock()
            .unwrap()
            .by_channel
            .contains_key(&channel);
        let class = self
            .core
            .runner
            .place_rule
            .class(&self.core.cfg, Some(&target));
        let why = if !listed {
            Some("is not a place the bindings file names")
        } else if class != theseus_core::places::PlaceClass::Private {
            Some("is a shared place, and a thread is private always")
        } else if t.state == ThreadState::Done {
            Some("is done")
        } else if t
            .surfaces
            .iter()
            .any(|s| s.starts_with("discord:") && *s != surface)
        {
            Some("is shown on another Discord place already")
        } else {
            None
        };
        if let Some(why) = why {
            self.board.error(
                "discord.thread",
                None,
                format!("the thread {slug} is not bound to {channel}: it {why}"),
            );
            return;
        }
        if let Err(e) = self
            .core
            .thread_surfaces(slug, Some(&surface), None, "config")
        {
            self.board.error("discord.thread", None, format!("{e:#}"));
            return;
        }
        self.threads.bind(channel, slug);
        self.tell_place(channel, ThreadMsg::Adopt(slug.to_string()));
    }

    /// Tell the place of `channel` something about its thread.
    pub(super) fn tell_place(&self, channel: u64, m: ThreadMsg) {
        let tx = self
            .routes
            .lock()
            .unwrap()
            .by_channel
            .get(&channel)
            .cloned();
        if let Some(tx) = tx {
            let _ = tx.send(PlaceMsg::Thread(m));
        }
    }

    /// Say a pointer to the thread's surface, or keep it for the surface the
    /// thread will have.
    fn post_pointer(&self, slug: &str, text: String, from: &str) {
        let surface = self
            .core
            .threads()
            .get(slug)
            .and_then(|t| t.surfaces.iter().find_map(|s| surface_channel(s)));
        let Some(channel) = surface else {
            self.threads.keep_pointer(slug, text);
            return;
        };
        self.say_pointer(channel, text, from);
    }

    /// A quiet notice in `channel`'s place: the lane writes it, in order with
    /// the place's other posts, and it pings no one.
    pub(super) fn say_pointer(&self, channel: u64, text: String, from: &str) {
        let target = format!("discord:channel:{channel}");
        let body = json!({"kind": "notice", "text": text, "quiet": true});
        if let Err(e) = self.core.outbox.post(from, "", &target, body) {
            self.board.error("post pointer", None, format!("{e:#}"));
        }
    }

    /// `/thread bind <slug>`: this channel, or Discord thread, runs the
    /// thread. The core judges the presser (the owner, from a private place);
    /// the place must be private, and a thread has one Discord surface.
    pub(super) async fn thread_bind(
        self: &Arc<Self>,
        slug: &str,
        origin: Option<DiscordOrigin>,
        who: &str,
        place: Option<(u64, Option<u64>)>,
    ) -> String {
        let (Some(origin), Some((channel, parent))) = (origin, place) else {
            return "⚠️ A thread binds to a channel or a Discord thread.".into();
        };
        if origin.guild_id.is_none() {
            return "⚠️ A DM is where every thread can be addressed; bind a channel, or a \
                    Discord thread, instead."
                .into();
        }
        // A Discord thread that is not a place yet has its parent's audience:
        // the core judges the press from the parent (`may_run_threads` has
        // checked the parent is private).
        let origin = match (
            self.routes
                .lock()
                .unwrap()
                .by_channel
                .contains_key(&channel),
            parent,
        ) {
            (false, Some(p)) => DiscordOrigin {
                channel_id: p.to_string(),
                ..origin
            },
            _ => origin,
        };
        let shown = self
            .rpc
            .call::<_, ThreadRow>(
                method::THREAD_SHOW,
                ThreadNameParams {
                    slug: slug.to_string(),
                    author: Some(who.to_string()),
                    discord: Some(origin.clone()),
                },
            )
            .await;
        let t = match shown {
            Ok(r) => r.thread,
            Err(e) => return format!("⚠️ No thread was bound: {}", e.message),
        };
        if t.state == ThreadState::Done {
            return format!("⚠️ The thread `{slug}` is done: `theseus thread reopen` first.");
        }
        let target = format!("discord:channel:{channel}");
        if let Err(why) = self.may_run_threads(channel, parent, &target) {
            return format!("⚠️ {why}");
        }
        if let Some(other) = self.threads.slug_of(channel).filter(|o| o != slug) {
            return format!(
                "⚠️ This place already runs the thread `{other}`: `/thread unbind` first."
            );
        }
        let surface = channel_surface(channel);
        if let Some(there) = t
            .surfaces
            .iter()
            .find(|s| s.starts_with("discord:") && **s != surface)
        {
            return format!(
                "⚠️ The thread `{slug}` is shown on <#{}> already: one Discord place to a thread, \
                 so `/thread unbind` there first.",
                surface_channel(there).unwrap_or(0)
            );
        }
        let mut surfaces = t.surfaces.clone();
        if !surfaces.contains(&surface) {
            surfaces.push(surface);
        }
        let edit = ThreadEditParams {
            slug: slug.to_string(),
            surfaces: Some(surfaces),
            author: Some(who.to_string()),
            discord: Some(origin),
            ..Default::default()
        };
        if let Err(e) = self
            .rpc
            .call::<_, ThreadRow>(method::THREAD_EDIT, edit)
            .await
        {
            return format!("⚠️ No thread was bound: {}", e.message);
        }
        self.threads.bind(channel, slug);
        if let Err(e) = self.ensure_place(channel, slug).await {
            return format!("⚠️ The thread is recorded, but this place did not start: {e:#}");
        }
        self.tell_place(channel, ThreadMsg::Adopt(slug.to_string()));
        format!(
            "🧵 This place runs the thread `{slug}` from now on: what is said here goes to its \
             session, and `/thread unbind` gives the place back."
        )
    }

    /// A place for `channel`, made now when the bindings file does not name it.
    async fn ensure_place(self: &Arc<Self>, channel: u64, slug: &str) -> anyhow::Result<()> {
        if self
            .routes
            .lock()
            .unwrap()
            .by_channel
            .contains_key(&channel)
        {
            return Ok(());
        }
        let label = self.thread_label(slug);
        let b = self.dynamic_lane(None, channel, &label)?;
        self.start_channel(&b).await
    }

    /// Whether `channel` may run a thread: a private place the bindings file
    /// names, or a Discord thread under one (or under `[threads]
    /// discord_channel`, where the binding makes its own).
    fn may_run_threads(
        &self,
        channel: u64,
        parent: Option<u64>,
        target: &str,
    ) -> Result<(), String> {
        let class_of = |t: &str| self.core.runner.place_rule.class(&self.core.cfg, Some(t));
        let known = self
            .routes
            .lock()
            .unwrap()
            .by_channel
            .contains_key(&channel);
        let private = theseus_core::places::PlaceClass::Private;
        if known {
            return match class_of(target) == private {
                true => Ok(()),
                false => Err(
                    "A shared channel cannot run a thread: a thread is private always, \
                              and the channel's audience is more than you."
                        .into(),
                ),
            };
        }
        let under = parent.is_some_and(|p| {
            class_of(&format!("discord:channel:{p}")) == private
                || self.core.cfg.threads.discord_channel.as_deref() == Some(&p.to_string())
        });
        match under {
            true => Ok(()),
            false => Err(
                "Only a private channel the bindings file names, or a Discord thread \
                          under one, can run a thread; this one is neither."
                    .into(),
            ),
        }
    }

    /// `/thread unbind`: this place stops running its thread.
    pub(super) async fn thread_unbind(
        self: &Arc<Self>,
        origin: Option<DiscordOrigin>,
        who: &str,
        channel: Option<u64>,
    ) -> String {
        let (Some(origin), Some(channel)) = (origin, channel) else {
            return "⚠️ A thread binds to a channel or a Discord thread.".into();
        };
        let Some(slug) = self.threads.slug_of(channel) else {
            return "⚠️ This place runs no thread.".into();
        };
        let surface = channel_surface(channel);
        let shown = self
            .rpc
            .call::<_, ThreadRow>(
                method::THREAD_SHOW,
                ThreadNameParams {
                    slug: slug.clone(),
                    author: Some(who.to_string()),
                    discord: Some(origin.clone()),
                },
            )
            .await;
        let t = match shown {
            Ok(r) => r.thread,
            Err(e) => return format!("⚠️ Nothing was unbound: {}", e.message),
        };
        let surfaces: Vec<String> = t.surfaces.into_iter().filter(|s| *s != surface).collect();
        let edit = ThreadEditParams {
            slug: slug.clone(),
            surfaces: Some(surfaces),
            author: Some(who.to_string()),
            discord: Some(origin),
            ..Default::default()
        };
        if let Err(e) = self
            .rpc
            .call::<_, ThreadRow>(method::THREAD_EDIT, edit)
            .await
        {
            return format!("⚠️ Nothing was unbound: {}", e.message);
        }
        self.threads.unbind(channel);
        self.tell_place(channel, ThreadMsg::Release);
        format!("🧵 This place no longer runs the thread `{slug}`; it has its own session again.")
    }
}

impl Place {
    /// The thread this place runs, if it runs one.
    pub(super) fn thread_slug(&self) -> Option<String> {
        self.shared.threads.slug_of(self.channel?.get())
    }

    /// The place runs a thread and its session is ready for a turn: the
    /// thread's own, opened by the first message, taken as this place's
    /// session, and its posts sent here. False, with a notice, when the
    /// thread takes no message.
    pub(super) async fn thread_ready(&mut self, slug: &str) -> bool {
        let rec = match self.shared.core.thread_place_session(slug, &self.target) {
            Ok(r) => r,
            Err(e) => {
                self.say(&format!("⚠️ {}", e.message), None);
                return false;
            }
        };
        if rec.session_id != self.session_id {
            self.take_thread_session(rec.session_id).await;
        }
        true
    }

    /// This place now runs on the thread's session `sid`: its posts go here,
    /// its events come here, and it is watched.
    async fn take_thread_session(&mut self, sid: String) {
        if let Err(e) = self.shared.core.outbox.place_bound(&self.key, &sid) {
            self.shared
                .board
                .error("thread session", Some(&sid), format!("{e:#}"));
        }
        self.shared.watch(&sid).await;
        self.shared.place_limit(&self.key, &self.label, &sid);
        self.adopt(sid).await;
    }

    /// A message for the place's actor: a thread's, or the usual.
    pub(super) async fn handle_in(&mut self, m: PlaceMsg) {
        match m {
            PlaceMsg::Thread(t) => self.on_thread(t).await,
            PlaceMsg::Inbound(i) if i.thread.is_some() => self.thread_turn(i),
            m => self.handle(m).await,
        }
    }

    /// What the binding tells a thread-bound place.
    pub(super) async fn on_thread(&mut self, m: ThreadMsg) {
        match m {
            ThreadMsg::Adopt(slug) => {
                let sid = self
                    .shared
                    .core
                    .threads()
                    .get(&slug)
                    .and_then(|t| t.session_id.clone());
                if let Some(sid) = sid.filter(|s| *s != self.session_id) {
                    if !self.shared.core.session_retired(&sid) {
                        self.take_thread_session(sid).await;
                    }
                }
                for p in self.shared.threads.take_pointers(&slug) {
                    self.shared
                        .say_pointer(self.channel.map_or(0, Id::get), p, &self.session_id);
                }
            }
            ThreadMsg::Release => self.release_thread().await,
            ThreadMsg::Failed(why) => self.say(&format!("⚠️ {why}"), None),
        }
    }

    /// The place runs as itself again: its own session if it had one.
    async fn release_thread(&mut self) {
        let own = self
            .shared
            .core
            .outbox
            .place_session(&self.key)
            .ok()
            .flatten();
        let old = self.session_id.clone();
        match own.filter(|o| *o != old && !self.shared.core.session_retired(o)) {
            Some(own) => {
                let _ = self.shared.core.outbox.place_bound(&self.key, &own);
                self.shared.watch(&own).await;
                self.adopt(own).await;
            }
            None => {
                self.shared.core.outbox.place_unbound(&old);
                self.shared.routes.lock().unwrap().by_session.remove(&old);
                self.shared.board.unplace(&self.label, &old);
                self.session_id = String::new();
                self.start_fresh();
                self.report();
            }
        }
    }

    /// A message in the owner's DM that addresses a thread: the turn runs in
    /// the thread's session and is answered here, where the owner spoke.
    pub(super) fn thread_turn(&mut self, m: Inbound) {
        let Some(slug) = m.thread.clone() else { return };
        self.last_activity_ms = theseus_protocol::now_unix_ms();
        self.shared.core.binding_ledger(
            theseus_protocol::LedgerKind::DiscordMessageIn,
            Some(&self.session_id),
            json!({"place": self.label, "author": m.author, "chars": m.text.chars().count(),
                   "message_id": m.message.to_string(), "thread": slug}),
        );
        // The reply is a reply to this message, and the owner's answer.
        let _ = self.lane.send(crate::courier::LaneMsg::Author(m.author_id));
        let _ = self
            .lane
            .send(crate::courier::LaneMsg::Anchor(m.message.get()));
        let dm_channel = self.channel.map(Id::get);
        let line = pointer(&m.text, dm_channel, m.message.get());
        self.shared.post_pointer(&slug, line, &self.session_id);
        let (rpc, tx) = (self.shared.rpc.clone(), self.tx.clone());
        let origin = m.origin();
        let answer_at = self.target.clone();
        let (input, message) = (m.text.clone(), m.message.get());
        let author = format!("discord:{}", m.author);
        let pending: Vec<Pending> = m.files.into_iter().collect();
        tokio::spawn(async move {
            let mut attachments = Vec::new();
            for p in pending {
                attachments.extend(p.wait().await);
            }
            let params = TurnSubmitParams {
                dir: None,
                carried: false,
                prompt: None,
                session_id: None,
                input,
                profile: None,
                provider: None,
                model: None,
                author: Some(author),
                attachments,
                reply_to: Some(message.to_string()),
                opened_from: None,
                thread: Some(slug),
                in_reply_to: None,
                speaking: Some(Speaking {
                    discord: origin,
                    answer_at: Some(answer_at),
                }),
            };
            let r = rpc
                .call::<_, TurnSubmitResult>(method::TURN_SUBMIT, params)
                .await;
            if let Err(e) = r {
                let _ = tx.send(PlaceMsg::Thread(ThreadMsg::Failed(e.message)));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pointer quotes a message's first line, at most 80 characters, with
    /// no mention and no code, and links the message when it knows its DM.
    #[test]
    fn a_pointer_quotes_the_first_line_and_links_the_message() {
        let long = "x".repeat(200);
        let p = pointer(&format!("{long}\nsecond"), Some(77), 5);
        assert!(p.starts_with("-# from the DM: "), "{p}");
        assert!(p.contains(&format!("{}…", "x".repeat(80))), "{p}");
        assert!(p.ends_with("https://discord.com/channels/@me/77/5"), "{p}");
        assert!(!p.contains("second"));
        let p = pointer("ping @everyone `code`", None, 5);
        assert_eq!(p, "-# from the DM: ping @\u{200b}everyone 'code'");
        assert!(pointer("", None, 5).contains("(a file)"));
    }

    /// `thread:<slug>` addresses a thread only with a slug and words after it.
    #[test]
    fn an_explicit_address_is_a_slug_and_words() {
        assert_eq!(
            explicit("thread:tides what now"),
            Some(("tides".into(), "what now".into()))
        );
        assert_eq!(
            explicit("  thread:tides\nand more"),
            Some(("tides".into(), "and more".into()))
        );
        for no in [
            "thread:tides",
            "thread:Bad Slug x",
            "thread: tides x",
            "about thread:tides x",
            "",
        ] {
            assert_eq!(explicit(no), None, "{no:?}");
        }
    }

    /// The state binds one channel to a thread and a thread to one channel.
    #[test]
    fn a_thread_has_one_channel_and_a_channel_one_thread() {
        let s = State::default();
        s.bind(1, "tides");
        s.bind(2, "tides");
        assert_eq!((s.slug_of(1), s.slug_of(2)), (None, Some("tides".into())));
        assert_eq!(s.channel_of("tides"), Some(2));
        assert_eq!(s.unbind(2).as_deref(), Some("tides"));
        assert_eq!(s.channel_of("tides"), None);
        s.keep_pointer("tides", "a".into());
        assert_eq!(s.take_pointers("tides"), ["a"]);
        assert!(s.take_pointers("tides").is_empty());
    }
}
