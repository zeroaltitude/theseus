//! The bindings file read while the binding runs (theseus-ocwt): after a
//! change, the binding is what a restart with the new file would make it,
//! with no restart. A child of `runtime`, apart from it for the shape
//! budget's file ceiling.
//!
//! - **The watch.** Every `PERIOD` the file is stat'ed; only when its mtime,
//!   size, or inode moved is it read and parsed, and only when its revision
//!   (`Bindings::revision`) moved is anything done. An editor's save by
//!   rename swaps the inode, so a stat follows it where a watch on the file
//!   would not. A file that does not load changes nothing: the places bound
//!   stay, and the board's detail, the log, and a `discord.error` row say
//!   why. Never unbind on a typo.
//! - **The places, diffed by key** (`channel:<id>`, `dm:<user>`). The core is
//!   told the new file first (`guilds::tell_core`: each place's class, each
//!   guild's word), as a start tells it before any message is read.
//!   - A **removed** place stops at once: its routes go, so a message typed
//!     there starts nothing, its actor ends, its session is unwatched, and
//!     `binds` no longer names it. Its lane is retired: it ends between posts,
//!     so a post it already sent settles as sent, and then it refuses the
//!     rest (`Shared::refuse_unbound`), as a start does.
//!   - An **added** place starts as `start_places` starts one: its lane, its
//!     session (a fresh one says the bind notice), its routes, its actor, and
//!     its viewers read when it is bound private outside a trusted guild. A
//!     place re-added while its old lane still drains keeps that lane.
//!   - A place whose **settings changed** is updated in place: its routes
//!     (who may drive it, `mention_only`), its actor's label, users and spend
//!     limit, and its lane's label. Its lane keeps its messages and its actor
//!     its turn, so a reply streaming there keeps editing its own messages.
//! - **What waits for the next start**, said on the board's detail and in
//!   the log: the check that the bot is in a guild added (the bot's roles in
//!   every guild are read again now), and voice channels added, removed, or
//!   changed. The file's format may change live: both formats read to the
//!   same places.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use theseus_protocol::SessionRef;
use tokio::sync::mpsc;
use twilight_model::id::Id;

use super::{guilds, Board, Place, PlaceMsg, Shared};
use crate::bindings::{snowflake, Bindings, ChannelBinding, DmBinding};
use crate::courier::{Lane, LaneMsg};

/// How often the file is stat'ed.
pub(crate) const PERIOD: Duration = Duration::from_secs(2);

/// What a stat of the file says moved: its mtime, size, and inode. None when
/// it cannot be read.
type Stamp = Option<(Option<std::time::SystemTime>, u64, u64)>;

async fn stamp(path: &Path) -> Stamp {
    use std::os::unix::fs::MetadataExt as _;
    let m = tokio::fs::metadata(path).await.ok()?;
    Some((m.modified().ok(), m.len(), m.ino()))
}

/// Whether a file stamped `s` has stood unchanged for a period or more: its
/// mtime is that old. A file whose mtime is ahead of the clock counts as
/// settled, or it would never be read.
fn settled(s: Stamp) -> bool {
    let Some((Some(mtime), _, _)) = s else {
        return true;
    };
    mtime.elapsed().map_or(true, |age| age >= PERIOD)
}

/// What each watch has done, by file: ticks run to their end, and of them
/// those that saw a stamp the tick before had not. What a test waits on to
/// know the watch has stat'ed a save, with no sleep that hopes a tick passed.
#[cfg(test)]
static TICKS: std::sync::Mutex<BTreeMap<PathBuf, (u64, u64)>> =
    std::sync::Mutex::new(BTreeMap::new());

/// The ticks of the watch of `path` that ended, and the sights among them
/// (a tick whose stamp was new).
#[cfg(test)]
pub(crate) fn ticks(path: &Path) -> (u64, u64) {
    TICKS.lock().unwrap().get(path).copied().unwrap_or_default()
}

/// A tick's end, counted for tests (`ticks`); `.1` is set when it saw a new
/// stamp.
struct Ticked<'a>(#[cfg_attr(not(test), allow(dead_code))] &'a Path, bool);

impl Drop for Ticked<'_> {
    fn drop(&mut self) {
        #[cfg(test)]
        {
            let mut t = TICKS.lock().unwrap();
            let n = t.entry(self.0.to_path_buf()).or_default();
            n.0 += 1;
            n.1 += u64::from(self.1);
        }
    }
}

async fn read(path: &Path) -> anyhow::Result<Bindings> {
    use anyhow::Context as _;
    let text = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("reading bindings file {}", path.display()))?;
    Bindings::parse(&text).with_context(|| format!("bindings file {}", path.display()))
}

/// Watch the file at `path`, whose places `bound` holds as the start bound
/// them, until the process ends.
pub(super) async fn watch(shared: Arc<Shared>, path: PathBuf, mut bound: Bindings) {
    let mut tick = tokio::time::interval(PERIOD);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // The stamp acted on, and the one the last tick saw: a stamp is acted on
    // only once it has held still a period and is no younger than one, so a
    // save seen half written is not taken for the file (theseus-sn2z). The
    // first tick acts on nothing, so a change between the start's read and
    // the watch's is not missed.
    let mut acted: Option<Stamp> = None;
    let mut last: Option<Stamp> = None;
    // What the board says while the file does not load, or what waits for
    // the next start.
    let mut note: Option<String> = None;
    // The last failure said: a save seen mid-write fails twice alike, and is
    // said once.
    let mut said: Option<String> = None;
    // The places whose bind failed, by key, with why: `bound` names them, but
    // they are tried again each tick until they bind (theseus-u6v6).
    let mut failed: BTreeMap<String, String> = BTreeMap::new();
    // What the start bound: what waits for the next start is measured from
    // it, however many files came since (theseus-btt4).
    let started = bound.clone();
    loop {
        tick.tick().await;
        let mut tick_end = Ticked(&path, false);
        if !failed.is_empty() && retry(&shared, &bound, &mut failed).await {
            set(
                &shared.board,
                &mut note,
                join(waits(&started, &bound), unbound(&failed)),
            );
        }
        let now = stamp(&path).await;
        let held = last.replace(now) == Some(now);
        tick_end.1 = !held;
        if acted == Some(now) || !(held && settled(now)) {
            keep(&shared.board, note.as_deref());
            continue;
        }
        acted = Some(now);
        match read(&path).await {
            Err(e) => {
                let why = one_line(&format!("{e:#}"));
                let n = format!(
                    "the bindings file does not load, so revision {} stays bound until it \
                     does: {why}",
                    bound.revision
                );
                if said.as_ref() != Some(&why) {
                    shared.board.error("bindings file", None, &why);
                    said = Some(why);
                }
                set(&shared.board, &mut note, Some(n));
            }
            Ok(new) if new.revision == bound.revision => {
                said = None;
                set(
                    &shared.board,
                    &mut note,
                    join(waits(&started, &bound), unbound(&failed)),
                );
            }
            Ok(mut new) => {
                said = None;
                shared.apply(&bound, &mut new, &mut failed).await;
                bound = new;
                let waits = waits(&started, &bound);
                if let Some(w) = &waits {
                    tracing::info!("discord: {w}");
                }
                set(&shared.board, &mut note, join(waits, unbound(&failed)));
            }
        }
    }
}

/// The places still not bound, said on the board.
fn unbound(failed: &BTreeMap<String, String>) -> Option<String> {
    if failed.is_empty() {
        return None;
    }
    let each: Vec<String> = failed
        .iter()
        .map(|(k, why)| format!("{k}: {why}"))
        .collect();
    Some(format!(
        "a place did not bind, and is tried again every {} s: {}",
        PERIOD.as_secs(),
        each.join("; ")
    ))
}

/// Two notes for the board's one line.
fn join(a: Option<String>, b: Option<String>) -> Option<String> {
    match (a, b) {
        (Some(a), Some(b)) => Some(format!("{a}; {b}")),
        (a, b) => a.or(b),
    }
}

/// Try the places that failed to bind again, each against the file bound now:
/// one the file dropped leaves the set, and one that binds does too. Whether
/// the set changed.
async fn retry(
    shared: &Arc<Shared>,
    bound: &Bindings,
    failed: &mut BTreeMap<String, String>,
) -> bool {
    let before = failed.len();
    let list = places(bound);
    let mut bound_now = Vec::new();
    for k in failed.keys().cloned().collect::<Vec<_>>() {
        let Some((_, p)) = list.iter().find(|(o, _)| *o == k) else {
            failed.remove(&k);
            continue;
        };
        if shared.bind(p).await.is_ok() {
            failed.remove(&k);
            bound_now.push(k);
        }
    }
    shared.check_privates(bound, &bound_now);
    failed.len() != before
}

/// A parse error on one line, for health's one line: TOML's names the
/// place on its first line and the fault on its last, with the file's line
/// and a caret between.
fn one_line(why: &str) -> String {
    let lines: Vec<&str> = why
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    match lines.as_slice() {
        [] => String::new(),
        [one] => (*one).to_string(),
        [first, .., last] => format!("{first}: {last}"),
    }
}

/// The board's detail says `n`, in place of the note before it.
fn set(board: &Board, note: &mut Option<String>, n: Option<String>) {
    let old = std::mem::replace(note, n);
    board.update(|s| {
        if s.detail.is_none() || s.detail == old {
            s.detail.clone_from(note);
        }
    });
}

/// The note stays on the board while it holds, unless the gateway's state
/// says something else there.
fn keep(board: &Board, note: Option<&str>) {
    let Some(n) = note else { return };
    board.update(|s| {
        if s.detail.is_none() {
            s.detail = Some(n.to_string());
        }
    });
}

/// A place's settings, changed by the file (`PlaceMsg::Rebound`).
pub(super) struct Rebound {
    label: String,
    users: Vec<u64>,
    mention_only: bool,
}

impl Place {
    /// Take the file's new settings, keeping the session, the turn, and the
    /// lane's messages.
    pub(super) fn rebound(&mut self, r: Rebound) {
        if r.label != self.label {
            self.shared.board.unplace(&self.label, &self.session_id);
        }
        self.label = r.label;
        self.users = r.users;
        self.mention_only = r.mention_only;
        let _ = self.lane.send(LaneMsg::Label(self.label.clone()));
        self.shared
            .place_limit(&self.key, &self.label, &self.session_id);
        self.report();
    }
}

impl Board {
    /// A place no longer bound, or no longer under `label`, leaves health.
    pub(super) fn unplace(&self, label: &str, session: &str) {
        let this = |p: &theseus_protocol::PlaceStatus| {
            p.label == label && p.session_id.as_deref() == Some(session)
        };
        self.update(|s| s.places.retain(|p| !this(p)));
    }
}

/// A place of a file: a `[[channel]]` or a `[[dm]]`.
#[derive(Clone, Copy)]
enum Spot<'a> {
    Channel(&'a ChannelBinding),
    Dm(&'a DmBinding),
}

impl Spot<'_> {
    fn label(&self) -> String {
        match self {
            Spot::Channel(c) => c.label(),
            Spot::Dm(d) => d.label(),
        }
    }

    /// Whether the place, as `now` in the file `new`, differs from this, as
    /// in `old`: any of its settings, or its class by its guild's word.
    fn differs(&self, old: &Bindings, now: &Spot<'_>, new: &Bindings) -> bool {
        match (self, now) {
            (Spot::Channel(a), Spot::Channel(b)) => {
                a != b || old.is_private(a) != new.is_private(b)
            }
            (Spot::Dm(a), Spot::Dm(b)) => a != b,
            _ => true,
        }
    }
}

/// The places of a file, by key (`channel:<id>`, `dm:<user>`).
fn places(b: &Bindings) -> Vec<(String, Spot<'_>)> {
    let c = b
        .channel
        .iter()
        .map(|c| (format!("channel:{}", c.id), Spot::Channel(c)));
    let d = b.dm.iter().map(|d| (format!("dm:{}", d.user), Spot::Dm(d)));
    c.chain(d).collect()
}

/// What of the bindings `new` waits for the next start, because the start that
/// bound `old` is what runs: said on the board while it holds.
fn waits(old: &Bindings, new: &Bindings) -> Option<String> {
    let ids =
        |b: &Bindings| -> BTreeSet<String> { b.guilds.iter().map(|g| g.id.clone()).collect() };
    let mut waits = Vec::new();
    if ids(old) != ids(new) {
        waits.push("the check that the bot is in each guild");
    }
    if voices(old) != voices(new) {
        waits.push("the voice channels");
    }
    if waits.is_empty() {
        return None;
    }
    Some(format!(
        "bindings revision {} is bound; {} wait for the next start",
        new.revision,
        waits.join(" and ")
    ))
}

/// A voice channel's settings, as `voice::Voice` reads them at the start.
fn voices(b: &Bindings) -> BTreeSet<(String, String, Vec<String>)> {
    let v = b.channel.iter().filter(|c| c.voice);
    v.map(|c| (c.id.clone(), c.label(), c.users.clone()))
        .collect()
}

impl Shared {
    /// A lane: the one writer of `target`'s messages (theseus-q4v).
    pub(super) fn start_lane(
        self: &Arc<Self>,
        target: String,
        kind: &'static str,
        label: String,
        channel: Option<u64>,
        dm_user: Option<u64>,
    ) {
        let (tx, rx) = mpsc::unbounded_channel();
        self.lanes.lock().unwrap().insert(target.clone(), tx);
        let lane = Lane::new(self.clone(), target, kind, label, channel, dm_user);
        tokio::spawn(lane.run(rx));
    }

    /// The DM `user`, labelled `label`, among the bound ones: once, however
    /// often its bind is tried.
    fn add_dm(&self, user: u64, label: String) {
        let mut r = self.routes.lock().unwrap();
        match r.dms.iter_mut().find(|(u, _)| *u == user) {
            Some(d) => d.1 = label,
            None => r.dms.push((user, label)),
        }
    }

    pub(super) fn start_channel_lane(self: &Arc<Self>, c: &ChannelBinding) -> anyhow::Result<()> {
        let id = snowflake("channel id", &c.id)?;
        let target = format!("discord:channel:{}", c.id);
        self.start_lane(target, "channel", c.label(), Some(id), None);
        Ok(())
    }

    pub(super) fn start_dm_lane(self: &Arc<Self>, d: &DmBinding) -> anyhow::Result<()> {
        let user = snowflake("dm user", &d.user)?;
        self.add_dm(user, d.label());
        let target = format!("discord:dm:{}", d.user);
        self.start_lane(target, "dm", d.label(), None, Some(user));
        Ok(())
    }

    /// The channel `c`'s place: its session, its routes, and its actor.
    pub(super) async fn start_channel(self: &Arc<Self>, c: &ChannelBinding) -> anyhow::Result<()> {
        let channel = Id::new(snowflake("channel id", &c.id)?);
        let users = c
            .users
            .iter()
            .map(|u| snowflake("user id", u))
            .collect::<anyhow::Result<Vec<_>>>()?;
        let key = format!("channel:{}", c.id);
        let (label, mention_only) = (c.label(), c.mention_only);
        self.clone()
            .start_place(key, "channel", label, Some(channel), users, mention_only)
            .await
    }

    /// The DM `d`'s place, its channel opened first when Discord answers.
    pub(super) async fn start_dm(self: &Arc<Self>, d: &DmBinding) -> anyhow::Result<()> {
        let user = snowflake("dm user", &d.user)?;
        let channel = match self.dm_channel(user).await {
            Ok(c) => Some(Id::new(c)),
            Err(e) => {
                self.board.error("open DM channel", None, &e.message);
                None
            }
        };
        let key = format!("dm:{}", d.user);
        self.clone()
            .start_place(key, "dm", d.label(), channel, vec![user], false)
            .await
    }

    /// Whether `target`'s lane was retired: its place is no longer bound.
    pub(crate) fn is_retired(&self, target: &str) -> bool {
        self.retired.lock().unwrap().contains(target)
    }

    /// A retired lane's own check, between posts: true, and its lane gone
    /// from `lanes`, when its place is no longer bound. Under the lanes' lock,
    /// so a place re-added meanwhile either keeps this lane or starts another.
    pub(crate) fn lane_retires(&self, target: &str) -> bool {
        let mut lanes = self.lanes.lock().unwrap();
        let gone = self.retired.lock().unwrap().remove(target);
        if gone {
            lanes.remove(target);
        }
        gone
    }

    /// Move the binding from the file `old` to the file `new`.
    async fn apply(
        self: &Arc<Self>,
        old: &Bindings,
        new: &mut Bindings,
        failed: &mut BTreeMap<String, String>,
    ) {
        // The core first, as at a start: a place's class before a message
        // from it is read; a place whose ceiling the config cannot serve is
        // taken out of `new` and stays unbound.
        guilds::tell_core(&self.core, new);
        self.place_bits.replace(new);
        let (was, now) = (places(old), places(new));
        fn find<'a>(list: &[(String, Spot<'a>)], k: &str) -> Option<Spot<'a>> {
            list.iter().find(|(o, _)| o == k).map(|(_, p)| *p)
        }
        let (mut removed, mut changed, mut added) = (Vec::new(), Vec::new(), Vec::new());
        for (k, p) in &was {
            if find(&now, k).is_none() {
                self.unbind(k, &p.label()).await;
                failed.remove(k);
                removed.push(k.clone());
            }
        }
        for (k, p) in &now {
            match find(&was, k) {
                // Added, or a place whose bind failed: bound as the file
                // has it now.
                None => self.bound_or_failed(k, p, failed, &mut added).await,
                Some(_) if failed.contains_key(k) => {
                    self.bound_or_failed(k, p, failed, &mut added).await;
                }
                Some(w) if w.differs(old, p, new) => {
                    self.rebind(k, p);
                    changed.push(k.clone());
                }
                Some(_) => {}
            }
        }
        self.read_new_privates(old, new);
        let guilds = guilds::guild_ids(new).unwrap_or_default();
        self.refresh_bot_roles(&guilds).await;
        self.board.update(|s| {
            s.guild_id = (new.guilds.len() == 1).then(|| new.guilds[0].id.clone());
            s.guilds = new.guild_infos();
            s.revision = Some(new.revision.clone());
        });
        self.refuse_unbound();
        self.wake_lanes();
        tracing::info!(
            revision = %new.revision, ?added, ?removed, ?changed,
            "discord: the bindings file changed"
        );
    }

    /// Bind the place `k`: it joins `added`, or its failure is said once, on
    /// the board and in the record, and kept in `failed` to be tried again.
    async fn bound_or_failed(
        self: &Arc<Self>,
        k: &str,
        p: &Spot<'_>,
        failed: &mut BTreeMap<String, String>,
        added: &mut Vec<String>,
    ) {
        match self.bind(p).await {
            Ok(()) => {
                failed.remove(k);
                added.push(k.to_string());
            }
            Err(e) => {
                let why = one_line(&format!("{e:#}"));
                if failed.insert(k.to_string(), why).is_none() {
                    self.board.error("bind place", None, format!("{k}: {e:#}"));
                }
            }
        }
    }

    /// Each channel newly bound private, outside a trusted guild: its
    /// viewers read once, as at a start (the place rule).
    fn read_new_privates(self: &Arc<Self>, old: &Bindings, new: &Bindings) {
        let read_before: Vec<&str> = old.read_at_start().map(|c| c.id.as_str()).collect();
        let to_read: Vec<(u64, String)> = new
            .read_at_start()
            .filter(|c| !read_before.contains(&c.id.as_str()))
            .filter_map(|c| Some((c.id.parse().ok()?, c.label())))
            .collect();
        self.spawn_checks(to_read);
    }

    /// The viewers of each of `keys`' channels read, when `b` binds them
    /// private outside a trusted guild: a place that bound late.
    fn check_privates(self: &Arc<Self>, b: &Bindings, keys: &[String]) {
        let to_read: Vec<(u64, String)> = b
            .read_at_start()
            .filter(|c| keys.contains(&format!("channel:{}", c.id)))
            .filter_map(|c| Some((c.id.parse().ok()?, c.label())))
            .collect();
        if !to_read.is_empty() {
            self.spawn_checks(to_read);
        }
    }

    fn spawn_checks(self: &Arc<Self>, to_read: Vec<(u64, String)>) {
        let checks = self.clone();
        tokio::spawn(async move {
            for (c, name) in &to_read {
                checks.check_private(*c, name).await;
            }
        });
    }

    /// The place `key` is no longer bound: its routes, its actor, its watch,
    /// and its lane, which ends between posts and refuses the rest.
    async fn unbind(self: &Arc<Self>, key: &str, label: &str) {
        let target = format!("discord:{key}");
        let (place, sid) = {
            let mut r = self.routes.lock().unwrap();
            let place = match key.split_once(':') {
                Some(("channel", id)) => id.parse().ok().and_then(|c: u64| {
                    r.users.remove(&c);
                    r.mention_only.remove(&c);
                    r.by_channel.remove(&c)
                }),
                Some(("dm", u)) => u.parse().ok().and_then(|u: u64| {
                    r.dms.retain(|(d, _)| *d != u);
                    r.by_dm_user.remove(&u)
                }),
                _ => None,
            };
            let sid = place.as_ref().and_then(|tx| {
                let sid = r
                    .by_session
                    .iter()
                    .find(|(_, t)| t.same_channel(tx))
                    .map(|(s, _)| s.clone())?;
                r.by_session.remove(&sid);
                Some(sid)
            });
            (place, sid)
        };
        if let Some(tx) = place {
            let _ = tx.send(PlaceMsg::Unbind);
        }
        {
            let lanes = self.lanes.lock().unwrap();
            if let Some(lane) = lanes.get(&target) {
                self.retired.lock().unwrap().insert(target.clone());
                let _ = lane.send(LaneMsg::Wake);
            }
        }
        if let Some(sid) = sid {
            self.board.unplace(label, &sid);
            let unwatch = self.rpc.call::<_, serde_json::Value>(
                theseus_protocol::method::SESSION_UNWATCH,
                SessionRef { session_id: sid },
            );
            let _ = unwatch.await;
        }
    }

    /// The place `key`'s settings changed: its routes now, its actor and
    /// lane by message.
    fn rebind(&self, key: &str, p: &Spot<'_>) {
        let (label, users, mention_only) = match p {
            Spot::Channel(c) => {
                let users: Vec<u64> = c.users.iter().filter_map(|u| u.parse().ok()).collect();
                (c.label(), users, c.mention_only)
            }
            Spot::Dm(d) => (d.label(), d.user.parse().into_iter().collect(), false),
        };
        let place = {
            let mut r = self.routes.lock().unwrap();
            match key.split_once(':') {
                Some(("channel", id)) => id.parse().ok().and_then(|c: u64| {
                    r.users.insert(c, users.clone());
                    if mention_only {
                        r.mention_only.insert(c);
                    } else {
                        r.mention_only.remove(&c);
                    }
                    r.by_channel.get(&c).cloned()
                }),
                Some(("dm", u)) => u.parse().ok().and_then(|u: u64| {
                    if let Some(d) = r.dms.iter_mut().find(|(d, _)| *d == u) {
                        d.1.clone_from(&label);
                    }
                    r.by_dm_user.get(&u).cloned()
                }),
                _ => None,
            }
        };
        if let Some(tx) = place {
            let r = Rebound {
                label,
                users,
                mention_only,
            };
            let _ = tx.send(PlaceMsg::Rebound(Box::new(r)));
        }
    }

    /// A lane for `target`, unless one still drains there: then that one,
    /// kept. Under the lanes' lock, against the lane's own check.
    fn keep_lane(&self, target: &str) -> bool {
        let lanes = self.lanes.lock().unwrap();
        lanes.contains_key(target) && {
            self.retired.lock().unwrap().remove(target);
            true
        }
    }

    async fn bind(self: &Arc<Self>, p: &Spot<'_>) -> anyhow::Result<()> {
        match p {
            Spot::Channel(c) => self.bind_channel(c).await,
            Spot::Dm(d) => self.bind_dm(d).await,
        }
    }

    async fn bind_channel(self: &Arc<Self>, c: &ChannelBinding) -> anyhow::Result<()> {
        let target = format!("discord:channel:{}", c.id);
        if self.keep_lane(&target) {
            if let Some(l) = self.lane(&target) {
                let _ = l.send(LaneMsg::Label(c.label()));
            }
        } else {
            self.start_channel_lane(c)?;
        }
        self.start_channel(c).await
    }

    async fn bind_dm(self: &Arc<Self>, d: &DmBinding) -> anyhow::Result<()> {
        let target = format!("discord:dm:{}", d.user);
        if self.keep_lane(&target) {
            let user = snowflake("dm user", &d.user)?;
            self.add_dm(user, d.label());
            if let Some(l) = self.lane(&target) {
                let _ = l.send(LaneMsg::Label(d.label()));
            }
        } else {
            self.start_dm_lane(d)?;
        }
        self.start_dm(d).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A TOML error's caret drawing is left out of health's one line.
    #[test]
    fn a_parse_error_is_said_on_one_line() {
        let e = Bindings::parse("guild_id = \"9\n[[channel\n").unwrap_err();
        let why = one_line(&format!("{e:#}"));
        assert!(!why.contains('\n') && !why.contains('^'), "{why}");
        assert!(why.starts_with("TOML parse error at line "), "{why}");
        assert_eq!(
            one_line("no [[channel]] and no [[dm]]"),
            "no [[channel]] and no [[dm]]"
        );
    }
}
