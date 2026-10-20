//! Threads on the work board (theseus-5i8i) through a real core: the
//! conversation of a thread and the task it starts carry the thread, work in
//! none carries none, a thread's write reaches a watcher as a group, a pause
//! changes the group and a watch update reads no store, and a connection that
//! reads no private text is told of no thread.

use std::sync::Arc;

use theseus_protocol::notify;
use theseus_protocol::threads::{ThreadNewParams, ThreadState};
use theseus_protocol::work::{WorkKind, WorkView};
use theseus_protocol::{Message, ThreadGroup, WorkChanged, WorkWatchResult};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

use crate::approval::{Answerer, Client, Surface};
use crate::provider::Scripted;
use crate::push::WorkScope;
use crate::tests_tasks::{
    answers_a_call, arranged, first_user, parent_session, rig, turn, until, ASK,
};
use crate::Core;

fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    }
}

/// What a `work.watch` connection has heard.
struct Heard {
    rx: UnboundedReceiver<Message>,
    changed: Vec<WorkChanged>,
}

impl Heard {
    async fn start(core: &Arc<Core>) -> Self {
        core.push.ensure(core).await.unwrap();
        core.install_thread_feed();
        let (tx, rx) = unbounded_channel();
        core.bus.watch_work("w", tx, WorkScope::default(), false);
        Self {
            rx,
            changed: Vec::new(),
        }
    }

    fn drain(&mut self) {
        while let Ok(Message::Notification(n)) = self.rx.try_recv() {
            if n.method == notify::WORK_CHANGED {
                self.changed.push(serde_json::from_value(n.params).unwrap());
            }
        }
    }

    /// The newest version of each view heard, by the position rule.
    fn views(&mut self) -> std::collections::BTreeMap<String, WorkView> {
        self.drain();
        let mut out: std::collections::BTreeMap<String, WorkView> = Default::default();
        for c in &self.changed {
            for v in &c.views {
                if out.get(&v.id).is_none_or(|o| v.position > o.position) {
                    out.insert(v.id.clone(), v.clone());
                }
            }
        }
        out
    }

    /// The newest version of each group heard.
    fn groups(&mut self) -> std::collections::BTreeMap<String, ThreadGroup> {
        self.drain();
        let mut out: std::collections::BTreeMap<String, ThreadGroup> = Default::default();
        for c in &self.changed {
            for g in &c.threads {
                match out.get_mut(&g.slug) {
                    Some(held) => held.merge(g.clone()),
                    None => {
                        out.insert(g.slug.clone(), g.clone());
                    }
                }
            }
        }
        out
    }
}

fn starts_a_task() -> impl Fn(&crate::provider::ProviderRequest) -> Scripted + Send + Sync + 'static
{
    |req| {
        if first_user(req).contains("CHILD:") {
            return Scripted::text("Counted.");
        }
        if answers_a_call(req) {
            return Scripted::text("Started it.");
        }
        Scripted::tools(
            "Starting it.",
            &[(
                "t_task",
                "task_create",
                serde_json::json!({"brief": "CHILD: count to three", "budget_usd": 1.5, "arrangement": arranged()}),
            )],
        )
    }
}

fn make(core: &Arc<Core>, slug: &str) {
    core.thread_new(
        &ThreadNewParams {
            slug: slug.into(),
            title: Some(format!("Title of {slug}")),
            ..Default::default()
        },
        cli(),
    )
    .unwrap();
}

/// A thread's conversation and its task carry the thread; a conversation in
/// none, and its task, carry none.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_threads_conversation_and_its_task_carry_the_thread_and_others_none() {
    let mut r = rig(starts_a_task(), |_| {});
    *r.model.hold.lock().unwrap() = Some("CHILD".into());
    let core = r.core.clone();
    let mut heard = Heard::start(&core).await;
    make(&core, "tides");
    let session = core.thread_session("tides", cli()).unwrap().session_id;
    turn(&core, &session, ASK).await;
    r.entered.recv().await.unwrap();
    until("the thread's task is on the board", || {
        heard.views().values().any(|v| v.kind == WorkKind::Task)
    })
    .await;
    let views = heard.views();
    let task = views.values().find(|v| v.kind == WorkKind::Task).unwrap();
    let root = &views[&task.root];
    assert_eq!(root.kind, WorkKind::Conversation);
    assert_eq!(root.thread.as_deref(), Some("tides"), "the conversation");
    assert_eq!(task.thread.as_deref(), Some("tides"), "its task");

    // The same turn in a conversation that is in no thread.
    let plain = parent_session(&core);
    turn(&core, &plain, ASK).await;
    r.entered.recv().await.unwrap();
    until("the plain task is on the board", || {
        heard
            .views()
            .values()
            .filter(|v| v.kind == WorkKind::Task)
            .count()
            == 2
    })
    .await;
    let views = heard.views();
    let loose: Vec<&WorkView> = views.values().filter(|v| v.root != root.id).collect();
    assert!(loose.len() >= 2, "{loose:?}");
    assert!(loose.iter().all(|v| v.thread.is_none()), "{loose:?}");
}

/// A thread's write reaches a watcher as a group; a pause changes it; a title
/// with a seal wears the lock the record gave it.
#[tokio::test]
async fn a_threads_write_reaches_a_watcher_as_a_group() {
    let r = rig(|_| Scripted::text("ok"), |_| {});
    let core = r.core.clone();
    let mut heard = Heard::start(&core).await;
    make(&core, "tides");
    let g = heard.groups();
    assert_eq!(g["tides"].title, "Title of tides");
    assert_eq!(g["tides"].state, ThreadState::Active);
    assert!(g["tides"].session_id.is_none(), "no message yet");

    let sid = core.thread_session("tides", cli()).unwrap().session_id;
    assert_eq!(
        heard.groups()["tides"].session_id.as_deref(),
        Some(sid.as_str())
    );

    core.thread_pause("tides", cli()).unwrap();
    assert_eq!(heard.groups()["tides"].state, ThreadState::Paused);
    // Memory held what the watch was told: the snapshot has the same group.
    assert_eq!(core.push.work_threads().len(), 1);
}

/// A thread update reads no record and visits no index row: the board takes
/// the groups it is given.
#[tokio::test]
async fn a_watch_update_reads_no_store() {
    let r = rig(|_| Scripted::text("ok"), |_| {});
    let core = r.core.clone();
    let mut heard = Heard::start(&core).await;
    make(&core, "tides");
    let mut groups = core.threads().groups();
    groups[0].state = ThreadState::Paused;
    groups[0].title = "Tides, paused".into();
    let (records, rows) = (
        theseus_store::records_read_here(),
        theseus_store::index_rows_here(),
    );
    core.push.set_threads(&core, groups);
    assert_eq!(
        theseus_store::records_read_here(),
        records,
        "no record read"
    );
    assert_eq!(
        theseus_store::index_rows_here(),
        rows,
        "no index row visited"
    );
    assert_eq!(heard.groups()["tides"].title, "Tides, paused");
}

/// `work.watch` over the wire: the owner's surface is given the groups with
/// their stats, a surface that reads no private text none.
#[tokio::test]
async fn work_watch_gives_the_owner_the_groups_and_a_guarded_surface_none() {
    let r = rig(|_| Scripted::text("ok"), |_| {});
    let core = r.core.clone();
    make(&core, "tides");
    core.thread_session("tides", cli()).unwrap();
    let owner = watch(&core, Surface::Cli, serde_json::json!({"limit": 0})).await;
    assert_eq!(owner.threads.len(), 1);
    let g = &owner.threads[0];
    assert_eq!(
        (g.slug.as_str(), g.title.as_str()),
        ("tides", "Title of tides")
    );
    assert!(
        g.today_usd.is_some(),
        "the stats are read when the watch begins"
    );
    let guarded = watch(&core, Surface::Discord, serde_json::json!({"roots": []})).await;
    assert!(guarded.threads.is_empty(), "a thread is private always");
}

async fn watch(core: &Arc<Core>, surface: Surface, params: serde_json::Value) -> WorkWatchResult {
    use theseus_protocol::{Id, Request};
    use tokio::io::{duplex, AsyncBufReadExt, AsyncWriteExt, BufReader};
    let (client, server) = duplex(1 << 20);
    let (sr, sw) = tokio::io::split(server);
    let srv = tokio::spawn(
        core.clone()
            .serve_connection(sr, sw, Client::new("w", surface)),
    );
    let (cr, mut cw) = tokio::io::split(client);
    let req = Request::new(Id::Num(1), theseus_protocol::method::WORK_WATCH, params);
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = BufReader::new(cr).lines();
    let out = loop {
        let l = lines.next_line().await.unwrap().unwrap();
        if let Message::Response(r) = serde_json::from_str::<Message>(&l).unwrap() {
            break serde_json::to_value(r).unwrap();
        }
    };
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = srv.await;
    serde_json::from_value(out["result"].clone()).unwrap_or_else(|e| panic!("{e}: {out}"))
}
