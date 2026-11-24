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
use theseus_sim::fake_discord::{FakeDiscord, Guild, HoldAt, Msg, DEFAULT_GUILD};

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

    /// The daemon's log lines of each card it wrote, for a failure's message.
    fn card_lines(&self) -> String {
        let log = self.log();
        let lines: Vec<&str> = log
            .lines()
            .filter(|l| l.contains("the schedule card"))
            .collect();
        lines.join("\n")
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
            + &self
                .fake
                .threads()
                .iter()
                .map(|t| {
                    format!(
                        "\nthread {} names={:?} tag_sets={:?} archived={}",
                        t.id, t.names, t.tag_sets, t.archived
                    )
                })
                .collect::<String>()
            + &format!("\nforums made: {}", self.fake.forums_made().len())
    }

    /// The task home's channel: where Components V2 messages are made outside
    /// the forum's posts (a thread's id is its first message's).
    fn home(&self) -> Option<String> {
        let threads: Vec<String> = self
            .fake
            .threads()
            .iter()
            .map(|t| t.id.to_string())
            .collect();
        self.fake
            .all_messages()
            .into_iter()
            .find(|m| m.flags & V2 != 0 && !threads.contains(&m.channel))
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
    r.settle("the reminder's card says it fired", || {
        r.cards("tide check")
            .first()
            .is_some_and(|m| m.content.contains("Fired") && m.buttons.is_empty())
    });
    r.settle("the reminder's post is done", || {
        r.posts("tide check").first().map(|p| p.1.clone()) == Some(vec!["done".to_string()])
    });
    // Time for a second pass to make anything twice.
    std::thread::sleep(Duration::from_secs(5));
    assert_eq!(
        r.cards("tide check").len(),
        1,
        "{:?}\nthe fake holds:\n{}",
        r.cards("tide check"),
        r.held()
    );
    assert_eq!(
        r.cards("Paint the buoy").len(),
        1,
        "the fake holds:\n{}",
        r.held()
    );
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

/// A reminder that fired and is set again from its notice (`wake.snooze` on the
/// fired wake, as the notice's Snooze buttons call it) stands again on the same
/// card, with its acts, and its post is `scheduled` again.
#[test]
fn a_fired_reminder_set_again_stands_again_on_its_card_and_post() {
    let r = Rig::new();
    let _daemon = r.spawn();
    let sid = r.session();
    let made = r
        .call(
            "wake.create",
            json!({"when": "in 3s", "note": "tide check", "session_id": sid}),
        )
        .unwrap();
    let wake = made["wake"]["wake_id"].as_str().unwrap().to_string();
    r.settle("the card says it fired and the post is done", || {
        r.cards("tide check")
            .first()
            .is_some_and(|m| m.content.contains("Fired") && m.buttons.is_empty())
            && r.posts("tide check").first().map(|p| p.1.clone()) == Some(vec!["done".to_string()])
    });
    let first = r.cards("tide check")[0].id.clone();
    r.call(
        "wake.snooze",
        json!({"wake": wake, "when": "in 90m", "fired": true}),
    )
    .unwrap();
    r.settle("the card stands again and the post is scheduled", || {
        r.cards("tide check")
            .first()
            .is_some_and(|m| !m.content.contains("Fired") && m.buttons.len() == 2)
            && r.posts("tide check").first().map(|p| p.1.clone())
                == Some(vec!["scheduled".to_string()])
    });
    assert_eq!(r.cards("tide check").len(), 1);
    assert_eq!(r.cards("tide check")[0].id, first, "the same card");
    assert_eq!(r.posts("tide check").len(), 1);
}

/// What is set for later, as a post is made for it.
#[derive(Clone, Copy)]
enum Item {
    Reminder,
    Task,
}

impl Item {
    fn words(self) -> &'static str {
        match self {
            Item::Reminder => "tide check",
            Item::Task => "Paint the buoy",
        }
    }

    fn set(self, r: &Rig, sid: &str) {
        match self {
            Item::Reminder => r
                .call(
                    "wake.create",
                    json!({"when": "in 90m", "note": self.words(), "session_id": sid}),
                )
                .map(|_| ()),
            Item::Task => r
                .call(
                    "task.start",
                    json!({"brief": self.words(), "home": sid, "start_when": "in 90m"}),
                )
                .map(|_| ()),
        }
        .unwrap();
    }
}

/// The daemon killed while its create of the item's forum post is held at `at`
/// (not yet at Discord, or at Discord and unanswered, so before the daemon's row
/// of it), then started again: one post for the item, and its first card and tag.
fn killed_with_the_post_held(item: Item, at: HoldAt) {
    let r = Rig::new();
    r.fake.hold_thread_creates(Some(at));
    let daemon = r.spawn();
    let sid = r.session();
    item.set(&r, &sid);
    r.settle("the post's create held", || {
        r.fake.held_thread_creates() >= 1
    });
    drop(daemon);
    r.fake.hold_thread_creates(None);
    std::thread::sleep(Duration::from_secs(1));
    let _daemon = r.spawn();
    r.settle("the item's post, scheduled", || {
        r.posts(item.words()).first().map(|p| p.1.clone()) == Some(vec!["scheduled".to_string()])
    });
    // Time for any second post to be made.
    std::thread::sleep(Duration::from_secs(4));
    assert_eq!(r.posts(item.words()).len(), 1, "{}", r.held());
    assert_eq!(
        r.cards(item.words()).len(),
        1,
        "the fake holds:\n{}\nthe daemon's card lines:\n{}",
        r.held(),
        r.card_lines()
    );
}

/// The post is at Discord and unanswered: the restarted daemon finds it by its
/// marker and makes none.
#[test]
fn a_reminders_post_made_at_discord_before_the_kill_is_found_not_made_again() {
    killed_with_the_post_held(Item::Reminder, HoldAt::AfterApply);
}

#[test]
fn a_tasks_post_made_at_discord_before_the_kill_is_found_not_made_again() {
    killed_with_the_post_held(Item::Task, HoldAt::AfterApply);
}

/// The create never reached Discord: the restarted daemon makes the item's
/// first post, and only one.
#[test]
fn a_reminders_post_whose_create_never_arrived_is_made_once_after_the_kill() {
    killed_with_the_post_held(Item::Reminder, HoldAt::BeforeApply);
}

#[test]
fn a_tasks_post_whose_create_never_arrived_is_made_once_after_the_kill() {
    killed_with_the_post_held(Item::Task, HoldAt::BeforeApply);
}

/// The card is at Discord and unanswered when the daemon is killed, and Discord's
/// nonce window has closed by the restart (as the real one does): the restarted
/// daemon finds the card by the item's short id in its words, and makes no second.
#[test]
fn a_card_made_at_discord_before_the_kill_is_found_not_made_again() {
    let r = Rig::new();
    r.fake.set_nonce_window_ms(1);
    r.fake.hold_writes_containing(Some("tide check"));
    let daemon = r.spawn();
    let sid = r.session();
    Item::Reminder.set(&r, &sid);
    r.settle("the card at Discord, unanswered", || {
        r.cards("tide check").len() == 1
    });
    drop(daemon);
    r.fake.hold_writes_containing(None);
    std::thread::sleep(Duration::from_secs(2));
    let _daemon = r.spawn();
    r.settle("the item's post, scheduled", || {
        r.posts("tide check").first().map(|p| p.1.clone()) == Some(vec!["scheduled".to_string()])
    });
    std::thread::sleep(Duration::from_secs(4));
    assert_eq!(r.cards("tide check").len(), 1, "{}", r.held());
}

/// The card's create never reached Discord (the daemon was killed with it held):
/// the restarted daemon makes the task's one card, and the pinned board, which
/// lists the task with the same marker, is never edited into it.
#[test]
fn a_cards_create_that_never_arrived_is_made_once_and_the_board_is_untouched() {
    let r = Rig::new();
    r.fake.drop_message_creates_containing(Some("Task · from"));
    let daemon = r.spawn();
    let sid = r.session();
    Item::Task.set(&r, &sid);
    r.settle("the card's create held", || {
        r.fake.held_message_creates() >= 1
    });
    // The pinned board is drawn on its own pace: it lists the task, with the
    // marker the card would carry, before the kill, so a card finder that took
    // any message with the marker would adopt it.
    r.settle("the board lists the task", || {
        r.fake.all_messages().iter().any(|m| {
            m.versions.first().is_some_and(|v| v.starts_with("📋"))
                && m.versions
                    .last()
                    .is_some_and(|v| v.contains("Paint the buoy"))
        })
    });
    drop(daemon);
    r.fake.drop_message_creates_containing(None);
    // The restarted daemon draws its board again, empty at first and then listing
    // the task. The read that tells whether the card exists fails until the board
    // lists the task again, so the finder reads a board with the marker in it.
    r.fake.fail_message_lists(true);
    std::thread::sleep(Duration::from_secs(1));
    let _daemon = r.spawn();
    std::thread::sleep(Duration::from_secs(5));
    r.fake.fail_message_lists(false);
    r.settle("the task's card", || r.cards("Paint the buoy").len() == 1);
    std::thread::sleep(Duration::from_secs(4));
    assert_eq!(r.cards("Paint the buoy").len(), 1, "{}", r.held());
    let boards: Vec<Msg> = r
        .fake
        .all_messages()
        .into_iter()
        .filter(|m| m.versions.first().is_some_and(|v| v.starts_with("📋")))
        .collect();
    assert!(!boards.is_empty(), "the board is there");
    for b in boards {
        assert!(
            b.versions.iter().all(|v| v.starts_with("📋")),
            "the board never showed a card: {:?}",
            b.versions
        );
    }
}
