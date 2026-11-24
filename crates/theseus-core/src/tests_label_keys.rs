//! One key for one turn and one label (issue 44): a `label:<flag>` touch is keyed at its turn's own time (the
//! UUIDv7 the runner minted the turn's id with), whatever path its frame took. A configured sensitive name in
//! the message lands its label before the exchange opens; a second label of the same flag in the same turn (the
//! reply's) lands after the exchange is stored; a frame the log kept while its append failed is indexed when the
//! store is opened again. Each leaves the turn one key, and a label's page lists the exchange once.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::json;
use theseus_judge::fake::{FakeJev, Scripted as Jev};
use theseus_protocol::{method, SessionKind};

use crate::approval::Surface;
use crate::content::{Flag, ItemKind, Label};
use crate::exchange::{self, Root, TOUCH_PREFIX};
use crate::item_labels;
use crate::node::Body;
use crate::rpc::Core;
use crate::session::SessionRecord;
use crate::store::Store;
use crate::tests_inbound::inbound_only;
use crate::tests_judge::{rig_on, texts, Rig};

/// A configured sensitive name the owner's message holds; the stand-in's replies read `Done: answer N.`, so
/// `answer` labels each reply the same way.
const NAME: &str = "kelpfarm";

/// The gap held between the message's label and the exchange's opening, so the two times differ.
const GAP: Duration = Duration::from_millis(40);

/// Every question of the content pack `no`: the labels here are the deterministic ones. The stand-in is kept
/// for as long as the rig.
fn rig() -> (Rig, FakeJev) {
    let jev = FakeJev::start().unwrap();
    for q in ["injection", "secret", "confidential", "personal"] {
        let no = Jev::Choice {
            option: "no".into(),
            confidence: 0.95,
        };
        jev.script(q, no);
    }
    let r = rig_on(texts(4), Some(&jev), |c| {
        inbound_only(c);
        c.judge.total_secs = 5;
        c.judge.packs.remove(crate::judge::content::PACK);
        c.privacy.sensitive_names = vec![NAME.into(), "answer".into()];
    });
    (r, jev)
}

fn session(c: &Core) -> String {
    let r = SessionRecord::new(SessionKind::Conversation, None);
    c.store.put_session(&r.session_id, &r).unwrap();
    r.session_id
}

/// The turn's message and its last reply, by node id, and the turn's id.
fn message_and_reply(c: &Core, sid: &str) -> (String, String, String) {
    let nodes = c.store.transcript(sid).unwrap();
    let message = (nodes.iter())
        .find(|(_, n)| matches!(n.body, Body::UserMessage { .. }))
        .map(|(_, n)| n.clone())
        .expect("a message");
    let reply = (nodes.iter().rev())
        .find(|(_, n)| matches!(n.body, Body::AssistantMessage { .. }))
        .map(|(_, n)| n.id.clone())
        .expect("a reply");
    (message.id, reply, message.turn_id.expect("its turn"))
}

/// Whether `node`'s record holds `flag`, waiting for it to land (the reply's label lands as its turn ends).
async fn holds(store: &Store, node: &str, flag: Flag) {
    let t0 = Instant::now();
    loop {
        let item = item_labels::read(store, node).unwrap();
        if item.is_some_and(|i| i.raised.iter().any(|l| l.flag == flag)) {
            return;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "no {flag:?} on {node}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// The times of `turn`'s keys under `flag`'s root, oldest first.
fn keys(store: &Store, flag: Flag, turn: &str) -> Vec<u64> {
    let root = item_labels::touch_root(flag);
    let mut at: Vec<u64> = (exchange::newest(store, Root::Thing(&root), 64)
        .unwrap()
        .into_iter())
    .filter(|t| t.exchange == turn)
    .map(|t| t.at_ms)
    .collect();
    at.sort_unstable();
    at
}

/// The store at `dir`, opened again once the core that held it is gone.
async fn reopened(dir: &std::path::Path) -> Store {
    for _ in 0..50 {
        if let Ok(s) = Store::open(dir) {
            return s;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Store::open(dir).unwrap()
}

/// KEY: the message's configured name raises `confidential` before its exchange opens (the deterministic path,
/// no race: the label is written and told before `exchange_open` reads its clock); the reply's raises it again
/// once the exchange is stored. The turn holds ONE key of the label, at its turn's time, and the label's page
/// lists the exchange once.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turns_message_and_reply_labels_of_one_flag_share_one_key() {
    let (r, _jev) = rig();
    let c: &Arc<Core> = &r.core;
    // Told as it is written, before the exchange opens: the wait holds the two times apart.
    c.runner.judge.label_told(|_, _| std::thread::sleep(GAP));
    let sid = session(c);
    crate::test_support::turn(c, &sid, &format!("the {NAME} numbers for March")).await;
    let (message, reply, turn) = message_and_reply(c, &sid);
    holds(&c.store, &message, Flag::Confidential).await;
    holds(&c.store, &reply, Flag::Confidential).await;
    let ex = exchange::exchange(&c.store, &turn)
        .unwrap()
        .expect("its exchange");
    let (_, said) = c.store.get_node(&message).unwrap().unwrap();
    assert!(
        ex.start_ms >= said.created_at_ms + GAP.as_millis() as u64,
        "the exchange opened after the message's label: {} then {}",
        said.created_at_ms,
        ex.start_ms
    );
    let at = crate::id_ms(&turn).expect("a live turn's id is a UUIDv7");
    assert_eq!(keys(&c.store, Flag::Confidential, &turn), [at]);
    let page = crate::tests_mcp_memory::rpc(
        c,
        Surface::Cli,
        method::HISTORY_READ,
        json!({"label": "confidential"}),
    )
    .await
    .unwrap();
    let handles: Vec<&str> = (page["lines"].as_array().unwrap().iter())
        .map(|l| l["handle"].as_str().unwrap())
        .collect();
    assert_eq!(handles, [turn.as_str()], "{page}");
}

/// KEY: the message's label frame is kept by the log while its append fails, before the exchange opens; the
/// exchange is then stored, the reply's label of the same flag lands, the message's label is landed again (the
/// retry), and the store is opened again, which indexes the kept frame. One key of the label, at the turn's
/// time, and the label's record beside it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_label_frame_the_log_kept_leaves_one_key_after_the_retry_and_a_reopen() {
    let (r, _jev) = rig();
    let c: &Arc<Core> = &r.core;
    let root = format!(
        "{TOUCH_PREFIX}{}",
        Root::Thing(&item_labels::touch_root(Flag::Confidential)).key()
    );
    let kept: Arc<Mutex<Vec<String>>> = Arc::default();
    let seen = kept.clone();
    c.store.retain_plain_frame(move |rows| {
        let hit = (rows.iter()).any(|x| x.key.as_deref().is_some_and(|k| k.starts_with(&root)));
        if hit {
            *seen.lock().unwrap() = rows.iter().filter_map(|x| x.key.clone()).collect();
            // The exchange opens after this frame: the two times differ.
            std::thread::sleep(GAP);
        }
        hit
    });
    let sid = session(c);
    crate::test_support::turn(c, &sid, &format!("the {NAME} numbers for March")).await;
    let (message, reply, turn) = message_and_reply(c, &sid);
    let frame = kept.lock().unwrap().clone();
    assert!(
        frame.iter().any(|k| *k == item_labels::key(&message)),
        "the message's label record rode the kept frame: {frame:?}"
    );
    assert!(
        exchange::exchange(&c.store, &turn).unwrap().is_some(),
        "the exchange is stored"
    );
    holds(&c.store, &reply, Flag::Confidential).await;
    // The retry: the message's label landed again, now that its exchange can be read.
    let label = Label {
        flag: Flag::Confidential,
        node: Some(message.clone()),
        kind: ItemKind::OperatorMessage,
        source: "operator message".into(),
        confidence: 1.0,
        band: theseus_judge::band::Band::Confirm,
        at_ms: theseus_protocol::now_unix_ms(),
        judgment: String::new(),
    };
    let (core, s) = (c.clone(), sid.clone());
    tokio::task::spawn_blocking(move || core.runner.judge.land_deterministic(&s, vec![(label, 1)]))
        .await
        .unwrap();
    holds(&c.store, &message, Flag::Confidential).await;
    let dir = c.store.dir().to_path_buf();
    let Rig { core, .. } = r;
    drop(core);
    let store = reopened(&dir).await;
    let at = crate::id_ms(&turn).expect("a live turn's id is a UUIDv7");
    assert_eq!(keys(&store, Flag::Confidential, &turn), [at]);
    let item = item_labels::read(&store, &message)
        .unwrap()
        .expect("its record");
    assert!(
        item.raised.iter().any(|l| l.flag == Flag::Confidential),
        "{item:?}"
    );
}
