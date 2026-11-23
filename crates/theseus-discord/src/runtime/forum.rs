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
//!   no post gets one. A post is named for its task and carries no mark while
//!   the task runs (the tags carry the state); at the task's end, a post that
//!   is not its task's place is renamed once as the guild thread is
//!   (`threads::task_name`: `✅` done, `❌` failed, cancelled or expired,
//!   theseus-emqbm). A post that is the place is renamed with its thread's row.
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

pub(crate) mod archive;
mod intent;
mod remake;
#[cfg(test)]
mod remake_tests;

/// A card is edited at most once in this long.
pub(crate) const CARD_EVERY: Duration = Duration::from_secs(2);

/// A post's tags are edited at most once in this long, bar the final state.
pub(crate) const TAGS_EVERY: Duration = Duration::from_secs(300);

/// How long a call that keeps failing waits between tries, at most.
const RETRY: Duration = Duration::from_secs(60);

/// A call that failed once is tried again after this long, and the wait
/// doubles with each failure in a row up to `RETRY`, as the courier's lane
/// backs off (`courier::Lane::away`). It was a flat `RETRY` until
/// theseus-o366s: one refused call then held a post for a minute, and the
/// waits of a forum test share one deadline (110 s from its rig's construction,
/// the connect and bind waits among them), so a single transient refusal could
/// take most of what was left and read as a hang rather than as a retry.
const FIRST_RETRY: Duration = Duration::from_secs(1);

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
    /// A scheduled item that stands (issue 30). Not in `ALL`: it is made in
    /// a forum the first time an item needs it, so a forum that has no
    /// scheduled item keeps the tags it had.
    Scheduled,
}

/// Whether `task` is a scheduled item's id (a reminder's or a check-in's wake,
/// or a routine's session), not a task's: its post is read by the same Clear
/// done as a task's (`archive`).
pub(crate) fn is_item_id(task: &str) -> bool {
    task.starts_with("wak_") || task.starts_with("ses_")
}

/// Whether `view` is a scheduled item (a reminder, a check-in or a routine)
/// that `runtime/sched.rs` made up for its post, not a task.
pub(crate) fn is_scheduled(view: &WorkView) -> bool {
    matches!(view.kind, WorkKind::Wake | WorkKind::Routine)
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
            Tag::Scheduled => "scheduled",
        }
    }

    /// The tag of a task's view: its state, but a paused task that has not
    /// ended is `paused`, and a task stopped by hand or by time `stopped`.
    pub(crate) fn of(view: &WorkView) -> Self {
        if is_scheduled(view) && !view.state.ended() {
            return Tag::Scheduled;
        }
        match view.state {
            WorkState::NeedsYou => Tag::NeedsYou,
            WorkState::Done => Tag::Done,
            WorkState::Failed => Tag::Failed,
            WorkState::Cancelled | WorkState::Expired => Tag::Stopped,
            _ if view.paused => Tag::Paused,
            // Set to start later and not yet started (needs you, paused and
            // the ends came first): scheduled until its time, then running.
            _ if crate::render::task_view::waits_to_start(view) => Tag::Scheduled,
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
    /// When the owner saw its task's end (the board's Clear done), and when
    /// the post was archived and locked for it (`archive`, theseus-n9dkb).
    #[serde(default)]
    pub read_ms: Option<u64>,
    #[serde(default)]
    pub archived_ms: Option<u64>,
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
    /// The forums a reopen was refused in for want of a permission, said once.
    forbidden: Mutex<HashSet<u64>>,
    /// Where a task whose post was its place works now, by task.
    moved: Mutex<HashMap<String, String>>,
    /// The scheduled items' faces, for their posts' cards (`sched.rs`).
    pub(super) sched: super::sched::Sched,
    /// Posts about to be made or made and not yet recorded (`intent.rs`).
    intents: Mutex<Vec<intent::Intent>>,
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
            .filter(|p| archive::kept(p, now, LINK_KEPT_MS))
            .collect();
        self.load_intents(store);
    }

    pub(super) fn save(&self, store: &theseus_core::store::Store) {
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

    /// The scheduled items as views of their own, each with the link to its
    /// card in the task home: the worker makes and keeps their posts in the
    /// default forum, as it does a task's index card. The home is private, so
    /// is each item.
    pub(super) fn tell_scheduled(
        &self,
        views: Vec<WorkView>,
        links: HashMap<String, String>,
        guild: &str,
    ) {
        if let Some(tx) = self.tx.get() {
            let _ = tx.send(Seen {
                views,
                private: true,
                guild: Some(guild.to_string()),
                links,
                home: None,
                homed: HashSet::new(),
                parent: None,
            });
        }
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
    /// The post took the mark of its task's end in its name (`Worker::name_at_end`).
    named: bool,
}

impl Slot {
    /// A finished task's post shows all of its last state: its card, its tag
    /// and, unless the post is its task's place (`homed`), its name.
    fn shows_the_end(&self, card: u64, tag: Tag, homed: bool) -> bool {
        self.card == Some(card) && self.tag == Some(tag) && (homed || self.named)
    }
}

/// The paces (a test shortens them): how often a card and a post's tags may
/// be edited, the longest a failing call waits between tries, and the two of
/// a post made again (`remake.rs`): the wait before the second, and how long a
/// post must stand for its going to be the owner's.
#[derive(Clone, Copy)]
struct Every {
    card: Duration,
    tags: Duration,
    retry: Duration,
    remake: Duration,
    stood: Duration,
}

/// A key whose last call failed: when it may be tried again, and how many
/// times in a row it has failed, which sets that wait.
struct Failed {
    until: Instant,
    runs: u32,
}

struct Worker {
    shared: Arc<Shared>,
    every: Every,
    entries: HashMap<String, Entry>,
    /// Posts the owner deleted while their item was over, and when: not made
    /// again, a restart included (`remake.rs`).
    deleted: HashMap<(String, u64), u64>,
    slots: HashMap<(String, u64), Slot>,
    tags: HashMap<u64, HashMap<Tag, String>>,
    failed: HashMap<(String, u64), Failed>,
    /// Posts found gone, per key, that are made again at a pace (`remake.rs`).
    remakes: HashMap<(String, u64), remake::Remakes>,
}

fn digest(s: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

/// The HTTP status of a failed call, when it was Discord's answer.
fn status_of(e: &anyhow::Error) -> Option<u16> {
    use twilight_http::error::ErrorType as E;
    match e.downcast_ref::<twilight_http::Error>()?.kind() {
        E::Response { status, .. } => Some(status.get()),
        _ => None,
    }
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
        // After the posts load: the core refreshes the views it holds when
        // the registry is set (theseus-emqbm), so the list must be read first.
        self.register_task_links();
        let (tx, rx) = mpsc::unbounded_channel();
        let (make_tx, make_rx) = mpsc::unbounded_channel();
        if self.task_posts.tx.set(tx).is_ok() && self.task_posts.make_tx.set(make_tx).is_ok() {
            tokio::spawn(run(self.clone(), rx, make_rx));
        }
        self.start_default_forum(b);
    }

    /// Where one task's post or thread is: a post first (it is the card), else
    /// its thread. The rule `task_links` applies to each row of the board.
    pub(crate) fn task_link(&self, id: &str) -> Option<String> {
        self.task_posts
            .url_of(id)
            .or_else(|| self.task_threads.url_of(id))
    }

    /// Tell the core where tasks are linked, so its work views carry it
    /// (`WorkView.forum_url`). Held weakly: `Shared` holds the core, and a
    /// strong reference back would be a cycle that never drops.
    pub(super) fn register_task_links(self: &Arc<Self>) {
        let me = Arc::downgrade(self);
        self.core
            .push
            .set_task_links(Arc::new(move |id| me.upgrade()?.task_link(id)));
    }

    /// Where each task of the projection's parts has a thread or a post, for
    /// the board's rows (a post first: it is the card).
    pub(crate) fn task_links(&self, p: &super::tasks::Projection) -> HashMap<String, String> {
        p.needs
            .iter()
            .chain(&p.working)
            .chain(&p.done)
            .filter_map(|v| Some((v.id.clone(), self.task_link(&v.id)?)))
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
    let mut w = Worker::new(
        shared,
        Every {
            card: CARD_EVERY,
            tags: TAGS_EVERY,
            retry: RETRY,
            remake: remake::REMAKE_FIRST,
            stood: remake::STOOD,
        },
    );
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
                if !takes(f, e.private, e.guild.as_deref()) || (is_scheduled(&e.view) && !f.default)
                {
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
        self.forget_remakes();
        let now = Instant::now();
        for t in self.failed.values().map(|f| f.until) {
            if t > now {
                keep(Some(t - now), now);
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
                named: false,
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

    /// A call on `key` failed: how long before it is tried again. The wait
    /// starts at `FIRST_RETRY` and doubles with each failure in a row, up to
    /// `every.retry`, which is how the courier's lane backs off from an
    /// absent Discord (`courier::Lane::away`). A success on the key clears
    /// the count, so a blip costs a second and a real outage still settles at
    /// a minute between tries (theseus-o366s).
    fn backed_off(&mut self, key: &(String, u64)) -> Duration {
        let cap = self.every.retry;
        let f = self.failed.entry(key.clone()).or_insert(Failed {
            until: Instant::now(),
            runs: 0,
        });
        f.runs = f.runs.saturating_add(1);
        let wait = FIRST_RETRY
            .saturating_mul(1u32 << (f.runs - 1).min(16))
            .min(cap);
        f.until = Instant::now() + wait;
        wait
    }

    /// A post the worker holds no slot for: the list's, taken up again; one
    /// made now; or none (a task's own post is made on request). How long to
    /// wait before looking again, if it must.
    async fn without_a_slot(
        &mut self,
        key: &(String, u64),
        f: &Forum,
        (view, link, homed, index): (&WorkView, Option<&str>, bool, bool),
    ) -> Option<Duration> {
        let (id, ended, now) = (key.0.as_str(), view.state.ended(), Instant::now());
        // An index card made before its task's thread was: it gets its link.
        if index {
            self.reopen_for_link(key, link);
        }
        // A scheduled item set again after it ended (a fired reminder
        // snoozed from its notice, under its own id): its post is opened
        // again, `scheduled`, as it was.
        if let Some(wait) = self.reopen_if_rearmed(key, view).await {
            return Some(wait);
        }
        // A post a stop cut off before it was recorded is found, not made again
        // (the read waits out the key's backoff as a create does).
        if self.shared.task_posts.intent_of(&key.0, key.1).is_some() {
            if let Some(t) = self.failed.get(key).map(|f| f.until).filter(|t| *t > now) {
                return Some(t - now);
            }
        }
        if let Err(e) = self.settle_intent(key, &f.guild).await {
            self.shared
                .board
                .error("task forum post", None, format!("{e:#}"));
            return Some(self.backed_off(key));
        }
        // A post the last run made (the list has it): taken up again.
        if let Some(held) = self.adopt(key) {
            return held;
        }
        // A task's own post is made on request, not by this sync; one that ended before any sync saw
        // it running is owed an index card in its final state (theseus-hzdlf).
        if homed || (ended && !self.shared.task_threads.owed(view) && !is_scheduled(view)) {
            return None;
        }
        // The owner deleted it once its item was over: no post is made again.
        if ended && self.deleted.contains_key(key) {
            return None;
        }
        self.unmark_deleted(key);
        // A post found gone is made again at a pace, and not for ever.
        if let Some(wait) = self.remake_gate(key) {
            return wait;
        }
        if let Some(t) = self.failed.get(key).map(|f| f.until).filter(|t| *t > now) {
            return Some(t - now);
        }
        let name = post_name(&view.id, &self.shared.core.exits.text(&view.title).0);
        if let Err(e) = self.create(key, f, view, link, name).await {
            self.shared.board.error(
                "task forum",
                Some(&view.session_id.clone().unwrap_or_default()),
                format!("{e:#}"),
            );
            return Some(self.backed_off(key));
        }
        self.failed.remove(key);
        if index {
            self.keep_index(key, link, id);
            self.card_made(key, id);
        }
        // A post made in its final state is looked at again at once: its name is still to come.
        ended.then_some(Duration::ZERO)
    }

    /// One task in one forum: its post made, its card and tags brought up to
    /// date as far as the pace allows. How long the rest must wait, if any.
    async fn one(&mut self, id: &str, f: &Forum) -> Option<Duration> {
        let key = (id.to_string(), f.channel);
        let (view, link, homed, index) = self.shown(id, f)?;
        let ended = view.state.ended();
        let now = Instant::now();
        if !self.slots.contains_key(&key) {
            return self
                .without_a_slot(&key, f, (&view, link.as_deref(), homed, index))
                .await;
        }
        // A call on the post failed: it waits out its backoff, as a create does.
        if let Some(t) = self.failed.get(&key).map(|f| f.until).filter(|t| *t > now) {
            return Some(t - now);
        }
        // A post the owner deleted under a live slot: every call on it failed.
        if let Some(wait) = self.forget_if_gone(&key, ended).await {
            return wait;
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
        // A scheduled item set again before its end was fully shown: the name
        // its end gave the post is taken off.
        earliest(self.unmark_rearmed(&key, &view).await);
        // The card.
        let card = self.card_of(&view, link.as_deref());
        let sent = digest(&serde_json::to_string(&card).unwrap_or_default());
        let slot = self.slots.get(&key)?;
        if slot.card != Some(sent) {
            match wait(slot.card_at, self.every.card, now, ended) {
                None => {
                    if self.edit_card(&key, &card).await {
                        self.failed.remove(&key);
                        if let Some(s) = self.slots.get_mut(&key) {
                            s.card = Some(sent);
                            s.card_at = Some(Instant::now());
                        }
                    } else {
                        earliest(Some(self.backed_off(&key)));
                    }
                }
                w => earliest(w),
            }
        }
        // The tags.
        let want = Tag::of(&view);
        let slot = self.slots.get(&key)?;
        if slot.tag != Some(want) {
            // The edits that leave `scheduled` (the item's start) or come back
            // to it (an item set again whose end was not yet fully shown, so its
            // slot is live and its tag still an end's) are shown at once, as an
            // end is: each is one more edit, never a stream.
            let leaving = slot.tag == Some(Tag::Scheduled)
                || (want == Tag::Scheduled
                    && matches!(slot.tag, Some(Tag::Done | Tag::Failed | Tag::Stopped)));
            match wait(slot.tag_at, self.every.tags, now, ended || leaving) {
                None => {
                    if self.set_tag(&key, f, want).await {
                        self.failed.remove(&key);
                        if let Some(s) = self.slots.get_mut(&key) {
                            s.tag = Some(want);
                            s.tag_at = Some(Instant::now());
                        }
                    } else {
                        earliest(Some(self.backed_off(&key)));
                    }
                }
                w => earliest(w),
            }
        }
        // The name, once, at the end.
        earliest(self.name_at_end(&key, &view, homed).await);
        // Over, and its last state shown: the post leaves the list.
        let slot = self.slots.get(&key)?;
        if ended && slot.shows_the_end(sent, want, homed) {
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
        if let Some(face) = Some(view)
            .filter(|v| is_scheduled(v))
            .and_then(|v| self.shared.task_posts.sched.face(&v.id))
        {
            return crate::render::sched::forum_card(
                &crate::render::leaving::face(&self.shared.core.exits, &face),
                link,
            );
        }
        let calls: Vec<String> = self
            .shared
            .tasks
            .children_of(&view.id, WorkKind::Job)
            .into_iter()
            .map(|j| j.title)
            .collect();
        let exits = &self.shared.core.exits;
        let Some(view) = crate::render::leaving::copy(exits, view) else {
            return vec![];
        };
        let calls: Vec<_> = calls.iter().map(|s| exits.text(s).0.into_owned()).collect();
        task_view::post_card(&view, &calls, link, self.shared.core.kernel.now_ms())
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
        if !self.slots.contains_key(&key) {
            self.settle_intent(&key, &f.guild).await?;
        }
        if !self.slots.contains_key(&key) && self.adopt(&key).is_none() {
            let name = task_name(
                Mark::of(view.state),
                &view.id,
                &self.shared.core.exits.text(&view.title).0,
            );
            self.create(&key, &f, view, None, name).await?;
        }
        let posts = self.shared.task_posts.posts.lock().unwrap();
        let post = posts
            .iter()
            .find(|p| p.task == key.0 && p.forum == key.1)
            .ok_or_else(|| anyhow::anyhow!("the task's post is gone"))?;
        Ok((post.post, f.guild.parse().ok()))
    }

    /// A post the owner deleted under a live slot: its calls fail (404) and the
    /// slot would be tried again for ever, with an error row each time. After a
    /// failure the post is read once, and its starter message (the card) with
    /// it; if either is gone the slot and the row go (a thread left standing
    /// with no card is not edited again: its item is given a post of its own). A
    /// standing item is looked at again, and gets a first post, at once for the
    /// first one found gone and at a pace for those in a row (`remake.rs`); an
    /// item that is over is left without (the owner deleted it). `None`: nothing was
    /// wrong, the caller goes on; `Some(wait)`: the key is settled for this pass.
    async fn forget_if_gone(
        &mut self,
        key: &(String, u64),
        ended: bool,
    ) -> Option<Option<Duration>> {
        if !self.failed.contains_key(key) {
            return None;
        }
        let (post, made_ms) = {
            let p = &self.slots.get(key)?.post;
            (p.post, p.created_ms)
        };
        // The thread is gone, or it stands and its starter message (the card) is not.
        let gone = self
            .answers_404(&HttpRoute::GetChannel { channel_id: post })
            .await?
            || self
                .answers_404(&HttpRoute::GetMessage {
                    channel_id: post,
                    message_id: post,
                })
                .await?;
        if !gone {
            return None;
        }
        self.slots.remove(key);
        self.failed.remove(key);
        self.forget_post(key);
        if ended {
            self.mark_deleted(key);
            return Some(None);
        }
        Some(self.remade_after(key, made_ms))
    }

    /// Whether Discord answers the read `route` with 404. None: the request
    /// could not be built.
    async fn answers_404(&self, route: &HttpRoute<'_>) -> Option<bool> {
        let req = Request::builder(route).build().ok()?;
        Some(match self.shared.http.request::<Value>(req).await {
            Err(e) => status_of(&anyhow::Error::from(e)) == Some(404),
            Ok(_) => false,
        })
    }

    /// A scheduled item that stands has its ended post opened again
    /// (`reopen_rearmed`). How long to wait before trying again, if that failed.
    async fn reopen_if_rearmed(
        &mut self,
        key: &(String, u64),
        view: &WorkView,
    ) -> Option<Duration> {
        if !is_scheduled(view) || view.state.ended() || !self.has_ended_post(key) {
            return None;
        }
        // A reopen that failed waits out the key's backoff, as a create does.
        let now = Instant::now();
        if let Some(t) = self.failed.get(key).map(|f| f.until).filter(|t| *t > now) {
            return Some(t - now);
        }
        match self.reopen_rearmed(key, view).await {
            Ok(()) => None,
            Err(e) => match status_of(&e) {
                // The owner deleted the post: there is none, and the item is
                // given its first (the caller goes on to adopt, then create).
                Some(404) => {
                    self.forget_post(key);
                    None
                }
                Some(403) => {
                    self.say_cannot_reopen(key.1);
                    Some(self.backed_off(key))
                }
                _ => {
                    self.shared
                        .board
                        .error("task forum reopen", None, format!("{e:#}"));
                    Some(self.backed_off(key))
                }
            },
        }
    }

    /// Whether the list holds an ended post for the key.
    fn has_ended_post(&self, key: &(String, u64)) -> bool {
        self.shared
            .task_posts
            .posts
            .lock()
            .unwrap()
            .iter()
            .any(|p| p.task == key.0 && p.forum == key.1 && p.ended_ms.is_some())
    }

    /// The post is gone (deleted): its row leaves the list.
    fn forget_post(&self, key: &(String, u64)) {
        let s = &self.shared;
        s.task_posts
            .posts
            .lock()
            .unwrap()
            .retain(|p| !(p.task == key.0 && p.forum == key.1));
        s.task_posts.save(&s.core.store);
    }

    /// A reopen was refused (403): said once, in health and the log, what the
    /// bot lacks. The forum's id only, no title.
    fn say_cannot_reopen(&self, forum: u64) {
        if !self
            .shared
            .task_posts
            .forbidden
            .lock()
            .unwrap()
            .insert(forum)
        {
            return;
        }
        tracing::warn!(
            forum,
            "a post could not be opened again: the bot lacks Manage Threads there"
        );
        self.shared.board.error(
            "task forum",
            None,
            format!("a post in forum {forum} cannot be opened again: the bot needs Manage Threads there"),
        );
    }

    /// A scheduled item set again before its end was fully shown: the name its
    /// end gave the post is taken off. How long to wait before trying again,
    /// if that failed.
    async fn unmark_rearmed(&mut self, key: &(String, u64), view: &WorkView) -> Option<Duration> {
        if !is_scheduled(view)
            || view.state.ended()
            || !self.slots.get(key).is_some_and(|s| s.named)
        {
            return None;
        }
        if !self
            .set_name(
                key,
                &post_name(&view.id, &self.shared.core.exits.text(&view.title).0),
            )
            .await
        {
            return Some(self.backed_off(key));
        }
        if let Some(s) = self.slots.get_mut(key) {
            s.named = false;
        }
        None
    }

    /// The post of a scheduled item that ended and stands again: it leaves the
    /// archive and is unlocked, takes the item's name back (its end's mark off),
    /// and its row is open again (not ended, not read, not archived), so
    /// `adopt` takes it up and its card and tag are sent anew. Nothing is done
    /// for a post that is not over.
    async fn reopen_rearmed(&mut self, key: &(String, u64), view: &WorkView) -> anyhow::Result<()> {
        let post = {
            let posts = self.shared.task_posts.posts.lock().unwrap();
            posts
                .iter()
                .find(|p| p.task == key.0 && p.forum == key.1 && p.ended_ms.is_some())
                .map(|p| p.post)
        };
        let Some(post) = post else {
            return Ok(());
        };
        let req = Request::builder(&HttpRoute::UpdateChannel { channel_id: post })
            .json(&json!({"name": post_name(&view.id, &self.shared.core.exits.text(&view.title).0), "archived": false, "locked": false}))
            .build()?;
        self.shared.http.request::<Value>(req).await?;
        let s = &self.shared;
        for p in s.task_posts.posts.lock().unwrap().iter_mut() {
            if p.task == key.0 && p.forum == key.1 {
                p.ended_ms = None;
                p.read_ms = None;
                p.archived_ms = None;
            }
        }
        s.task_posts.save(&s.core.store);
        Ok(())
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
    async fn tag_ids(&mut self, f: &Forum, need: Tag) -> HashMap<Tag, String> {
        if let Some(t) = self.tags.get(&f.channel) {
            if need != Tag::Scheduled || t.contains_key(&Tag::Scheduled) {
                return t.clone();
            }
        }
        match self.read_tags(f, need == Tag::Scheduled).await {
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

    async fn read_tags(&self, f: &Forum, scheduled: bool) -> anyhow::Result<HashMap<Tag, String>> {
        let wanted: Vec<Tag> = Tag::ALL
            .into_iter()
            .chain(scheduled.then_some(Tag::Scheduled))
            .collect();
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
        let missing: Vec<Tag> = wanted
            .iter()
            .copied()
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
        for t in wanted {
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
        let ids = self.tag_ids(f, want).await;
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
        .json(&{
            crate::render::leaving::json(&self.shared.core.exits, &mut body);
            body
        })
        .build()?;
        // The intent is on the list before the create goes out (`intent.rs`).
        let store = &self.shared.core.store;
        self.shared.task_posts.intend(store, &key.0, f.channel);
        let sent: anyhow::Result<u64> = async {
            let made: Value = self
                .shared
                .http
                .request::<Value>(req)
                .await?
                .model()
                .await?;
            made["id"]
                .as_str()
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| anyhow::anyhow!("the new post has no id"))
        }
        .await;
        let post = match sent {
            Ok(post) => post,
            Err(e) => {
                // A create that certainly made nothing leaves no intent; one that
                // may have gone keeps it, and is settled before the next try.
                if intent::made_nothing(&e) {
                    self.shared.task_posts.settle(store, &key.0, f.channel);
                }
                return Err(e);
            }
        };
        let post = Post {
            task: key.0.clone(),
            forum: f.channel,
            post,
            guild: f.guild.clone(),
            created_ms: theseus_protocol::now_unix_ms(),
            ended_ms: None,
            link: link.map(str::to_string),
            bound: None,
            read_ms: None,
            archived_ms: None,
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
                named: false,
            },
        );
        let s = &self.shared;
        s.task_posts.posts.lock().unwrap().push(post);
        s.task_posts.save(&s.core.store);
        s.task_posts.settle(&s.core.store, &key.0, f.channel);
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
        let mut body = json!({"flags": v2::IS_COMPONENTS_V2, "components": card,
                          "allowed_mentions": {"parse": []}});
        let req = Request::builder(&HttpRoute::UpdateMessage {
            channel_id: slot.post.post,
            message_id: slot.post.post,
        })
        .json(&{
            crate::render::leaving::json(&self.shared.core.exits, &mut body);
            body
        })
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

    /// A post that is not its task's place takes the thread's mark in its name
    /// once, when its task ends (`task_name`: `✅` done, `❌` failed, cancelled
    /// or expired), as the guild thread's name does; a post that is the place is
    /// renamed by `finish_task_thread`, with its thread's row. How long to wait
    /// before trying again, if the rename failed.
    async fn name_at_end(
        &mut self,
        key: &(String, u64),
        view: &WorkView,
        homed: bool,
    ) -> Option<Duration> {
        if !view.state.ended() || homed || self.slots.get(key)?.named {
            return None;
        }
        let name = task_name(
            Mark::of(view.state),
            &view.id,
            &self.shared.core.exits.text(&view.title).0,
        );
        if !self.set_name(key, &name).await {
            return Some(self.backed_off(key));
        }
        self.failed.remove(key);
        if let Some(s) = self.slots.get_mut(key) {
            s.named = true;
        }
        None
    }

    /// The post's name, set to `name`. True when it went.
    async fn set_name(&mut self, key: &(String, u64), name: &str) -> bool {
        let Some(slot) = self.slots.get(key) else {
            return false;
        };
        let req = Request::builder(&HttpRoute::UpdateChannel {
            channel_id: slot.post.post,
        })
        // As a tag edit, a rename of an archived post unarchives it first.
        .json(&json!({"name": name, "archived": false}))
        .build();
        let sent = match req {
            Ok(r) => self.shared.http.request::<Value>(r).await.map(|_| ()),
            Err(e) => {
                self.shared.board.error("task forum name", None, e);
                return false;
            }
        };
        match sent {
            Ok(()) => true,
            Err(e) => {
                self.shared.board.error("task forum name", None, e);
                false
            }
        }
    }

    /// The post's tag, set to the state's. True when it went (or the forum
    /// has no such tag to set).
    async fn set_tag(&mut self, key: &(String, u64), f: &Forum, want: Tag) -> bool {
        let ids = self.tag_ids(f, want).await;
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

    #[tokio::test]
    async fn forum_names_and_cards_mask_known_values() {
        let (r, mut w) = worker_over_the_fake().await;
        let value = "vault-forum-4c8e1f7a92b3";
        r.core.secrets.hold(
            "forum_token",
            theseus_core::secrets::Secret::new(value.into()),
        );
        let mut view = crate::render::task_view::tests::task();
        view.title = format!("{}{}", "x".repeat(69), value);
        w.see(Seen {
            views: vec![view.clone()],
            private: true,
            guild: None,
            links: HashMap::new(),
            home: None,
            homed: HashSet::new(),
            parent: None,
        });
        w.drive().await;
        let post = r
            .fake
            .threads()
            .into_iter()
            .find(|t| t.parent == FORUM)
            .unwrap();
        assert!(!post.name.contains(value), "{}", post.name);
        assert!(
            post.name.contains("[redacted:forum_token]"),
            "{}",
            post.name
        );
        let first = r.fake.messages(post.id).remove(0);
        assert!(!first.content.contains(value), "{}", first.content);
        assert!(
            first.content.contains("[redacted:forum_token]"),
            "{}",
            first.content
        );
        view.title = format!("Updated {value}");
        assert!(
            w.edit_card(&(view.id.clone(), FORUM), &w.card_of(&view, None))
                .await
        );
        let edited = r.fake.messages(post.id).remove(0);
        assert!(
            edited.content.contains("Updated [redacted:forum_token]"),
            "{}",
            edited.content
        );
        assert!(!edited.content.contains(value));
        r.stop();
    }

    /// The tag of a task set to start later: `scheduled` until it starts,
    /// `running` once it has, `needs you` over it, `paused` and the ends as
    /// they were.
    #[test]
    fn a_task_set_for_later_is_scheduled_until_its_start_and_running_after() {
        use crate::render::task_view::tests::{deferred, task};
        let at = 1_790_003_600_000;
        assert_eq!(Tag::of(&deferred(at)), Tag::Scheduled);
        let mut started = deferred(at);
        started.state = WorkState::Running;
        assert_eq!(Tag::of(&started), Tag::Running);
        let mut asks = deferred(at);
        asks.state = WorkState::NeedsYou;
        assert_eq!(Tag::of(&asks), Tag::NeedsYou, "a question wins");
        let mut held = deferred(at);
        held.paused = true;
        assert_eq!(Tag::of(&held), Tag::Paused);
        let mut done = deferred(at);
        done.state = WorkState::Done;
        assert_eq!(Tag::of(&done), Tag::Done);
        // A task started at once, or one that slept after working, is running.
        assert_eq!(Tag::of(&task()), Tag::Running);
        let mut slept = deferred(at);
        slept.doing.as_mut().unwrap().turn = 2;
        assert_eq!(Tag::of(&slept), Tag::Running);
    }

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

    pub(super) const FORUM: u64 = 900_000_000_000_000_030;

    /// A rig for the stand-in's REST, and a worker over it with short
    /// intervals (the binding is not asked about the forum: the worker is run
    /// by hand).
    pub(super) async fn worker_over_the_fake() -> (crate::tests_gateway::Rig, Worker) {
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
        let w = worker_on(shared_over(&r));
        (r, w)
    }

    /// The binding's state over the rig's core, as a start makes it: nothing
    /// in memory, the store as the last run left it.
    pub(super) fn shared_over(r: &crate::tests_gateway::Rig) -> Arc<Shared> {
        use theseus_sim::fake_discord::DEFAULT_GUILD;
        let shared = crate::runtime::shared_for_tests(&r.core);
        // The id the stand-in's `users/@me` gives: this shared state is not the
        // running binding's, so its preamble did not set it.
        shared.set_bot_id_for_tests(theseus_sim::fake_discord::BOT_ID);
        *shared.task_posts.forums.lock().unwrap() = vec![Forum {
            channel: FORUM,
            guild: DEFAULT_GUILD.to_string(),
            private: true,
            default: false,
        }];
        shared
    }

    /// A worker over `shared`, with short intervals.
    pub(super) fn worker_on(shared: Arc<Shared>) -> Worker {
        Worker::new(
            shared,
            Every {
                card: Duration::from_millis(40),
                tags: Duration::from_millis(600),
                retry: Duration::from_millis(200),
                remake: Duration::from_millis(60),
                stood: Duration::from_secs(300),
            },
        )
    }

    /// A post the owner deleted under a live slot (every call on it answers 404):
    /// the item that stands is given a first post again.
    #[tokio::test]
    async fn a_post_deleted_under_a_live_slot_is_made_again_for_a_standing_item() {
        use crate::runtime::voice_status::tests::view;
        let (r, mut w) = worker_over_the_fake().await;
        let tell = |w: &mut Worker, title: &str| {
            w.see(Seen {
                views: vec![view(
                    "exe_01J000000000000000task01",
                    "task",
                    "running",
                    title,
                    None,
                )],
                private: true,
                guild: None,
                links: HashMap::new(),
                home: None,
                homed: HashSet::new(),
                parent: None,
            });
        };
        let live = || -> Vec<u64> {
            r.fake
                .threads()
                .into_iter()
                .filter(|t| t.parent == FORUM)
                .map(|t| t.id)
                .collect()
        };
        tell(&mut w, "Chart the reef");
        w.drive().await;
        let first = live();
        assert_eq!(first.len(), 1);
        r.fake.delete_thread(first[0]);
        // Its card has something to say: the call fails, then the post is read.
        tell(&mut w, "Chart the whole reef");
        for _ in 0..4 {
            w.drive().await;
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
        let now = live();
        assert_eq!(now.len(), 1, "one live post: {now:?}");
        assert_ne!(now[0], first[0], "a new one");
    }

    /// The same, once the item is over: the owner deleted it, so none is made.
    #[tokio::test]
    async fn a_post_deleted_under_a_live_slot_is_not_made_again_for_an_item_that_is_over() {
        use crate::runtime::voice_status::tests::view;
        let (r, mut w) = worker_over_the_fake().await;
        let tell = |w: &mut Worker, state: &str| {
            w.see(Seen {
                views: vec![view(
                    "exe_01J000000000000000task01",
                    "task",
                    state,
                    "Chart the reef",
                    None,
                )],
                private: true,
                guild: None,
                links: HashMap::new(),
                home: None,
                homed: HashSet::new(),
                parent: None,
            });
        };
        tell(&mut w, "running");
        w.drive().await;
        let post = r
            .fake
            .threads()
            .into_iter()
            .find(|t| t.parent == FORUM)
            .unwrap()
            .id;
        r.fake.delete_thread(post);
        tell(&mut w, "done");
        for _ in 0..4 {
            w.drive().await;
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
        let live = r
            .fake
            .threads()
            .into_iter()
            .filter(|t| t.parent == FORUM)
            .count();
        assert_eq!(live, 0, "none made again");
        assert!(
            w.shared.task_posts.posts.lock().unwrap().is_empty(),
            "its row is gone"
        );
    }

    /// An item set again while its slot is still live with an end's tag (the end's
    /// name was not yet written): its tag comes back to `scheduled` at once, not
    /// after the five minutes a tag edit otherwise waits.
    #[tokio::test]
    async fn a_set_again_item_with_a_live_slot_gets_its_scheduled_tag_back_at_once() {
        use crate::render::task_view::tests::deferred;
        let (r, mut w) = worker_over_the_fake().await;
        let at = 1_790_003_600_000;
        let tell = |w: &mut Worker, title: &str, later: bool| {
            let mut v = deferred(at);
            if !later {
                v.state = WorkState::Running;
                v.doing = None;
            }
            v.id = "exe_01J000000000000000task01".into();
            v.title = title.into();
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
        // Made running, so its first tag is not the one under test.
        tell(&mut w, "Chart the reef", false);
        w.drive().await;
        // The end's tag was written, its name not: the slot stays.
        let key = ("exe_01J000000000000000task01".to_string(), FORUM);
        let slot = w.slots.get_mut(&key).expect("a slot");
        slot.tag = Some(Tag::Done);
        slot.tag_at = Some(Instant::now());
        tell(&mut w, "Chart the reef again", true);
        w.drive().await;
        let post = r
            .fake
            .threads()
            .into_iter()
            .find(|t| t.parent == FORUM)
            .unwrap();
        assert_eq!(
            post.tag_sets.len(),
            2,
            "its tag was written again: {post:?}"
        );
        let scheduled = r
            .fake
            .forum_tags(FORUM)
            .into_iter()
            .find(|t| t.name == "scheduled")
            .expect("the scheduled tag")
            .id;
        assert_eq!(
            post.tag_sets.last().cloned(),
            Some(vec![scheduled]),
            "{post:?}"
        );
    }

    /// A refused call is tried again in a second, not in a minute, and the
    /// wait doubles only while the refusals go on. A flat minute could take
    /// most of the one 110 s deadline a forum test's waits share (its rig's
    /// connect and bind waits come out of it too), so one refusal under load
    /// read as a hang (theseus-o366s).
    #[tokio::test]
    async fn a_refused_call_is_tried_again_in_a_second_and_the_post_is_then_made() {
        use crate::runtime::voice_status::tests::view;
        use theseus_sim::fake_discord::Mode;

        let (r, mut w) = worker_over_the_fake().await;
        // The paces a daemon runs at, so the ceiling under test is the real one.
        w.every.retry = RETRY;
        let tell = |w: &mut Worker| {
            w.see(Seen {
                views: vec![view(
                    "exe_01J000000000000000task01",
                    "task",
                    "running",
                    "Chart the reef",
                    None,
                )],
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
                .count()
        };

        r.fake.set_mode(Mode::Fail);
        tell(&mut w);
        let t0 = Instant::now();
        let next = w.drive().await.expect("a refused create holds a retry");
        assert_eq!(posts(), 0, "the refusal made no post");
        let held = next.saturating_duration_since(t0);
        assert!(
            held <= Duration::from_secs(2),
            "the first retry is soon, not a flat minute: {held:?}"
        );

        // Still refused, once that wait is over: the next one doubles, and
        // stays under the ceiling. Driven before it is over, the worker calls
        // nothing at all, which is what the wait is for.
        let refused = || {
            r.fake
                .seen()
                .iter()
                .filter(|s| s.outcome == "failed")
                .count()
        };
        let so_far = refused();
        w.drive().await.expect("the wait is not over");
        assert_eq!(
            refused(),
            so_far,
            "nothing is tried again before its wait is over"
        );
        tokio::time::sleep(held + Duration::from_millis(100)).await;
        let t1 = Instant::now();
        let again = w.drive().await.expect("a second refusal holds a retry");
        let held2 = again.saturating_duration_since(t1);
        assert!(
            held2 > held && held2 <= RETRY,
            "it doubles up to the ceiling: {held:?} then {held2:?}"
        );

        // Discord answers again: the post is made at the next try, and the
        // key's count is forgotten so the next blip is cheap again.
        r.fake.set_mode(Mode::Up);
        w.failed.clear();
        w.drive().await;
        assert_eq!(posts(), 1, "the post is made once Discord answers");
        assert!(w.failed.is_empty(), "a made post forgets its failures");
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

    /// The names a post carried after a task ran and then ended as `end`, and
    /// the names it should have carried: made with no mark, then renamed once
    /// with `mark`. The first drive makes the post, the second shows the end,
    /// the third and fourth are the worker seeing nothing new.
    async fn post_names_through(end: &str, mark: Mark) -> (Vec<String>, Vec<String>) {
        use crate::runtime::voice_status::tests::view;

        const ID: &str = "exe_01Jreef0chart0task";
        const TITLE: &str = "Chart the reef";
        let (r, mut w) = worker_over_the_fake().await;
        let mut position = 0;
        let mut tell = |w: &mut Worker, state: &str| {
            position += 1;
            let mut v = view(ID, "task", state, TITLE, None);
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
        tell(&mut w, "running");
        w.drive().await;
        tell(&mut w, end);
        w.drive().await;
        w.drive().await;
        w.drive().await;
        let posts: Vec<_> = r
            .fake
            .threads()
            .into_iter()
            .filter(|t| t.parent == FORUM)
            .collect();
        assert_eq!(posts.len(), 1, "one post: {posts:#?}");
        let wanted = [post_name(ID, TITLE), task_name(mark, ID, TITLE)];
        (posts[0].names.clone(), Vec::from(wanted))
    }

    /// A post that is not its task's place is renamed once when the task ends,
    /// as the guild thread is (`threads.rs`): `✅` for done. Running, it
    /// carries no mark, since its tag is the state.
    #[tokio::test]
    async fn a_finished_post_is_renamed_done_once_as_the_thread_is() {
        let (names, wanted) = post_names_through("done", Mark::Done).await;
        assert!(wanted[1].starts_with("✅ "), "{wanted:?}");
        assert_eq!(names, wanted, "created unmarked, renamed once");
    }

    /// A failed, a cancelled and an expired task end `❌`, the thread's rule.
    #[tokio::test]
    async fn a_failed_cancelled_or_expired_post_is_renamed_failed() {
        for end in ["failed", "cancelled", "expired"] {
            let (names, wanted) = post_names_through(end, Mark::Failed).await;
            assert!(wanted[1].starts_with("❌ "), "{wanted:?}");
            assert_eq!(names, wanted, "{end}: renamed once");
        }
    }

    /// A task already over when the worker first sees it, and owed its post
    /// (it began after the binding did, theseus-hzdlf), gets the post in its
    /// final state and is looked at again at once: the name takes the end's
    /// mark without another task's change to wake the worker.
    #[tokio::test]
    async fn an_owed_posts_name_takes_the_mark_without_another_wake() {
        use crate::runtime::voice_status::tests::view;

        const ID: &str = "exe_01Jreef0chart0task";
        const TITLE: &str = "Chart the reef";
        let (r, mut w) = worker_over_the_fake().await;
        let mut v = view(ID, "task", "done", TITLE, None);
        v.started_at_ms = theseus_protocol::now_unix_ms() + 60_000;
        w.see(Seen {
            views: vec![v],
            private: true,
            guild: None,
            links: HashMap::new(),
            home: None,
            homed: HashSet::new(),
            parent: None,
        });
        let again = w.drive().await;
        assert!(again.is_some(), "the new post is looked at again at once");
        w.drive().await;
        let names = r
            .fake
            .threads()
            .into_iter()
            .find(|t| t.parent == FORUM)
            .map(|t| t.names);
        let wanted = [post_name(ID, TITLE), task_name(Mark::Done, ID, TITLE)];
        assert_eq!(names, Some(Vec::from(wanted)));
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
        let got = w.read_tags(&f, false).await.unwrap();
        assert!(got.contains_key(&Tag::Paused) && got.contains_key(&Tag::Stopped));
        assert_eq!(patches(), 1);
        w.read_tags(&f, false).await.unwrap();
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

    /// What the runtime tells the core about where a task is linked
    /// (`WorkView.forum_url`): the post's address when it has one, else its
    /// thread's, else nothing. Both halves assert, and a second task proves it
    /// is keyed by the task.
    #[tokio::test]
    async fn the_registered_link_is_the_post_then_the_thread_and_else_nothing() {
        use crate::runtime::threads::{Made, PlaceKind};
        use crate::runtime::{shared_for_tests, tests::core_with};
        let dir = tempfile::tempdir().unwrap();
        let core = core_with(
            dir.path(),
            theseus_core::secrets::SecretBoard::empty(),
            |_| {},
        );
        let shared = shared_for_tests(&core);
        shared.task_posts.posts.lock().unwrap().push(Post {
            task: "exe_reef".into(),
            forum: 5,
            post: 777,
            guild: "4242".into(),
            created_ms: 1,
            ended_ms: None,
            link: None,
            bound: None,
            read_ms: None,
            archived_ms: None,
        });
        shared.task_threads.add(Made {
            task: "exe_kelp".into(),
            session: "ses_kelp".into(),
            thread: 888,
            kind: PlaceKind::Thread,
            parent: 1,
            parent_session: "ses_dm".into(),
            name: "kelp".into(),
            guild: Some(4242),
            private: true,
            ceiling: None,
            created_ms: 1,
            ended_ms: None,
        });
        assert_eq!(
            shared.task_link("exe_reef").as_deref(),
            Some("https://discord.com/channels/4242/777")
        );
        assert_eq!(
            shared.task_link("exe_kelp").as_deref(),
            Some("https://discord.com/channels/4242/888")
        );
        assert_eq!(shared.task_link("exe_none"), None);
    }
}
