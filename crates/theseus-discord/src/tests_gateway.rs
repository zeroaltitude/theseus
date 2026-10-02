//! The binding end to end through a stand-in for all of Discord it talks to
//! (theseus-6g62): `theseus_sim::fake_discord` for REST, with its gateway.
//! A test types a message as a user and presses a card's button as one, the
//! way Discord sends both, and reads back what the binding posted and
//! answered (theseus-qifw). The guild the stand-in holds answers the viewer
//! check, so a card in a trusted guild channel is tested end to end
//! (theseus-ck0k). A real core over a scratch store; the model is scripted.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use theseus_core::config::ApprovalConfig;
use theseus_core::policy::Posture;
use theseus_core::provider::{FakeProvider, Provider, Scripted};
use theseus_core::secrets::{Secret, SecretBoard};
use theseus_core::{Config, Core};
use theseus_sim::fake_discord::{FakeDiscord, Guild, Msg, Pressed, Typed, BOT_ID, DEFAULT_GUILD};

/// Invented people and places.
const ANA: u64 = 900_000_000_000_000_101;
const BEN: u64 = 900_000_000_000_000_202;
const CY: u64 = 900_000_000_000_000_303;
const LAB: u64 = 900_000_000_000_000_010;
/// The stand-in's DM channel with `ANA`.
const ANA_DM: u64 = ANA + 1;

/// `#lab`, where ana may drive Theseus, and ana's DM.
fn bindings() -> String {
    format!(
        "guild_id = \"{DEFAULT_GUILD}\"\n\
         [[channel]]\nid = \"{LAB}\"\nname = \"lab\"\nusers = [\"{ANA}\"]\nmention_only = false\n\
         [[dm]]\nuser = \"{ANA}\"\nname = \"ana\"\n"
    )
}

/// ana and ben are trusted; approvals may happen in the CLI, a trusted DM,
/// and `#lab`.
fn approval() -> ApprovalConfig {
    ApprovalConfig {
        trusted_users: vec![format!("discord:{ANA}"), format!("discord:{BEN}")],
        channels: vec!["cli".into(), "discord:dm".into(), format!("discord:{LAB}")],
    }
}

/// The guild: ana owns it, ben and cy are members, and `lab` is private to
/// ana and ben, or with `open_lab` visible to everyone, cy included.
fn guild(open_lab: bool) -> Guild {
    let g = Guild::new(DEFAULT_GUILD, (ANA, "ana"))
        .member(BEN, "ben")
        .member(CY, "cy");
    if open_lab {
        g.channel(LAB, "lab")
    } else {
        g.private_channel(LAB, "lab", &[ANA, BEN])
    }
}

struct Rig {
    dir: tempfile::TempDir,
    fake: Arc<FakeDiscord>,
    core: Arc<Core>,
}

impl Rig {
    /// A core whose binding talks only to the stand-in, bound to `#lab` and
    /// ana's DM, its gateway connected. `script` gets the rig's directory.
    async fn start(script: impl FnOnce(&Path) -> Vec<Scripted>, open_lab: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let fake = FakeDiscord::start_with_gateway();
        fake.set_guild(guild(open_lab));
        let core = core_at(dir.path(), &fake, script(dir.path()));
        let path = dir.path().join("bindings.toml");
        std::fs::write(&path, bindings()).unwrap();
        // The continuation driver, as the daemon starts it: an answered
        // card's call runs in the turn it resumes.
        tokio::spawn(theseus_core::harness::drive(core.clone()));
        tokio::spawn(crate::run(core.clone(), core.cfg.discord.clone(), path));
        let gw = fake.gateway().unwrap().clone();
        let connected =
            tokio::task::spawn_blocking(move || gw.wait_connected(Duration::from_secs(30)))
                .await
                .unwrap();
        assert!(
            connected,
            "the gateway never identified: {:?}",
            fake.gateway_state()
        );
        let r = Self { dir, fake, core };
        r.until("both places bound", || {
            [format!("channel:{LAB}"), format!("dm:{ANA}")]
                .iter()
                .all(|k| r.core.outbox.place_session(k).unwrap().is_some())
        })
        .await;
        r
    }

    async fn until(&self, what: &str, f: impl Fn() -> bool) {
        let t0 = Instant::now();
        while !f() {
            assert!(
                t0.elapsed() < Duration::from_secs(20),
                "timed out: {what}; #lab holds {:#?}; the DM {:#?}; answers {:#?}",
                self.fake.messages(LAB),
                self.fake.messages(ANA_DM),
                self.fake.replies()
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    fn say(&self, user: (u64, &str), channel: Option<u64>, content: &str) -> String {
        self.fake
            .say(&Typed {
                user: user.0,
                name: user.1,
                channel,
                content,
            })
            .unwrap()
    }

    fn press(&self, message: &str, button: &str, user: (u64, &str)) -> String {
        self.fake
            .press(&Pressed {
                message,
                button,
                user: user.0,
                name: user.1,
            })
            .unwrap()
    }

    /// What the bot posted in `channel`, in order.
    fn posted(&self, channel: u64) -> Vec<Msg> {
        self.fake
            .messages(channel)
            .into_iter()
            .filter(|m| m.author == BOT_ID.to_string())
            .collect()
    }

    /// The message in `channel` that had buttons when it was posted: a card.
    fn card(&self, channel: u64) -> Option<Msg> {
        self.posted(channel)
            .into_iter()
            .find(|m| m.versions[0].contains("**Approve?**"))
    }

    fn ledger(&self, kind: &str) -> Vec<serde_json::Value> {
        let rows: Vec<(u64, theseus_core::ledger::LedgerRow)> =
            self.core.store.ledger_tail(2000).unwrap();
        rows.into_iter()
            .filter(|(_, r)| r.kind == kind)
            .map(|(_, r)| r.data)
            .collect()
    }

    fn waiting(&self) -> usize {
        self.core.confirm_list().map_or(0, |l| l.len())
    }
}

/// Where the scripted write goes: outside the workspace's roots.
fn outside(dir: &Path) -> PathBuf {
    dir.join("outside").join("proof.txt")
}

/// A core over `dir` whose binding talks to `fake` and never to Discord.
fn core_at(dir: &Path, fake: &FakeDiscord, script: Vec<Scripted>) -> Arc<Core> {
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.to_string_lossy().into_owned();
    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::create_dir_all(dir.join("outside")).unwrap();
    cfg.tools.projects_dir = Some(work.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = Posture::Notify;
    cfg.discord.rest_proxy = Some(fake.addr.clone());
    cfg.discord.gateway_proxy = fake.gateway().map(|g| g.url());
    cfg.discord.edit_interval_ms = 250;
    cfg.approval = Some(approval());
    let name = cfg.discord.token_secret.clone();
    let secrets = SecretBoard::new([name.clone()], Instant::now());
    secrets.publish(
        [(name, Ok(Secret::new("fake-token-not-a-secret".into())))].into(),
        "test",
    );
    let store = theseus_core::store::Store::open(&dir.join("store")).unwrap();
    let model: Arc<dyn Provider> = Arc::new(FakeProvider::scripted(script));
    let providers = [(cfg.model.provider.clone(), model)].into_iter().collect();
    Core::build(theseus_core::rpc::Parts {
        cfg,
        providers,
        store,
        secrets,
        startup_log: Arc::default(),
        telemetry: Some(theseus_core::telemetry::Telemetry::disabled()),
        scrubber: Arc::new(theseus_core::scrub::Scrubber::default()),
        launcher: Arc::new(theseus_core::toolrun::InlineLauncher),
        config_gate: theseus_core::config_gate::ConfigGate::file("test"),
        toollets: vec![],
    })
    .unwrap()
}

/// The write outside the roots that waits for its approval, then the reply
/// once it ran.
fn write_script(dir: &Path) -> Vec<Scripted> {
    vec![
        Scripted::tools(
            "Writing it.",
            &[(
                "t1",
                "fs_write",
                serde_json::json!({"path": outside(dir).to_string_lossy(), "content": "written through Discord"}),
            )],
        ),
        Scripted::text("Done: the file is written."),
    ]
}

/// theseus-6g62: a message typed in a bound channel, as Discord sends it, is
/// a turn, and its reply answers that message there. A message from someone
/// the place doesn't list starts nothing, and is counted.
#[tokio::test]
async fn a_typed_message_is_a_turn_and_its_reply_answers_it_in_the_channel() {
    let r = Rig::start(|_| vec![Scripted::text("ready")], false).await;
    let typed = r.say(
        (ANA, "ana"),
        Some(LAB),
        "Reply with exactly one word: ready",
    );
    r.until("the reply in #lab", || {
        r.posted(LAB).iter().any(|m| m.content.contains("ready"))
    })
    .await;
    let reply = r
        .posted(LAB)
        .into_iter()
        .find(|m| m.content.contains("ready"))
        .unwrap();
    assert_eq!(reply.reply_to.as_deref(), Some(typed.as_str()), "{reply:?}");
    let turns = r.ledger("discord.message.in").len();
    // ben is not one of #lab's users.
    r.say(
        (BEN, "ben"),
        Some(LAB),
        "Reply with exactly one word: ready",
    );
    // The row names the author as a string, as Discord's ids are.
    let ben = BEN.to_string();
    r.until("ben's message ignored", || {
        r.ledger("discord.ignored")
            .iter()
            .any(|d| d["author_id"] == ben)
    })
    .await;
    assert_eq!(
        r.ledger("discord.message.in").len(),
        turns,
        "ben started nothing"
    );
}

/// theseus-6g62 and theseus-ck0k: a write outside the roots waits, and its
/// card stays in `#lab`, a channel `[approval]` lists that the viewer check
/// trusts (only ana and ben can view it), naming its one answerer. Pressed
/// by ben, who is trusted but not one of `#lab`'s users, it is refused and
/// the call keeps waiting; pressed by ana, Approve is acknowledged, the call
/// runs, the card says so and loses its buttons, and the reply comes.
#[tokio::test]
async fn approve_pressed_in_a_trusted_channel_runs_the_call_and_settles_the_card() {
    let r = Rig::start(write_script, false).await;
    r.say((ANA, "ana"), Some(LAB), "Write the proof file.");
    r.until("the card in #lab", || r.card(LAB).is_some()).await;
    let card = r.card(LAB).unwrap();
    assert!(
        card.content
            .starts_with(&format!("<@{ANA}> **Approve?** `fs.write`")),
        "{}",
        card.content
    );
    assert_eq!(card.mentions, [ANA.to_string()], "only ana can answer it");
    let labels: Vec<&str> = card.buttons.iter().map(|b| b.label.as_str()).collect();
    assert_eq!(labels, ["Approve", "Decline"]);
    assert_eq!(r.waiting(), 1);
    let checked = r.core.approval.checked(LAB).expect("#lab was checked");
    assert!(checked.trusted, "{}", checked.detail);

    let refused = r.press(&card.id, "Approve", (BEN, "ben"));
    r.until("ben is told no", || {
        r.fake
            .replies()
            .iter()
            .any(|a| a.interaction.as_deref() == Some(refused.as_str()))
    })
    .await;
    let answer = r
        .fake
        .replies()
        .into_iter()
        .find(|a| a.interaction.as_deref() == Some(refused.as_str()))
        .unwrap();
    assert_eq!(
        (
            answer.response_type,
            answer.ephemeral,
            answer.content.as_deref()
        ),
        (
            Some(4),
            true,
            Some("Only the people this place is bound to can do that.")
        )
    );
    assert_eq!(r.waiting(), 1, "the call still waits");
    assert!(!outside(r.dir.path()).exists());

    let pressed = r.press(&card.id, "Approve", (ANA, "ana"));
    r.until("the file written", || outside(r.dir.path()).exists())
        .await;
    assert_eq!(
        std::fs::read_to_string(outside(r.dir.path())).unwrap(),
        "written through Discord"
    );
    r.until("the card settled and the reply posted", || {
        r.card(LAB).is_some_and(|c| c.buttons.is_empty())
            && r.posted(LAB)
                .iter()
                .any(|m| m.content.contains("the file is written"))
    })
    .await;
    let acks: Vec<Option<u64>> = r
        .fake
        .replies()
        .iter()
        .filter(|a| a.interaction.as_deref() == Some(pressed.as_str()))
        .map(|a| a.response_type)
        .collect();
    assert_eq!(acks, [Some(6)], "acknowledged as a deferred update");
    let card = r.card(LAB).unwrap();
    assert_eq!(card.versions.len(), 2, "{:?}", card.versions);
    assert!(
        card.content
            .starts_with("✅ **Approved** by discord:ana · `fs.write`"),
        "{}",
        card.content
    );
    let confirms = r.ledger("discord.confirm");
    assert_eq!(
        confirms.len(),
        1,
        "ben's press never reached the core: {confirms:?}"
    );
    assert_eq!(confirms[0]["ok"], true);
    assert_eq!(r.waiting(), 0);
}

/// theseus-ck0k: when cy, who is not trusted, can view `#lab`, the viewer
/// check does not trust it, so the card goes to ana's DM with a note in
/// `#lab`, and a press there approves it.
#[tokio::test]
async fn a_listed_channel_an_outsider_can_view_sends_its_card_to_the_dm() {
    let r = Rig::start(write_script, true).await;
    r.say((ANA, "ana"), Some(LAB), "Write the proof file.");
    r.until("the card in the DM", || r.card(ANA_DM).is_some())
        .await;
    assert!(r.card(LAB).is_none(), "no card in #lab");
    let checked = r.core.approval.checked(LAB).expect("#lab was checked");
    assert!(!checked.trusted);
    assert!(
        checked.detail.contains(&format!("cy ({CY})")),
        "{}",
        checked.detail
    );
    r.until("the note in #lab", || {
        r.posted(LAB).iter().any(|m| m.content.starts_with("🔐"))
    })
    .await;
    let card = r.card(ANA_DM).unwrap();
    r.press(&card.id, "Approve", (ANA, "ana"));
    r.until("the file written", || outside(r.dir.path()).exists())
        .await;
    r.until("the DM's card settled", || {
        r.card(ANA_DM).is_some_and(|c| c.buttons.is_empty())
    })
    .await;
}
