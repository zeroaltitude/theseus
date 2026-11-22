//! Jev's labels on the item, through the whole core (issue 44): a real turn, the judge stand-in the content
//! tests already use, and the item's own record read by the one reader (`item_labels::of_node`), against the
//! session's flags, which stay exactly as they were.

use std::sync::Arc;
use std::time::{Duration, Instant};

use theseus_judge::fake::{FakeJev, FakeMode, Scripted as Jev};
use theseus_protocol::{ItemLabelsView, SessionKind};

use crate::exchange::{self, Exchange, Opener};
use crate::item_labels;
use crate::node::Body;
use crate::rpc::Core;
use crate::session::SessionRecord;
use crate::tests_inbound::inbound_only;
use crate::tests_judge::{rig_on, texts, Rig};

/// The words the stand-in calls `personal`.
const PRIVATE: &str = "my diagnosis came back";

/// Content judging live against the stand-in, the inbound point's own packs off.
fn rig(jev: &FakeJev) -> Rig {
    rig_on(texts(4), Some(jev), |c| {
        inbound_only(c);
        c.judge.total_secs = 5;
        // content.v4 (the judge rigs turn it off): this is its subject.
        c.judge.packs.remove(crate::judge::content::PACK);
    })
}

/// Every question `no`, and `personal` `yes` of an item that holds [`PRIVATE`].
fn jev_says_personal(jev: &FakeJev) {
    for q in ["injection", "secret", "confidential", "personal"] {
        let no = Jev::Choice {
            option: "no".into(),
            confidence: 0.95,
        };
        jev.script(q, no);
    }
    let yes = Jev::Choice {
        option: "yes".into(),
        confidence: 0.97,
    };
    jev.script_when(PRIVATE, None, "personal", yes);
}

fn session(c: &Core) -> String {
    let r = SessionRecord::new(SessionKind::Conversation, None);
    c.store.put_session(&r.session_id, &r).unwrap();
    r.session_id
}

/// The node of `sid`'s first message, and of its last reply.
fn message_and_reply(c: &Core, sid: &str) -> (String, String) {
    let nodes = c.store.transcript(sid).unwrap();
    let message = nodes
        .iter()
        .find(|(_, n)| matches!(n.body, Body::UserMessage { .. }))
        .map(|(_, n)| n.id.clone())
        .expect("a message");
    let reply = nodes
        .iter()
        .rev()
        .find(|(_, n)| matches!(n.body, Body::AssistantMessage { .. }))
        .map(|(_, n)| n.id.clone())
        .expect("a reply");
    (message, reply)
}

/// `node`'s labels once its record is written (the landing is off the turn's path).
async fn landed(c: &Core, node: &str) -> ItemLabelsView {
    let t0 = Instant::now();
    loop {
        let v = item_labels::of_node(&c.store, None, node).unwrap();
        if v.state != "not_judged" {
            return v;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "no labels landed on {node}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn flags(c: &Core, sid: &str) -> crate::content::ContentFlags {
    let rec: SessionRecord = c.store.get_session(sid).unwrap().unwrap();
    rec.data.map(|b| *b).unwrap_or_default()
}

/// A person's message the stand-in calls `personal`: its item says so, with the band and the pack; the
/// session carries the flag as before, from the same answer (the band agrees); the reply is asked the same
/// four questions and reads "asked, clean"; and a view's piece of that exchange shows the label on its line.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_personal_message_is_labelled_on_its_item_and_the_reply_is_judged_too() {
    let jev = FakeJev::start().unwrap();
    jev_says_personal(&jev);
    let r = rig(&jev);
    let c: &Arc<Core> = &r.core;
    let sid = session(c);
    crate::test_support::turn(c, &sid, &format!("FYI, {PRIVATE} today.")).await;
    let (message, reply) = message_and_reply(c, &sid);

    let m = landed(c, &message).await;
    assert_eq!(m.state, "labelled", "{m:?}");
    assert_eq!(m.pack.as_deref(), Some(crate::judge::content::PACK));
    let p = (m.flags.iter().find(|f| f.flag == "personal")).unwrap_or_else(|| panic!("{m:?}"));
    assert_eq!(p.pack, crate::judge::content::PACK);
    let held = flags(c, &sid);
    let on_session = held
        .label(crate::content::Flag::Personal)
        .expect("the session carries it");
    assert_eq!(on_session.node.as_deref(), Some(message.as_str()));
    assert_eq!(
        p.band,
        serde_json::to_value(on_session.band)
            .unwrap()
            .as_str()
            .unwrap()
    );
    assert!(m.line().starts_with("personal ("), "{}", m.line());

    let rp = landed(c, &reply).await;
    assert_eq!(rp.kind.as_deref(), Some("reply"), "{rp:?}");
    assert_eq!(
        (rp.state.as_str(), rp.line()),
        ("clean", "asked, clean".into())
    );
    let injection = rp.questions.iter().find(|q| q.question == "injection");
    assert_eq!(injection.map(|q| q.said.as_str()), Some("no"), "{rp:?}");

    // `view show`'s line for a piece of that exchange: the reader the view's pieces use.
    let ex = Exchange {
        turn_id: "t-labels".into(),
        session_id: sid.clone(),
        place: "terminal".into(),
        private: true,
        opened_by: Opener::Message,
        person: None,
        start_ms: 1_000,
        end_ms: Some(2_000),
        ask: Some(message.clone()),
        reply: Some(reply.clone()),
        messages: vec![message.clone()],
        stop: None,
        rebuilt: false,
    };
    c.store
        .append(&[exchange::exchange_record(&ex).unwrap()])
        .unwrap();
    let lines = item_labels::piece_lines(&c.store, None, ("exchanges", "t-labels", "t-labels"));
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(
        lines[0].starts_with(&format!("{message}: personal (")),
        "{lines:?}"
    );
    assert_eq!(lines[1], format!("{reply}: asked, clean"));
}

/// An ask that fails, and fails again when it is asked once more, is kept as `failed`: never "clean", and
/// the session carries nothing for it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_ask_that_fails_twice_reads_failed_never_clean() {
    let jev = FakeJev::start().unwrap();
    jev_says_personal(&jev);
    jev.set_mode(FakeMode::Down);
    let r = rig(&jev);
    let c: &Arc<Core> = &r.core;
    let sid = session(c);
    crate::test_support::turn(c, &sid, &format!("FYI, {PRIVATE} today.")).await;
    let (message, _) = message_and_reply(c, &sid);
    let m = landed(c, &message).await;
    assert_eq!(m.state, "failed", "{m:?}");
    assert_eq!(m.line(), "failed (failed)");
    assert!(m.flags.is_empty(), "{m:?}");
    assert!(flags(c, &sid).labels.is_empty());
}

/// An item from before items kept their labels has no record: "not judged then", whatever its session
/// carries.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_item_from_before_reads_not_judged_then() {
    let jev = FakeJev::start().unwrap();
    let r = rig(&jev);
    let v = item_labels::of_node(&r.core.store, None, "nod_from_before").unwrap();
    assert_eq!(
        (v.state.as_str(), v.line()),
        ("not_judged", "not judged then".into())
    );
}

/// The owner removing the flag is recorded on the item and the session alike (c), through
/// `content.unlabel`'s own path.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_owners_removal_is_on_the_item_and_the_session() {
    let jev = FakeJev::start().unwrap();
    jev_says_personal(&jev);
    let r = rig(&jev);
    let c: &Arc<Core> = &r.core;
    let sid = session(c);
    crate::test_support::turn(c, &sid, &format!("FYI, {PRIVATE} today.")).await;
    let (message, _) = message_and_reply(c, &sid);
    assert_eq!(landed(c, &message).await.state, "labelled");
    let cli = crate::approval::Answerer {
        label: "the CLI".into(),
        surface: crate::approval::Surface::Cli,
        discord: None,
        agent: None,
    };
    c.unlabel_session(&sid, "personal", cli).unwrap();
    assert!(!flags(c, &sid).has(crate::content::Flag::Personal));
    let stored = item_labels::read(&c.store, &message).unwrap().unwrap();
    assert_eq!(stored.removed.len(), 1, "{stored:?}");
    let v = item_labels::of_node(&c.store, None, &message).unwrap();
    assert_eq!(v.state, "clean", "{v:?}");
    assert!(v.line().contains("personal removed"), "{}", v.line());
}

/// A clean tool result reads "asked, clean": every question answered `no`, `injection` included (a result is
/// asked it), the kind `tool_result`, and the session carries nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_clean_tool_result_reads_asked_clean() {
    use crate::provider::Scripted;
    let jev = FakeJev::start().unwrap();
    jev_says_personal(&jev);
    let echo = Scripted::tools(
        "",
        &[(
            "t1",
            "proc_run",
            serde_json::json!({"argv": ["echo", "the weather is fine"]}),
        )],
    );
    let r = rig_on(vec![echo, Scripted::text("Ran it.")], Some(&jev), |c| {
        inbound_only(c);
        c.judge.total_secs = 5;
        c.judge.packs.remove(crate::judge::content::PACK);
        c.tools.proc_sync_secs = 5;
        c.tools.projects_dir = Some(std::env::temp_dir().to_string_lossy().into_owned());
        c.policy
            .tools
            .insert("proc.run".into(), crate::policy::Posture::Open);
    });
    let c: &Arc<Core> = &r.core;
    let first = crate::tests_route::try_turn_attached(c, None, "run it", None, vec![])
        .await
        .unwrap();
    let nodes = c.store.transcript(&first.session_id).unwrap();
    let result = nodes
        .iter()
        .find(|(_, n)| matches!(&n.body, Body::ToolResult { tool, .. } if tool == "proc.run"))
        .map(|(_, n)| n.id.clone())
        .expect("a proc.run result");
    let v = landed(c, &result).await;
    assert_eq!(v.kind.as_deref(), Some("tool_result"), "{v:?}");
    assert_eq!(
        (v.state.as_str(), v.line()),
        ("clean", "asked, clean".into())
    );
    assert!(
        v.questions.iter().all(|q| q.said == "no"),
        "every question asked and answered no: {v:?}"
    );
    assert!(flags(c, &first.session_id).labels.is_empty());
}
