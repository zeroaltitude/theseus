//! The guild calendar (theseus-4zc4d; horizon HZ11, owner decision D-H7): the
//! next days of reminders, routine runs and milestones as scheduled events in
//! one guild's Events tab, a calendar every Discord client already shows, with
//! Discord's own "starting now" notice for anyone marked Interested.
//!
//! **Off by default** (`[discord.calendar] enabled`): off, nothing here runs
//! and not a request is made. On, nothing happens before Ready.
//!
//! - **The mirror.** A pass reads `work.agenda`'s next `days` once, takes the
//!   soonest `max_events` of its reminders, routine runs and milestones, and
//!   brings the guild's events to them: a new item is an event
//!   (`entity_type` 3, an external one, since an event needs a place and
//!   ours is a channel or "Theseus"), a moved one is edited, a cancelled one
//!   deleted, and one whose time has passed is settled (active, then
//!   completed: Discord moves a status only forward). Each occurrence of a
//!   series is its own event: the scheduled-event model has no recurrence
//!   rule the bot could set. twilight types none, and an event Discord has
//!   begun cannot be given one.
//! - **Found again.** Each event's id is kept in META under `discord.calendar`
//!   by the item's key (the work's id and the occurrence), and its description
//!   ends with `theseus-calendar:<key>`. The first pass of a run lists the
//!   guild's events and reconciles the two, so a restart makes no duplicate
//!   even when the write that named an event was lost, and a doubled event is
//!   deleted.
//! - **The pace.** A change to the board's wakes, routines or projects waits
//!   10 minutes (from the first change, so a burst is one pass); an event's
//!   settling waits a minute after its time; and a pass at least every six
//!   hours rolls the window. A pass writes only what differs, so a steady day
//!   is a handful of writes.
//! - **Private stays private.** The guild must be one the bindings file trusts
//!   (`private = true`); where it is not, nothing is written and health says
//!   why. An event is named for its title only when the item speaks in a
//!   private place, else `Theseus · reminder`, and a title with a figure of
//!   money is never shown (`plan::name_of`).
//! - **The guild's limit** is 100 active and scheduled events (Discord's
//!   published figure, and its error 30038 when passed). `max_events` stays at
//!   50 or less, the guild's own events are counted at each listing, and a
//!   refusal is told once in health and the writes go on without it.
//!
//! Tests: `tests_gateway/calendar.rs` through the stand-in's scheduled-event
//! routes; `plan.rs`'s own for the rules.

mod plan;
mod rest;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;
use theseus_core::config::calendar::{CalendarConfig, GUILD_EVENT_LIMIT};
use theseus_core::config::DiscordConfig;
use theseus_core::push::WorkScope;
use theseus_core::Core;
use theseus_protocol::work::horizon::AgendaParams;
use theseus_protocol::work::WorkKind;
use theseus_protocol::{notify, Message, WorkChanged};
use tokio::sync::mpsc;
use tokio::time::Instant;
use twilight_model::util::Timestamp;

use self::plan::{Entry, Item, Op, Shown};
use self::rest::{Fail, Rest};
use super::Shared;
use crate::bindings::Bindings;

/// The META key holding the events made.
const META_KEY: &str = "discord.calendar";
/// The work-board watch's connection name.
const CONN: &str = "discord-calendar";

/// How the calendar paces itself.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Pace {
    /// A change waits this long, measured from the first of a burst, and a
    /// pass follows another by at least as long.
    pub(crate) debounce: Duration,
    /// An event is settled this long after its time.
    pub(crate) grace: Duration,
    /// A pass at least this often, to roll the window.
    pub(crate) every: Duration,
    /// An item starting sooner than this from now is left alone: Discord
    /// refuses a start in the past, and a pass is not instant.
    pub(crate) lead_ms: u64,
}

impl Pace {
    const LIVE: Pace = Pace {
        debounce: Duration::from_secs(600),
        grace: Duration::from_secs(60),
        every: Duration::from_secs(6 * 3600),
        lead_ms: 30_000,
    };
}

/// A test's pace, by the state dir of the core it runs (one entry per test's
/// scratch directory, so tests in one process do not meet).
#[cfg(test)]
pub(crate) static PACES: std::sync::Mutex<Vec<(String, Pace)>> = std::sync::Mutex::new(Vec::new());

fn pace_of(core: &Core) -> Pace {
    #[cfg(test)]
    if let Some((_, p)) = PACES
        .lock()
        .unwrap()
        .iter()
        .find(|(d, _)| *d == core.cfg.server.state_dir)
    {
        return *p;
    }
    let _ = core;
    Pace::LIVE
}

/// What the store remembers: the guild, and the events made there.
#[derive(Default, Serialize, Deserialize)]
struct Saved {
    guild: String,
    entries: Vec<Entry>,
}

/// The running calendar; ended, with its watch, when dropped.
pub(super) struct Handle {
    task: tokio::task::JoinHandle<()>,
    core: Arc<Core>,
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.task.abort();
        self.core.bus.unwatch_work(CONN);
    }
}

/// Start the calendar at Ready, once: a second Ready finds it running. Off,
/// untrusted, or without a token it starts nothing (and the last two say why).
pub(super) fn ready(
    slot: &mut Option<Handle>,
    shared: &Arc<Shared>,
    cfg: &DiscordConfig,
    bindings: &Bindings,
) {
    let c: &CalendarConfig = &cfg.calendar;
    if !c.enabled || slot.is_some() {
        return;
    }
    let tell: Tell = {
        let shared = Arc::clone(shared);
        Arc::new(move |why: String| shared.board.error("calendar", None, why))
    };
    let Ok(_) = c.guild.parse::<u64>() else {
        return tell(format!("the guild {:?} is not a Discord id", c.guild));
    };
    if !bindings.trusted().contains(&c.guild) {
        return tell(format!(
            "off: guild {} is not one the bindings file trusts (`private = true` on its [[guild]]); \
             the calendar's events are read by everyone in the guild, so nothing is written",
            c.guild
        ));
    }
    let names = bindings
        .channel
        .iter()
        .filter(|ch| ch.guild == c.guild)
        .map(|ch| {
            (
                ch.id.clone(),
                format!("#{}", ch.name.as_deref().unwrap_or("channel")),
            )
        })
        .collect();
    let pace = pace_of(&shared.core);
    let Some(cx) = Cx::open(&shared.core, c, names, pace.lead_ms, Arc::clone(&tell)) else {
        return tell("off: the bot's token is not ready".into());
    };
    let core = Arc::clone(&shared.core);
    let task = tokio::spawn(run(cx, pace));
    *slot = Some(Handle { task, core });
}

type Tell = Arc<dyn Fn(String) + Send + Sync>;

/// What a pass works with.
pub(crate) struct Cx {
    core: Arc<Core>,
    rest: Rest,
    guild: u64,
    guild_id: String,
    days: u64,
    max: usize,
    /// The events made, by key; Discord's listing corrects them.
    entries: Vec<Entry>,
    /// Events in the guild that are not ours, as the last listing counted.
    foreign: usize,
    listed: bool,
    told: HashSet<&'static str>,
    tell: Tell,
    /// Channel id → `#name`, for the trusted guild's bound channels.
    names: HashMap<String, String>,
    lead_ms: u64,
}

/// A pass stops here: Discord refused us, or is not answering.
struct Abort;

/// The watch, then the passes: the first at once, then on a change after the
/// debounce, on an event's time passing, and every `every`.
async fn run(mut cx: Cx, pace: Pace) {
    let (tx, mut feed) = mpsc::unbounded_channel::<Message>();
    let mut watched = match cx.core.push.ensure(&cx.core).await {
        Ok(()) => {
            cx.core
                .bus
                .watch_work(CONN, tx, WorkScope::default(), false);
            true
        }
        Err(e) => {
            tracing::warn!(error = %e, "discord: the calendar cannot watch the work board");
            false
        }
    };
    let start = Instant::now();
    let mut change_at: Option<Instant> = Some(start);
    let mut every_at = start + pace.every;
    let mut settle_at: Option<Instant> = None;
    let mut last_pass: Option<Instant> = None;
    loop {
        let settle = settle_at.map(|s| match last_pass {
            Some(l) => s.max(l + pace.debounce),
            None => s,
        });
        let due = [change_at, settle, Some(every_at)]
            .into_iter()
            .flatten()
            .min()
            .unwrap_or(every_at);
        tokio::select! {
            m = feed.recv(), if watched => {
                let Some(m) = m else { watched = false; continue };
                if changed(&m) && change_at.is_none() {
                    change_at = Some(Instant::now() + pace.debounce);
                }
            }
            () = tokio::time::sleep_until(due) => {
                if due >= every_at {
                    cx.listed = false;
                    every_at = Instant::now() + pace.every;
                }
                change_at = None;
                let earliest = cx.pass().await;
                last_pass = Some(Instant::now());
                let now_ms = cx.core.kernel.now_ms();
                settle_at = earliest.map(|at| {
                    Instant::now() + Duration::from_millis(at.saturating_sub(now_ms)) + pace.grace
                });
            }
            () = cx.core.outbox.stopped() => break,
        }
    }
    cx.core.bus.unwatch_work(CONN);
}

/// Whether a work-board change can move the agenda: a wake, a routine or a
/// project changed, or something went.
fn changed(m: &Message) -> bool {
    let Message::Notification(n) = m else {
        return false;
    };
    if n.method != notify::WORK_CHANGED {
        return false;
    }
    let Ok(c) = serde_json::from_value::<WorkChanged>(n.params.clone()) else {
        return false;
    };
    !c.gone.is_empty()
        || c.views.iter().any(|v| {
            matches!(
                v.kind,
                WorkKind::Wake | WorkKind::Routine | WorkKind::Project
            )
        })
}

/// An RFC 3339 time for Discord.
fn stamp(ms: u64) -> String {
    Timestamp::from_micros(ms as i64 * 1000)
        .map(|t| t.iso_8601().to_string())
        .unwrap_or_default()
}

/// Milliseconds from Discord's time.
fn unstamp(s: &str) -> Option<u64> {
    let micros = s.parse::<Timestamp>().ok()?.as_micros();
    u64::try_from(micros / 1000).ok()
}

impl Cx {
    /// A pass's context for `c`, with the events the store remembers. None
    /// until the bot's token resolves.
    pub(crate) fn open(
        core: &Arc<Core>,
        c: &CalendarConfig,
        names: HashMap<String, String>,
        lead_ms: u64,
        tell: Tell,
    ) -> Option<Self> {
        let saved: Saved = core
            .store
            .get_meta(META_KEY)
            .ok()
            .flatten()
            .unwrap_or_default();
        let entries = if saved.guild == c.guild {
            saved.entries
        } else {
            Vec::new()
        };
        Some(Self {
            core: Arc::clone(core),
            rest: Rest::of(core)?,
            guild: c.guild.parse().ok()?,
            guild_id: c.guild.clone(),
            days: u64::from(c.days),
            max: c.max_events as usize,
            entries,
            foreign: 0,
            listed: false,
            told: HashSet::new(),
            tell,
            names,
            lead_ms,
        })
    }

    /// Tell health once per kind of trouble.
    fn say(&mut self, kind: &'static str, why: String) {
        if self.told.insert(kind) {
            (self.tell)(why);
        }
    }

    fn save(&self) {
        let saved = Saved {
            guild: self.guild_id.clone(),
            entries: self.entries.clone(),
        };
        if let Err(e) = self.core.store.put_meta(META_KEY, &saved) {
            tracing::warn!(error = %format!("{e:#}"), "discord: the calendar's events are not saved");
        }
    }

    /// One pass. The time of the earliest event still standing, when there is one.
    pub(crate) async fn pass(&mut self) -> Option<u64> {
        let core = Arc::clone(&self.core);
        if core.push.ensure(&core).await.is_err() {
            return self.earliest();
        }
        let now = core.kernel.now_ms();
        let agenda = core.push.work_agenda(
            &AgendaParams {
                from_ms: now,
                to_ms: now + self.days * 86_400_000,
                roots: None,
            },
            &WorkScope::default(),
            &core.kernel.config().zone,
        );
        let _ = self.apply(&agenda.lines, now).await;
        self.earliest()
    }

    fn earliest(&self) -> Option<u64> {
        self.entries.iter().map(|e| e.at_ms).min()
    }

    async fn apply(
        &mut self,
        lines: &[theseus_protocol::work::horizon::AgendaLine],
        now: u64,
    ) -> Result<(), Abort> {
        if !self.listed {
            self.reconcile().await?;
        }
        let core = Arc::clone(&self.core);
        let names = self.names.clone();
        let want = plan::items(lines, now, self.lead_ms, self.max, |l| {
            let Some(sid) = l.session_id.as_deref() else {
                return Shown::default();
            };
            if !core.runner.speaks_privately(sid) {
                return Shown::default();
            }
            let place = core
                .outbox
                .try_target(sid)
                .ok()
                .flatten()
                .and_then(|t| t.strip_prefix("discord:channel:").map(str::to_string))
                .and_then(|id| names.get(&id).cloned());
            Shown { title: true, place }
        });
        let mut ops = plan::diff(&self.entries, &want, now, usize::MAX);
        // The guild's room: its limit, less what stands and is not ours, less
        // what stands of ours, plus what this pass removes.
        let removals = ops
            .iter()
            .filter(|o| matches!(o, Op::Settle(_) | Op::Delete(_)))
            .count();
        let room = (GUILD_EVENT_LIMIT as usize).saturating_sub(self.foreign + self.entries.len())
            + removals;
        let mut made = 0;
        let mut held_back = false;
        ops.retain(|o| {
            if !matches!(o, Op::Create(_)) {
                return true;
            }
            made += 1;
            held_back |= made > room;
            made <= room
        });
        if held_back {
            self.say(
                "full",
                format!(
                    "the guild holds {} events and Discord allows {GUILD_EVENT_LIMIT}: the calendar \
                     waits for room",
                    self.foreign + self.entries.len()
                ),
            );
        }
        for op in ops {
            self.write(op).await?;
        }
        Ok(())
    }

    /// The first pass of a run (and one in six hours): the guild's events
    /// listed, ours matched to what the store holds by their marker, a
    /// doubled one deleted, one that is gone forgotten, one the store lost
    /// adopted, the rest counted as the guild's own.
    async fn reconcile(&mut self) -> Result<(), Abort> {
        let listed = match self.rest.list(self.guild).await {
            Ok(l) => l,
            Err(f) => return self.refused("list", &f),
        };
        let mut found: Vec<Entry> = Vec::new();
        let mut foreign = 0;
        for ev in &listed {
            let status = ev["status"].as_u64().unwrap_or(0) as u8;
            if !(1..=2).contains(&status) {
                continue;
            }
            let marked = plan::marker_of(ev["description"].as_str().unwrap_or(""));
            let at = ev["scheduled_start_time"].as_str().and_then(unstamp);
            let (Some(key), Some(at_ms)) = (marked, at) else {
                foreign += 1;
                continue;
            };
            found.push(Entry {
                key: key.to_string(),
                event_id: ev["id"].as_str().unwrap_or_default().to_string(),
                at_ms,
                end_ms: ev["scheduled_end_time"]
                    .as_str()
                    .and_then(unstamp)
                    .unwrap_or(at_ms),
                name: ev["name"].as_str().unwrap_or_default().to_string(),
                location: ev["entity_metadata"]["location"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                status,
            });
        }
        // One event to a key: the one the store names, else the first listed.
        let mut ours: Vec<Entry> = Vec::new();
        let mut doubled: Vec<String> = Vec::new();
        for e in &found {
            let named = |x: &Entry| {
                self.entries
                    .iter()
                    .any(|k| k.key == x.key && k.event_id == x.event_id)
            };
            let rivals: Vec<&Entry> = found.iter().filter(|x| x.key == e.key).collect();
            let keeper = rivals
                .iter()
                .find(|x| named(x))
                .or(rivals.first())
                .map(|x| x.event_id.as_str());
            if keeper == Some(e.event_id.as_str()) {
                ours.push(e.clone());
            } else {
                doubled.push(e.event_id.clone());
            }
        }
        self.foreign = foreign;
        for id in doubled {
            if let Err(f) = self.rest.delete(self.guild, &id).await {
                if !f.gone() {
                    self.refused("delete", &f)?;
                }
            }
        }
        self.entries = ours;
        self.listed = true;
        self.save();
        Ok(())
    }

    /// A refusal: the bot may not manage events, Discord is not answering, or
    /// something else. Told once in health, except what passes.
    fn refused(&mut self, what: &str, f: &Fail) -> Result<(), Abort> {
        if f.forbidden() {
            self.say(
                "forbidden",
                format!(
                    "Discord refused it (403): the bot needs the Manage Events permission in guild {}; \
                     the calendar writes nothing until it has it",
                    self.guild
                ),
            );
            return Err(Abort);
        }
        if f.transient() {
            tracing::debug!(%f, what, "discord: the calendar will try again");
            return Err(Abort);
        }
        self.say("refused", format!("{what}: {f}"));
        Err(Abort)
    }

    /// One write, and the store told of it.
    async fn write(&mut self, op: Op) -> Result<(), Abort> {
        match op {
            Op::Settle(e) => self.settle(e).await,
            Op::Delete(e) => {
                match self.rest.delete(self.guild, &e.event_id).await {
                    Ok(()) => {}
                    Err(f) if f.gone() => {}
                    Err(f) => return self.refused("delete", &f),
                }
                self.forget(&e.key);
                Ok(())
            }
            Op::Edit(e, i) => {
                let body = json!({
                    "name": i.name,
                    "description": plan::description(i.kind, &i.key),
                    "scheduled_start_time": stamp(i.at_ms),
                    "scheduled_end_time": stamp(i.end_ms),
                    "entity_metadata": {"location": i.location},
                });
                match self.rest.edit(self.guild, &e.event_id, &body).await {
                    Ok(_) => {
                        self.remember(Entry {
                            event_id: e.event_id,
                            status: e.status,
                            ..entry_of(&i, "")
                        });
                        Ok(())
                    }
                    Err(f) if f.gone() => {
                        self.forget(&e.key);
                        Ok(())
                    }
                    Err(f) => self.refused("edit", &f),
                }
            }
            Op::Create(i) => {
                let body = json!({
                    "name": i.name,
                    "description": plan::description(i.kind, &i.key),
                    "scheduled_start_time": stamp(i.at_ms),
                    "scheduled_end_time": stamp(i.end_ms),
                    "entity_type": 3,
                    "privacy_level": 2,
                    "entity_metadata": {"location": i.location},
                });
                match self.rest.create(self.guild, &body).await {
                    Ok(v) => {
                        let id = v["id"].as_str().unwrap_or_default();
                        self.remember(entry_of(&i, id));
                        Ok(())
                    }
                    Err(f) if f.full() => {
                        self.say(
                            "full",
                            format!("Discord refused a new event ({f}): the guild is full; the calendar waits for room"),
                        );
                        Ok(())
                    }
                    Err(f) => self.refused("create", &f),
                }
            }
        }
    }

    /// An event whose time has passed: active, then completed. Discord moves
    /// a status forward only, so a scheduled event is started first; an event
    /// it will not finish is let go (it is Discord's to end).
    async fn settle(&mut self, e: Entry) -> Result<(), Abort> {
        if e.status == 1 {
            match self
                .rest
                .edit(self.guild, &e.event_id, &json!({"status": 2}))
                .await
            {
                Ok(_) => self.set_status(&e.key, 2),
                Err(f) if f.forbidden() || f.transient() => return self.refused("settle", &f),
                Err(_) => {}
            }
        }
        match self
            .rest
            .edit(self.guild, &e.event_id, &json!({"status": 3}))
            .await
        {
            Ok(_) => {}
            Err(f) if f.forbidden() || f.transient() => return self.refused("settle", &f),
            Err(_) => {}
        }
        self.forget(&e.key);
        Ok(())
    }

    fn set_status(&mut self, key: &str, status: u8) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.key == key) {
            e.status = status;
        }
    }

    fn remember(&mut self, e: Entry) {
        self.entries.retain(|x| x.key != e.key);
        self.entries.push(e);
        self.save();
    }

    fn forget(&mut self, key: &str) {
        self.entries.retain(|x| x.key != key);
        self.save();
    }
}

fn entry_of(i: &Item, id: &str) -> Entry {
    Entry {
        key: i.key.clone(),
        event_id: id.to_string(),
        at_ms: i.at_ms,
        end_ms: i.end_ms,
        name: i.name.clone(),
        location: i.location.clone(),
        status: 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_time_goes_to_discord_and_back() {
        let ms = 1_790_000_123_000;
        assert_eq!(unstamp(&stamp(ms)), Some(ms));
        assert_eq!(unstamp("not a time"), None);
    }
}
