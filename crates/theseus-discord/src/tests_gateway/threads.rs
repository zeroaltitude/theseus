//! Threads on Discord (theseus-sjzm), end to end through the stand-in: a
//! channel bound as a thread (and a shared one refused), the DM's tags and
//! pointers and the reply that addresses a thread, one Discord thread per
//! activity thread, a persona's signed reply (a webhook, else a bold name),
//! and no change to how chat messages ping.

use std::sync::Arc;

use theseus_core::approval::{Answerer, Surface};
use theseus_core::provider::{FakeProvider, Scripted};
use theseus_core::session::SessionRecord;
use theseus_core::Config;
use theseus_protocol::threads::{ThreadEditParams, ThreadNewParams};
use theseus_sim::fake_discord::{Command, Guild, Msg, Typed, BOT_ID, DEFAULT_GUILD};

use super::{Rig, ANA, ANA_DM, BEN, LAB};

const TOWN: u64 = 900_000_000_000_000_020;
const DESK: u64 = 900_000_000_000_000_030;
const PIT: u64 = 900_000_000_000_000_777;

fn bindings(mention_only: bool) -> String {
    format!(
        "guild_id = \"{DEFAULT_GUILD}\"\n\
         [[channel]]\nid = \"{LAB}\"\nname = \"lab\"\nusers = [\"{ANA}\"]\nmention_only = {mention_only}\nprivate = true\n\
         [[channel]]\nid = \"{TOWN}\"\nname = \"town\"\nusers = [\"{ANA}\"]\nmention_only = false\nprivate = false\n\
         [[dm]]\nuser = \"{ANA}\"\nname = \"ana\"\n"
    )
}

fn guild() -> Guild {
    Guild::new(DEFAULT_GUILD, (ANA, "ana"))
        .member(BEN, "ben")
        .private_channel(LAB, "lab", &[ANA])
        .channel(TOWN, "town")
        .channel(DESK, "desk")
}

async fn rig_with(texts: &[&str], mention_only: bool, tweak: impl FnOnce(&mut Config)) -> Rig {
    let script: Vec<Scripted> = texts.iter().map(|t| Scripted::text(t)).collect();
    let model = move |_: &std::path::Path, _| -> Arc<dyn theseus_core::provider::Provider> {
        Arc::new(FakeProvider::scripted(script))
    };
    Rig::start_tweaked(
        model,
        guild(),
        &bindings(mention_only),
        &[format!("channel:{TOWN}")],
        tweak,
    )
    .await
}

async fn rig(texts: &[&str]) -> Rig {
    rig_with(texts, false, |_| {}).await
}

fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    }
}

fn make(r: &Rig, slug: &str) {
    r.core
        .thread_new(
            &ThreadNewParams {
                slug: slug.into(),
                title: Some(format!("Title of {slug}")),
                ..Default::default()
            },
            cli(),
        )
        .unwrap();
}

fn surfaces(r: &Rig, slug: &str) -> Vec<String> {
    r.core.threads().get(slug).unwrap().surfaces.clone()
}

fn session_of(r: &Rig, slug: &str) -> Option<String> {
    r.core.threads().get(slug).unwrap().session_id.clone()
}

fn turns(r: &Rig, sid: &str) -> u32 {
    r.core
        .store
        .get_session::<SessionRecord>(sid)
        .unwrap()
        .unwrap()
        .turns as u32
}

/// Wait for the session's books to show `n` turns. A turn's reply is posted
/// to the place before the turn ends, and the turn counts itself into the
/// session record in its last frame, so a reply seen in Discord does not yet
/// mean a turn booked.
async fn booked(r: &Rig, sid: &str, n: u32) {
    r.until("the turn booked in its session", || turns(r, sid) == n)
        .await;
}

/// `/thread <sub>` typed by `user` in `channel` (a Discord thread under
/// `parent` when that is given), and the words the binding answered it with.
async fn slash(
    r: &Rig,
    user: (u64, &str),
    place: (u64, Option<u64>),
    sub: &str,
    options: &[(&str, &str)],
) -> String {
    let id = r
        .fake
        .command(&Command {
            user: user.0,
            name: user.1,
            channel: place.0,
            parent: place.1,
            command: "thread",
            sub,
            options,
        })
        .unwrap();
    let answer = |r: &Rig| {
        r.fake
            .replies()
            .into_iter()
            .find(|x| x.interaction.as_deref() == Some(id.as_str()))
            .and_then(|x| x.content)
    };
    r.until("the command's answer", || answer(r).is_some())
        .await;
    answer(r).unwrap()
}

/// The bot's messages in `channel`.
fn bot(r: &Rig, channel: u64) -> Vec<Msg> {
    r.posted(channel)
}

/// A message of the bot's in `channel` holding `needle`.
fn holding(r: &Rig, channel: u64, needle: &str) -> Option<Msg> {
    bot(r, channel)
        .into_iter()
        .find(|m| m.content.contains(needle))
}

fn dm(r: &Rig, content: &str) -> String {
    r.say((ANA, "ana"), None, content)
}

fn dm_reply(r: &Rig, content: &str, to: &str) -> String {
    r.fake
        .say_reply(
            &Typed {
                user: ANA,
                name: "ana",
                channel: None,
                content,
                file: None,
            },
            to,
        )
        .unwrap()
}

/// A private channel binds as a thread and runs it; a shared one refuses;
/// a message runs in the thread's session, whose posts come back to the
/// channel; a paused thread says so; and `unbind` gives the channel its own
/// session again, leaving the thread's untouched.
#[tokio::test]
async fn a_private_channel_runs_a_thread_and_a_shared_one_refuses() {
    let r = rig(&["tides are in", "a plain answer"]).await;
    make(&r, "tides");

    let said = slash(&r, (ANA, "ana"), (TOWN, None), "bind", &[("slug", "tides")]).await;
    assert!(said.contains("shared"), "{said}");
    assert!(
        surfaces(&r, "tides").is_empty(),
        "a shared channel never binds"
    );

    let said = slash(&r, (ANA, "ana"), (LAB, None), "bind", &[("slug", "tides")]).await;
    assert!(said.contains("runs the thread `tides`"), "{said}");
    let lab = format!("discord:channel:{LAB}");
    assert_eq!(surfaces(&r, "tides"), std::slice::from_ref(&lab));
    let edited = r.ledger("thread.edited");
    assert_eq!(edited.len(), 1);
    assert_eq!(edited[0]["changed"], serde_json::json!(["surfaces"]));
    assert_eq!(edited[0]["via"], format!("discord:{LAB}"));

    // A message runs in the thread's session, and its reply comes back here,
    // untagged (the place says it) and pinging as every chat reply does.
    let typed = r.say((ANA, "ana"), Some(LAB), "what do the gauges say");
    r.until("the reply in #lab", || {
        holding(&r, LAB, "tides are in").is_some()
    })
    .await;
    let reply = holding(&r, LAB, "tides are in").unwrap();
    assert_eq!(reply.reply_to.as_deref(), Some(typed.as_str()));
    assert!(!reply.content.contains("-# →"), "{}", reply.content);
    assert!(!reply.silent(), "chat messages ping as ever: {reply:?}");
    let sid = session_of(&r, "tides").expect("the thread's session");
    booked(&r, &sid, 1).await;
    assert_eq!(turns(&r, &sid), 1);
    assert_eq!(r.core.outbox.target(&sid).as_deref(), Some(lab.as_str()));

    // A paused thread takes no message, and the place says how to change that.
    r.core.thread_pause("tides", cli()).unwrap();
    r.say((ANA, "ana"), Some(LAB), "still there?");
    r.until("the pause said", || {
        holding(&r, LAB, "thread resume").is_some()
    })
    .await;
    assert_eq!(turns(&r, &sid), 1);
    r.core.thread_resume("tides", cli()).unwrap();

    // Unbound, the channel has a session of its own, and the thread's rests.
    let said = slash(&r, (ANA, "ana"), (LAB, None), "unbind", &[]).await;
    assert!(said.contains("no longer runs"), "{said}");
    assert!(surfaces(&r, "tides").is_empty());
    r.say((ANA, "ana"), Some(LAB), "and now?");
    r.until("a plain answer", || {
        holding(&r, LAB, "a plain answer").is_some()
    })
    .await;
    assert_eq!(turns(&r, &sid), 1, "the thread's session took no more");
    assert!(r.core.outbox.target(&sid).is_none());
}

/// A Discord thread under a private channel binds too; one under a shared
/// channel does not.
#[tokio::test]
async fn a_discord_thread_under_a_private_channel_binds_and_one_under_a_shared_one_refuses() {
    let r = rig(&["from the pit"]).await;
    make(&r, "tides");
    let said = slash(
        &r,
        (ANA, "ana"),
        (PIT, Some(TOWN)),
        "bind",
        &[("slug", "tides")],
    )
    .await;
    assert!(
        said.contains("neither") || said.contains("private"),
        "{said}"
    );
    assert!(surfaces(&r, "tides").is_empty());
    let said = slash(
        &r,
        (ANA, "ana"),
        (PIT, Some(LAB)),
        "bind",
        &[("slug", "tides")],
    )
    .await;
    assert!(said.contains("runs the thread"), "{said}");
    assert_eq!(surfaces(&r, "tides"), [format!("discord:channel:{PIT}")]);
    r.say((ANA, "ana"), Some(PIT), "hello from the pit");
    r.until("the reply in the Discord thread", || {
        holding(&r, PIT, "from the pit").is_some()
    })
    .await;
    let sid = session_of(&r, "tides").unwrap();
    booked(&r, &sid, 1).await;
    assert_eq!(turns(&r, &sid), 1);
}

/// The owner's DM addresses a thread by `thread:<slug>` or by replying to a
/// message that carries its tag: the reply comes back in the DM, tagged, the
/// thread's own place gets a quiet pointer with a link, and the DM's own
/// session is left alone. A message spoken in the thread's own place is
/// pointed at nothing.
#[tokio::test]
async fn the_dm_tags_a_threads_reply_and_points_at_it_from_the_thread() {
    let r = rig(&["tide one", "tide two", "dm plain", "lab one"]).await;
    make(&r, "tides");
    slash(&r, (ANA, "ana"), (LAB, None), "bind", &[("slug", "tides")]).await;

    let said = dm(&r, "thread:tides what now");
    r.until("the tagged reply in the DM", || {
        holding(&r, ANA_DM, "tide one").is_some()
    })
    .await;
    let reply = holding(&r, ANA_DM, "tide one").unwrap();
    assert!(
        reply.content.starts_with("-# → Title of tides\ntide one"),
        "{}",
        reply.content
    );
    assert_eq!(reply.reply_to.as_deref(), Some(said.as_str()));
    assert!(!reply.silent(), "the answer still pings");
    // The pointer in the thread's place: a quiet line and the link.
    r.until("the pointer in #lab", || {
        holding(&r, LAB, "-# from the DM: what now").is_some()
    })
    .await;
    let pointer = holding(&r, LAB, "-# from the DM: what now").unwrap();
    assert!(
        pointer
            .content
            .contains(&format!("https://discord.com/channels/@me/{ANA_DM}/{said}")),
        "{}",
        pointer.content
    );
    assert!(pointer.silent(), "a pointer is history, not news");
    let sid = session_of(&r, "tides").unwrap();
    booked(&r, &sid, 1).await;
    assert_eq!(turns(&r, &sid), 1);
    assert!(
        r.core
            .outbox
            .place_session(&format!("dm:{ANA}"))
            .unwrap()
            .is_none(),
        "the DM's own session was not opened"
    );

    // A reply to the tagged message addresses the thread again.
    let again = dm_reply(&r, "and again", &reply.id);
    r.until("the second tagged reply", || {
        holding(&r, ANA_DM, "tide two").is_some()
    })
    .await;
    let second = holding(&r, ANA_DM, "tide two").unwrap();
    assert!(second.content.starts_with("-# → Title of tides\n"));
    assert_eq!(second.reply_to.as_deref(), Some(again.as_str()));
    booked(&r, &sid, 2).await;
    assert_eq!(turns(&r, &sid), 2);
    r.until("the second pointer", || {
        holding(&r, LAB, "-# from the DM: and again").is_some()
    })
    .await;

    // A plain DM message is the DM's own turn: no tag, no pointer.
    dm(&r, "just chatting");
    r.until("a plain DM reply", || {
        holding(&r, ANA_DM, "dm plain").is_some()
    })
    .await;
    let plain = holding(&r, ANA_DM, "dm plain").unwrap();
    assert!(!plain.content.contains("-# →"), "{}", plain.content);
    assert!(holding(&r, LAB, "just chatting").is_none());
    assert_eq!(turns(&r, &sid), 2);

    // Spoken in the thread's own place, `thread:tides` is only words: the
    // turn runs there and nothing points at it.
    r.say((ANA, "ana"), Some(LAB), "thread:tides in the lab");
    r.until("the lab's reply", || holding(&r, LAB, "lab one").is_some())
        .await;
    assert!(holding(&r, LAB, "from the DM: in the lab").is_none());
    assert!(!holding(&r, LAB, "lab one")
        .unwrap()
        .content
        .contains("-# →"));
}

/// With `[threads] discord_channel`, a thread's first message makes it a
/// Discord thread named by its title, recorded in its surfaces; pause and
/// done archive it, resume and reopen unarchive it, an edit renames it and a
/// seal puts 🔒 first; a message in it runs the thread; the DM's pointer waits
/// for it and lands there; and nothing about the pings changes.
#[tokio::test]
async fn a_thread_gets_a_discord_thread_on_its_first_message() {
    let r = rig_with(&["from the dm", "in the thread"], false, |c| {
        c.threads.discord_channel = Some(DESK.to_string())
    })
    .await;
    make(&r, "tides");
    assert!(
        r.fake.threads().is_empty(),
        "nothing before a first message"
    );

    dm(&r, "thread:tides hello there");
    r.until("the Discord thread", || !r.fake.threads().is_empty())
        .await;
    let t = r.fake.threads()[0].clone();
    assert_eq!(
        (t.name.as_str(), t.parent, t.archived),
        ("Title of tides", DESK, false)
    );
    r.until("its surface recorded", || !surfaces(&r, "tides").is_empty())
        .await;
    assert_eq!(surfaces(&r, "tides"), [format!("discord:channel:{}", t.id)]);
    // The DM's reply is in the DM; the thread has the pointer, not the reply.
    r.until("the pointer in the thread", || {
        holding(&r, t.id, "-# from the DM: hello there").is_some()
    })
    .await;
    r.until("the reply in the DM", || {
        holding(&r, ANA_DM, "from the dm").is_some()
    })
    .await;
    assert!(holding(&r, t.id, "from the dm").is_none());

    // A message in the Discord thread addresses the thread; its reply is here.
    r.say((ANA, "ana"), Some(t.id), "now in the thread");
    r.until("the reply in the thread", || {
        holding(&r, t.id, "in the thread").is_some()
    })
    .await;
    let reply = holding(&r, t.id, "in the thread").unwrap();
    assert!(
        !reply.silent() && !reply.content.contains("-# →"),
        "{reply:?}"
    );
    let sid = session_of(&r, "tides").unwrap();
    assert_eq!(turns(&r, &sid), 2);
    assert_eq!(
        r.core.outbox.target(&sid).as_deref(),
        Some(format!("discord:channel:{}", t.id).as_str())
    );

    // Pause and done archive it; resume and reopen unarchive it.
    let archived = |r: &Rig| r.fake.threads()[0].archived;
    r.core.thread_pause("tides", cli()).unwrap();
    r.until("archived by a pause", || archived(&r)).await;
    r.core.thread_resume("tides", cli()).unwrap();
    r.until("unarchived by a resume", || !archived(&r)).await;
    r.core.thread_done("tides", cli()).unwrap();
    r.until("archived by done", || archived(&r)).await;
    r.core.thread_reopen("tides", cli()).unwrap();
    r.until("unarchived by a reopen", || !archived(&r)).await;

    // A new title renames it; a sealed thread's name starts with a lock.
    let retitle = |title: &str| {
        r.core
            .thread_edit(
                &ThreadEditParams {
                    slug: "tides".into(),
                    title: Some(title.into()),
                    ..Default::default()
                },
                cli(),
            )
            .unwrap();
    };
    retitle("Gauge four");
    r.until("renamed", || r.fake.threads()[0].name == "Gauge four")
        .await;
    r.core
        .runner
        .threads
        .write(
            &r.core.store,
            |b| {
                let mut t = b.get("tides").unwrap().clone();
                t.sealed = Some(theseus_protocol::threads::Seal {
                    by: "the owner".into(),
                    at_ms: 1,
                    reason: "client work".into(),
                });
                Ok(vec![t])
            },
            |_, _| Ok(vec![]),
        )
        .unwrap();
    retitle("Gauge five");
    r.until("sealed", || r.fake.threads()[0].name == "🔒 Gauge five")
        .await;
}

/// In a forum a thread is a post, with its first message.
#[tokio::test]
async fn in_a_forum_the_discord_thread_is_a_post_with_a_first_message() {
    let r = rig_with(&["x"], false, |c| {
        c.threads.discord_channel = Some(DESK.to_string())
    })
    .await;
    r.fake.set_forum(DESK);
    make(&r, "tides");
    r.core.thread_session("tides", cli()).unwrap();
    r.until("the forum post", || !r.fake.threads().is_empty())
        .await;
    let t = r.fake.threads()[0].clone();
    assert!(t.forum);
    let first = r.fake.messages(t.id);
    assert!(first[0].content.contains("thread:tides"), "{first:?}");
}

/// A thread with no `discord_channel` gets none; a thread already bound to a
/// channel gets none either.
#[tokio::test]
async fn no_discord_thread_is_made_without_the_setting_or_for_a_bound_thread() {
    let r = rig(&["x"]).await;
    make(&r, "tides");
    r.core.thread_session("tides", cli()).unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(r.fake.threads().is_empty());
    drop(r);
    let r = rig_with(&["x"], false, |c| {
        c.threads.discord_channel = Some(DESK.to_string())
    })
    .await;
    make(&r, "tides");
    slash(&r, (ANA, "ana"), (LAB, None), "bind", &[("slug", "tides")]).await;
    r.core.thread_session("tides", cli()).unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(r.fake.threads().is_empty(), "the channel is its place");
}

/// A thread with a persona is signed in its place: through a webhook sent
/// under the persona's name where the bot may manage webhooks, else a bold
/// name; and a reply to a webhook's message is still a reply to Theseus.
#[tokio::test]
async fn a_personas_reply_is_signed_by_a_webhook_or_else_a_bold_name() {
    let r = rig_with(&["the tide is out", "and in", "bold reply"], true, |_| {}).await;
    make(&r, "tides");
    r.core
        .thread_edit(
            &ThreadEditParams {
                slug: "tides".into(),
                persona: Some("Ada".into()),
                ..Default::default()
            },
            cli(),
        )
        .unwrap();
    slash(&r, (ANA, "ana"), (LAB, None), "bind", &[("slug", "tides")]).await;
    let mention = format!("<@{BOT_ID}>");
    let hooked = |r: &Rig| -> Option<Msg> {
        r.fake
            .messages(LAB)
            .into_iter()
            .find(|m| m.hook.is_some() && m.content.contains("the tide is out"))
    };
    r.say(
        (ANA, "ana"),
        Some(LAB),
        &format!("{mention} how is the tide"),
    );
    r.until("the webhook's message", || hooked(&r).is_some())
        .await;
    let m = hooked(&r).unwrap();
    assert_eq!(m.hook.as_deref(), Some("Ada"));
    assert!(
        !m.content.starts_with("**Ada**"),
        "a webhook needs no bold name: {}",
        m.content
    );
    assert_eq!(r.fake.webhooks().len(), 1);
    assert_ne!(m.author, BOT_ID.to_string());
    assert!(
        holding(&r, LAB, "the tide is out").is_none(),
        "the bot wrote none of it"
    );

    // Replying to the webhook's message, with no mention, is answered.
    r.fake
        .say_reply(
            &Typed {
                user: ANA,
                name: "ana",
                channel: Some(LAB),
                content: "and then?",
                file: None,
            },
            &m.id,
        )
        .unwrap();
    r.until("the answer to the reply", || {
        r.fake
            .messages(LAB)
            .iter()
            .any(|m| m.hook.is_some() && m.content.contains("and in"))
    })
    .await;
}

#[tokio::test]
async fn without_manage_webhooks_a_personas_reply_starts_with_a_bold_name() {
    let r = rig(&["bold reply"]).await;
    r.fake.refuse_webhooks(true);
    make(&r, "tides");
    r.core
        .thread_edit(
            &ThreadEditParams {
                slug: "tides".into(),
                persona: Some("Ada".into()),
                ..Default::default()
            },
            cli(),
        )
        .unwrap();
    slash(&r, (ANA, "ana"), (LAB, None), "bind", &[("slug", "tides")]).await;
    r.say((ANA, "ana"), Some(LAB), "how is the tide");
    r.until("the bold reply", || {
        holding(&r, LAB, "**Ada**: bold reply").is_some()
    })
    .await;
    assert!(r.fake.webhooks().is_empty());
    assert!(r.fake.messages(LAB).iter().all(|m| m.hook.is_none()));
}

/// A persona's reply in the DM is signed with the bold name under the tag.
#[tokio::test]
async fn a_personas_reply_in_the_dm_has_the_tag_then_the_bold_name() {
    let r = rig(&["reply from the dm"]).await;
    make(&r, "tides");
    r.core
        .thread_edit(
            &ThreadEditParams {
                slug: "tides".into(),
                persona: Some("Ada".into()),
                ..Default::default()
            },
            cli(),
        )
        .unwrap();
    dm(&r, "thread:tides who are you");
    r.until("the reply", || {
        holding(&r, ANA_DM, "reply from the dm").is_some()
    })
    .await;
    let m = holding(&r, ANA_DM, "reply from the dm").unwrap();
    assert!(
        m.content
            .starts_with("-# → Title of tides\n**Ada**: reply from the dm"),
        "{}",
        m.content
    );
}

/// `[[discord.thread]]` binds a channel the bindings file names, as soon as
/// its thread exists; a channel that is shared, or not named, is said on the
/// board and bound to nothing.
#[tokio::test]
async fn the_config_binds_a_channel_when_its_thread_exists() {
    let r = rig_with(&["via the config"], false, |c| {
        c.discord.thread = vec![
            theseus_core::config::discord::ThreadBind {
                channel: LAB.to_string(),
                thread: "tides".into(),
            },
            theseus_core::config::discord::ThreadBind {
                channel: TOWN.to_string(),
                thread: "moons".into(),
            },
        ]
    })
    .await;
    make(&r, "tides");
    make(&r, "moons");
    r.until("tides bound", || !surfaces(&r, "tides").is_empty())
        .await;
    assert_eq!(surfaces(&r, "tides"), [format!("discord:channel:{LAB}")]);
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(
        surfaces(&r, "moons").is_empty(),
        "a shared channel never binds"
    );
    r.say((ANA, "ana"), Some(LAB), "hello");
    r.until("the reply", || holding(&r, LAB, "via the config").is_some())
        .await;
    let sid = session_of(&r, "tides").unwrap();
    booked(&r, &sid, 1).await;
    assert_eq!(turns(&r, &sid), 1);
}

/// A thread's tasks post their board where the thread is shown: the task's
/// home is the thread's session, whose place is the Discord thread.
#[tokio::test]
async fn a_threads_task_board_lands_in_its_discord_thread() {
    use theseus_core::task_graph::{record, NewTask, TaskOrigin, TaskState};
    let r = rig_with(&["noted"], false, |c| {
        c.threads.discord_channel = Some(DESK.to_string());
        // The text board this test reads (discord-board's V2 board is the default).
        c.discord.live_board = false;
    })
    .await;
    make(&r, "tides");
    dm(&r, "thread:tides begin");
    r.until("the Discord thread", || {
        !surfaces(&r, "tides").is_empty() && !r.fake.threads().is_empty()
    })
    .await;
    let t = r.fake.threads()[0].clone();
    let sid = session_of(&r, "tides").unwrap();
    let task = NewTask {
        id: "tsk_00000000000000000000000000lagoon".into(),
        title: "Chart the lagoon",
        objective: "chart the lagoon".into(),
        acceptance: vec![],
        parent: None,
        deps: vec![],
        session: None,
        origin: TaskOrigin {
            session: sid,
            principal: "operator".into(),
            by_model: true,
            ..Default::default()
        },
        state: TaskState::Accepted,
    }
    .build(theseus_protocol::now_unix_ms());
    r.core.store.append(&[record(&task).unwrap()]).unwrap();
    let changed = theseus_protocol::Event::TaskChanged(theseus_protocol::tasks::TaskChanged {
        session_id: "ses_0000000000000000000000terminal".into(),
        verb: "created".into(),
        task,
        turn_id: None,
    });
    r.until("the binding hears every session", || {
        r.core.bus.watching_all() > 0
    })
    .await;
    r.core.bus.publish(
        "ses_0000000000000000000000terminal",
        &theseus_protocol::Message::from(changed),
        None,
    );
    r.until("the board in the Discord thread", || {
        holding(&r, t.id, "Chart the lagoon").is_some()
    })
    .await;
    assert!(holding(&r, ANA_DM, "Chart the lagoon").is_none());
}

/// A paused thread addressed from the DM is not silently a plain message:
/// the DM is told how to resume it, and the thread's session runs nothing.
#[tokio::test]
async fn a_paused_thread_addressed_from_the_dm_says_how_to_resume_it() {
    let r = rig(&["never said"]).await;
    make(&r, "tides");
    r.core.thread_pause("tides", cli()).unwrap();
    dm(&r, "thread:tides are you there");
    r.until("the refusal in the DM", || {
        holding(&r, ANA_DM, "thread resume").is_some()
    })
    .await;
    assert!(session_of(&r, "tides").is_none());
    assert!(holding(&r, ANA_DM, "never said").is_none());
    // A name that is no thread is only words.
    dm(&r, "thread:nothing at all");
    r.until("a plain reply", || {
        holding(&r, ANA_DM, "never said").is_some()
    })
    .await;
}
