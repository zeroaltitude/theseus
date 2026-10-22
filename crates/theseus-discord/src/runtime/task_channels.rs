//! A private text channel per qualifying task (theseus-bh3t.1; the owner,
//! 2026-10-17: every task that is multi-step or expected to take over ten
//! minutes gets a channel of its own). A child of `runtime`, beside the
//! thread helper (`threads.rs`), whose rows and places it shares.
//!
//! - **Where.** The one guild the bindings file trusts (`private = true`);
//!   with none, or several, nothing changes. Only a task whose place is
//!   private (the DM, or a channel bound private) gets one: a task in a shared
//!   channel keeps its public thread, because its guests are there.
//! - **Qualifies** (`qualifies`, pure): two plan steps or more, `expect =
//!   long`, or still working `task_channel_after_minutes` after it began (one
//!   timer a task, `Place::promote_later`, which sends the place its next sync;
//!   no periodic one). A task with a thread when it qualifies has the channel
//!   in its place: the thread says `moved to`, and is archived after `GRACE`,
//!   the posts of the session going to the channel from now.
//! - **The channel** is a row of `TaskThreads` like a thread (`Made.kind`):
//!   `⚙️ Theseus tasks` holds it, `@everyone` cannot view it, the owner and the
//!   bot can. Its place takes the owner's messages as steers (`task_message`),
//!   and no guest is added. The parent place gets one quiet line, the Follow
//!   mirror `-# in <#channel>`.
//! - **At the end** the report is posted in the channel; one PATCH moves it
//!   under `⚙️ Theseus tasks · done`, names it `✅`/`❌`, and replaces the
//!   whole overwrite set (Discord's Modify Channel replaces, never merges):
//!   every overwrite but `@everyone`'s View Channel deny, the operator's read
//!   (view and history, no Send Messages) and the bot's own goes, so a war
//!   room's guest and a role someone added by hand lose access. The guest
//!   list (META `discord.task_guests.<task>`) is cleared, and a `share:` of
//!   the ended task is refused (`war_room.rs`). A member with Administrator,
//!   and the guild's owner, see every channel whatever the overwrites say:
//!   Discord allows nothing narrower. A sweep at the start and once a day
//!   deletes the channels that ended `task_channel_keep_days` ago.
//! - **Fallbacks.** A 403 turns channels off until the next start (the owner
//!   gets one DM line saying which permissions are missing) and the task goes
//!   on as before; so does a task past `task_channel_cap` open channels, and
//!   one whose create failed otherwise.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use theseus_protocol::task_start::Expect;
use theseus_protocol::work::{Progress, WorkState, WorkView};
use tokio::sync::mpsc;
use twilight_model::channel::permission_overwrite::{PermissionOverwrite, PermissionOverwriteType};
use twilight_model::channel::ChannelType;
use twilight_model::guild::Permissions;
use twilight_model::id::marker::ChannelMarker;
use twilight_model::id::Id;

use super::follow::report_line;
use super::guilds::INVITE_PERMISSIONS;
use super::threads::{clip, Made, Mark, Parent, PlaceKind};
use super::{Place, PlaceMsg, Shared};
use crate::render::board_v2;
use theseus_protocol::Event as CoreEvent;

const CATEGORY: &str = "⚙️ Theseus tasks";
const CATEGORY_DONE: &str = "⚙️ Theseus tasks · done";
/// META keys of the two categories' ids.
const CATEGORY_KEY: &str = "discord.task_category";
const CATEGORY_DONE_KEY: &str = "discord.task_category_done";

/// Discord's longest channel name, and most channels in a category.
const NAME_MAX: usize = 100;
const CATEGORY_MAX: usize = 50;

const DAY: Duration = Duration::from_secs(24 * 3600);

/// Why a task has a channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Why {
    /// Its plan has two steps or more.
    Steps,
    /// It was started expecting to run long.
    Long,
    /// It is still working after the minutes the config gives.
    Promoted,
}

/// Whether the task `v`, seen at `now_ms`, wants a channel: not once it has
/// ended; two plan steps, a `long` expectation, or `after_ms` of working.
pub(crate) fn qualifies(v: &WorkView, now_ms: u64, after_ms: u64) -> Option<Why> {
    if v.state.ended() {
        return None;
    }
    if v.progress.as_ref().is_some_and(|p| p.steps_total >= 2) {
        return Some(Why::Steps);
    }
    if v.expect == Some(Expect::Long) {
        return Some(Why::Long);
    }
    let old = now_ms.saturating_sub(v.started_at_ms) >= after_ms;
    (working(v) && v.started_at_ms > 0 && old).then_some(Why::Promoted)
}

/// A task that works: running or asking, or waiting on its own calls (a
/// deferred start asleep has a time it starts at).
fn working(v: &WorkView) -> bool {
    match v.state {
        WorkState::Running | WorkState::NeedsYou => true,
        WorkState::Waiting => v.next_at_ms.is_none(),
        _ => false,
    }
}

/// What a channel call said: a refusal for want of permission, or any other
/// failure, in words.
#[derive(Debug)]
pub(crate) enum Refused {
    Forbidden,
    Other(String),
}

impl Refused {
    fn of(e: &twilight_http::Error) -> Self {
        use twilight_http::error::ErrorType as E;
        match e.kind() {
            E::Response { status, .. } if status.get() == 403 => Self::Forbidden,
            _ => Self::Other(e.to_string()),
        }
    }

    fn other(e: impl std::fmt::Display) -> Self {
        Self::Other(e.to_string())
    }
}

/// Whether `e` says the category (or any id in the call) is not there.
fn missing(e: &twilight_http::Error) -> bool {
    use twilight_http::error::ErrorType as E;
    matches!(e.kind(), E::Response { status, .. } if matches!(status.get(), 400 | 404))
}

#[derive(Default)]
struct ChanInner {
    /// A call was refused for want of permission: no more are tried until the
    /// next start, and the owner was told.
    off: bool,
    /// Tasks whose channel failed to be made another way: they go on as before.
    failed: HashSet<String>,
    /// Tasks with a promotion timer set, and those whose timer fired.
    woken: HashSet<String>,
    due: HashSet<String>,
    /// Tasks with a channel being made now (they count against the cap).
    inflight: HashSet<String>,
}

/// What task channels keep beside the rows (in `TaskThreads`).
#[derive(Default)]
pub(crate) struct Chan(Mutex<ChanInner>);

impl Chan {
    fn off(&self) -> bool {
        self.0.lock().unwrap().off
    }

    /// Channels stop; true the first time (the owner is told once a run).
    fn refuse(&self) -> bool {
        !std::mem::replace(&mut self.0.lock().unwrap().off, true)
    }

    fn failed(&self, task: &str) -> bool {
        self.0.lock().unwrap().failed.contains(task)
    }

    /// The timer of `task` is set; false when it already was.
    fn wake(&self, task: &str) -> bool {
        self.0.lock().unwrap().woken.insert(task.to_string())
    }

    fn fire(&self, task: &str) {
        self.0.lock().unwrap().due.insert(task.to_string());
    }

    fn due(&self, task: &str) -> bool {
        self.0.lock().unwrap().due.contains(task)
    }

    pub(super) fn begin(&self, task: &str) {
        self.0.lock().unwrap().inflight.insert(task.to_string());
    }

    fn end(&self, task: &str, failed: bool) {
        let mut i = self.0.lock().unwrap();
        i.inflight.remove(task);
        if failed {
            i.failed.insert(task.to_string());
        }
    }

    fn inflight(&self) -> usize {
        self.0.lock().unwrap().inflight.len()
    }

    fn forget(&self, task: &str) {
        let mut i = self.0.lock().unwrap();
        i.woken.remove(task);
        i.due.remove(task);
        i.failed.remove(task);
    }
}

/// `<short6>-<title-slug>`, lowercase a-z0-9 and `-`, within Discord's limit;
/// the mark first at the end (`✅-`, `❌-`).
pub(crate) fn channel_name(mark: Mark, task: &str, title: &str) -> String {
    let mut slug = String::new();
    for c in title.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-');
    let slug = if slug.is_empty() { "task" } else { slug };
    let head = match mark {
        Mark::Running => "",
        Mark::Done => "✅-",
        Mark::Failed => "❌-",
    };
    let name = format!("{head}{}-{slug}", board_v2::short(task).to_lowercase());
    let name: String = name.chars().take(NAME_MAX).collect();
    name.trim_end_matches('-').to_string()
}

/// The channel's topic: the task's title and its short id.
fn topic(task: &str, title: &str) -> String {
    format!("{} · …{}", clip(title), board_v2::short(task))
}

impl Shared {
    /// Whether this daemon makes task channels for the place `target` now:
    /// on, one trusted guild to make them in, not refused, and a private place.
    pub(super) fn channels_wanted(&self, target: &str) -> bool {
        self.core.cfg.discord.task_channels
            && self.place_bits.trusted_guild().is_some()
            && !self.task_threads.chan.off()
            && self.is_private(target)
    }

    /// Whether the task `v` gets a channel now, and why.
    pub(super) fn channel_why(&self, v: &WorkView) -> Option<Why> {
        let cfg = &self.core.cfg.discord;
        let chan = &self.task_threads.chan;
        let rows = self.task_threads.all();
        let mine = |m: &Made| m.task == v.id && m.kind == PlaceKind::Channel;
        if rows.iter().any(mine) || chan.failed(&v.id) {
            return None;
        }
        let open = rows
            .iter()
            .filter(|m| m.kind == PlaceKind::Channel && m.ended_ms.is_none())
            .count();
        if (open + chan.inflight()) as u64 >= cfg.task_channel_cap {
            return None;
        }
        let after_ms = cfg.task_channel_after_minutes * 60_000;
        qualifies(v, theseus_protocol::now_unix_ms(), after_ms)
            .or_else(|| (!v.state.ended() && chan.due(&v.id)).then_some(Why::Promoted))
    }

    /// The owners' ids on Discord.
    fn owner_ids(&self) -> Vec<u64> {
        let owners = self.core.runner.place_rule.owners(&self.core.cfg);
        owners
            .iter()
            .filter_map(|o| o.strip_prefix("discord:")?.parse().ok())
            .filter(|&id| id != 0)
            .collect()
    }

    /// Who may see the channel, and nobody else: the owners (and the bot)
    /// view, write and react; `closed` takes the owners' writing away.
    fn task_overwrites(&self, guild: u64, closed: bool) -> Vec<PermissionOverwrite> {
        let read = Permissions::VIEW_CHANNEL
            | Permissions::READ_MESSAGE_HISTORY
            | Permissions::ADD_REACTIONS;
        let write = Permissions::SEND_MESSAGES;
        let mut over = vec![PermissionOverwrite {
            allow: Permissions::empty(),
            deny: Permissions::VIEW_CHANNEL,
            id: Id::new(guild),
            kind: PermissionOverwriteType::Role,
        }];
        for owner in self.owner_ids() {
            over.push(PermissionOverwrite {
                allow: if closed { read } else { read | write },
                deny: if closed { write } else { Permissions::empty() },
                id: Id::new(owner),
                kind: PermissionOverwriteType::Member,
            });
        }
        // The bot's own id is known once it has connected.
        if self.bot_id() != 0 {
            over.push(PermissionOverwrite {
                allow: read | write | Permissions::MANAGE_CHANNELS,
                deny: Permissions::empty(),
                id: Id::new(self.bot_id()),
                kind: PermissionOverwriteType::Member,
            });
        }
        over
    }

    /// The category `⚙️ Theseus tasks` (or its `done` twin) in `guild`: its
    /// id from META, else made now. `fresh` makes it again (Discord said it
    /// is gone).
    async fn ensure_category(
        &self,
        guild: u64,
        done: bool,
        fresh: bool,
    ) -> Result<Id<ChannelMarker>, Refused> {
        let key = if done {
            CATEGORY_DONE_KEY
        } else {
            CATEGORY_KEY
        };
        if !fresh {
            if let Ok(Some(id)) = self.core.store.get_meta::<u64>(key) {
                return Ok(Id::new(id));
            }
        }
        let name = if done { CATEGORY_DONE } else { CATEGORY };
        let over = self.task_overwrites(guild, false);
        let made = self
            .http
            .create_guild_channel(Id::new(guild), name)
            .kind(ChannelType::GuildCategory)
            .permission_overwrites(&over)
            .await
            .map_err(|e| Refused::of(&e))?
            .model()
            .await
            .map_err(Refused::other)?;
        self.core
            .store
            .put_meta(key, &made.id.get())
            .map_err(Refused::other)?;
        Ok(made.id)
    }

    /// A private text channel for the task `v` under the tasks category:
    /// its id. The category is made once, and again if Discord lost it.
    pub(crate) async fn task_channel_create(
        &self,
        guild: u64,
        name: &str,
        v: &WorkView,
    ) -> Result<u64, Refused> {
        let over = self.task_overwrites(guild, false);
        let topic = topic(&v.id, &v.title);
        let mut fresh = false;
        loop {
            let parent = self.ensure_category(guild, false, fresh).await?;
            let made = self
                .http
                .create_guild_channel(Id::new(guild), name)
                .kind(ChannelType::GuildText)
                .parent_id(parent)
                .topic(&topic)
                .permission_overwrites(&over)
                .await;
            match made {
                Ok(r) => return Ok(r.model().await.map_err(Refused::other)?.id.get()),
                Err(e) if !fresh && missing(&e) => fresh = true,
                Err(e) => return Err(Refused::of(&e)),
            }
        }
    }

    /// The ended task's channel: under the done category, named with its
    /// mark, and read-only for the owner. A full category loses its oldest.
    async fn task_channel_close(&self, m: &Made, guild: u64, name: &str) -> Result<(), Refused> {
        self.make_room_in_done().await;
        let over = self.task_overwrites(guild, true);
        let mut fresh = false;
        loop {
            let parent = self.ensure_category(guild, true, fresh).await?;
            let moved = self
                .http
                .update_channel(Id::new(m.thread))
                .name(name)
                .parent_id(Some(parent))
                .permission_overwrites(&over)
                .await;
            match moved {
                Ok(_) => return Ok(()),
                Err(e) if !fresh && missing(&e) => fresh = true,
                Err(e) => return Err(Refused::of(&e)),
            }
        }
    }

    /// Delete the oldest ended channel while the done category holds 50.
    async fn make_room_in_done(&self) {
        let mut ended: Vec<Made> = self
            .task_threads
            .all()
            .into_iter()
            .filter(|m| m.kind == PlaceKind::Channel && m.ended_ms.is_some())
            .collect();
        ended.sort_by_key(|m| m.ended_ms);
        let extra = (ended.len() + 1).saturating_sub(CATEGORY_MAX);
        for m in ended.into_iter().take(extra) {
            self.delete_task_channel(&m).await;
        }
    }

    /// Delete a task's channel and its row; a channel already gone is done.
    async fn delete_task_channel(&self, m: &Made) {
        if let Err(e) = self.http.delete_channel(Id::new(m.thread)).await {
            if !gone(&e) {
                self.board
                    .error("task channel", Some(&m.session), e.to_string());
                return;
            }
        }
        self.task_threads.drop_made(&m.task);
        self.task_threads.save(&self.core.store);
    }

    /// Every channel that ended `task_channel_keep_days` ago goes, and every
    /// read forum post a day over leaves the forum (`forum::archive`); at the
    /// start, then once a day, until the daemon stops. The forum's archive
    /// also has an hourly tick of its own, so a read post waits no day for it.
    pub(super) fn sweep_task_channels(self: &Arc<Self>) {
        self.archive_tick();
        let s = self.clone();
        tokio::spawn(async move {
            let stopped = s.core.outbox.stopped();
            tokio::pin!(stopped);
            loop {
                s.sweep_once().await;
                tokio::select! {
                    () = tokio::time::sleep(DAY) => {}
                    () = &mut stopped => return,
                }
            }
        });
    }

    pub(crate) async fn sweep_once(&self) {
        let keep = self.core.cfg.discord.task_channel_keep_days * DAY.as_millis() as u64;
        let now = theseus_protocol::now_unix_ms();
        for m in self.task_threads.all() {
            let old = m.ended_ms.is_some_and(|e| now.saturating_sub(e) >= keep);
            if m.kind == PlaceKind::Channel && old {
                self.delete_task_channel(&m).await;
            }
        }
        self.archive_read_posts().await;
    }

    /// The task has a channel: made, listed, run as a place, and linked.
    /// `from` is the thread it leaves.
    async fn open_channel(self: &Arc<Self>, p: Parent, v: WorkView, why: Why, from: Option<Made>) {
        let (Some(session), Some(guild)) = (v.session_id.clone(), self.place_bits.trusted_guild())
        else {
            self.task_threads.chan.end(&v.id, false);
            self.task_threads.settle(&v.id);
            return;
        };
        let name = channel_name(Mark::Running, &v.id, &v.title);
        let mailbox = mpsc::unbounded_channel();
        if from.is_none() {
            self.task_threads.running(&session, mailbox.0.clone());
            self.watch(&session).await;
        }
        let made = self.task_channel_create(guild, &name, &v).await;
        let channel = match made {
            Ok(c) => c,
            Err(why_not) => {
                if from.is_none() {
                    self.task_threads.not_running(&session);
                }
                self.channel_failed(&p, &v, why_not, guild).await;
                return;
            }
        };
        let m = Made {
            task: v.id.clone(),
            session: session.clone(),
            thread: channel,
            kind: PlaceKind::Channel,
            parent: p.channel,
            parent_session: p.session.clone(),
            name,
            guild: Some(guild),
            private: true,
            ceiling: None,
            created_ms: theseus_protocol::now_unix_ms(),
            ended_ms: None,
        };
        self.task_threads.add(m.clone());
        self.task_threads.save(&self.core.store);
        if from.is_some() {
            self.task_threads.running(&session, mailbox.0.clone());
            self.watch(&session).await;
        }
        let place = format!("channel:{channel}");
        if let Err(e) = self.core.outbox.bind_place(&place, &session) {
            self.board
                .error("task channel", Some(&session), format!("{e:#}"));
        }
        if let Err(e) = self.start_task_place(&m, &p, mailbox) {
            self.board
                .error("task channel", Some(&session), format!("{e:#}"));
        }
        let origin = if p.threads {
            format!("-# ↩ from <#{}>", p.channel)
        } else {
            "-# ↩ from the DM".to_string()
        };
        let first = match why {
            Why::Promoted => summary(&v, &origin),
            Why::Steps | Why::Long => origin,
        };
        self.say_pointer(channel, first, &session);
        self.say_to(
            &p.target,
            format!("📺 <#{channel}> · {}", clip(&v.title)),
            &p.session,
        );
        if let Some(old) = from {
            self.say_pointer(old.thread, format!("-# moved to <#{channel}>"), &session);
            self.close_old_thread(old);
        }
        self.task_threads.chan.end(&v.id, false);
        self.task_threads.settle(&v.id);
        // The task may have ended while the channel was made.
        self.tell_channel(p.channel, PlaceMsg::Tasks);
    }

    /// A channel could not be made: a refusal for want of permission turns
    /// channels off and tells the owner once; any failure leaves the task as
    /// it was, and the next sync gives it the thread it would have had.
    async fn channel_failed(&self, p: &Parent, v: &WorkView, why_not: Refused, guild: u64) {
        match why_not {
            Refused::Forbidden => {
                if self.task_threads.chan.refuse() {
                    let line = format!(
                        "-# I can't make task channels in guild {guild}: grant the bot Manage \
                         Channels and Manage Roles (re-invite: {})",
                        self.invite_url().await
                    );
                    self.say_to(&self.owner_dm_target(p), line, &p.session);
                }
            }
            Refused::Other(e) => self.board.error("task channel", v.session_id.as_deref(), e),
        }
        self.task_threads.chan.end(&v.id, true);
        self.task_threads.settle(&v.id);
        self.tell_channel(p.channel, PlaceMsg::Tasks);
    }

    /// Where the owner's DM is: the parent's own when it is one.
    fn owner_dm_target(&self, p: &Parent) -> String {
        if p.target.starts_with("discord:dm:") {
            return p.target.clone();
        }
        let owner = self.owner_ids().into_iter().next().unwrap_or_default();
        format!("discord:dm:{owner}")
    }

    /// The link that invites the bot with the permissions channels need.
    async fn invite_url(&self) -> String {
        let app = match self.http.current_user_application().await {
            Ok(r) => r.model().await.map(|a| a.id.get()).unwrap_or_default(),
            Err(_) => 0,
        };
        format!(
            "https://discord.com/oauth2/authorize?client_id={app}&scope=bot+applications.commands&permissions={INVITE_PERMISSIONS}"
        )
    }

    /// The thread a task left for its channel: archived, and its place, lane
    /// and routes gone, after `GRACE` (the pointer it was left with is posted
    /// by then).
    fn close_old_thread(self: &Arc<Self>, old: Made) {
        // A post of the default forum stays, unarchived, as the task's index
        // card linking the channel (theseus-bh3t.6): only its place goes. The
        // sweep archives it once read and a day over (theseus-n9dkb).
        let post = self.task_posts.owns(&old.task);
        if post {
            if let Some(url) = self.task_threads.url_of(&old.task) {
                self.task_posts.moved(&old.task, &url);
            }
        }
        let s = self.clone();
        tokio::spawn(async move {
            let stopped = s.core.outbox.stopped();
            tokio::pin!(stopped);
            tokio::select! {
                () = tokio::time::sleep(super::threads::GRACE) => {}
                () = &mut stopped => return,
            }
            if !post {
                if let Err(e) = s.guild_thread_set(old.thread, Some(true), None).await {
                    s.board
                        .error("task thread", Some(&old.session), format!("{e:#}"));
                }
            }
            s.forget_thread_place(&old);
        });
    }

    /// A task with no thread, made a channel.
    pub(super) async fn make_task_channel(self: &Arc<Self>, p: Parent, v: WorkView, why: Why) {
        self.open_channel(p, v, why, None).await;
    }

    /// A task with a thread, moved to a channel.
    pub(super) async fn promote_task_thread(
        self: &Arc<Self>,
        p: Parent,
        old: Made,
        v: WorkView,
        why: Why,
    ) {
        self.open_channel(p, v, why, Some(old)).await;
    }

    /// The task ended: its report is said in the channel, and the channel
    /// moves to the done category, closed to the owner's words.
    pub(super) async fn finish_task_channel(self: &Arc<Self>, m: Made, mark: Mark, v: WorkView) {
        let target = format!("discord:channel:{}", m.thread);
        self.say_to(&target, format!("🏁 {}", report_line(&v)), &m.session);
        let name = channel_name(mark, &m.task, &v.title);
        let guild = m.guild.unwrap_or_default();
        let closed = self.task_channel_close(&m, guild, &name).await;
        match closed {
            Ok(()) => self.end_guests(&m).await,
            Err(Refused::Forbidden) => {
                if self.task_threads.chan.refuse() {
                    let line = "-# I can't move task channels: grant the bot Manage Channels \
                                and Manage Roles"
                        .to_string();
                    self.say_to(&target, line, &m.session);
                }
            }
            Err(Refused::Other(e)) => self.board.error("task channel", Some(&m.session), e),
        }
        self.task_threads.update(&m.task, |x| {
            x.name.clone_from(&name);
            x.ended_ms = Some(theseus_protocol::now_unix_ms());
        });
        self.task_threads.save(&self.core.store);
        self.task_threads.chan.forget(&m.task);
        self.task_threads.settle(&m.task);
        self.retire_later(m.task);
    }
}

/// An error that says the thing is already gone.
fn gone(e: &twilight_http::Error) -> bool {
    use twilight_http::error::ErrorType as E;
    matches!(e.kind(), E::Response { status, .. } if status.get() == 404)
}

/// What happened so far, for the first line of a channel a working task was
/// moved to.
fn summary(v: &WorkView, origin: &str) -> String {
    let mut t = format!("{origin}\n**{}**", clip(&v.title));
    if let Some(d) = &v.doing {
        t.push_str(&format!("\n-# {}", d.label));
    }
    if let Some(line) = v.progress.as_ref().and_then(Progress::line) {
        t.push_str(&format!("\n{}", clip(&line)));
    }
    t
}

impl Place {
    /// A task's channel (or thread) was made while its turn ran: the turn's
    /// start was never heard here, and the renderer shows no loop of a turn it
    /// did not see begin. The first event that names such a turn tells the
    /// renderer the turn started (a task is mostly one long turn). A thread is
    /// begun before its task's turn on a busy machine, and after it on a fast
    /// one, whose task's whole first turn then never reached its thread
    /// (theseus-ihu7l).
    pub(super) fn adopt_turn(&mut self, e: &CoreEvent) {
        let (Some(turn), Some(channel)) = (e.turn_id(), self.channel) else {
            return;
        };
        let ours = self.shared.task_threads.made_in(channel.get()).is_some();
        let ends = matches!(
            e,
            CoreEvent::TurnStarted(_) | CoreEvent::TurnEnded(_) | CoreEvent::TurnFailed(_)
        );
        if !ours || ends || self.renderer.knows(turn) {
            return;
        }
        let started = CoreEvent::TurnStarted(theseus_protocol::TurnStarted {
            session_id: self.session_id.clone(),
            turn_id: turn.to_string(),
            ..Default::default()
        });
        let mut ops = self.view.on_event(&started);
        ops.extend(self.view.fold(self.renderer.on_event(&started)));
        self.apply(ops);
    }

    /// A working task that does not qualify yet: one timer, at the minute the
    /// config gives, sends this place its next sync (the task then qualifies
    /// by `Why::Promoted`). No periodic one.
    pub(super) fn promote_later(&self, v: &WorkView) {
        let s = &self.shared;
        if v.session_id.is_none()
            || !working(v)
            || s.task_threads.channel_of(&v.id).is_some()
            || !s.task_threads.chan.wake(&v.id)
        {
            return;
        }
        let after = s.core.cfg.discord.task_channel_after_minutes * 60_000;
        let now = theseus_protocol::now_unix_ms();
        let started = if v.started_at_ms == 0 {
            now
        } else {
            v.started_at_ms
        };
        let wait = Duration::from_millis((started + after).saturating_sub(now));
        let (shared, task, tx) = (s.clone(), v.id.clone(), self.tx.clone());
        tokio::spawn(async move {
            let stopped = shared.core.outbox.stopped();
            tokio::pin!(stopped);
            tokio::select! {
                () = tokio::time::sleep(wait) => {}
                () = &mut stopped => return,
            }
            shared.task_threads.chan.fire(&task);
            let _ = tx.send(PlaceMsg::Tasks);
        });
    }
}

/// A task's work view, for the tests of `tests_task_channels.rs`.
#[cfg(test)]
pub(crate) fn work_view(id: &str, state: &str) -> WorkView {
    let mut v = super::voice_status::tests::view(id, "task", state, "Chart the reef", None);
    v.session_id = Some(format!("ses_{id}"));
    v.started_at_ms = 1_000;
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::tests::{core_with, place_for_tests};
    use crate::runtime::voice_status::tests::view;

    /// A working task that does not qualify yet gets exactly one timer, which
    /// sends its place the next sync at the config's minute (the paused clock
    /// is the test's: the wall clock does not move) and marks the task due.
    #[tokio::test(start_paused = true)]
    async fn one_timer_a_task_wakes_its_place_at_the_minute_and_marks_it_due() {
        let dir = tempfile::tempdir().unwrap();
        let core = core_with(
            dir.path(),
            theseus_core::secrets::SecretBoard::empty(),
            |_| {},
        );
        let (place, mut rx) = place_for_tests(&core, "ses_parent");
        let mut v = view("exe_reef", "task", "running", "Chart the reef", None);
        v.session_id = Some("ses_exe_reef".into());
        v.started_at_ms = theseus_protocol::now_unix_ms();
        let chan = &place.shared.task_threads.chan;
        // Two syncs, one timer; an ended or sleeping task sets none.
        place.promote_later(&v);
        place.promote_later(&v);
        let mut over = v.clone();
        over.id = "exe_over".into();
        over.state = WorkState::Done;
        place.promote_later(&over);
        tokio::time::advance(Duration::from_secs(9 * 60)).await;
        tokio::task::yield_now().await;
        assert!(rx.try_recv().is_err(), "nothing before the minute");
        assert!(!chan.due("exe_reef"));
        tokio::time::advance(Duration::from_secs(61)).await;
        let m = rx.recv().await.expect("the place's next sync");
        assert!(matches!(m, PlaceMsg::Tasks));
        assert!(chan.due("exe_reef"), "the timer is what makes it due");
        assert!(!chan.due("exe_over"));
        assert!(rx.try_recv().is_err(), "one timer, one message");
        // The sync then asks the rule, which the timer satisfies.
        let why = place.shared.channel_why(&v);
        assert_eq!(why, Some(Why::Promoted));
    }
}
