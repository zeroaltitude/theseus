//! The task home (theseus-xlxp4): one private text channel in the owner's
//! guild, the dashboard of the owner's tasks. `[discord] task_home` (a name or
//! an id, `theseus-tasks`; empty is off) names it; like the default forum it
//! acts only when the bindings trust exactly one guild.
//!
//! - **Found or made** (`Shared::ensure_task_home`), at the start and at each
//!   gateway READY: the recorded channel (META `discord.task_home`, kept while
//!   `task_home` is what it was resolved from), else a configured id, else the
//!   first text channel of that name, else one is made, private (`@everyone`
//!   cannot view it; the owners and the bot can: the forum's `OWNER_BITS` and
//!   `BOT_BITS`). A refusal (403) or an id that is no text channel turns it off
//!   for the run with one quiet line to the owner; a failure of the network is
//!   tried again. Its lane and bound place return at each start from the
//!   record, whatever the switch says, so a report staged for it is delivered.
//! - **What lands there.** Through the core's seam, the task's `reports_to`
//!   (`Outbox::home_tasks`): a task started from the owner's DM or from a
//!   voice place reports to the home, so its report, and the core's record of
//!   where it went, name the home. The DM place's live board is written to the
//!   home's lane (one board, pinned there), and each task's forum post is
//!   pointed at by one quiet line (`🧵 <#post> · title`).
//! - **A board the DM drew first** (before the state held the home) is removed
//!   when the home first stands (`LaneMsg::BoardMoved`), and the places draw
//!   again at the home. What is not ordered here is the core's watch scope: a
//!   view that changes between the state holding the home and the watch's next
//!   answer (`Tasks::rewatch`) reaches the board with that answer.
//! - **A voice place is never a task's home.** Whatever the switch says, a
//!   task started from a voice place reports to the home, else the owner's DM,
//!   gets no board in the voice channel's chat, and its card goes to its post.
//! - **A message in the home** starts nothing: the place that draws the board
//!   hears it and answers with one quiet line.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use theseus_core::outbox::TaskHome as Homed;
use theseus_core::places::{BoundPlace, PlaceClass};
use tokio::sync::{mpsc, watch};
use twilight_http::request::Request;
use twilight_http::routing::Route as HttpRoute;

use super::default_forum::{BOT_BITS, OWNER_BITS, VIEW_CHANNEL};
use super::{LaneMsg, Place, PlaceMsg, Shared};
use crate::bindings::Bindings;

/// META key of the channel found or made.
const RECORD_KEY: &str = "discord.task_home";

/// A channel that could not be reached is looked for again after this long.
const AGAIN: Duration = Duration::from_secs(60);

/// A channel's type: text.
const TEXT: u64 = 0;

/// The home's state, in `TaskPosts`.
pub(crate) struct TaskHome {
    /// The channel, once it stands.
    state: watch::Sender<Option<u64>>,
    /// The trusted guild and the config's `task_home`, set at the start.
    wanted: Mutex<Option<(String, String)>>,
    /// Its name, as Discord answered it.
    name: Mutex<String>,
    /// Off for this run: a refusal, or an id that is no text channel.
    gone: AtomicBool,
    /// The owner has been told.
    said: AtomicBool,
    /// One search at a time.
    gate: tokio::sync::Mutex<()>,
    /// The bindings' voice channels.
    voice: Mutex<HashSet<u64>>,
}

impl Default for TaskHome {
    fn default() -> Self {
        Self {
            state: watch::channel(None).0,
            wanted: Mutex::new(None),
            name: Mutex::new(String::new()),
            gone: AtomicBool::new(false),
            said: AtomicBool::new(false),
            gate: tokio::sync::Mutex::new(()),
            voice: Mutex::new(HashSet::new()),
        }
    }
}

fn target_of(channel: u64) -> String {
    format!("discord:channel:{channel}")
}

impl TaskHome {
    /// The channel, when it stands.
    pub(crate) fn channel(&self) -> Option<u64> {
        *self.state.borrow()
    }

    /// Whether `channel` is the home.
    pub(crate) fn is_home(&self, channel: u64) -> bool {
        self.channel() == Some(channel)
    }

    /// Whether `channel` is a voice binding's text chat.
    pub(crate) fn is_voice(&self, channel: u64) -> bool {
        self.voice.lock().unwrap().contains(&channel)
    }

    /// The voice channels the bindings name.
    pub(super) fn set_voice(&self, b: &Bindings) {
        *self.voice.lock().unwrap() = b
            .channel
            .iter()
            .filter(|c| c.voice)
            .filter_map(|c| c.id.parse().ok())
            .collect();
    }

    /// Where the DM's board and quiet lines go instead of the DM, when the home
    /// stands: the home's target, for the owner's DM alone.
    pub(crate) fn instead_of(&self, shared: &Shared, target: &str) -> Option<String> {
        let home = self.channel()?;
        (shared.owner_dm_place()? == target).then(|| target_of(home))
    }
}

/// What the search found out.
enum Why {
    /// Discord answered 403: the bot may not.
    Refused(String),
    /// The configured id is no text channel of the guild.
    NoChannel(String),
    /// Anything else: tried again.
    Other(anyhow::Error),
}

impl From<twilight_http::Error> for Why {
    fn from(e: twilight_http::Error) -> Self {
        use twilight_http::error::ErrorType as E;
        match e.kind() {
            E::Response { status, .. } if status.get() == 403 => Self::Refused(e.to_string()),
            _ => Self::Other(anyhow::anyhow!("{e}")),
        }
    }
}

impl From<twilight_http::response::DeserializeBodyError> for Why {
    fn from(e: twilight_http::response::DeserializeBodyError) -> Self {
        Self::Other(e.into())
    }
}

/// The channel found or made, as META keeps it.
#[derive(Serialize, Deserialize)]
struct Record {
    guild: String,
    channel: u64,
    /// The `task_home` it was resolved from.
    wanted: String,
    #[serde(default)]
    name: String,
}

/// A Discord id: 15 to 21 digits.
fn is_id(s: &str) -> bool {
    (15..=21).contains(&s.len()) && s.bytes().all(|c| c.is_ascii_digit())
}

/// A channel of this JSON is a text channel (type 0) of `guild`.
fn text_in(c: &Value, guild: &str) -> Option<u64> {
    (c["type"].as_u64() == Some(TEXT) && c["guild_id"].as_str().is_none_or(|g| g == guild))
        .then(|| c["id"].as_str()?.parse().ok())?
}

/// Called at each gateway READY: the home is looked at again.
pub(super) fn ready(shared: &Arc<Shared>) {
    let s = shared.clone();
    tokio::spawn(async move { s.ensure_task_home().await });
}

impl Shared {
    /// The target of the owner's DM place: the first the bindings name.
    pub(super) fn owner_dm_place(&self) -> Option<String> {
        let dm = self.core.runner.place_rule.owner_dm_places();
        dm.into_iter().next().map(|p| format!("discord:{p}"))
    }

    /// At the start: the voice channels are read, the recorded home's lane
    /// returns, and, when the switch is on and exactly one guild is trusted,
    /// the home is looked for without waiting for it.
    pub(super) fn start_task_home(self: &Arc<Self>, b: &Bindings) {
        let d = &self.task_posts.home;
        d.set_voice(b);
        let cfg = &self.core.cfg.discord;
        let guild = self.place_bits.trusted_guild().map(|g| g.to_string());
        if let Some(guild) = guild.filter(|_| !cfg.task_home.is_empty()) {
            *d.wanted.lock().unwrap() = Some((guild, cfg.task_home.clone()));
        } else {
            *d.wanted.lock().unwrap() = None;
        }
        self.register_task_homes();
        ready(self);
    }

    /// The recorded home's lane and bound place, before the start refuses the
    /// posts of places it has no lane for (a report staged for it waits there).
    pub(super) fn revive_task_home(self: &Arc<Self>) {
        let recorded: Option<Record> = self.core.store.get_meta(RECORD_KEY).ok().flatten();
        if let Some(r) = recorded {
            self.home_lane(r.channel, &r.guild, &r.name);
        }
    }

    /// The open index cards a task's session answers at, from the last run:
    /// their lanes and bound places, as a thread's return.
    pub(super) fn revive_card_places(self: &Arc<Self>) {
        self.task_posts.load(&self.core.store);
        let bound: Vec<_> = self
            .task_posts
            .posts
            .lock()
            .unwrap()
            .iter()
            .filter(|p| p.ended_ms.is_none())
            .filter_map(|p| Some((p.post, p.bound.clone()?, p.task.clone())))
            .collect();
        for (post, (session, parent), task) in bound {
            self.bind_card_place(&session, post, &parent, &super::forum::post_name(&task, ""));
        }
    }

    /// An ended task's card stops being its session's place after the grace
    /// a thread has: the session posts where it reports again, and the lane goes.
    pub(super) fn retire_card_place_later(self: &Arc<Self>, session: String, post: u64) {
        let s = self.clone();
        tokio::spawn(async move {
            let stopped = s.core.outbox.stopped();
            tokio::pin!(stopped);
            tokio::select! {
                () = tokio::time::sleep(super::threads::GRACE) => {}
                () = &mut stopped => return,
            }
            s.core.outbox.place_unbound(&session);
            let target = target_of(post);
            if let Some(lane) = s.lane(&target) {
                s.retired.lock().unwrap().insert(target);
                let _ = lane.send(LaneMsg::Wake);
            }
        });
    }

    /// The home's lane, and its place bound private with the DM's ceiling, so
    /// a task that reports there keeps the class it had.
    fn home_lane(self: &Arc<Self>, channel: u64, guild: &str, name: &str) {
        let target = target_of(channel);
        let dm = self.owner_dm_place();
        if self.lane(&target).is_none() {
            if let Some(dm) = &dm {
                self.pings.set(&target, self.pings.of(dm));
            }
            let label = format!("#{name}");
            self.start_lane(target.clone(), "channel", label, Some(channel), None);
        }
        let key = dm.as_deref().and_then(|t| t.strip_prefix("discord:"));
        let bits = key.map(|k| self.place_bits.get(k)).unwrap_or_default();
        let rule = &self.core.runner.place_rule;
        rule.bind_more(BoundPlace {
            persona: dm.as_deref().and_then(|t| rule.persona_of(t)),
            target,
            name: format!("#{name}"),
            private: true,
            guild: Some(guild.to_string()),
            ceiling: bits.ceiling,
        });
    }

    /// Tell the core where tasks started from the owner's DM and from each
    /// voice place report: the home when it stands (the DM's too), else the
    /// DM for a voice place's. Again whenever the home's state changes.
    pub(super) fn register_task_homes(&self) {
        let d = &self.task_posts.home;
        let outbox = &self.core.outbox;
        let dm = self.owner_dm_place();
        let forum = if self.core.cfg.discord.forum {
            " and its forum post"
        } else {
            ""
        };
        let home = d.channel().map(|c| Homed {
            target: target_of(c),
            label: format!("#{}{forum}", d.name.lock().unwrap()),
        });
        if let Some(dm) = &dm {
            outbox.home_tasks(dm, home.clone());
        }
        let to_dm = dm.map(|target| Homed {
            target,
            label: format!("the DM{forum}"),
        });
        for v in d.voice.lock().unwrap().iter() {
            outbox.home_tasks(&target_of(*v), home.clone().or(to_dm.clone()));
        }
    }

    /// Find or make the home, and say where it stands.
    pub(super) async fn ensure_task_home(self: &Arc<Self>) {
        let d = &self.task_posts.home;
        let _one = d.gate.lock().await;
        let wanted = d.wanted.lock().unwrap().clone();
        let Some((guild, wanted)) = wanted else {
            return;
        };
        if d.gone.load(Ordering::SeqCst) {
            return;
        }
        match self.find_task_home(&guild, &wanted).await {
            Ok((channel, name)) => {
                *d.name.lock().unwrap() = name.clone();
                self.home_lane(channel, &guild, &name);
                let was = d.state.send_replace(Some(channel));
                self.register_task_homes();
                if was != Some(channel) {
                    self.wake_places();
                    // A board the DM drew before the home answered is stale.
                    if let Some(dm) = self.owner_dm_place().and_then(|t| self.lane(&t)) {
                        let _ = dm.send(crate::courier::LaneMsg::BoardMoved);
                    }
                }
            }
            Err(Why::Refused(e)) => {
                self.board.error("task home", None, &e);
                self.task_home_off(format!(
                    "-# I can't make the task channel in {guild}: the bot needs Manage Channels \
                     and Manage Roles there (or set [discord] task_home = \"\")"
                ));
            }
            Err(Why::NoChannel(id)) => {
                self.task_home_off(format!(
                    "-# [discord] task_home {id} is not a text channel in {guild}, so I made none \
                     (set it to a text channel's id or a name, or set task_home = \"\")"
                ));
            }
            Err(Why::Other(e)) => {
                self.board.error("task home", None, format!("{e:#}"));
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
    fn task_home_off(&self, line: String) {
        let d = &self.task_posts.home;
        d.gone.store(true, Ordering::SeqCst);
        d.state.send_replace(None);
        self.register_task_homes();
        if d.said.swap(true, Ordering::SeqCst) {
            return;
        }
        // A quiet line in the owner's DM, sent as no session's.
        let body = json!({"kind": "notice", "text": line, "quiet": true});
        let sent = self
            .owner_dm_place()
            .map(|dm| self.core.outbox.post("", "", &dm, body));
        if let Some(Err(e)) = sent {
            self.board.error("task home", None, format!("{e:#}"));
        }
    }

    async fn get_channel_json(&self, id: u64) -> Result<Option<Value>, Why> {
        let req = Request::builder(&HttpRoute::GetChannel { channel_id: id }).build()?;
        match self.http.request::<Value>(req).await {
            Ok(r) => Ok(Some(r.model().await?)),
            Err(e) => match e.kind() {
                twilight_http::error::ErrorType::Response { status, .. } if status.get() == 404 => {
                    Ok(None)
                }
                _ => Err(Why::from(e)),
            },
        }
    }

    fn record_task_home(&self, guild: &str, channel: u64, wanted: &str, name: &str) {
        let r = Record {
            guild: guild.to_string(),
            channel,
            wanted: wanted.to_string(),
            name: name.to_string(),
        };
        if let Err(e) = self.core.store.put_meta(RECORD_KEY, &r) {
            tracing::warn!(error = %format!("{e:#}"), "the task home was not recorded");
        }
    }

    /// The home: recorded, or a configured id, or one of that name, or made.
    async fn find_task_home(&self, guild: &str, wanted: &str) -> Result<(u64, String), Why> {
        let name_of = |c: &Value| c["name"].as_str().unwrap_or("").to_string();
        let recorded: Option<Record> = self.core.store.get_meta(RECORD_KEY).ok().flatten();
        if let Some(r) = recorded.filter(|r| r.wanted == wanted && r.guild == guild) {
            if let Some(c) = self.get_channel_json(r.channel).await? {
                if let Some(id) = text_in(&c, guild) {
                    return Ok((id, name_of(&c)));
                }
            }
        }
        let found = if is_id(wanted) {
            let c = self.get_channel_json(wanted.parse().unwrap_or(0)).await?;
            let c = c.filter(|c| text_in(c, guild).is_some());
            Some(c.ok_or_else(|| Why::NoChannel(wanted.to_string()))?)
        } else {
            let req = Request::builder(&HttpRoute::GetChannels {
                guild_id: guild.parse().unwrap_or(0),
            })
            .build()?;
            let all: Vec<Value> = self.http.request::<Vec<Value>>(req).await?.model().await?;
            all.into_iter().find(|c| {
                text_in(c, guild).is_some()
                    && c["name"]
                        .as_str()
                        .is_some_and(|n| n.eq_ignore_ascii_case(wanted))
            })
        };
        let c = match found {
            Some(c) => c,
            None => self.make_task_home(guild, wanted).await?,
        };
        let id = text_in(&c, guild)
            .or_else(|| c["id"].as_str()?.parse().ok())
            .ok_or_else(|| Why::Other(anyhow::anyhow!("the task home has no id")))?;
        let name = name_of(&c);
        self.record_task_home(guild, id, wanted, &name);
        Ok((id, name))
    }

    /// A text channel nobody but the owners and the bot can view.
    async fn make_task_home(&self, guild: &str, name: &str) -> Result<Value, Why> {
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
        let body = json!({
            "name": name,
            "type": TEXT,
            "topic": "Your tasks: the live board, each report, and a line for each task's post.",
            "permission_overwrites": overwrites,
        });
        let req = Request::builder(&HttpRoute::CreateChannel {
            guild_id: guild.parse().unwrap_or(0),
        })
        .json(&body)
        .build()?;
        Ok(self.http.request::<Value>(req).await?.model().await?)
    }

    /// The home hears the owner's board's presses and a message typed in it:
    /// the DM's place answers both, as an index card's channel is heard by the
    /// place that told of its task.
    pub(super) fn route_task_home(&self, place: &mpsc::UnboundedSender<PlaceMsg>) {
        let Some(home) = self.task_posts.home.channel() else {
            return;
        };
        let owners: Vec<u64> = self
            .core
            .runner
            .place_rule
            .owners(&self.core.cfg)
            .iter()
            .filter_map(|o| o.strip_prefix("discord:")?.parse().ok())
            .collect();
        let mut r = self.routes.lock().unwrap();
        r.by_channel.insert(home, place.clone());
        r.users.entry(home).or_insert(owners);
    }

    /// One quiet line in the home pointing at a task's post, when the home
    /// stands (the DM's own tasks' pointer goes there from `make_task_thread`).
    pub(super) fn point_home_at(&self, session: &str, post: u64, title: &str) {
        let Some(home) = self.task_posts.home.channel() else {
            return;
        };
        let line = format!("🧵 <#{post}> · {}", super::threads::clip(title));
        self.say_to(&target_of(home), line, session);
    }

    /// An index card's task answers at the card: its session's cards and posts
    /// go to the post, never to the place that told of the task with a mention
    /// (theseus-90qqy). Its lane and bound place keep the parent's class.
    pub(super) fn bind_card_place(
        self: &Arc<Self>,
        session: &str,
        post: u64,
        parent_key: &str,
        label: &str,
    ) {
        let target = target_of(post);
        let parent = format!("discord:{parent_key}");
        if self.lane(&target).is_none() {
            self.pings.set(&target, self.pings.of(&parent));
            self.start_lane(
                target.clone(),
                "channel",
                label.to_string(),
                Some(post),
                None,
            );
        }
        let rule = &self.core.runner.place_rule;
        let class = rule.class(&self.core.cfg, Some(&parent));
        let bits = self.place_bits.get(parent_key);
        rule.bind_more(BoundPlace {
            persona: rule.persona_of(&parent),
            target,
            name: label.to_string(),
            private: class == PlaceClass::Private,
            guild: bits.guild,
            ceiling: bits.ceiling,
        });
        if let Err(e) = self
            .core
            .outbox
            .place_bound(&format!("channel:{post}"), session)
        {
            self.board
                .error("task post", Some(session), format!("{e:#}"));
        }
    }
}

impl Shared {
    /// The conversations of the voice places, whose tasks the owner's DM
    /// place shows on the home's board while the home stands.
    pub(super) fn board_sessions(&self, target: &str) -> Vec<String> {
        let d = &self.task_posts.home;
        if d.instead_of(self, target).is_none() {
            return Vec::new();
        }
        let voice: Vec<u64> = d.voice.lock().unwrap().iter().copied().collect();
        voice
            .into_iter()
            .filter_map(|v| {
                self.core
                    .outbox
                    .place_session(&format!("channel:{v}"))
                    .ok()?
            })
            .collect()
    }

    /// The owner's DM place, which draws the board a voice place's tasks are on.
    fn dm_place(&self) -> Option<mpsc::UnboundedSender<PlaceMsg>> {
        let user: u64 = self
            .owner_dm_place()?
            .strip_prefix("discord:dm:")?
            .parse()
            .ok()?;
        self.routes.lock().unwrap().by_dm_user.get(&user).cloned()
    }
}

impl Place {
    /// The lane this place's board is written to: its own, the home's for the
    /// owner's DM while the home stands. A voice place has none (a voice
    /// channel's chat shows no board): its tasks are on the DM place's board,
    /// which it asks to be drawn again.
    pub(super) fn board_lane(&self) -> Option<mpsc::UnboundedSender<LaneMsg>> {
        let s = &self.shared;
        if self
            .channel
            .is_some_and(|c| s.task_posts.home.is_voice(c.get()))
        {
            if s.task_posts.home.channel().is_some() {
                if let Some(dm) = s.dm_place() {
                    let _ = dm.send(PlaceMsg::Tasks);
                }
            }
            return None;
        }
        match s.task_posts.home.instead_of(s, &self.target) {
            Some(home) => {
                s.route_task_home(&self.tx);
                s.lane(&home).or_else(|| Some(self.lane.clone()))
            }
            None => Some(self.lane.clone()),
        }
    }
}

#[cfg(test)]
impl Shared {
    /// A binding over the same store looks for the home as a start does (the
    /// trusted `guild`, the config's `wanted`), and says where it stands.
    pub(crate) async fn look_for_task_home(
        self: &Arc<Self>,
        guild: &str,
        wanted: &str,
    ) -> Option<u64> {
        let d = &self.task_posts.home;
        *d.wanted.lock().unwrap() = Some((guild.to_string(), wanted.to_string()));
        self.ensure_task_home().await;
        d.channel()
    }
}
