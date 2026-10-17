//! The default forum (theseus-bh3t.6): one private forum in the owner's guild
//! that is the home of every task, a post each, so it is the complete index of
//! tasks. `[discord] forum` (on) and `forum_channel` (a Discord id or a name,
//! `theseus`) name it; it acts only when the bindings trust exactly one guild.
//!
//! - **Found or made** (`Shared::ensure_forum`), at the start and at each
//!   gateway READY: the recorded channel (META `discord.default_forum`, kept
//!   while `forum_channel` is what it was resolved from) is the forum
//!   whatever it is called now; else a configured id must resolve to a forum
//!   in the guild; else the first forum of that name; else one is made, private
//!   (`@everyone` cannot view it; the owners and the bot can), with the four
//!   state tags. A refusal (403) or an id that resolves to nothing turns it
//!   off for the run and tells the owner once; a failure of the network is
//!   tried again at the next READY.
//! - **Every task gets a post** (the worker's, `forum.rs`, is the one maker).
//!   A task with no other home, the DM's and a private channel's, works in
//!   its post (`Home::Post`: a row of `TaskThreads` as a thread is); any other
//!   task gets an index card linking where it works.
//! - **A message in an index card** starts nothing: one quiet line says where
//!   the task works.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use theseus_core::places::PlaceClass;
use tokio::sync::watch;
use twilight_http::request::Request;
use twilight_http::routing::Route as HttpRoute;
use twilight_model::channel::message::MessageFlags;

use super::forum::{Forum, Tag};
use super::inbound::Inbound;
use super::{Place, Shared};
use crate::bindings::Bindings;

/// META key of the forum found or made.
const RECORD_KEY: &str = "discord.default_forum";

/// How long a task waits for the forum to be found before it goes on without.
const RESOLVE_WAIT: Duration = Duration::from_secs(30);

/// A forum that could not be reached is looked for again after this long.
const AGAIN: Duration = Duration::from_secs(60);

const VIEW_CHANNEL: u64 = 1 << 10;
const SEND_MESSAGES: u64 = 1 << 11;
const ADD_REACTIONS: u64 = 1 << 6;
const READ_MESSAGE_HISTORY: u64 = 1 << 16;
const SEND_MESSAGES_IN_THREADS: u64 = 1 << 38;
const MANAGE_CHANNELS: u64 = 1 << 4;
const MANAGE_THREADS: u64 = 1 << 34;

/// What an owner may do in the forum: view, send, react, read the history and
/// send in threads (1,024 + 2,048 + 64 + 65,536 + 274,877,906,944 =
/// 274,877,975,616).
pub(crate) const OWNER_BITS: u64 =
    VIEW_CHANNEL | SEND_MESSAGES | ADD_REACTIONS | READ_MESSAGE_HISTORY | SEND_MESSAGES_IN_THREADS;

/// What the bot may: the owner's, and manage the channel and its threads
/// (274,877,975,616 + 16 + 17,179,869,184 = 292,057,844,816).
pub(crate) const BOT_BITS: u64 = OWNER_BITS | MANAGE_CHANNELS | MANAGE_THREADS;

/// Where a new task works.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Home {
    /// A post in the default forum.
    Post,
    /// A public thread in its channel.
    Thread,
    /// Nowhere of its own: its place.
    None,
}

/// Where a task works: its post when the default forum homes its place, else
/// a thread when the place is a channel that has them. Whatever this says, the
/// task has a post in the forum: this one's, or an index card (`!= Post`).
pub(crate) fn home_of(forum_homed: bool, is_channel_place: bool) -> Home {
    if forum_homed {
        Home::Post
    } else if is_channel_place {
        Home::Thread
    } else {
        Home::None
    }
}

/// Where the default forum stands.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Stand {
    /// Being found or made.
    Pending,
    On(Forum),
    Off,
}

/// The default forum's state, in `TaskPosts`.
pub(crate) struct DefaultForum {
    state: watch::Sender<Stand>,
    /// The trusted guild and the config's `forum_channel`, set at the start.
    wanted: Mutex<Option<(String, String)>>,
    /// Off for this run: a refusal, or an id that is no forum.
    gone: AtomicBool,
    /// The owner has been told.
    said: AtomicBool,
    /// One search at a time.
    gate: tokio::sync::Mutex<()>,
}

impl Default for DefaultForum {
    fn default() -> Self {
        Self {
            state: watch::channel(Stand::Off).0,
            wanted: Mutex::new(None),
            gone: AtomicBool::new(false),
            said: AtomicBool::new(false),
            gate: tokio::sync::Mutex::new(()),
        }
    }
}

impl DefaultForum {
    pub(crate) fn forum(&self) -> Option<Forum> {
        match &*self.state.borrow() {
            Stand::On(f) => Some(f.clone()),
            _ => None,
        }
    }

    /// It stands, or is being found.
    pub(crate) fn may_stand(&self) -> bool {
        !matches!(*self.state.borrow(), Stand::Off)
    }

    /// Whether the forum stands, once it is no longer being found (a task
    /// goes on without it after `RESOLVE_WAIT`).
    pub(crate) async fn settled(&self) -> bool {
        let mut rx = self.state.subscribe();
        let waited =
            tokio::time::timeout(RESOLVE_WAIT, rx.wait_for(|s| !matches!(s, Stand::Pending))).await;
        let on = matches!(waited, Ok(Ok(ref s)) if matches!(**s, Stand::On(_)));
        on
    }
}

/// What the search found out.
enum Why {
    /// Discord answered 403: the bot may not.
    Refused(String),
    /// The configured id is no forum of the guild.
    NoForum(String),
    /// Anything else: tried again.
    Other(anyhow::Error),
}

impl Why {
    fn of(e: &twilight_http::Error) -> Self {
        use twilight_http::error::ErrorType as E;
        match e.kind() {
            E::Response { status, .. } if status.get() == 403 => Self::Refused(e.to_string()),
            _ => Self::Other(anyhow::anyhow!("{e}")),
        }
    }
}

impl From<twilight_http::Error> for Why {
    fn from(e: twilight_http::Error) -> Self {
        Self::of(&e)
    }
}

impl From<twilight_http::response::DeserializeBodyError> for Why {
    fn from(e: twilight_http::response::DeserializeBodyError) -> Self {
        Self::Other(e.into())
    }
}

/// The forum found or made, as META keeps it.
#[derive(Serialize, Deserialize)]
struct Record {
    guild: String,
    channel: u64,
    /// The `forum_channel` it was resolved from.
    wanted: String,
}

/// A Discord id: 15 to 21 digits.
fn is_id(s: &str) -> bool {
    (15..=21).contains(&s.len()) && s.bytes().all(|c| c.is_ascii_digit())
}

/// A channel of this JSON is a forum (type 15) of `guild`.
fn forum_in(c: &Value, guild: &str) -> Option<u64> {
    (c["type"].as_u64() == Some(15) && c["guild_id"].as_str().is_none_or(|g| g == guild))
        .then(|| c["id"].as_str()?.parse().ok())?
}

/// Called at each gateway READY: the forum is looked at again.
pub(super) fn ready(shared: &Arc<Shared>) {
    let s = shared.clone();
    tokio::spawn(async move { s.ensure_forum().await });
}

impl Shared {
    /// Decide at the start whether the feature acts (on, and exactly one
    /// trusted guild), and look for the forum without waiting for it.
    pub(super) fn start_default_forum(self: &Arc<Self>, b: &Bindings) {
        let d = &self.task_posts.default;
        let cfg = &self.core.cfg.discord;
        let trusted = b.trusted();
        let guild = trusted.iter().next().filter(|_| trusted.len() == 1);
        let Some(guild) = guild.filter(|_| cfg.forum) else {
            d.state.send_replace(Stand::Off);
            return;
        };
        *d.wanted.lock().unwrap() = Some((guild.clone(), cfg.forum_channel.clone()));
        d.state.send_replace(Stand::Pending);
        ready(self);
    }

    /// Find or make the forum, and say where it stands.
    pub(super) async fn ensure_forum(self: &Arc<Self>) {
        let d = &self.task_posts.default;
        let _one = d.gate.lock().await;
        let wanted = d.wanted.lock().unwrap().clone();
        let Some((guild, wanted)) = wanted else {
            return;
        };
        if d.gone.load(Ordering::SeqCst) {
            return;
        }
        match self.find_forum(&guild, &wanted).await {
            Ok(channel) => {
                let on = Forum {
                    channel,
                    guild,
                    private: true,
                    default: true,
                };
                let was = d.state.send_replace(Stand::On(on.clone()));
                if was != Stand::On(on) {
                    self.task_posts.nudge();
                    self.wake_places();
                }
            }
            Err(Why::Refused(e)) => {
                self.board.error("default forum", None, &e);
                self.forum_off(format!(
                    "-# I can't make the Theseus forum in {guild}: the bot needs Manage Channels \
                     and Manage Roles there (or set [discord] forum = false)"
                ));
            }
            Err(Why::NoForum(id)) => {
                self.forum_off(format!(
                    "-# [discord] forum_channel {id} is not a forum in {guild}, so I made none \
                     (set it to a forum's id or a name, or set forum = false)"
                ));
            }
            Err(Why::Other(e)) => {
                self.board.error("default forum", None, format!("{e:#}"));
                d.state.send_replace(Stand::Off);
                let s = self.clone();
                tokio::spawn(async move {
                    let stopped = s.core.outbox.stopped();
                    tokio::pin!(stopped);
                    tokio::select! {
                        () = tokio::time::sleep(AGAIN) => ready(&s),
                        () = &mut stopped => {}
                    }
                });
            }
        }
    }

    /// Off for this run, said to the owner once.
    fn forum_off(&self, line: String) {
        let d = &self.task_posts.default;
        d.gone.store(true, Ordering::SeqCst);
        d.state.send_replace(Stand::Off);
        if d.said.swap(true, Ordering::SeqCst) {
            return;
        }
        // A quiet line in the owner's DM (the first the bindings name), sent
        // as no session's: the lane of that DM writes it.
        let body = json!({"kind": "notice", "text": line, "quiet": true});
        let dm = self
            .core
            .runner
            .place_rule
            .owner_dm_places()
            .into_iter()
            .next();
        let sent = dm.map(|p| self.core.outbox.post("", "", &format!("discord:{p}"), body));
        if let Some(Err(e)) = sent {
            self.board.error("default forum", None, format!("{e:#}"));
        }
    }

    /// Every place looks at its tasks again: the forum is up.
    fn wake_places(&self) {
        let places: Vec<_> = self
            .routes
            .lock()
            .unwrap()
            .by_session
            .values()
            .cloned()
            .collect();
        for p in places {
            let _ = p.send(super::PlaceMsg::Tasks);
        }
    }

    async fn get_channel(&self, id: u64) -> Result<Option<Value>, Why> {
        let req = Request::builder(&HttpRoute::GetChannel { channel_id: id }).build()?;
        match self.http.request::<Value>(req).await {
            Ok(r) => Ok(Some(r.model().await?)),
            Err(e) => match e.kind() {
                twilight_http::error::ErrorType::Response { status, .. } if status.get() == 404 => {
                    Ok(None)
                }
                _ => Err(Why::of(&e)),
            },
        }
    }

    fn record(&self, guild: &str, channel: u64, wanted: &str) {
        let r = Record {
            guild: guild.to_string(),
            channel,
            wanted: wanted.to_string(),
        };
        if let Err(e) = self.core.store.put_meta(RECORD_KEY, &r) {
            tracing::warn!(error = %format!("{e:#}"), "the default forum was not recorded");
        }
    }

    /// The forum: recorded, or a configured id, or one of that name, or made.
    async fn find_forum(&self, guild: &str, wanted: &str) -> Result<u64, Why> {
        let recorded: Option<Record> = self.core.store.get_meta(RECORD_KEY).ok().flatten();
        if let Some(r) = recorded.filter(|r| r.wanted == wanted && r.guild == guild) {
            if let Some(c) = self.get_channel(r.channel).await? {
                if let Some(id) = forum_in(&c, guild) {
                    return Ok(id);
                }
            }
        }
        if is_id(wanted) {
            let found = self.get_channel(wanted.parse().unwrap_or(0)).await?;
            let id = found.and_then(|c| forum_in(&c, guild));
            let id = id.ok_or_else(|| Why::NoForum(wanted.to_string()))?;
            self.record(guild, id, wanted);
            return Ok(id);
        }
        let req = Request::builder(&HttpRoute::GetChannels {
            guild_id: guild.parse().unwrap_or(0),
        })
        .build()?;
        let all: Vec<Value> = self.http.request::<Vec<Value>>(req).await?.model().await?;
        let named = all.iter().find(|c| {
            c["name"]
                .as_str()
                .is_some_and(|n| n.eq_ignore_ascii_case(wanted))
        });
        let id = match named.and_then(|c| forum_in(c, guild)) {
            Some(id) => id,
            None => self.make_forum(guild, wanted).await?,
        };
        self.record(guild, id, wanted);
        Ok(id)
    }

    /// A forum nobody but the owners and the bot can view, with the state
    /// tags.
    async fn make_forum(&self, guild: &str, name: &str) -> Result<u64, Why> {
        if self.bot_id() == 0 {
            return Err(Why::Other(anyhow::anyhow!("the bot is not known yet")));
        }
        let everyone =
            json!({"id": guild, "type": 0, "allow": "0", "deny": VIEW_CHANNEL.to_string()});
        let allow = |id: String, bits: u64| json!({"id": id, "type": 1, "allow": bits.to_string(), "deny": "0"});
        let owners = self.core.runner.place_rule.owners(&self.core.cfg);
        let mut overwrites = vec![everyone];
        overwrites.extend(
            owners
                .iter()
                .filter_map(|o| o.strip_prefix("discord:")?.parse::<u64>().ok())
                .map(|u| allow(u.to_string(), OWNER_BITS)),
        );
        overwrites.push(allow(self.bot_id().to_string(), BOT_BITS));
        let tags: Vec<Value> = Tag::ALL.iter().map(|t| json!({"name": t.name()})).collect();
        let body = json!({
            "name": name,
            "type": 15,
            "default_auto_archive_duration": 1440,
            "available_tags": tags,
            "permission_overwrites": overwrites,
        });
        let req = Request::builder(&HttpRoute::CreateChannel {
            guild_id: guild.parse().unwrap_or(0),
        })
        .json(&body)
        .build()?;
        let made: Value = self.http.request::<Value>(req).await?.model().await?;
        made["id"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| Why::Other(anyhow::anyhow!("the new forum has no id")))
    }

    /// An index card's channel is heard by `home`, the place that told of its
    /// task, with the owners allowed.
    pub(super) fn route_index(
        &self,
        post: u64,
        home: tokio::sync::mpsc::UnboundedSender<super::PlaceMsg>,
    ) {
        let owners: Vec<u64> = self
            .core
            .runner
            .place_rule
            .owners(&self.core.cfg)
            .iter()
            .filter_map(|o| o.strip_prefix("discord:")?.parse().ok())
            .collect();
        let mut r = self.routes.lock().unwrap();
        r.by_channel.entry(post).or_insert(home);
        r.users.entry(post).or_insert(owners);
    }
}

impl Place {
    /// Whether the default forum homes the tasks of this place: it stands or
    /// may soon, the place is private, and it is the DM or a channel that
    /// gives its tasks threads (so not a task's own place or a thread's).
    pub(super) fn forum_homes(&self) -> bool {
        let s = &self.shared;
        let private = s
            .core
            .runner
            .place_rule
            .class(&s.core.cfg, Some(&self.target))
            == PlaceClass::Private;
        if !private || !s.task_posts.default.may_stand() {
            return false;
        }
        match (self.kind, self.channel) {
            ("dm", _) => true,
            ("channel", Some(c)) => {
                let c = c.get();
                s.task_threads.enabled(c)
                    && !s.task_threads.is_thread(c)
                    && s.threads.slug_of(c).is_none()
            }
            _ => false,
        }
    }

    /// Where this place's new tasks work (`home_of`).
    pub(super) fn home(&self) -> Home {
        let s = &self.shared;
        let channel_place = self.kind == "channel"
            && self.channel.is_some_and(|c| {
                let c = c.get();
                s.task_threads.enabled(c)
                    && !s.task_threads.is_thread(c)
                    && s.threads.slug_of(c).is_none()
            });
        home_of(self.forum_homes(), channel_place)
    }

    /// A message in an index card starts nothing: one quiet line says where
    /// the task works.
    pub(super) fn index_pointer(&self, m: &Inbound) {
        let channel = m.channel;
        let link = self
            .shared
            .task_posts
            .index_of(channel.get())
            .unwrap_or_default();
        let line = if link.is_empty() {
            "-# This post starts nothing: its task works elsewhere, or is over.".to_string()
        } else {
            format!("-# This task works in {link}.")
        };
        let http = self.shared.http.clone();
        tokio::spawn(async move {
            let none = twilight_model::channel::message::AllowedMentions::default();
            let sent = http
                .create_message(channel)
                .content(&line)
                .flags(MessageFlags::SUPPRESS_NOTIFICATIONS)
                .allowed_mentions(Some(&none))
                .await;
            if let Err(e) = sent {
                tracing::warn!(error = %e, "an index card's pointer was not sent");
            }
        });
    }
}

#[cfg(test)]
impl Shared {
    /// A binding over the same store looks for the forum as a start does
    /// (the bindings' trusted `guild`, the config's `wanted`), and says where
    /// it stands.
    pub(crate) async fn look_for_forum(self: &Arc<Self>, guild: &str, wanted: &str) -> Stand {
        let d = &self.task_posts.default;
        *d.wanted.lock().unwrap() = Some((guild.to_string(), wanted.to_string()));
        self.ensure_forum().await;
        let stand = d.state.borrow().clone();
        stand
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rule of where a task works: its post when the forum homes its
    /// place, else a thread in a channel, else nowhere of its own; and the
    /// index card is the answer's every other case.
    #[test]
    fn a_task_works_in_its_post_when_the_forum_homes_its_place() {
        assert_eq!(home_of(true, false), Home::Post, "the DM's task");
        assert_eq!(home_of(true, true), Home::Post, "a private channel's");
        assert_eq!(home_of(false, true), Home::Thread, "a shared channel's");
        assert_eq!(home_of(false, false), Home::None, "a place with no thread");
        for (homed, channel) in [(false, true), (false, false)] {
            assert!(
                home_of(homed, channel) != Home::Post,
                "any other task is an index card"
            );
        }
    }

    /// The two permission numbers are the sums of their bits.
    #[test]
    fn the_forums_overwrites_are_the_bits_they_name() {
        assert_eq!(OWNER_BITS, 274_877_975_616);
        assert_eq!(BOT_BITS, 292_057_844_816);
        assert_eq!(BOT_BITS & OWNER_BITS, OWNER_BITS);
    }

    #[test]
    fn a_configured_value_is_an_id_or_a_name() {
        assert!(is_id("900000000000000030"));
        assert!(!is_id("theseus"));
        assert!(!is_id("12345"));
    }
}
