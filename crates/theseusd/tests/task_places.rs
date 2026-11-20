//! A task's own channel made once across a kill of the real `theseusd`
//! (issue 30): its Discord binding talks to a stand-in for Discord's REST API
//! (`theseus_sim::fake_discord`), its gateway to a port nothing listens on, and
//! its model to a stand-in for the Messages API. Nothing reaches Discord.
//! - a task started in the DM with `expect: long` is waiting on an answer from
//!   its owner, so it works and qualifies for a channel of its own at once;
//!   the daemon is killed while the create of that channel is held at the
//!   stand-in, before it has the channel or after it has it and has not
//!   answered, and starts again: the task has one channel (the one Discord has,
//!   found by its name and the task's marker), never a second.
//!   (The task's thread has no daemon test: a guild channel's place comes with
//!   the gateway, which this rig does not have. `threads::intent`'s tests cut a
//!   create off in flight over one store and one stand-in instead.)

mod common;

use common::{table, Fixture, FAKE_OP};

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use common::model::FakeModel;
use common::Daemon;
use serde_json::{json, Value};
use theseus_sim::fake_discord::{FakeChannel, FakeDiscord, Guild, HoldAt, DEFAULT_GUILD};

const USER: u64 = 271_828_182_845_904_523;

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
    fn new() -> Self {
        let model = FakeModel::start(script);
        let fake = FakeDiscord::start();
        fake.set_guild(Guild::new(DEFAULT_GUILD, (USER, "ana")));
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
        let discord = table(&mut t, "discord");
        discord.insert("enabled".into(), true.into());
        discord.insert("rest_proxy".into(), fake.addr.clone().into());
        discord.insert("gateway_proxy".into(), "ws://127.0.0.1:9".into());
        discord.insert("task_channels".into(), true.into());
        table(&mut t, "initiative").insert("day_usd".into(), 0.0.into());
        std::fs::write(path("config.toml"), toml::to_string(&t).unwrap()).unwrap();
        std::fs::write(
            path("state/bindings.toml"),
            format!(
                "guild_id = \"{DEFAULT_GUILD}\"\nprivate = true\n[[dm]]\nuser = \"{USER}\"\nname = \"ana\"\n"
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

    /// The DM place's session, once the binding has bound it.
    fn session(&self) -> String {
        self.wait("the DM place bound", || {
            let h = self.call("health", Value::Null).ok()?;
            h["bindings"][0]["places"][0]["session_id"]
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
                "no {what} in 40 s; the stand-in holds {} channels: {:?}\nthe daemon's log ends:\n{}",
                self.channels().len(),
                self.channels().iter().map(|c| &c.name).collect::<Vec<_>>(),
                common::tail(&self.log(), 20)
            );
            std::thread::sleep(Duration::from_millis(20));
        }
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

    fn start_task(&self, sid: &str) {
        self.call(
            "task.start",
            json!({"brief": "Chart the reef", "home": sid, "expect": "long"}),
        )
        .unwrap();
    }
}

/// The daemon killed while its create of the task's channel is held at `at`
/// (not yet at Discord, or at Discord and unanswered, so before the daemon's row
/// of it), then started again: one channel for the task. The id of the channel
/// Discord had before the kill, if it had one.
fn killed_with_the_channel_held(at: HoldAt) -> (Rig, Option<u64>) {
    let r = Rig::new();
    r.fake.hold_channel_creates(Some(at));
    let daemon = r.spawn();
    let sid = r.session();
    r.start_task(&sid);
    r.settle("the channel's create held", || {
        r.fake.held_channel_creates() >= 1
    });
    let before = r.channels().first().map(|c| c.id);
    drop(daemon);
    r.fake.hold_channel_creates(None);
    std::thread::sleep(Duration::from_secs(1));
    let _daemon = r.spawn();
    r.settle("the task's channel", || r.channels().len() == 1);
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
/// its name and the task's marker and makes none.
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
