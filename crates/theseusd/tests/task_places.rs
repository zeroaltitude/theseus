//! A task's own thread and its own channel made at most once across a kill of
//! the real `theseusd` (issue 35): its Discord binding talks to a stand-in for
//! Discord's REST API and gateway (`theseus_sim::fake_discord`), and its model
//! to a stand-in for the Messages API. Nothing reaches Discord.
//! - a task started in `#lab` works (it asks its owner a question and stays
//!   asking), so it has a thread of its own; one started in the DM with
//!   `expect: long` has a channel of its own.
//! - the daemon is killed while the create is held at the stand-in, after the
//!   stand-in has the place (the answer is withheld) or before it arrives, and
//!   starts again. After the restart the task has one place: the one Discord
//!   had (found, its row written, so its first line lands there), or the one
//!   made once. A member's thread named as the task's would be, standing in
//!   the same channel, is never taken for the task's.

mod common;

use common::{table, Fixture, FAKE_OP};

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use common::model::FakeModel;
use common::Daemon;
use serde_json::{json, Value};
use theseus_sim::fake_discord::{
    FakeChannel, FakeDiscord, FakeThread, Guild, HoldAt, BOT_ID, DEFAULT_GUILD,
};

const USER: u64 = 271_828_182_845_904_523;
const MEMBER: u64 = 271_828_182_845_904_524;
const LAB: u64 = 900_000_000_000_000_010;

/// What the stand-in model calls for a prompt: the task asks its owner which
/// plan to take, and so stays working until it is answered (which nothing does).
fn script(prompt: &str) -> Vec<(&'static str, Value)> {
    if prompt.starts_with("[Task ") && prompt.contains("Chart the reef") {
        return vec![(
            "owner_ask",
            json!({"question": "Which reef first?",
                   "options": [{"label": "North"}, {"label": "South"}],
                   "default": 1}),
        )];
    }
    vec![]
}

struct Rig {
    dir: tempfile::TempDir,
    fake: Arc<FakeDiscord>,
    _model: FakeModel,
}

impl Fixture for Rig {
    fn dir(&self) -> &std::path::Path {
        self.dir.path()
    }
}

impl Rig {
    /// With `lab`, a gateway and the private channel `#lab` the bindings file
    /// binds, and no forum or task home (the task's place is a thread there);
    /// without, the DM alone, whose tasks that qualify get channels.
    fn new(lab: bool) -> Self {
        let model = FakeModel::start(script);
        let fake = if lab {
            FakeDiscord::start_with_gateway()
        } else {
            FakeDiscord::start()
        };
        let mut guild = Guild::new(DEFAULT_GUILD, (USER, "ana"));
        if lab {
            guild = guild.private_channel(LAB, "lab", &[USER]);
        }
        fake.set_guild(guild);
        let dir = tempfile::tempdir().unwrap();
        let path = |p: &str| dir.path().join(p);
        let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
        for d in ["bin", "projects", "state"] {
            std::fs::create_dir_all(path(d)).unwrap();
        }
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(path("bin/op"), FAKE_OP).unwrap();
        std::fs::set_permissions(path("bin/op"), std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut t: toml::Table = common::safe_note(&theseusd, &path("projects"), 100.0)
            .parse()
            .unwrap();
        table(&mut t, "model").insert("api_base".into(), model.base.clone().into());
        for (_, p) in table(&mut t, "providers").iter_mut() {
            p.as_table_mut()
                .unwrap()
                .insert("api_base".into(), model.base.clone().into());
        }
        table(&mut t, "policy").insert("enforcement".into(), "notify".into());
        let gateway = match fake.gateway() {
            Some(g) => format!("ws://{}", g.addr),
            None => "ws://127.0.0.1:9".to_string(),
        };
        let discord = table(&mut t, "discord");
        discord.insert("enabled".into(), true.into());
        discord.insert("rest_proxy".into(), fake.addr.clone().into());
        discord.insert("gateway_proxy".into(), gateway.into());
        discord.insert("task_channels".into(), true.into());
        if lab {
            discord.insert("forum".into(), false.into());
            discord.insert("task_home".into(), "".into());
        }
        table(&mut t, "initiative").insert("day_usd".into(), 0.0.into());
        std::fs::write(path("config.toml"), toml::to_string(&t).unwrap()).unwrap();
        let lab_binding = if lab {
            format!(
                "[[channel]]\nid = \"{LAB}\"\nname = \"lab\"\nusers = [\"{USER}\"]\nmention_only = false\nprivate = true\n"
            )
        } else {
            String::new()
        };
        std::fs::write(
            path("state/bindings.toml"),
            format!(
                "guild_id = \"{DEFAULT_GUILD}\"\nprivate = true\n{lab_binding}[[dm]]\nuser = \"{USER}\"\nname = \"ana\"\n"
            ),
        )
        .unwrap();
        Self {
            dir,
            fake,
            _model: model,
        }
    }

    fn spawn(&self) -> Daemon {
        self.spawn_open_at_bind()
    }

    /// The session of the place of kind `kind` (`channel` with the id `LAB`, or
    /// the `dm`), once the binding has bound it.
    fn session(&self, kind: &str) -> String {
        self.wait("the place bound", || {
            let h = self.call("health", Value::Null).ok()?;
            h["bindings"][0]["places"].as_array()?.iter().find(|p| {
                p["kind"] == kind
                    && (kind == "dm" || p["channel_id"].as_str() == Some(LAB.to_string().as_str()))
            })?["session_id"]
                .as_str()
                .map(str::to_string)
        })
    }

    /// Wait for `ok`, and fail with what the stand-in holds.
    fn settle(&self, what: &str, ok: impl Fn() -> bool) {
        let t0 = std::time::Instant::now();
        while !ok() {
            assert!(
                t0.elapsed() < Duration::from_secs(40),
                "no {what} in 40 s; the stand-in holds threads {:?} and channels {:?}\nthe daemon's log ends:\n{}",
                self.threads().iter().map(|t| (t.id, t.owner, &t.name)).collect::<Vec<_>>(),
                self.channels().iter().map(|c| &c.name).collect::<Vec<_>>(),
                common::tail(&self.log(), 20)
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The threads the bot made in `#lab`.
    fn threads(&self) -> Vec<FakeThread> {
        self.fake
            .threads()
            .into_iter()
            .filter(|t| t.parent == LAB && t.owner == BOT_ID)
            .collect()
    }

    /// The task channels the bot made and kept: text channels under the tasks
    /// category (not the category, nor the task home, which has no parent).
    fn channels(&self) -> Vec<FakeChannel> {
        self.fake
            .made_channels()
            .into_iter()
            .filter(|c| c.kind == 0 && !c.deleted && c.parent.is_some())
            .collect()
    }

    /// Whether the place `id` has its first line (the pointer the binding says
    /// once it has recorded the place and started it).
    fn pointed(&self, id: u64, words: &str) -> bool {
        self.fake
            .messages(id)
            .iter()
            .any(|m| m.content.contains(words))
    }
}

/// The daemon killed while its create of the task's thread (in `#lab`) is held
/// at `at`, then started again. `decoy` is a member's thread named as the
/// task's would be, made while the create is held. Returns the rig, and the id
/// of the bot's thread at the stand-in before the kill if it had one.
fn killed_with_the_thread_held(at: HoldAt, decoy: bool) -> (Rig, Option<u64>, Option<u64>) {
    let r = Rig::new(true);
    r.fake.hold_thread_creates(Some(at));
    let daemon = r.spawn();
    let lab = r.session("channel");
    let started = r
        .call(
            "task.start",
            json!({"brief": "Chart the reef", "home": lab}),
        )
        .unwrap();
    r.settle("the thread's create held", || {
        r.fake.held_thread_creates() >= 1
    });
    let theirs = decoy.then(|| {
        let short = started["short"].as_str().unwrap();
        r.fake
            .member_thread(LAB, MEMBER, &format!("⚙️ {short} · Chart the reef"))
    });
    let before = r.threads().first().map(|t| t.id);
    drop(daemon);
    r.fake.hold_thread_creates(None);
    std::thread::sleep(Duration::from_secs(1));
    let _daemon = r.spawn();
    r.settle("the task's thread with its first line", || {
        r.threads().iter().any(|t| r.pointed(t.id, "back to"))
    });
    // Time for any second thread to be made.
    std::thread::sleep(Duration::from_secs(4));
    assert_eq!(
        r.threads().len(),
        1,
        "{:?}",
        r.threads().iter().map(|t| &t.name).collect::<Vec<_>>()
    );
    if let Some(m) = theirs {
        assert!(
            !r.pointed(m, "back to"),
            "the member's thread was never taken for the task's"
        );
    }
    (r, before, theirs)
}

/// The thread is at Discord and unanswered: the restarted daemon finds it (the
/// bot's own, in `#lab`, named as the task's thread is) and makes none.
#[test]
fn a_tasks_thread_made_at_discord_before_the_kill_is_found_not_made_again() {
    let (r, before, _) = killed_with_the_thread_held(HoldAt::AfterApply, false);
    let first = before.expect("Discord had the thread before the kill");
    assert_eq!(r.threads()[0].id, first, "the same thread, found");
}

/// The create never reached Discord, and a member's thread is named as the
/// task's would be: the restarted daemon makes the task's own thread, once.
#[test]
fn a_tasks_thread_whose_create_never_arrived_is_made_once_and_a_members_is_left_alone() {
    let (_r, before, theirs) = killed_with_the_thread_held(HoldAt::BeforeApply, true);
    assert!(before.is_none(), "Discord never had it before the kill");
    assert!(theirs.is_some());
}

/// The daemon killed while its create of the task's channel is held at `at`,
/// then started again: one channel for the task, with its first line. The id of
/// the channel Discord had before the kill, if it had one.
fn killed_with_the_channel_held(at: HoldAt) -> (Rig, Option<u64>) {
    let r = Rig::new(false);
    r.fake.hold_channel_creates(Some(at));
    let daemon = r.spawn();
    let dm = r.session("dm");
    r.call(
        "task.start",
        json!({"brief": "Chart the reef", "home": dm, "expect": "long"}),
    )
    .unwrap();
    r.settle("the channel's create held", || {
        r.fake.held_channel_creates() >= 1
    });
    let before = r.channels().first().map(|c| c.id);
    drop(daemon);
    r.fake.hold_channel_creates(None);
    std::thread::sleep(Duration::from_secs(1));
    let _daemon = r.spawn();
    r.settle("the task's channel with its first line", || {
        r.channels().iter().any(|c| r.pointed(c.id, "from the DM"))
    });
    // Time for any second channel to be made.
    std::thread::sleep(Duration::from_secs(4));
    assert_eq!(
        r.channels().len(),
        1,
        "{:?}",
        r.channels().iter().map(|c| &c.name).collect::<Vec<_>>()
    );
    (r, before)
}

/// The channel is at Discord and unanswered: the restarted daemon finds it by
/// its exact name under the tasks category and makes none.
#[test]
fn a_tasks_channel_made_at_discord_before_the_kill_is_found_not_made_again() {
    let (r, before) = killed_with_the_channel_held(HoldAt::AfterApply);
    let first = before.expect("Discord had the channel before the kill");
    assert_eq!(r.channels()[0].id, first, "the same channel, found");
}

/// The create never reached Discord: the restarted daemon makes the task's
/// first channel, and only one.
#[test]
fn a_tasks_channel_whose_create_never_arrived_is_made_once_after_the_kill() {
    let (_r, before) = killed_with_the_channel_held(HoldAt::BeforeApply);
    assert!(before.is_none(), "Discord never had it before the kill");
}
