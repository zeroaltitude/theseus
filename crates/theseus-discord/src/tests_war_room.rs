//! The war room (theseus-bh3t.4), end to end on the rig of
//! `tests_task_threads.rs`: a guest's comment in a private task's place
//! reaches no session and no model, the owner hears it in the channel, Details
//! lists it and the report counts it; the owner shares and unshares a guest
//! (a thread's member, a channel's overwrite) and nobody else can.

use std::sync::{Arc, Mutex};

use serde_json::Value;
use theseus_core::provider::{DeltaSink, Provider, ProviderFuture, ProviderRequest, Scripted};
use theseus_sim::fake_discord::commands::Sender;
use theseus_sim::fake_discord::{Guild, DEFAULT_GUILD};
use twilight_model::guild::Permissions;
use twilight_model::id::Id;

use crate::runtime::threads::Made;
use crate::runtime::war_room::Guest;
use crate::tests_gateway::{Rig, ANA, BEN, CY, LAB};
use crate::tests_task_threads::{
    answers, asks, bindings, bound, gate, held, model, open_gate, parent_session, rig, said, start,
    task_session, threads, tweak, until, ASK, REEF,
};

const DAN: u64 = 900_000_000_000_000_404;
const WAR: u64 = 900_000_000_000_000_050;

/// The provider under the rig's, noting every request it is asked.
struct Spy {
    inner: Arc<dyn Provider>,
    asked: Arc<Mutex<Vec<String>>>,
}

impl Provider for Spy {
    fn name(&self) -> &str {
        "fake"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        let all = Value::Array(req.messages.clone()).to_string();
        self.asked.lock().unwrap().push(all);
        self.inner.stream_message(req, on_delta)
    }
}

fn from(user: u64, name: &'static str) -> Sender<'static> {
    Sender {
        user,
        name,
        channel: Some(LAB),
    }
}

/// `#lab`, with dan who cannot see it.
fn guild_with_dan() -> Guild {
    Guild::new(DEFAULT_GUILD, (ANA, "ana"))
        .member(BEN, "ben")
        .member(CY, "cy")
        .member(DAN, "dan")
        .private_channel(LAB, "lab", &[ANA, BEN, CY])
        .private_channel(WAR, "war", &[ANA])
}

/// The id of the one task.
fn task_id(r: &Rig) -> String {
    r.core.kernel.tasks(None).unwrap()[0].id.to_string()
}

/// The text a command answered, once it has: a view's, or a plain line.
async fn answered(r: &Rig, iid: &str) -> String {
    let t0 = std::time::Instant::now();
    loop {
        if let Some(m) = r.fake.reply_message(iid) {
            return m.content;
        }
        let line = r.fake.replies().into_iter().find_map(|x| {
            (x.interaction.as_deref() == Some(iid))
                .then_some(x.content)
                .flatten()
        });
        if let Some(l) = line {
            return l;
        }
        assert!(t0.elapsed().as_secs() < 20, "no answer to {iid}");
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
}

/// A running task in a private channel's thread, with the spy over its model.
async fn running(
    extra: impl FnOnce(&std::path::Path) -> Vec<Scripted>,
) -> (Rig, u64, Arc<Mutex<Vec<String>>>) {
    let asked = Arc::new(Mutex::new(Vec::new()));
    let spy = asked.clone();
    let r = Rig::start_tweaked(
        |dir, _| {
            Arc::new(Spy {
                inner: model(
                    vec![
                        start(&[REEF]),
                        Scripted::text("Started."),
                        Scripted::text("Noted."),
                    ],
                    vec![(REEF, [vec![gate(dir)], extra(dir)].concat())],
                ),
                asked: spy,
            })
        },
        guild_with_dan(),
        &bindings(true, ""),
        &[],
        tweak,
    )
    .await;
    r.say((ANA, "ana"), Some(LAB), ASK);
    until(&r, "a thread for the task", || !threads(&r).is_empty()).await;
    let t = threads(&r).remove(0);
    bound(&r, t.id).await;
    (r, t.id, asked)
}

/// A guest's comments, while the task runs: no session holds them, no model
/// is asked with them, the owner hears each in the channel, the thread says
/// once that the task does not read them, Details lists them, and the report
/// counts them.
#[tokio::test]
async fn a_guests_comment_reaches_no_session_and_no_model_and_the_owner_hears_it() {
    const ONE: &str = "cy wonders about the tide tables";
    const TWO: &str = "cy adds that the north buoy drifts";
    let (r, thread, asked) = running(|_| answers("Reef charted.")).await;
    r.say((CY, "cy"), Some(thread), ONE);
    r.say((CY, "cy"), Some(thread), TWO);
    until(&r, "both comments relayed in the channel", || {
        let lab = r.posted(LAB);
        let line = |text: &str| {
            lab.iter().any(|m| {
                m.content.starts_with("-# 💬 cy on “")
                    && m.content.contains(text)
                    && m.content.contains("discord.com/channels/")
            })
        };
        line(ONE) && line(TWO)
    })
    .await;
    assert!(
        r.posted(LAB)
            .iter()
            .all(|m| !m.content.contains("💬") || m.silent()),
        "a relay never pings"
    );
    let said_once = said(&r, thread)
        .matches("The task doesn't read guests' messages")
        .count();
    assert_eq!(
        said_once,
        1,
        "the thread says it once: {}",
        said(&r, thread)
    );
    let (task, parent) = (task_session(&r), parent_session(&r));
    // Details lists them, newest first.
    let id = task_id(&r);
    let iid = r
        .fake
        .slash(&from(ANA, "ana"), "task", &[("name", &id[id.len() - 6..])])
        .unwrap();
    let details = answered(&r, &iid).await;
    assert!(details.contains("Comments** (2)"), "{details}");
    assert!(details.contains(TWO) && details.contains(ONE), "{details}");
    open_gate(&r);
    until(&r, "the report in the channel", || {
        r.posted(LAB)
            .iter()
            .any(|m| m.content.contains("finished") && m.content.contains("2 guest comments"))
    })
    .await;
    for sid in [&task, &parent] {
        assert!(
            !held(&r, sid)
                .iter()
                .any(|b| b.contains(ONE) || b.contains(TWO)),
            "a comment became a node in {sid}"
        );
    }
    assert_eq!(r.core.runner.tools.job_waits.steers.joined(&task), 0);
    let asked = asked.lock().unwrap();
    assert!(!asked.is_empty(), "the model was asked something");
    assert!(
        asked
            .iter()
            .all(|a| !a.contains("tide tables") && !a.contains("north buoy")),
        "a comment reached a model request"
    );
    let rows = r.ledger("discord.message.in");
    let rows: Vec<&Value> = rows.iter().filter(|d| d["comment"] == true).collect();
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert!(
        rows.iter().all(|d| !d.to_string().contains("tide")),
        "a row has no text"
    );
    let mut chars: Vec<u64> = rows.iter().filter_map(|d| d["chars"].as_u64()).collect();
    chars.sort_unstable();
    assert_eq!(
        chars,
        vec![ONE.chars().count() as u64, TWO.chars().count() as u64]
    );
}

/// The same while the task waits on a card, where the kernel's own rule
/// would have taken a guest's message: still a comment.
#[tokio::test]
async fn a_comment_on_a_waiting_task_is_not_taken_by_it() {
    const WAIT: &str = "cy asks whether the reef file is final";
    let (r, thread, _) = running(|dir| asks(dir, "reef.txt")).await;
    open_gate(&r);
    until(&r, "the card in the thread", || {
        r.posted(thread)
            .iter()
            .any(|m| m.buttons.iter().any(|b| b.custom_id.starts_with("wa:")))
    })
    .await;
    r.say((CY, "cy"), Some(thread), WAIT);
    until(&r, "the comment relayed", || {
        r.posted(LAB).iter().any(|m| m.content.contains(WAIT))
    })
    .await;
    // The owner's own message still steers the waiting task.
    r.say((ANA, "ana"), Some(thread), "use the north jetty instead");
    let task = task_session(&r);
    until(&r, "the owner's message reached the task", || {
        held(&r, &task)
            .iter()
            .any(|b| b.contains("use the north jetty instead"))
    })
    .await;
    assert!(!held(&r, &task).iter().any(|b| b.contains(WAIT)));
    assert!(!held(&r, &parent_session(&r))
        .iter()
        .any(|b| b.contains(WAIT)));
}

/// Share and unshare on a task thread: owner only, the guest must see the
/// parent, one option at a time.
#[tokio::test]
async fn sharing_a_thread_adds_a_member_the_owner_alone_and_unshare_removes_them() {
    let (r, thread, _) = running(|_| answers("Reef charted.")).await;
    let id = task_id(&r);
    let name = id[id.len() - 6..].to_string();
    let ask = |who: Sender<'static>, users: &[(&str, u64)]| {
        r.fake
            .slash_with(&who, "task", &[("name", &name)], users)
            .unwrap()
    };
    let members = || r.fake.thread_members(thread);
    // A guest cannot share.
    let iid = ask(from(CY, "cy"), &[("share", BEN)]);
    assert!(answered(&r, &iid).await.contains("Only the owner"));
    assert!(members().is_empty());
    // dan cannot see #lab, so he cannot see its threads.
    let iid = ask(from(ANA, "ana"), &[("share", DAN)]);
    assert!(answered(&r, &iid).await.contains("can't see"));
    assert!(members().is_empty());
    // One verb at a time.
    let iid = ask(from(ANA, "ana"), &[("share", BEN), ("unshare", BEN)]);
    assert!(answered(&r, &iid).await.contains("not both"));
    assert!(members().is_empty());
    // ben can see #lab.
    let iid = ask(from(ANA, "ana"), &[("share", BEN)]);
    assert!(answered(&r, &iid).await.contains("can watch and comment"));
    assert_eq!(members(), vec![BEN]);
    let guests: Vec<u64> = r
        .core
        .store
        .get_meta(&format!("discord.task_guests.{id}"))
        .unwrap()
        .unwrap();
    assert_eq!(guests, vec![BEN]);
    let iid = ask(from(ANA, "ana"), &[("unshare", BEN)]);
    assert!(answered(&r, &iid)
        .await
        .contains("stays visible to everyone who sees its channel"));
    assert!(members().is_empty());
    open_gate(&r);
}

/// A task with a channel of its own gets the guest as a member overwrite,
/// and unshare takes it away.
#[tokio::test]
async fn sharing_a_channel_sets_a_member_overwrite_and_unshare_deletes_it() {
    let r = Rig::start_tweaked(
        |_, _| model(vec![Scripted::text("Hello.")], vec![]),
        guild_with_dan(),
        &bindings(true, ""),
        &[],
        tweak,
    )
    .await;
    let shared = crate::runtime::shared_for_tests(&r.core);
    let made = Made {
        task: "exe_reef".into(),
        session: "ses_reef".into(),
        thread: WAR,
        parent: LAB,
        parent_session: "ses_parent".into(),
        name: "reef".into(),
        guild: Some(DEFAULT_GUILD),
        private: true,
        ceiling: None,
        created_ms: 1,
        ended_ms: None,
    };
    let view = Permissions::VIEW_CHANNEL.bits();
    let theirs = |r: &Rig| r.fake.overwrites_of(WAR).into_iter().find(|o| o.id == DAN);
    assert!(theirs(&r).is_none());
    shared
        .set_guest(&made, Guest::Add(Id::new(DAN)))
        .await
        .unwrap();
    let o = theirs(&r).expect("dan has an overwrite");
    assert_eq!(o.kind, 1, "a member's");
    assert_eq!(o.allow & view, view);
    assert_eq!(o.deny, 0);
    shared
        .set_guest(&made, Guest::Remove(Id::new(DAN)))
        .await
        .unwrap();
    assert!(theirs(&r).is_none());
}

/// A shared place is not narrowed to guests, and its thread keeps the guest
/// rule: a guest's message goes to the conversation that started the task.
#[tokio::test]
async fn a_shared_place_refuses_a_share_and_keeps_its_guests_rule() {
    let r = rig(
        vec![
            start(&[REEF]),
            Scripted::text("Started."),
            Scripted::text("Noted."),
        ],
        |_| vec![(REEF, answers("Reef charted."))],
        false,
    )
    .await;
    r.say((ANA, "ana"), Some(LAB), ASK);
    until(&r, "a thread for the task", || !threads(&r).is_empty()).await;
    let t = threads(&r).remove(0);
    until(&r, "the thread is renamed at the task's end", || {
        threads(&r)[0].names.len() == 2
    })
    .await;
    let id = task_id(&r);
    let iid = r
        .fake
        .slash_with(
            &from(ANA, "ana"),
            "task",
            &[("name", &id[id.len() - 6..])],
            &[("share", CY)],
        )
        .unwrap();
    assert!(answered(&r, &iid).await.contains("private place"));
    assert!(r.fake.thread_members(t.id).is_empty());
    let parent = parent_session(&r);
    r.say((CY, "cy"), Some(t.id), "cy wonders about the reef");
    until(&r, "the conversation heard the guest", || {
        held(&r, &parent)
            .iter()
            .any(|b| b.contains("cy wonders about the reef") && b.contains("[Written to the task"))
    })
    .await;
    assert!(r.posted(LAB).iter().all(|m| !m.content.contains("💬")));
}
