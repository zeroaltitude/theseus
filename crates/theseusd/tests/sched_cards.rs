//! A scheduled item's card and post across a restart of the real `theseusd`
//! (issue 30): its Discord binding talks to a stand-in for Discord's REST API
//! (`theseus_sim::fake_discord`), its gateway to a port nothing listens on, and
//! its model to a stand-in for the Messages API. Nothing reaches Discord.
//! - a reminder and a task set for later each have one card in the task home
//!   and one post in the default forum; the daemon is killed, the reminder
//!   falls due while it is down, and it starts again: there is still one card
//!   and one post each, the reminder's card says it fired and its post is
//!   done, the task's still waits (scheduled, its acts kept).

mod common;

use common::{table, Fixture, FAKE_OP};

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use common::model::FakeModel;
use common::Daemon;
use serde_json::{json, Value};
use theseus_sim::fake_discord::{FakeDiscord, Guild, Msg, DEFAULT_GUILD};

const USER: u64 = 271_828_182_845_904_523;
/// Components V2.
const V2: u64 = 1 << 15;

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
        let model = FakeModel::start(|_| vec![]);
        let fake = FakeDiscord::start();
        // The one trusted guild, with the owner in it: the task home and the
        // forum are made there.
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

    /// Wait for `ok`, and fail with what the fake holds.
    fn settle(&self, what: &str, ok: impl Fn() -> bool) {
        let t0 = std::time::Instant::now();
        while !ok() {
            assert!(
                t0.elapsed() < Duration::from_secs(40),
                "no {what} in 40 s; the fake holds:\n{}\nthe daemon's log ends:\n{}",
                self.held(),
                common::tail(&self.log(), 20)
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// What the fake holds, for a failure's message: each message's channel,
    /// flags and first words.
    fn held(&self) -> String {
        self.fake
            .all_messages()
            .iter()
            .map(|m| {
                format!(
                    "{} flags={} {:?}",
                    m.channel,
                    m.flags,
                    m.content.chars().take(60).collect::<String>()
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The task home's channel: where the pinned board is.
    fn home(&self) -> Option<String> {
        self.fake
            .all_messages()
            .into_iter()
            .find(|m| m.content.starts_with("📋 **Tasks**"))
            .map(|m| m.channel)
    }

    /// Every card of a scheduled item in the task home: Components V2 messages
    /// there, no board, whose words hold `needle`. (The reminder itself is
    /// delivered in the DM, and a post's first message is another card.)
    fn cards(&self, needle: &str) -> Vec<Msg> {
        let Some(home) = self.home() else {
            return Vec::new();
        };
        self.fake
            .messages(home.parse().unwrap())
            .into_iter()
            .filter(|m| m.flags & V2 != 0 && m.content.contains(needle))
            .filter(|m| !m.buttons.iter().any(|b| b.label.contains("Clear")))
            .collect()
    }

    /// The forum posts whose first message holds `needle`, and the names of
    /// the tags each carries now.
    fn posts(&self, needle: &str) -> Vec<(u64, Vec<String>)> {
        self.fake
            .threads()
            .into_iter()
            .filter(|t| t.forum)
            .filter(|t| {
                self.fake
                    .messages(t.id)
                    .first()
                    .is_some_and(|m| m.content.contains(needle))
            })
            .map(|t| {
                let tags = self.fake.forum_tags(t.parent);
                let on = t.tag_sets.last().cloned().unwrap_or_default();
                let names = tags
                    .into_iter()
                    .filter(|g| on.contains(&g.id))
                    .map(|g| g.name)
                    .collect();
                (t.id, names)
            })
            .collect()
    }
}

/// Killed while a reminder and a task wait, and down across the reminder's
/// time: after the restart each has one card and one post, in its true state.
#[test]
fn a_restart_keeps_one_card_and_one_post_each_in_their_true_states() {
    let r = Rig::new();
    let daemon = r.spawn();
    let sid = r.session();
    r.call(
        "wake.create",
        json!({"when": "in 8s", "note": "tide check", "session_id": sid}),
    )
    .unwrap();
    r.call(
        "task.start",
        json!({"brief": "Paint the buoy", "home": sid, "start_when": "in 90m"}),
    )
    .unwrap();
    r.settle("a card for each", || {
        r.cards("tide check").len() == 1 && r.cards("Paint the buoy").len() == 1
    });
    r.wait("a post for each, scheduled", || {
        (r.posts("tide check").len() == 1 && r.posts("Paint the buoy").len() == 1).then_some(())
    });
    assert_eq!(r.posts("tide check")[0].1, ["scheduled"]);
    assert_eq!(r.posts("Paint the buoy")[0].1, ["scheduled"]);
    // Killed before the reminder is due, and down for nine seconds.
    drop(daemon);
    std::thread::sleep(Duration::from_secs(9));
    let _daemon = r.spawn();
    r.wait("the reminder's card says it fired", || {
        r.cards("tide check")
            .first()
            .is_some_and(|m| m.content.contains("Fired") && m.buttons.is_empty())
            .then_some(())
    });
    r.wait("the reminder's post is done", || {
        (r.posts("tide check").first().map(|p| p.1.clone()) == Some(vec!["done".to_string()]))
            .then_some(())
    });
    // Time for a second pass to make anything twice.
    std::thread::sleep(Duration::from_secs(5));
    assert_eq!(
        r.cards("tide check").len(),
        1,
        "{:?}",
        r.cards("tide check")
    );
    assert_eq!(r.cards("Paint the buoy").len(), 1);
    assert_eq!(r.posts("tide check").len(), 1);
    assert_eq!(r.posts("Paint the buoy").len(), 1);
    let task = &r.cards("Paint the buoy")[0];
    assert!(
        task.content.starts_with("🕒") && !task.buttons.is_empty(),
        "the task still waits: {}",
        task.content
    );
    assert_eq!(r.posts("Paint the buoy")[0].1, ["scheduled"]);
}
