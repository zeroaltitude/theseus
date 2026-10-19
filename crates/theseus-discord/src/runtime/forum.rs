//! The task forum (theseus-i3do6): a forum channel the owner binds
//! (`[[channel]] kind = "forum"`; off unless bound) shows each task as a
//! post. The post's first message is the task's live Components V2 card,
//! edited on change, and its tags are the task's state: `needs you`,
//! `running`, `done`, `failed`. A child of `runtime`.
//!
//! - **One worker.** A place that sees its tasks change tells the worker
//!   (`Place::sync_task_posts`: the tasks, the place's class and guild); the
//!   worker is the only one that talks to the forum, one request at a time,
//!   and keeps what it shows of each post (`Slot`). A task seen working with
//!   no post gets one; the post is renamed never (the tags carry the state).
//! - **The tags** are created once if the forum lacks them (`tag_ids`: its
//!   available tags read, the missing ones added with the others kept; a
//!   forum holds 20, a post 5, and a post here holds one). A tag edit is made
//!   at most once in `TAGS_EVERY` (5 minutes: Discord limits channel edits
//!   tightly), later ones waiting for the latest state; the task's final
//!   state is always written at once (`Pace::wait`). The card is edited at
//!   most once in `CARD_EVERY` with the latest state, and not at all when it
//!   is unchanged.
//! - **The place rule** (`takes`): a private forum (the owner's alone) shows
//!   every task, a shared one only the tasks of shared places in its own guild,
//!   so a private title never reaches it. The card has no button that acts: a
//!   post is no place that answers, so it links to the task's thread, and
//!   the DM's board links to the post (`TaskPosts::url_of`).
//! - **What a restart finds.** The open posts are one META list
//!   (`discord.task_posts`, no store format: a key), written at a post's
//!   creation and when its task's last state is shown. A task's post is
//!   found there; a task already over with no post gets none.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use theseus_core::places::PlaceClass;
use theseus_protocol::work::{WorkKind, WorkState, WorkView};
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;
use twilight_http::request::Request;
use twilight_http::routing::Route as HttpRoute;

use super::default_forum::{DefaultForum, Home};
use super::threads::{task_name, Mark};
use super::{Place, Shared};
use crate::bindings::Bindings;
use crate::render::{board_v2, task_view, v2};

/// A card is edited at most once in this long.
pub(crate) const CARD_EVERY: Duration = Duration::from_secs(2);

/// A post's tags are edited at most once in this long, bar the final state.
pub(crate) const TAGS_EVERY: Duration = Duration::from_secs(300);

/// A failed create is tried again after this long.
const RETRY: Duration = Duration::from_secs(60);

/// A post archives itself after a day of quiet.
const ARCHIVE_MINUTES: u64 = 1440;

/// META key of the open posts.
const OPEN_KEY: &str = "discord.task_posts";

/// A post's name is at most this long.
const NAME_MAX: usize = 100;

/// A forum's tag for a task's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Tag {
    NeedsYou,
    Running,
    Done,
    Failed,
    /// The owner paused the task (theseus-ypnxt.2).
    Paused,
    /// It was cancelled or expired.
    Stopped,
}

impl Tag {
    pub(crate) const ALL: [Tag; 6] = [
        Tag::NeedsYou,
        Tag::Running,
        Tag::Done,
        Tag::Failed,
        Tag::Paused,
        Tag::Stopped,
    ];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Tag::NeedsYou => "needs you",
            Tag::Running => "running",
            Tag::Done => "done",
            Tag::Failed => "failed",
            Tag::Paused => "paused",
            Tag::Stopped => "stopped",
        }
    }

    /// The tag of a task's view: its state, but a paused task that has not
    /// ended is `paused`, and a task stopped by hand or by time `stopped`.
    pub(crate) fn of(view: &WorkView) -> Self {
        match view.state {
            WorkState::NeedsYou => Tag::NeedsYou,
            WorkState::Done => Tag::Done,
            WorkState::Failed => Tag::Failed,
            WorkState::Cancelled | WorkState::Expired => Tag::Stopped,
            _ if view.paused => Tag::Paused,
            _ => Tag::Running,
        }
    }
}

/// A forum the bindings file binds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Forum {
    pub channel: u64,
    pub guild: String,
    /// Only the owner can view it (the place rule: its channel's own word,
    /// else its guild's).
    pub private: bool,
    /// The default forum (theseus-bh3t.6), not one the file binds.
    pub default: bool,
}

/// Whether `forum` shows the tasks of a place of this class and guild: a
/// private forum all of them, a shared one the shared places' of its guild.
pub(crate) fn takes(forum: &Forum, home_private: bool, home_guild: Option<&str>) -> bool {
    forum.private || (!home_private && home_guild == Some(forum.guild.as_str()))
}

/// How long to wait before a write that may be made once in `every`; none
/// when it is due now: none was made yet, `every` has passed, or it is the
/// last one (a task's final state is always written).
pub(crate) fn wait(
    last: Option<Instant>,
    every: Duration,
    now: Instant,
    is_final: bool,
) -> Option<Duration> {
    let last = last.filter(|_| !is_final)?;
    let due = last + every;
    (due > now).then(|| due - now)
}

/// A task's post, as the list keeps it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Post {
    pub task: String,
    pub forum: u64,
    /// The post's thread id, which is also its first message's id.
    pub post: u64,
    pub guild: String,
    pub created_ms: u64,
    /// When its task's last state was shown: the post stays in the list a day
    /// more, for the board's link to it.
    #[serde(default)]
    pub ended_ms: Option<u64>,
    /// Where the task works, when this post is only its index card.
    #[serde(default)]
    pub link: Option<String>,
    /// For an index card whose task has no place of its own: the task's session
    /// and the key of the place that told of it, so a restart binds the post
    /// as the place its cards go to (theseus-90qqy).
    #[serde(default)]
    pub bound: Option<(String, String)>,
}

/// How long a finished task's post stays in the list for its link (the board
/// shows a finished task a day).
const LINK_KEPT_MS: u64 = 24 * 3_600_000;

/// A place's tasks, told to the worker.
pub(super) struct Seen {
    views: Vec<WorkView>,
    private: bool,
    guild: Option<String>,
    /// Each task's thread link, for its card.
    links: HashMap<String, String>,
    /// The place that told, which draws its board again when a post is made.
    home: Option<mpsc::UnboundedSender<super::PlaceMsg>>,
    /// The tasks whose post in the default forum is their place: made on
    /// request (`TaskPosts::make`), never by the worker's own sync.
    homed: HashSet<String>,
    /// The key of the place that told, for a card's bound place.
    parent: Option<String>,
}

/// A request for the post that is a task's place.
struct Make {
    view: WorkView,
    reply: oneshot::Sender<anyhow::Result<(u64, Option<u64>)>>,
}

/// The binding's forums and posts, in `Shared`.
#[derive(Default)]
pub(crate) struct TaskPosts {
    tx: OnceLock<mpsc::UnboundedSender<Seen>>,
    make_tx: OnceLock<mpsc::UnboundedSender<Make>>,
    forums: Mutex<Vec<Forum>>,
    pub(super) posts: Mutex<Vec<Post>>,
    loaded: AtomicBool,
    /// The default forum (theseus-bh3t.6), its own slot: a bindings reload
    /// replaces `forums`.
    pub(super) default: DefaultForum,
    /// The task home (theseus-xlxp4), its own slot beside the forum's.
    pub(super) home: super::task_home::TaskHome,
    /// Where a task whose post was its place works now, by task.
    moved: Mutex<HashMap<String, String>>,
}

impl TaskPosts {
    /// The forums the file binds now.
    pub(crate) fn set_forums(&self, b: &Bindings) {
        let forums = b
            .forums
            .iter()
            .filter_map(|f| {
                Some(Forum {
                    channel: f.id.parse().ok()?,
                    guild: f.guild.clone(),
                    private: b.is_private(f),
                    default: false,
                })
            })
            .collect();
        *self.forums.lock().unwrap() = forums;
    }

    /// The bound forums and the default one when it stands.
    fn forums(&self) -> Vec<Forum> {
        let mut forums = self.forums.lock().unwrap().clone();
        if let Some(d) = self.default.forum() {
            forums.retain(|f| f.channel != d.channel);
            forums.push(d);
        }
        forums
    }

    /// Whether a forum stands or may soon: a place has tasks to tell it.
    fn wants_tasks(&self) -> bool {
        !self.forums.lock().unwrap().is_empty() || self.default.may_stand()
    }

    pub(super) fn load(&self, store: &theseus_core::store::Store) {
        if self.loaded.swap(true, Ordering::SeqCst) {
            return;
        }
        let now = theseus_protocol::now_unix_ms();
        let list: Vec<Post> = store.get_meta(OPEN_KEY).ok().flatten().unwrap_or_default();
        *self.posts.lock().unwrap() = list
            .into_iter()
            .filter(|p| {
                p.ended_ms
                    .is_none_or(|e| now.saturating_sub(e) < LINK_KEPT_MS)
            })
            .collect();
    }

    fn save(&self, store: &theseus_core::store::Store) {
        let list = self.posts.lock().unwrap().clone();
        if let Err(e) = store.put_meta(OPEN_KEY, &list) {
            tracing::warn!(error = %format!("{e:#}"), "the open task posts were not saved");
        }
    }

    /// The link to a task's post, if it has one (the default forum's first).
    pub(crate) fn url_of(&self, task: &str) -> Option<String> {
        let posts = self.posts.lock().unwrap();
        let default = self.default.forum().map(|f| f.channel);
        let mut of = posts.iter().filter(|p| p.task == task);
        let p = of
            .clone()
            .find(|p| Some(p.forum) == default)
            .or(of.next())?;
        Some(format!(
            "https://discord.com/channels/{}/{}",
            p.guild, p.post
        ))
    }
}

impl TaskPosts {
    /// The post that is the task's place, made by the worker, the one maker
    /// of posts: its id and its guild.
    pub(crate) async fn make(&self, view: WorkView) -> anyhow::Result<(u64, Option<u64>)> {
        let tx = self
            .make_tx
            .get()
            .ok_or_else(|| anyhow::anyhow!("no forum worker"))?;
        let (reply, rx) = oneshot::channel();
        tx.send(Make { view, reply })
            .map_err(|_| anyhow::anyhow!("the forum worker has stopped"))?;
        rx.await?
    }

    /// A task whose post was its place now works at `url`: the post stays
    /// listed and live as the task's index card, its tag following the state.
    pub(crate) fn moved(&self, task: &str, url: &str) {
        self.moved
            .lock()
            .unwrap()
            .insert(task.to_string(), url.to_string());
        if let Some(tx) = self.tx.get() {
            let _ = tx.send(Seen::nudge());
        }
    }

    /// The id of the task's post, whether it works there or the post is only
    /// its index card.
    pub(crate) fn post_of(&self, task: &str) -> Option<u64> {
        let posts = self.posts.lock().unwrap();
        let default = self.default.forum().map(|f| f.channel);
        let mut of = posts.iter().filter(|p| !task.is_empty() && p.task == task);
        let p = of
            .clone()
            .find(|p| Some(p.forum) == default)
            .or(of.next())?;
        Some(p.post)
    }

    /// The worker keeps a post for the task.
    pub(crate) fn owns(&self, task: &str) -> bool {
        self.posts.lock().unwrap().iter().any(|p| p.task == task)
    }

    /// `channel` is a task's index card: where the task works ("" when it
    /// has no other place). A card's messages start nothing.
    pub(crate) fn index_of(&self, channel: u64) -> Option<String> {
        let posts = self.posts.lock().unwrap();
        let p = posts.iter().find(|p| p.post == channel)?;
        p.link.clone().or(Some(String::new()))
    }

    /// Tell the worker to look again (the default forum came up).
    pub(super) fn nudge(&self) {
        if let Some(tx) = self.tx.get() {
            let _ = tx.send(Seen::nudge());
        }
    }
}

impl Seen {
    /// No tasks: the worker is woken to do what is due.
    fn nudge() -> Self {
        Self {
            views: Vec::new(),
            private: false,
            guild: None,
            links: HashMap::new(),
            home: None,
            homed: HashSet::new(),
            parent: None,
        }
    }
}

/// A task as the worker last heard it.
struct Entry {
    view: WorkView,
    private: bool,
    guild: Option<String>,
    link: Option<String>,
    home: Option<mpsc::UnboundedSender<super::PlaceMsg>>,
    homed: bool,
    parent: Option<String>,
}

/// What the worker shows of one post.
struct Slot {
    post: Post,
    /// The card's digest as last sent, and when.
    card: Option<u64>,
    card_at: Option<Instant>,
    tag: Option<Tag>,
    tag_at: Option<Instant>,
}

/// The two intervals (a test shortens them).
#[derive(Clone, Copy)]
struct Every {
    card: Duration,
    tags: Duration,
}

struct Worker {
    shared: Arc<Shared>,
    every: Every,
    entries: HashMap<String, Entry>,
    slots: HashMap<(String, u64), Slot>,
    tags: HashMap<u64, HashMap<Tag, String>>,
    failed: HashMap<(String, u64), Instant>,
}

fn digest(s: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

/// A post's name: the task's short id and its title, within Discord's limit.
pub(crate) fn post_name(task: &str, title: &str) -> String {
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let head = format!("{} · ", board_v2::short(task));
    let room = NAME_MAX.saturating_sub(head.chars().count());
    let title: String = title.chars().take(room).collect();
    format!("{head}{}", if title.is_empty() { "task" } else { &title })
}

impl Shared {
    /// Start the forum worker, once, and read the forums the file binds.
    pub(super) fn start_task_forums(self: &Arc<Self>, b: &Bindings) {
        self.task_posts.set_forums(b);
        self.task_posts.load(&self.core.store);
        let (tx, rx) = mpsc::unbounded_channel();
        let (make_tx, make_rx) = mpsc::unbounded_channel();
        if self.task_posts.tx.set(tx).is_ok() && self.task_posts.make_tx.set(make_tx).is_ok() {
            tokio::spawn(run(self.clone(), rx, make_rx));
        }
        self.start_default_forum(b);
    }

    /// Where each task of the projection's parts has a thread or a post, for
    /// the board's rows (a post first: it is the card).
    pub(crate) fn task_links(&self, p: &super::tasks::Projection) -> HashMap<String, String> {
        p.needs
            .iter()
            .chain(&p.working)
            .chain(&p.done)
            .filter_map(|v| {
                let url = self
                    .task_posts
                    .url_of(&v.id)
                    .or_else(|| self.task_threads.url_of(&v.id))?;
                Some((v.id.clone(), url))
            })
            .collect()
    }
}

impl Place {
    /// This place's tasks, told to the forum worker when a forum is bound.
    pub(super) fn sync_task_posts(&self) {
        let s = &self.shared;
        let Some(tx) = s.task_posts.tx.get() else {
            return;
        };
        if !s.task_posts.wants_tasks() || self.session_id.is_empty() {
            return;
        }
        let views = s.tasks.all_of(&self.session_id);
        if views.is_empty() {
            return;
        }
        let class = s
            .core
            .runner
            .place_rule
            .class(&s.core.cfg, Some(&self.target));
        let links = views
            .iter()
            .filter_map(|v| Some((v.id.clone(), s.task_threads.url_of(&v.id)?)))
            .collect();
        let homed = match self.home() {
            // A task with a channel of its own works there: its post is an
            // index card (task channels, theseus-bh3t.1).
            Home::Post => views
                .iter()
                .filter(|v| !s.task_threads.has_channel(&v.id))
                .map(|v| v.id.clone())
                .collect(),
            _ => HashSet::new(),
        };
        let _ = tx.send(Seen {
            views,
            private: class == PlaceClass::Private,
            guild: s.place_bits.get(&self.key).guild,
            links,
            home: Some(self.tx.clone()),
            homed,
            parent: Some(self.key.clone()),
        });
    }
}

/// The worker: each place's tasks as they change, and the waits for the
/// edits that were held back, until the daemon stops.
async fn run(
    shared: Arc<Shared>,
    mut rx: mpsc::UnboundedReceiver<Seen>,
    mut makes: mpsc::UnboundedReceiver<Make>,
) {
    let core = shared.core.clone();
    let stopped = core.outbox.stopped();
    tokio::pin!(stopped);
    let mut w = Worker {
        shared,
        every: Every {
            card: CARD_EVERY,
            tags: TAGS_EVERY,
        },
        entries: HashMap::new(),
        slots: HashMap::new(),
        tags: HashMap::new(),
        failed: HashMap::new(),
    };
    let mut next: Option<Instant> = None;
    loop {
        let sleep = async {
            match next {
                Some(t) => tokio::time::sleep_until(t).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            s = rx.recv() => match s {
                Some(s) => w.see(s),
                None => break,
            },
            Some(m) = makes.recv() => {
                let made = w.make_post(&m.view).await;
                let _ = m.reply.send(made);
            }
            () = sleep => {}
            () = &mut stopped => break,
        }
        next = w.drive().await;
    }
}

impl Worker {
    fn see(&mut self, s: Seen) {
        for v in s.views {
            if self
                .entries
                .get(&v.id)
                .is_some_and(|e| e.view.position > v.position)
            {
                continue;
            }
            let link = s.links.get(&v.id).cloned();
            let homed = s.homed.contains(&v.id);
            self.entries.insert(
                v.id.clone(),
                Entry {
                    view: v,
                    private: s.private,
                    guild: s.guild.clone(),
                    link,
                    home: s.home.clone(),
                    homed,
                    parent: s.parent.clone(),
                },
            );
        }
    }

    /// Do what is due; the time the next held-back write is due, if any.
    async fn drive(&mut self) -> Option<Instant> {
        let forums = self.shared.task_posts.forums();
        let mut next: Option<Instant> = None;
        let mut keep = |t: Option<Duration>, now: Instant| {
            if let Some(d) = t {
                let at = now + d;
                next = Some(next.map_or(at, |n| n.min(at)));
            }
        };
        let ids: Vec<String> = self.entries.keys().cloned().collect();
        for id in ids {
            for f in &forums {
                let Some(e) = self.entries.get(&id) else {
                    break;
                };
                if !takes(f, e.private, e.guild.as_deref()) {
                    continue;
                }
                let now = Instant::now();
                let held = self.one(&id, f).await;
                keep(held, now);
            }
            // A task over, its last state shown everywhere: forgotten.
            let over = self.entries.get(&id).is_some_and(|e| e.view.state.ended());
            if over && !self.slots.keys().any(|(t, _)| *t == id) {
                self.entries.remove(&id);
            }
        }
        let now = Instant::now();
        for t in self.failed.values() {
            if *t > now {
                keep(Some(*t - now), now);
            }
        }
        next
    }

    /// A post the last run made, taken up again: its card and tag are sent
    /// anew, never a second post. None: the list has no post for the task;
    /// Some: it has, and what `one` answers (a post already over is left).
    fn adopt(&mut self, key: &(String, u64)) -> Option<Option<Duration>> {
        let post = self
            .shared
            .task_posts
            .posts
            .lock()
            .unwrap()
            .iter()
            .find(|p| p.task == key.0 && p.forum == key.1)
            .cloned()?;
        if post.ended_ms.is_some() {
            return Some(None);
        }
        self.slots.insert(
            key.clone(),
            Slot {
                post,
                card: None,
                card_at: None,
                tag: None,
                tag_at: None,
            },
        );
        // Look at it again at once, now that it has a slot.
        Some(Some(Duration::ZERO))
    }

    /// What the worker shows of the task `id` in `f`: its view, the link its
    /// card carries, whether its post is its place (then it links nowhere),
    /// and whether the post is an index card of the default forum. A task
    /// that moved links where it works now (theseus-bh3t.6).
    fn shown(&self, id: &str, f: &Forum) -> Option<(WorkView, Option<String>, bool, bool)> {
        let e = self.entries.get(id)?;
        let homed = e.homed && f.default;
        let moved = self
            .shared
            .task_posts
            .moved
            .lock()
            .unwrap()
            .get(id)
            .cloned();
        let index = f.default && (!homed || moved.is_some());
        let link = moved.or(e.link.clone().filter(|_| !homed));
        Some((e.view.clone(), link, homed, index))
    }

    /// One task in one forum: its post made, its card and tags brought up to
    /// date as far as the pace allows. How long the rest must wait, if any.
    async fn one(&mut self, id: &str, f: &Forum) -> Option<Duration> {
        let key = (id.to_string(), f.channel);
        let (view, link, homed, index) = self.shown(id, f)?;
        let ended = view.state.ended();
        let now = Instant::now();
        if !self.slots.contains_key(&key) {
            // An index card made before its task's thread was: it gets its link.
            if index {
                self.reopen_for_link(&key, link.as_deref());
            }
            // A post the last run made (the list has it): taken up again.
            if let Some(held) = self.adopt(&key) {
                return held;
            }
            // A task's own post is made on request, not by this sync.
            if ended || homed {
                return None;
            }
            if let Some(t) = self.failed.get(&key).filter(|t| **t > now) {
                return Some(*t - now);
            }
            let name = post_name(&view.id, &view.title);
            if let Err(e) = self.create(&key, f, &view, link.as_deref(), name).await {
                self.shared.board.error(
                    "task forum",
                    Some(&view.session_id.clone().unwrap_or_default()),
                    format!("{e:#}"),
                );
                self.failed.insert(key, Instant::now() + RETRY);
                return Some(RETRY);
            }
            self.failed.remove(&key);
            if index {
                self.keep_index(&key, link.as_deref(), id);
                self.card_made(&key, id);
            }
            return None;
        }
        let mut held: Option<Duration> = None;
        let mut earliest = |d: Option<Duration>| {
            if let Some(d) = d {
                held = Some(held.map_or(d, |h| h.min(d)));
            }
        };
        if index {
            self.keep_index(&key, link.as_deref(), id);
        }
        // The card.
        let card = self.card_of(&view, link.as_deref());
        let sent = digest(&serde_json::to_string(&card).unwrap_or_default());
        let slot = self.slots.get(&key)?;
        if slot.card != Some(sent) {
            match wait(slot.card_at, self.every.card, now, ended) {
                None => {
                    if self.edit_card(&key, &card).await {
                        if let Some(s) = self.slots.get_mut(&key) {
                            s.card = Some(sent);
                            s.card_at = Some(Instant::now());
                        }
                    } else {
                        earliest(Some(RETRY));
                    }
                }
                w => earliest(w),
            }
        }
        // The tags.
        let want = Tag::of(&view);
        let slot = self.slots.get(&key)?;
        if slot.tag != Some(want) {
            match wait(slot.tag_at, self.every.tags, now, ended) {
                None => {
                    if self.set_tag(&key, f, want).await {
                        if let Some(s) = self.slots.get_mut(&key) {
                            s.tag = Some(want);
                            s.tag_at = Some(Instant::now());
                        }
                    } else {
                        earliest(Some(RETRY));
                    }
                }
                w => earliest(w),
            }
        }
        // Over, and its last state shown: the post leaves the list.
        let slot = self.slots.get(&key)?;
        if ended && slot.card == Some(sent) && slot.tag == Some(want) {
            self.slots.remove(&key);
            let s = &self.shared;
            let mut bound = None;
            for p in s.task_posts.posts.lock().unwrap().iter_mut() {
                if p.task == key.0 && p.forum == key.1 {
                    p.ended_ms = Some(theseus_protocol::now_unix_ms());
                    bound = p.bound.clone().map(|b| (b.0, p.post));
                }
            }
            s.task_posts.save(&s.core.store);
            if let Some((session, post)) = bound {
                s.retire_card_place_later(session, post);
            }
        }
        held
    }

    fn card_of(&self, view: &WorkView, link: Option<&str>) -> Vec<Value> {
        let calls: Vec<String> = self
            .shared
            .tasks
            .children_of(&view.id, WorkKind::Job)
            .into_iter()
            .map(|j| j.title)
            .collect();
        task_view::post_card(view, &calls, link)
    }

    /// The post that is a task's place, in the default forum: the one the list
    /// has (a restart), else made now. Its id and its guild.
    async fn make_post(&mut self, view: &WorkView) -> anyhow::Result<(u64, Option<u64>)> {
        let f = self
            .shared
            .task_posts
            .default
            .forum()
            .ok_or_else(|| anyhow::anyhow!("the default forum is not standing"))?;
        let key = (view.id.clone(), f.channel);
        if !self.slots.contains_key(&key) && self.adopt(&key).is_none() {
            let name = task_name(Mark::of(view.state), &view.id, &view.title);
            self.create(&key, &f, view, None, name).await?;
        }
        let posts = self.shared.task_posts.posts.lock().unwrap();
        let post = posts
            .iter()
            .find(|p| p.task == key.0 && p.forum == key.1)
            .ok_or_else(|| anyhow::anyhow!("the task's post is gone"))?;
        Ok((post.post, f.guild.parse().ok()))
    }

    /// A finished index card that never had its link, and has it now, is
    /// worked once more so that its card shows it.
    fn reopen_for_link(&self, key: &(String, u64), link: Option<&str>) {
        if link.is_none() {
            return;
        }
        let s = &self.shared;
        let mut posts = s.task_posts.posts.lock().unwrap();
        let stale = posts.iter_mut().find(|p| {
            p.task == key.0 && p.forum == key.1 && p.ended_ms.is_some() && p.link.is_none()
        });
        if let Some(p) = stale {
            p.ended_ms = None;
        }
    }

    /// An index card in the default forum: where the task works is kept with
    /// the post (a restart finds it), and a message in it is heard by the
    /// place that told of the task, which answers with a pointer.
    fn keep_index(&self, key: &(String, u64), link: Option<&str>, task: &str) {
        let s = &self.shared;
        let changed = {
            let mut posts = s.task_posts.posts.lock().unwrap();
            posts
                .iter_mut()
                .find(|p| p.task == key.0 && p.forum == key.1)
                .is_some_and(|p| {
                    let changed = p.link.as_deref() != link;
                    p.link = link.map(str::to_string);
                    changed
                })
        };
        if changed {
            s.task_posts.save(&s.core.store);
        }
        let post = self.slots.get(key).map(|slot| slot.post.post);
        let home = self.entries.get(task).and_then(|e| e.home.clone());
        if let (Some(post), Some(home)) = (post, home) {
            s.route_index(post, home);
        }
    }

    /// An index card was made: one quiet pointer to it in the task home, and,
    /// for a task with no place of its own, the card is the place its session's
    /// cards go to (theseus-xlxp4, theseus-90qqy).
    fn card_made(&self, key: &(String, u64), task: &str) {
        let s = &self.shared;
        let (Some(slot), Some(e)) = (self.slots.get(key), self.entries.get(task)) else {
            return;
        };
        let Some(session) = e.view.session_id.clone() else {
            return;
        };
        let post = slot.post.post;
        s.point_home_at(&session, post, &e.view.title);
        if s.task_threads.place_of_task(&session, task).is_some() {
            return;
        }
        let Some(parent) = e.parent.clone() else {
            return;
        };
        s.bind_card_place(&session, post, &parent, &post_name(task, &e.view.title));
        let mut posts = s.task_posts.posts.lock().unwrap();
        if let Some(p) = posts
            .iter_mut()
            .find(|p| p.task == key.0 && p.forum == key.1)
        {
            p.bound = Some((session, parent));
        }
        drop(posts);
        s.task_posts.save(&s.core.store);
    }

    /// The forum's tag ids by state, the missing tags added once.
    async fn tag_ids(&mut self, f: &Forum) -> HashMap<Tag, String> {
        if let Some(t) = self.tags.get(&f.channel) {
            return t.clone();
        }
        match self.read_tags(f).await {
            Ok(t) => {
                self.tags.insert(f.channel, t.clone());
                t
            }
            Err(e) => {
                self.shared
                    .board
                    .error("task forum tags", None, format!("{e:#}"));
                HashMap::new()
            }
        }
    }

    async fn read_tags(&self, f: &Forum) -> anyhow::Result<HashMap<Tag, String>> {
        let http = &self.shared.http;
        let req = Request::builder(&HttpRoute::GetChannel {
            channel_id: f.channel,
        })
        .build()?;
        let channel: Value = http.request::<Value>(req).await?.model().await?;
        let mut available: Vec<Value> = channel["available_tags"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let has = |a: &[Value], t: Tag| {
            a.iter().any(|x| {
                x["name"]
                    .as_str()
                    .is_some_and(|n| n.eq_ignore_ascii_case(t.name()))
            })
        };
        let missing: Vec<Tag> = Tag::ALL
            .into_iter()
            .filter(|t| !has(&available, *t))
            .collect();
        if !missing.is_empty() {
            anyhow::ensure!(
                available.len() + missing.len() <= 20,
                "the forum holds {} tags and Discord allows 20: there is no room for ours",
                available.len()
            );
            available.extend(missing.iter().map(|t| json!({"name": t.name()})));
            let req = Request::builder(&HttpRoute::UpdateChannel {
                channel_id: f.channel,
            })
            .json(&json!({"available_tags": available}))
            .build()?;
            let channel: Value = http.request::<Value>(req).await?.model().await?;
            available = channel["available_tags"]
                .as_array()
                .cloned()
                .unwrap_or_default();
        }
        let mut out = HashMap::new();
        for t in Tag::ALL {
            if let Some(id) = available
                .iter()
                .find(|x| {
                    x["name"]
                        .as_str()
                        .is_some_and(|n| n.eq_ignore_ascii_case(t.name()))
                })
                .and_then(|x| x["id"].as_str())
            {
                out.insert(t, id.to_string());
            }
        }
        Ok(out)
    }

    /// The task's post: made in the forum with its card and its state's tag.
    async fn create(
        &mut self,
        key: &(String, u64),
        f: &Forum,
        view: &WorkView,
        link: Option<&str>,
        name: String,
    ) -> anyhow::Result<()> {
        let want = Tag::of(view);
        let ids = self.tag_ids(f).await;
        let card = self.card_of(view, link);
        v2::validate(&card).map_err(|e| anyhow::anyhow!("the card does not fit: {e}"))?;
        let mut message = v2::body(&card, true);
        message["allowed_mentions"] = json!({"parse": []});
        let mut body = json!({
            "name": name,
            "auto_archive_duration": ARCHIVE_MINUTES,
            "message": message,
        });
        if let Some(id) = ids.get(&want) {
            body["applied_tags"] = json!([id]);
        }
        let req = Request::builder(&HttpRoute::CreateForumThread {
            channel_id: f.channel,
        })
        .json(&body)
        .build()?;
        let made: Value = self
            .shared
            .http
            .request::<Value>(req)
            .await?
            .model()
            .await?;
        let post: u64 = made["id"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| anyhow::anyhow!("the new post has no id"))?;
        let post = Post {
            task: key.0.clone(),
            forum: f.channel,
            post,
            guild: f.guild.clone(),
            created_ms: theseus_protocol::now_unix_ms(),
            ended_ms: None,
            link: link.map(str::to_string),
            bound: None,
        };
        let now = Instant::now();
        self.slots.insert(
            key.clone(),
            Slot {
                post: post.clone(),
                card: Some(digest(&serde_json::to_string(&card).unwrap_or_default())),
                card_at: Some(now),
                tag: ids.get(&want).map(|_| want),
                tag_at: Some(now),
            },
        );
        let s = &self.shared;
        s.task_posts.posts.lock().unwrap().push(post);
        s.task_posts.save(&s.core.store);
        // The place's board draws its row's link to the post.
        if let Some(home) = self.entries.get(&key.0).and_then(|e| e.home.as_ref()) {
            let _ = home.send(super::PlaceMsg::Tasks);
        }
        Ok(())
    }

    /// The post's first message, edited to `card`. True when it went.
    async fn edit_card(&self, key: &(String, u64), card: &[Value]) -> bool {
        let Some(slot) = self.slots.get(key) else {
            return false;
        };
        let body = json!({"flags": v2::IS_COMPONENTS_V2, "components": card,
                          "allowed_mentions": {"parse": []}});
        let req = Request::builder(&HttpRoute::UpdateMessage {
            channel_id: slot.post.post,
            message_id: slot.post.post,
        })
        .json(&body)
        .build();
        let sent = match req {
            Ok(r) => self.shared.http.request::<Value>(r).await.map(|_| ()),
            Err(e) => {
                self.shared.board.error("task forum card", None, e);
                return false;
            }
        };
        match sent {
            Ok(()) => true,
            Err(e) => {
                self.shared.board.error("task forum card", None, e);
                false
            }
        }
    }

    /// The post's tag, set to the state's. True when it went (or the forum
    /// has no such tag to set).
    async fn set_tag(&mut self, key: &(String, u64), f: &Forum, want: Tag) -> bool {
        let ids = self.tag_ids(f).await;
        let Some(id) = ids.get(&want) else {
            return true;
        };
        let Some(slot) = self.slots.get(key) else {
            return false;
        };
        let req = Request::builder(&HttpRoute::UpdateChannel {
            channel_id: slot.post.post,
        })
        // Discord refuses a tag edit on an archived post unless it unarchives it.
        .json(&json!({"applied_tags": [id], "archived": false}))
        .build();
        let sent = match req {
            Ok(r) => self.shared.http.request::<Value>(r).await.map(|_| ()),
            Err(e) => {
                self.shared.board.error("task forum tag", None, e);
                return false;
            }
        };
        match sent {
            Ok(()) => true,
            Err(e) => {
                self.shared.board.error("task forum tag", None, e);
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn forum(private: bool) -> Forum {
        Forum {
            channel: 1,
            guild: "100000000000000001".into(),
            private,
            default: false,
        }
    }

    /// The place rule for a forum: a private one shows every task, a shared
    /// one only a shared place's of its own guild.
    #[test]
    fn a_shared_forum_shows_only_a_shared_places_tasks_of_its_guild() {
        let home = "100000000000000001";
        let other = "100000000000000002";
        assert!(
            takes(&forum(true), true, None),
            "the owner's forum takes a DM's task"
        );
        assert!(takes(&forum(true), false, Some(other)));
        assert!(takes(&forum(false), false, Some(home)));
        assert!(
            !takes(&forum(false), true, Some(home)),
            "a private place's title stays out"
        );
        assert!(!takes(&forum(false), true, None), "a DM's task stays out");
        assert!(
            !takes(&forum(false), false, Some(other)),
            "another guild's stays out"
        );
        assert!(!takes(&forum(false), false, None));
    }

    /// The pace: a write is made at once the first time and after the
    /// interval, waits in between, and the final one never waits.
    #[tokio::test(start_paused = true)]
    async fn a_held_write_waits_for_the_interval_and_the_final_one_never_does() {
        let t0 = Instant::now();
        assert_eq!(wait(None, TAGS_EVERY, t0, false), None, "the first is due");
        let later = t0 + Duration::from_secs(100);
        assert_eq!(
            wait(Some(t0), TAGS_EVERY, later, false),
            Some(Duration::from_secs(200))
        );
        assert_eq!(
            wait(Some(t0), TAGS_EVERY, later, true),
            None,
            "the final state is always written"
        );
        assert_eq!(wait(Some(t0), TAGS_EVERY, t0 + TAGS_EVERY, false), None);
    }

    const FORUM: u64 = 900_000_000_000_000_030;

    /// A rig for the stand-in's REST, and a worker over it with short
    /// intervals (the binding is not asked about the forum: the worker is run
    /// by hand).
    async fn worker_over_the_fake() -> (crate::tests_gateway::Rig, Worker) {
        use crate::tests_gateway::{Rig, ANA, BEN, CY, LAB};
        use theseus_core::provider::FakeProvider;
        use theseus_sim::fake_discord::{Guild, DEFAULT_GUILD};

        let guild = Guild::new(DEFAULT_GUILD, (ANA, "ana"))
            .member(BEN, "ben")
            .member(CY, "cy")
            .private_channel(LAB, "lab", &[ANA, BEN, CY])
            .channel(FORUM, "tasks");
        let r = Rig::start_on(
            |_, _| Arc::new(FakeProvider::scripted(vec![])),
            guild,
            &format!(
                "guild_id = \"{DEFAULT_GUILD}\"\n[[channel]]\nid = \"{LAB}\"\nname = \"lab\"\nusers = [\"{ANA}\"]\nprivate = true\n\
                 [[dm]]\nuser = \"{ANA}\"\nname = \"ana\"\n"
            ),
            &[],
        )
        .await;
        r.fake.set_forum(FORUM);
        let shared = crate::runtime::shared_for_tests(&r.core);
        *shared.task_posts.forums.lock().unwrap() = vec![Forum {
            channel: FORUM,
            guild: DEFAULT_GUILD.to_string(),
            private: true,
            default: false,
        }];
        let w = Worker {
            shared,
            every: Every {
                card: Duration::from_millis(40),
                tags: Duration::from_millis(600),
            },
            entries: HashMap::new(),
            slots: HashMap::new(),
            tags: HashMap::new(),
            failed: HashMap::new(),
        };
        (r, w)
    }

    /// A paused task's post is tagged `paused` and has `running` back on
    /// resume; a cancelled task's is `stopped`, a failed one's `failed`
    /// (theseus-ypnxt.2).
    #[tokio::test]
    async fn a_paused_post_is_tagged_paused_and_a_cancelled_one_stopped() {
        use crate::runtime::voice_status::tests::view;

        let (r, mut w) = worker_over_the_fake().await;
        let mut position = 0;
        let mut tell = |w: &mut Worker, state: &str, paused: bool| {
            position += 1;
            let mut v = view(
                "exe_01J000000000000000task01",
                "task",
                state,
                "Chart the reef",
                None,
            );
            v.position = position;
            v.paused = paused;
            w.see(Seen {
                views: vec![v],
                private: true,
                guild: None,
                links: HashMap::new(),
                home: None,
                homed: HashSet::new(),
                parent: None,
            });
        };
        let last = || {
            r.fake
                .threads()
                .into_iter()
                .find(|t| t.parent == FORUM)
                .unwrap()
                .tag_sets
                .last()
                .cloned()
        };
        let id_of = |n: &str| {
            r.fake
                .forum_tags(FORUM)
                .iter()
                .find(|t| t.name == n)
                .unwrap()
                .id
                .clone()
        };
        tell(&mut w, "running", true);
        w.drive().await;
        assert_eq!(last(), Some(vec![id_of("paused")]));
        tokio::time::sleep(Duration::from_millis(700)).await;
        tell(&mut w, "running", false);
        w.drive().await;
        assert_eq!(last(), Some(vec![id_of("running")]));
        tell(&mut w, "cancelled", false);
        w.drive().await;
        assert_eq!(last(), Some(vec![id_of("stopped")]));
    }

    /// A forum made before the build holds four tags: it gains `paused` and
    /// `stopped` on the first start that uses it, and a second worker adds
    /// no more.
    #[tokio::test]
    async fn an_old_forum_gains_the_two_new_tags_once() {
        let (r, w) = worker_over_the_fake().await;
        r.fake
            .seed_forum_tags(FORUM, &["needs you", "running", "done", "failed"]);
        let patches = || {
            r.fake
                .seen()
                .iter()
                .filter(|s| s.outcome == "forum_tags")
                .count()
        };
        let f = w.shared.task_posts.forums.lock().unwrap()[0].clone();
        let got = w.read_tags(&f).await.unwrap();
        assert!(got.contains_key(&Tag::Paused) && got.contains_key(&Tag::Stopped));
        assert_eq!(patches(), 1);
        w.read_tags(&f).await.unwrap();
        assert_eq!(patches(), 1, "a forum that has them is not changed again");
        let names: Vec<String> = r
            .fake
            .forum_tags(FORUM)
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(
            names,
            [
                "needs you",
                "running",
                "done",
                "failed",
                "paused",
                "stopped"
            ]
        );
    }

    /// A post's tags change at most once in the interval however fast the
    /// task's state moves, and end on its final state at once.
    #[tokio::test]
    async fn a_posts_tags_change_at_most_once_in_the_interval_and_end_on_the_final_state() {
        use crate::runtime::voice_status::tests::view;

        let (r, mut w) = worker_over_the_fake().await;
        let mut position = 0;
        let mut tell = |w: &mut Worker, state: &str| {
            position += 1;
            let mut v = view(
                "exe_01J000000000000000task01",
                "task",
                state,
                "Chart the reef",
                None,
            );
            v.position = position;
            w.see(Seen {
                views: vec![v],
                private: true,
                guild: None,
                links: HashMap::new(),
                home: None,
                homed: HashSet::new(),
                parent: None,
            });
        };
        let posts = || {
            r.fake
                .threads()
                .into_iter()
                .filter(|t| t.parent == FORUM)
                .collect::<Vec<_>>()
        };
        tell(&mut w, "running");
        w.drive().await;
        let tags = r.fake.forum_tags(FORUM);
        let id_of = |n: &str| tags.iter().find(|t| t.name == n).unwrap().id.clone();
        assert_eq!(posts().len(), 1);
        assert_eq!(posts()[0].tag_sets, [[id_of("running")]]);
        // Twenty moves in a row, none of them final: no tag is written.
        let mut held = None;
        for i in 0..21 {
            tell(&mut w, if i % 2 == 0 { "needs_you" } else { "running" });
            held = w.drive().await;
        }
        assert_eq!(posts()[0].tag_sets.len(), 1, "{:?}", posts()[0].tag_sets);
        assert!(held.is_some(), "the held tag edit is due later");
        // Once the interval is out, the latest state is written, once.
        tell(&mut w, "needs_you");
        tokio::time::sleep(Duration::from_millis(700)).await;
        w.drive().await;
        let sets = posts()[0].tag_sets.clone();
        assert_eq!(
            sets,
            [vec![id_of("running")], vec![id_of("needs you")]],
            "{sets:?}"
        );
        let times = posts()[0].tag_times.clone();
        assert!(
            times[1] - times[0] >= 600,
            "a tag edit at most once in the interval: {times:?}"
        );
        // The final state is written at once, inside the interval.
        tell(&mut w, "done");
        w.drive().await;
        let sets = posts()[0].tag_sets.clone();
        assert_eq!(sets.len(), 3, "{sets:?}");
        assert_eq!(sets[2], [id_of("done")], "{sets:?}");
        assert!(
            r.fake.messages(posts()[0].id)[0].content.contains("done"),
            "the card shows the end"
        );
        assert!(w.slots.is_empty(), "a finished post is no longer worked");
        assert!(
            w.shared
                .task_posts
                .url_of("exe_01J000000000000000task01")
                .is_some(),
            "a finished post stays in the list a day, for the board's link"
        );
    }

    /// A task whose post was its place and now works elsewhere keeps its post
    /// as its index card: still listed, its card linking the new place, its
    /// tag still following the state, and a message in it answered with the
    /// pointer (theseus-bh3t.6).
    #[tokio::test]
    async fn a_moved_post_stays_listed_and_live_as_the_tasks_index_card() {
        use crate::runtime::voice_status::tests::view;

        const TASK: &str = "exe_01J000000000000000task01";
        const OLD: &str = "https://discord.com/channels/1/10";
        const NEW: &str = "https://discord.com/channels/1/20";
        let (r, mut w) = worker_over_the_fake().await;
        w.shared.task_posts.forums.lock().unwrap()[0].default = true;
        let mut position = 0;
        let mut tell = |w: &mut Worker, state: &str| {
            position += 1;
            let mut v = view(TASK, "task", state, "Chart the reef", None);
            v.position = position;
            w.see(Seen {
                views: vec![v],
                private: true,
                guild: None,
                links: HashMap::from([(TASK.to_string(), OLD.to_string())]),
                home: None,
                homed: HashSet::new(),
                parent: None,
            });
        };
        let post = || {
            r.fake
                .threads()
                .into_iter()
                .find(|t| t.parent == FORUM)
                .unwrap()
        };
        let linked = |to: &str| {
            r.fake.messages(post().id)[0]
                .components_json
                .to_string()
                .contains(to)
        };
        let shared = w.shared.clone();
        let posts = &shared.task_posts;
        tell(&mut w, "running");
        w.drive().await;
        let id = post().id;
        assert!(posts.owns(TASK) && linked(OLD));
        assert_eq!(posts.index_of(id).as_deref(), Some(OLD));
        assert_eq!(posts.index_of(id + 1), None, "only a post of ours");
        // Moved: the card links the new place, though the place still tells the old.
        posts.moved(TASK, NEW);
        tokio::time::sleep(Duration::from_millis(80)).await;
        tell(&mut w, "running");
        w.drive().await;
        assert!(linked(NEW) && !linked(OLD), "the card follows the move");
        assert_eq!(posts.index_of(id).as_deref(), Some(NEW));
        // Still live: the end writes its final tag.
        tell(&mut w, "done");
        w.drive().await;
        let tags = r.fake.forum_tags(FORUM);
        let done = tags.iter().find(|t| t.name == "done").unwrap().id.clone();
        assert_eq!(post().tag_sets.last(), Some(&vec![done]));
        assert!(posts.owns(TASK), "it stays listed a day after the end");
    }

    #[test]
    fn a_state_has_one_tag_and_a_post_is_named_by_its_task() {
        use WorkState::*;
        let tags: Vec<&str> = [NeedsYou, Running, Waiting, Queued, Done, Failed, Cancelled]
            .map(|s| {
                let mut v = crate::runtime::voice_status::tests::view(
                    "exe_01J000000000000000task01",
                    "task",
                    "running",
                    "Chart the reef",
                    None,
                );
                v.state = s;
                Tag::of(&v).name()
            })
            .to_vec();
        assert_eq!(
            tags,
            [
                "needs you",
                "running",
                "running",
                "running",
                "done",
                "failed",
                "stopped"
            ]
        );
        let name = post_name(
            "exe_01J00000000000000000a1b2c3",
            &format!("Fix  {}", "x".repeat(200)),
        );
        assert!(name.starts_with("a1b2c3 · Fix x"), "{name}");
        assert_eq!(name.chars().count(), 100);
    }
}
