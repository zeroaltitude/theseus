//! The binding end to end through a stand-in for all of Discord it talks to
//! (theseus-6g62): `theseus_sim::fake_discord` for REST, with its gateway.
//! A test types a message as a user and presses a card's button as one, the
//! way Discord sends both, and reads back what the binding posted and
//! answered (theseus-qifw). The guild the stand-in holds answers the place
//! rule's viewer read, and a card in a private channel, or one sent to the
//! owner's DM from a shared channel, is tested end to end (theseus-ck0k,
//! theseus-zmgb). A real core over a scratch store; the model is scripted.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use theseus_core::policy::Posture;
use theseus_core::provider::{FakeProvider, Provider, Scripted};
use theseus_core::secrets::{Secret, SecretBoard};
use theseus_core::{Config, Core};
use theseus_sim::fake_discord::{FakeDiscord, Guild, Msg, Pressed, Typed, BOT_ID, DEFAULT_GUILD};

/// Invented people and places.
pub(crate) const ANA: u64 = 900_000_000_000_000_101;
pub(crate) const BEN: u64 = 900_000_000_000_000_202;
const CY: u64 = 900_000_000_000_000_303;
pub(crate) const LAB: u64 = 900_000_000_000_000_010;
/// The stand-in's DM channel with `ANA`.
pub(crate) const ANA_DM: u64 = ANA + 1;

/// `#lab`, where ana may drive Theseus, bound private or shared, and ana's DM.
fn bindings(lab_private: bool) -> String {
    format!(
        "guild_id = \"{DEFAULT_GUILD}\"\n\
         [[channel]]\nid = \"{LAB}\"\nname = \"lab\"\nusers = [\"{ANA}\"]\nmention_only = false\nprivate = {lab_private}\n\
         [[dm]]\nuser = \"{ANA}\"\nname = \"ana\"\n"
    )
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

pub(crate) struct Rig {
    pub(crate) dir: tempfile::TempDir,
    pub(crate) fake: Arc<FakeDiscord>,
    pub(crate) core: Arc<Core>,
}

impl Rig {
    /// A core whose binding talks only to the stand-in, bound to `#lab`
    /// (private) and ana's DM, its gateway connected. `script` gets the rig's
    /// directory.
    async fn start(script: impl FnOnce(&Path) -> Vec<Scripted>, open_lab: bool) -> Self {
        Self::start_with(
            |dir, _| Arc::new(FakeProvider::scripted(script(dir))),
            open_lab,
            true,
        )
        .await
    }

    /// The same, with `#lab` bound shared and open to everyone.
    async fn shared(script: impl FnOnce(&Path) -> Vec<Scripted>) -> Self {
        Self::start_with(
            |dir, _| Arc::new(FakeProvider::scripted(script(dir))),
            true,
            false,
        )
        .await
    }

    /// A rig whose model is `model`, given the stand-in, so an answer can
    /// change the guild while its turn runs (M4 19c).
    async fn start_with(
        model: impl FnOnce(&Path, Arc<FakeDiscord>) -> Arc<dyn Provider>,
        open_lab: bool,
        lab_private: bool,
    ) -> Self {
        Self::start_on(model, guild(open_lab), &bindings(lab_private), &[]).await
    }

    /// The same, on `guild` and the bindings file `bindings`, waiting for
    /// `#lab`, ana's DM, and each place of `more` (`channel:<id>`) to bind.
    pub(crate) async fn start_on(
        model: impl FnOnce(&Path, Arc<FakeDiscord>) -> Arc<dyn Provider>,
        guild: Guild,
        bindings: &str,
        more: &[String],
    ) -> Self {
        Self::start_tweaked(model, guild, bindings, more, |_| {}).await
    }

    /// The same, with the config changed by `tweak` before the core builds.
    async fn start_tweaked(
        model: impl FnOnce(&Path, Arc<FakeDiscord>) -> Arc<dyn Provider>,
        guild: Guild,
        bindings: &str,
        more: &[String],
        tweak: impl FnOnce(&mut Config),
    ) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let fake = FakeDiscord::start_with_gateway();
        fake.set_guild(guild);
        let core = core_at(dir.path(), &fake, model(dir.path(), fake.clone()), tweak);
        // The ladder's warm read, as the daemon reads it after serving: no
        // judged point reads it (theseus-289c).
        if core.cfg.judge.enabled {
            core.runner.judge.read_ladder();
        }
        let path = dir.path().join("bindings.toml");
        std::fs::write(&path, bindings).unwrap();
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
        r.until("every place bound", || {
            [format!("channel:{LAB}"), format!("dm:{ANA}")]
                .iter()
                .chain(more)
                .all(|k| r.core.outbox.place_session(k).unwrap().is_some())
        })
        .await;
        r
    }

    pub(crate) async fn until(&self, what: &str, f: impl Fn() -> bool) {
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

    pub(crate) fn say(&self, user: (u64, &str), channel: Option<u64>, content: &str) -> String {
        self.fake
            .say(&Typed {
                user: user.0,
                name: user.1,
                channel,
                content,
                file: None,
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
    pub(crate) fn posted(&self, channel: u64) -> Vec<Msg> {
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

    pub(crate) fn ledger(&self, kind: &str) -> Vec<serde_json::Value> {
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

/// Where the scripted write goes: outside the workspace's roots, under a
/// path on the approve list, so it waits (a write outside the roots alone
/// takes its tool's posture, theseus-ewi).
fn outside(dir: &Path) -> PathBuf {
    dir.join("outside").join("proof.txt")
}

/// A core over `dir` whose binding talks to `fake` and never to Discord.
fn core_at(
    dir: &Path,
    fake: &FakeDiscord,
    model: Arc<dyn Provider>,
    tweak: impl FnOnce(&mut Config),
) -> Arc<Core> {
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.to_string_lossy().into_owned();
    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::create_dir_all(dir.join("outside")).unwrap();
    cfg.tools.projects_dir = Some(work.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.tools
        .approve_paths
        .push(dir.join("outside").to_string_lossy().into_owned());
    cfg.policy.enforcement = Posture::Notify;
    cfg.discord.rest_proxy = Some(fake.addr.clone());
    cfg.discord.gateway_proxy = fake.gateway().map(|g| g.url());
    cfg.discord.edit_interval_ms = 250;
    // ana and ben are the owners (the place rule, theseus-zmgb). A wake
    // asks first, so a shared place has a call that waits.
    cfg.places.owner = Some(vec![format!("discord:{ANA}"), format!("discord:{BEN}")]);
    cfg.policy.tools.insert("wake.at".into(), Posture::Approve);
    tweak(&mut cfg);
    let name = cfg.discord.token_secret.clone();
    let jev = cfg.judge.key_secret.clone();
    let secrets = SecretBoard::new([name.clone(), jev.clone()], Instant::now());
    secrets.publish(
        [
            (name, Ok(Secret::new("fake-token-not-a-secret".into()))),
            (jev, Ok(Secret::new("jev-test-key-0123456789".into()))),
        ]
        .into(),
        "test",
    );
    let store = theseus_core::store::Store::open(&dir.join("store")).unwrap();
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
        cpu_cores: None,
    })
    .unwrap()
}

/// The write on the approve list that waits for its approval, then the reply
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

/// theseus-6g62 and the place rule (theseus-zmgb): a write on the approve
/// list waits, and its card stays in `#lab`, a channel bound private, naming
/// its one answerer, the place's user who is an owner. Pressed by ben, an
/// owner but not one of `#lab`'s users, it is refused and the call keeps
/// waiting; pressed by ana, Approve is acknowledged, the call runs, the card
/// says so and loses its buttons, and the reply comes.
/// The owner's case (theseus-c9l6): a PDF attached to a typed message is
/// downloaded from the CDN (the stand-in's), kept whole, and read by the
/// model as a document block of its own bytes, with its line before it.
#[tokio::test]
async fn a_pdf_attached_to_a_typed_message_reaches_the_model_as_a_document() {
    let provider = Arc::new(FakeProvider::scripted(vec![Scripted::text(
        "Odile Varnack.",
    )]));
    let model = provider.clone();
    let r = Rig::start_with(move |_, _| model, false, true).await;
    let pdf = theseus_files::pdf::sample(&["The harbour master is Odile Varnack."]);
    let path = r.dir.path().join("orders.pdf");
    std::fs::write(&path, &pdf).unwrap();
    r.fake
        .say(&Typed {
            user: ANA,
            name: "ana",
            channel: Some(LAB),
            content: "Who is the harbour master?",
            file: Some(&path),
        })
        .unwrap();
    r.until("the reply in #lab", || {
        r.posted(LAB)
            .iter()
            .any(|m| m.content.contains("Odile Varnack."))
    })
    .await;
    let req = provider.requests().pop().expect("a request");
    let user = req
        .messages
        .iter()
        .rev()
        .find(|m| m["role"] == "user")
        .unwrap();
    let blocks = user["content"].as_array().unwrap();
    assert!(
        blocks[0]["text"]
            .as_str()
            .unwrap()
            .starts_with("[PDF orders.pdf from discord:ana, "),
        "{blocks:?}"
    );
    assert_eq!(blocks[1]["type"], "document");
    assert_eq!(
        theseus_core::blobs::decode(blocks[1]["source"]["data"].as_str().unwrap()).unwrap(),
        pdf
    );
    assert_eq!(blocks[2]["text"], "Who is the harbour master?");
}

#[tokio::test]
async fn approve_pressed_in_a_private_channel_runs_the_call_and_settles_the_card() {
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

/// The place rule (theseus-zmgb): `#lab` is bound shared, so a call that
/// waits there sends its card to ana's DM, an owner's, with a note in `#lab`,
/// and her press there approves it.
#[tokio::test]
async fn a_shared_channels_card_goes_to_the_owners_dm() {
    let r = Rig::shared(|_| {
        vec![
            Scripted::tools(
                "Setting it.",
                &[(
                    "t1",
                    "wake_at",
                    serde_json::json!({"after": "10m", "note": "check the build"}),
                )],
            ),
            Scripted::text("Set: I will check the build in ten minutes."),
        ]
    })
    .await;
    r.say((ANA, "ana"), Some(LAB), "Remind me to check the build.");
    r.until("the card in the DM", || r.card(ANA_DM).is_some())
        .await;
    assert!(r.card(LAB).is_none(), "no card in #lab");
    r.until("the note in #lab", || {
        r.posted(LAB).iter().any(|m| {
            m.content.starts_with("🔐")
                && m.content
                    .contains("this channel is shared, and an answer counts only")
        })
    })
    .await;
    let card = r.card(ANA_DM).unwrap();
    r.press(&card.id, "Approve", (ANA, "ana"));
    r.until("the wake set and the reply in #lab", || {
        r.posted(LAB)
            .iter()
            .any(|m| m.content.contains("I will check the build"))
    })
    .await;
    r.until("the DM's card settled", || {
        r.card(ANA_DM).is_some_and(|c| c.buttons.is_empty())
    })
    .await;
    assert_eq!(r.waiting(), 0);
}

/// The place rule (theseus-nbsh): `#lab` is bound private, so ana's turn
/// there reads the owner's file whole, whoever can view the channel: the
/// operator's word decides, and the start-time read only warns.
#[tokio::test]
async fn a_channel_bound_private_reads_the_owners_file_whole() {
    for open in [true, false] {
        let r = Rig::start(
            |dir| {
                std::fs::create_dir_all(dir.join("work")).unwrap();
                std::fs::write(
                    dir.join("work").join("notes.txt"),
                    "the vault code is 4417\n",
                )
                .unwrap();
                vec![
                    Scripted::tools(
                        "Reading it.",
                        &[("t1", "fs_read", serde_json::json!({"path": "notes.txt"}))],
                    ),
                    Scripted::text("Done."),
                ]
            },
            open,
        )
        .await;
        r.say((ANA, "ana"), Some(LAB), "read notes.txt");
        r.until("the reply in #lab", || {
            r.posted(LAB).iter().any(|m| m.content.contains("Done."))
        })
        .await;
        let compiled = r.ledger("context.compiled");
        assert!(
            compiled.iter().all(|c| c["class"] == "private"),
            "{compiled:?}"
        );
        assert!(r.ledger("tool.invalid_input").is_empty(), "open: {open}");
        let sid = r
            .core
            .outbox
            .place_session(&format!("channel:{LAB}"))
            .unwrap()
            .unwrap();
        let read = r
            .core
            .store
            .session_nodes(&sid)
            .unwrap()
            .into_iter()
            .any(|(_, n)| {
                matches!(&n.body, theseus_core::node::Body::ToolResult { content, .. }
                if content.contains("4417"))
            });
        assert!(read, "the owner's file, whole: open {open}");
    }
}

/// The place rule's one check (theseus-nbsh): `#lab` is bound private, so
/// the binding reads who can view it once, as it starts. When cy, who is not
/// an owner, can view it, health names cy; when only ana and ben (the
/// owners) can, nobody.
#[tokio::test]
async fn a_channel_bound_private_is_read_at_the_start_and_an_outsider_named() {
    for (open, others) in [(true, vec!["cy".to_string()]), (false, vec![])] {
        let r = Rig::start(|_| vec![], open).await;
        let lab = format!("discord:channel:{LAB}");
        let viewed = || {
            r.core
                .health()
                .places
                .and_then(|h| h.places.into_iter().find(|p| p.place == lab))
                .and_then(|p| p.others)
        };
        r.until("#lab's viewers read", || viewed().is_some()).await;
        assert_eq!(viewed().unwrap(), others, "open: {open}");
        let class = r.core.health().places.unwrap().places;
        assert!(
            class
                .iter()
                .any(|p| p.place == lab && p.class == theseus_core::places::PlaceClass::Private),
            "{class:?}"
        );
    }
}

/// A channel bound `private = false` in the trusted guild (theseus-rdqg).
const HALL: u64 = 900_000_000_000_000_020;

/// theseus-rdqg: the guild trusted whole (`private = true` beside
/// `guild_id`), with `#lab`, which says nothing of `private`, `#hall`, which
/// says `private = false`, and ana's DM.
fn trusted_bindings() -> String {
    format!(
        "guild_id = \"{DEFAULT_GUILD}\"\nprivate = true\n\
         [[channel]]\nid = \"{LAB}\"\nname = \"lab\"\nusers = [\"{ANA}\"]\nmention_only = false\n\
         [[channel]]\nid = \"{HALL}\"\nname = \"hall\"\nusers = [\"{ANA}\"]\nmention_only = false\nprivate = false\n\
         [[dm]]\nuser = \"{ANA}\"\nname = \"ana\"\n"
    )
}

/// The guild with `#lab` and `#hall` both open to everyone, cy included.
fn open_guild() -> Guild {
    guild(true).channel(HALL, "hall")
}

/// The writes of the trusted guild's test: one in `#lab` (`t1`), approved
/// there, then one in `#hall` (`t2`), which the gate refuses.
fn two_writes(dir: &Path) -> Vec<Scripted> {
    let write = |id: &str, file: &str| {
        Scripted::tools(
            "Writing it.",
            &[(
                id,
                "fs_write",
                serde_json::json!({"path": dir.join("outside").join(file).to_string_lossy(), "content": "written"}),
            )],
        )
    };
    vec![
        write("t1", "lab.txt"),
        Scripted::text("Done: written in lab."),
        write("t2", "hall.txt"),
        Scripted::text("Done in hall."),
    ]
}

/// The tools a request offered, by their wire names.
fn offered(req: &theseus_core::provider::ProviderRequest) -> Vec<String> {
    let names = req.tools.iter().filter_map(|t| t["name"].as_str());
    names.map(str::to_string).collect()
}

/// theseus-rdqg, end to end from the bindings file. In a guild trusted whole,
/// `#lab`, which says nothing of `private`, is private though cy can view it:
/// the binding reads nobody's view of it (no members read), health names it
/// as in a trusted guild with no warning, its model is offered every tool,
/// and ana's Approve on its write, pressed in `#lab`, counts and runs it.
/// `#hall`, bound `private = false` there, is shared: its write is not
/// offered, and the gate refuses it. Without the guild's word the same file's
/// `#lab` is shared, and bound private it is read and cy named (the old
/// meaning).
#[tokio::test]
async fn a_trusted_guilds_channel_is_private_and_unread_and_one_bound_shared_is_not() {
    let slot = Arc::new(std::sync::Mutex::new(None));
    let kept = slot.clone();
    let model = move |dir: &Path, _: Arc<FakeDiscord>| -> Arc<dyn Provider> {
        let m = Arc::new(FakeProvider::scripted(two_writes(dir)));
        *kept.lock().unwrap() = Some(m.clone());
        m
    };
    let hall = [format!("channel:{HALL}")];
    let r = Rig::start_on(model, open_guild(), &trusted_bindings(), &hall).await;
    let model = slot.lock().unwrap().clone().unwrap();
    use theseus_core::places::PlaceClass;
    let lab = place_in(&r, LAB);
    assert_eq!((lab.class, lab.trusted_guild), (PlaceClass::Private, true));
    let shared = place_in(&r, HALL);
    assert_eq!(
        (shared.class, shared.trusted_guild),
        (PlaceClass::Shared, false)
    );

    r.say((ANA, "ana"), Some(LAB), "Write the lab file.");
    r.until("the card in #lab", || r.card(LAB).is_some()).await;
    for tool in ["fs_write", "proc_run"] {
        assert!(
            offered(&model.requests()[0]).contains(&tool.to_string()),
            "{tool}"
        );
    }
    let card = r.card(LAB).unwrap();
    r.press(&card.id, "Approve", (ANA, "ana"));
    let written = r.dir.path().join("outside").join("lab.txt");
    r.until("the lab file written", || written.exists()).await;
    r.until("the reply in #lab", || {
        r.posted(LAB)
            .iter()
            .any(|m| m.content.contains("written in lab"))
    })
    .await;
    assert!(r.card(ANA_DM).is_none(), "the card stayed in #lab");

    r.say((ANA, "ana"), Some(HALL), "Write the hall file.");
    r.until("the reply in #hall", || {
        r.fake
            .messages(HALL)
            .iter()
            .any(|m| m.content.contains("Done in hall."))
    })
    .await;
    let asked = model.requests();
    let hall_req = asked
        .iter()
        .find(|q| !offered(q).contains(&"proc_run".to_string()));
    assert!(hall_req.is_some_and(|q| !offered(q).contains(&"fs_write".to_string())));
    let refused = r.ledger("tool.invalid_input");
    assert!(
        refused.iter().any(|row| row["tool_use_id"] == "t2"
            && row["reason"]
                .as_str()
                .unwrap_or_default()
                .starts_with("place: fs.write is not offered")),
        "{refused:?}"
    );
    assert!(!r.dir.path().join("outside").join("hall.txt").exists());

    // Nothing was read: no member list read, and health names no viewer.
    let lab = place_in(&r, LAB);
    assert!(lab.others.is_none() && lab.unchecked.is_none(), "{lab:?}");
    assert!(!read_the_members(&r), "{:?}", r.fake.seen());
    assert!(r.ledger("place.viewed").is_empty());
    drop(r);

    // An old file: the same guild, without the guild's word.
    let quiet = |_: &Path, _: Arc<FakeDiscord>| -> Arc<dyn Provider> {
        Arc::new(FakeProvider::scripted(vec![]))
    };
    let old = trusted_bindings().replacen("private = true\n", "", 1);
    let r = Rig::start_on(quiet, open_guild(), &old, &hall).await;
    assert_eq!(
        place_in(&r, LAB).class,
        PlaceClass::Shared,
        "the old meaning"
    );
    drop(r);
    let bound = old.replacen(
        "mention_only = false\n",
        "mention_only = false\nprivate = true\n",
        1,
    );
    let r = Rig::start_on(quiet, open_guild(), &bound, &hall).await;
    r.until("#lab's viewers read", || place_in(&r, LAB).others.is_some())
        .await;
    let lab = place_in(&r, LAB);
    assert_eq!(
        (lab.others, lab.trusted_guild),
        (Some(vec!["cy".to_string()]), false)
    );
    assert!(read_the_members(&r), "{:?}", r.fake.seen());
}

/// Whether the binding read the guild's member list, which only the viewer
/// read does (`check_private`); the bot's read of its own member, for its
/// roles, is another route.
fn read_the_members(r: &Rig) -> bool {
    r.fake
        .seen()
        .iter()
        .any(|s| s.method == "GET" && s.path.ends_with("/members"))
}

/// Health's entry for the channel `id`.
fn place_in(r: &Rig, id: u64) -> theseus_protocol::PlaceInfo {
    let target = format!("discord:channel:{id}");
    let h = r.core.health().places.unwrap();
    h.places.into_iter().find(|p| p.place == target).unwrap()
}

/// Jev's live notice (step 24's notices, theseus-0j2.13): an open call
/// `security.v3` is 95% sure was risky gets a notice after it ran, in the
/// owner's DM alone, with right / wrong / noise. Ana's press of Noise in her
/// DM labels the whole judgment as hers, and the notice says so and loses
/// its buttons. A press from a shared channel (a forged copy of the buttons
/// in `#lab`, bound shared) is refused, and only the presser is told why.
#[tokio::test]
async fn a_jev_notice_goes_to_the_owners_dm_and_a_press_there_labels_it() {
    use theseus_judge::fake::{FakeJev, Scripted as Jev};
    let jev = FakeJev::start().unwrap();
    jev.script("risky", Jev::Noul(0.95));
    let base = jev.base();
    let model = |_: &Path, _| -> Arc<dyn Provider> {
        Arc::new(FakeProvider::scripted(vec![
            Scripted::tools(
                "",
                &[(
                    "r1",
                    "proc_run",
                    serde_json::json!({"argv": ["echo", "hi"]}),
                )],
            ),
            Scripted::text("Done."),
        ]))
    };
    let r = Rig::start_tweaked(model, guild(true), &bindings(false), &[], move |c| {
        c.judge.enabled = true;
        c.judge.api_base = base;
        c.judge.connect_secs = 1;
        c.judge.total_secs = 2;
        c.policy.tools.insert("proc.run".into(), Posture::Open);
    })
    .await;
    r.say((ANA, "ana"), None, "Run echo hi.");
    let is_notice = |m: &Msg| m.versions[0].contains("notified after it ran");
    r.until("the notice in ana's DM", || {
        r.posted(ANA_DM).iter().any(is_notice)
    })
    .await;
    let notice = r.posted(ANA_DM).into_iter().find(is_notice).unwrap();
    assert!(
        notice.content.contains("Jev: 95% risky"),
        "{}",
        notice.content
    );
    let labels: Vec<&str> = notice.buttons.iter().map(|b| b.label.as_str()).collect();
    assert_eq!(
        labels,
        ["Right: it was risky", "Wrong: it was fine", "Noise"]
    );
    let judgment = notice
        .buttons
        .last()
        .and_then(|b| b.custom_id.strip_prefix("jev:noise:"))
        .unwrap()
        .to_string();
    assert!(r.posted(LAB).iter().all(|m| !is_notice(m)), "never in #lab");

    // A forged copy of the buttons in #lab, a shared channel: refused.
    let forged_id = forge(&r, LAB, &judgment).await;
    let pressed = r.press(&forged_id, "Noise", (ANA, "ana"));
    r.until("ana is told no", || {
        r.fake.replies().iter().any(|x| {
            x.interaction.as_deref() == Some(pressed.as_str())
                && x.content
                    .as_deref()
                    .is_some_and(|c| c.starts_with("🔐 Your label did not count"))
        })
    })
    .await;
    assert!(r.ledger("judge.label").is_empty(), "nothing was written");

    // Ana's press in her DM counts, as hers.
    r.press(&notice.id, "Noise", (ANA, "ana"));
    r.until("the label", || !r.ledger("judge.label").is_empty())
        .await;
    let l = &r.ledger("judge.label")[0];
    assert_eq!(
        (
            l["judgment"].as_str(),
            l["label"].as_str(),
            l["via"].as_str()
        ),
        (Some(judgment.as_str()), Some("noise"), Some("discord:dm"))
    );
    assert!(l["who"].as_str().unwrap().contains(&ANA.to_string()), "{l}");
    r.until("the notice says it", || {
        r.posted(ANA_DM)
            .iter()
            .any(|m| m.id == notice.id && m.content.contains("labeled **noise**"))
    })
    .await;
    let edited = r
        .posted(ANA_DM)
        .into_iter()
        .find(|m| m.id == notice.id)
        .unwrap();
    assert!(
        edited.buttons.is_empty(),
        "its buttons are gone: {edited:?}"
    );
}

/// A message with a Jev notice's Noise button for `judgment`, posted in
/// `channel` as the bot's, as a forged or stale copy would be. Its id.
async fn forge(r: &Rig, channel: u64, judgment: &str) -> String {
    let forged = serde_json::json!({"content": "a forged notice", "components": [{"type": 1,
        "components": [{"type": 2, "style": 4, "label": "Noise",
                        "custom_id": format!("jev:noise:{judgment}")}]}]});
    let url = format!("http://{}/api/v10/channels/{channel}/messages", r.fake.addr);
    let posted: serde_json::Value = reqwest::Client::new()
        .post(url)
        .json(&forged)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    posted["id"].as_str().unwrap().to_string()
}
