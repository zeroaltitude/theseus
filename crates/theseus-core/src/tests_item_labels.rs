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

/// A reply the stand-in calls an injection is labelled on its own item, and its session carries nothing: no
/// label, no `content.labelled` row, so nothing that acts on a session's labels (the freeze, the card) reads
/// it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reply_flagged_is_labelled_on_the_reply_alone() {
    let jev = FakeJev::start().unwrap();
    jev_says_personal(&jev);
    let yes = Jev::Choice {
        option: "yes".into(),
        confidence: 0.97,
    };
    jev.script_when("Done: answer", None, "injection", yes);
    let r = rig(&jev);
    let c: &Arc<Core> = &r.core;
    let sid = session(c);
    crate::test_support::turn(c, &sid, "What is the plan for today?").await;
    let (_, reply) = message_and_reply(c, &sid);
    let v = landed(c, &reply).await;
    assert_eq!(v.state, "labelled", "{v:?}");
    assert!(v.line().starts_with("injection ("), "{}", v.line());
    assert!(
        flags(c, &sid).labels.is_empty(),
        "the session carries nothing"
    );
    assert!(
        crate::tests_judge::kinds(&c.store, "content.labelled").is_empty(),
        "no row says a session was labelled"
    );
}

// ------------------------------------------------------------ round 2 (the review of the labels unit)

/// A rig whose model first writes `first` beside a `proc.run` call, then `Done.`.
fn rig_two_replies(jev: &FakeJev, first: &str) -> Rig {
    use crate::provider::Scripted;
    let echo = Scripted::tools(
        first,
        &[(
            "t1",
            "proc_run",
            serde_json::json!({"argv": ["echo", "the weather is fine"]}),
        )],
    );
    rig_on(vec![echo, Scripted::text("Done.")], Some(jev), |c| {
        inbound_only(c);
        c.judge.total_secs = 5;
        c.judge.packs.remove(crate::judge::content::PACK);
        c.tools.proc_sync_secs = 5;
        c.tools.projects_dir = Some(std::env::temp_dir().to_string_lossy().into_owned());
        c.policy
            .tools
            .insert("proc.run".into(), crate::policy::Posture::Open);
    })
}

/// Every assistant node of `sid` that carries text, in order.
fn replies(c: &Core, sid: &str) -> Vec<String> {
    let nodes = c.store.transcript(sid).unwrap();
    nodes
        .iter()
        .filter(|(_, n)| match &n.body {
            Body::AssistantMessage { blocks, .. } => !crate::provider::text_of(blocks).is_empty(),
            _ => false,
        })
        .map(|(_, n)| n.id.clone())
        .collect()
}

/// **1. A reply that repeats a configured sensitive name** settles its turn: the name's label is kept on the
/// reply's item (no model raised it), and its session carries nothing. It used to take the session's lock a
/// second time, on the thread that held it for the turn's last frame, and panic.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reply_holding_a_sensitive_name_settles_and_is_labelled_on_its_item() {
    let jev = FakeJev::start().unwrap();
    jev_says_personal(&jev);
    let r = rig_on(texts(4), Some(&jev), |c| {
        inbound_only(c);
        c.judge.total_secs = 5;
        c.judge.packs.remove(crate::judge::content::PACK);
        // The stand-in's replies read `Done: answer N.`.
        c.privacy.sensitive_names = vec!["answer".into()];
    });
    let c: &Arc<Core> = &r.core;
    let sid = session(c);
    crate::test_support::turn(c, &sid, "What is the plan for today?").await;
    let (_, reply) = message_and_reply(c, &sid);
    let v = landed(c, &reply).await;
    let f = (v.flags.iter().find(|f| f.flag == "confidential")).unwrap_or_else(|| panic!("{v:?}"));
    assert_eq!(f.pack, item_labels::DETERMINISTIC, "{v:?}");
    assert!(
        flags(c, &sid).labels.is_empty(),
        "the session carries nothing"
    );
}

/// **2. Each reply is judged under its own node, with its own text.** A turn that writes a message before a
/// tool call and then `Done.`: the first message holds the personal words and is labelled; `Done.` is
/// clean. (The turn's joined output used to be judged under its last node.)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn each_reply_of_a_turn_is_judged_by_its_own_text() {
    let jev = FakeJev::start().unwrap();
    jev_says_personal(&jev);
    let r = rig_two_replies(&jev, &format!("Noted: {PRIVATE}. Checking."));
    let c: &Arc<Core> = &r.core;
    let first = crate::tests_route::try_turn_attached(c, None, "run it", None, vec![])
        .await
        .unwrap();
    let said = replies(c, &first.session_id);
    assert_eq!(said.len(), 2, "two replies: {said:?}");
    let a = landed(c, &said[0]).await;
    assert_eq!(a.state, "labelled", "{a:?}");
    assert!(a.line().starts_with("personal ("), "{}", a.line());
    let b = landed(c, &said[1]).await;
    assert_eq!(
        (b.state.as_str(), b.line()),
        ("clean", "asked, clean".into())
    );
}

/// **3. A removal in a fork is the fork's.** A session's message is labelled `personal`; a fork of it carries
/// the label; the owner removes it in the fork. Read from its own session the item still says `personal`,
/// as that session still carries it; read from the fork it says removed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_removal_in_a_fork_leaves_the_original_session_s_item_as_it_was() {
    let jev = FakeJev::start().unwrap();
    jev_says_personal(&jev);
    let r = rig(&jev);
    let c: &Arc<Core> = &r.core;
    let sid = session(c);
    crate::test_support::turn(c, &sid, &format!("FYI, {PRIVATE} today.")).await;
    let (message, reply) = message_and_reply(c, &sid);
    assert_eq!(landed(c, &message).await.state, "labelled");
    let fork = c
        .session_fork(
            theseus_protocol::SessionForkParams {
                session: sid.clone(),
                at: reply,
                ..Default::default()
            },
            crate::approval::Surface::Cli,
        )
        .unwrap();
    assert!(flags(c, &fork.session_id).has(crate::content::Flag::Personal));
    let cli = crate::approval::Answerer {
        label: "the CLI".into(),
        surface: crate::approval::Surface::Cli,
        discord: None,
        agent: None,
    };
    c.unlabel_session(&fork.session_id, "personal", cli)
        .unwrap();
    assert!(
        flags(c, &sid).has(crate::content::Flag::Personal),
        "the source keeps it"
    );
    let here = item_labels::of_node(&c.store, None, &message).unwrap();
    assert_eq!(
        here.state, "labelled",
        "read from its own session: {here:?}"
    );
    assert!(!here.line().contains("removed"), "{}", here.line());
    let there = item_labels::of_node_in(&c.store, None, &message, &fork.session_id).unwrap();
    assert!(
        there.line().contains("personal removed"),
        "read from the fork: {}",
        there.line()
    );
}

/// **4. An exchange's topic is found by its own touch, however many touch the topic after it.** The touch
/// `names::topics` writes is keyed at the exchange's start; four later exchanges of the same topic do not
/// hide it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_topic_is_found_behind_four_later_touches() {
    use theseus_ontology::{Category, CategoryId, Origin, Record};
    let jev = FakeJev::start().unwrap();
    let r = rig(&jev);
    let c: &Arc<Core> = &r.core;
    let topic = CategoryId::parse("topic:ledger").unwrap();
    let cat = Record::Category(Category::new(topic.clone(), "Ledger", "owner"));
    c.runner
        .ontology
        .write(&c.store, vec![cat], Origin::Operator, |_| Ok(vec![]))
        .unwrap();
    let sid = session(c);
    let ex = Exchange {
        turn_id: "t-long".into(),
        session_id: sid.clone(),
        place: "terminal".into(),
        private: true,
        opened_by: Opener::Message,
        person: None,
        start_ms: 10_000,
        end_ms: Some(90_000),
        ask: None,
        reply: None,
        messages: vec![],
        stop: None,
        rebuilt: false,
    };
    let mut recs = vec![exchange::exchange_record(&ex).unwrap()];
    let root = exchange::Root::Thing(topic.as_str());
    recs.extend(exchange::touch(root, "t-long", &sid, 10_000));
    for (k, at) in [20_000u64, 30_000, 40_000, 50_000].into_iter().enumerate() {
        recs.extend(exchange::touch(root, &format!("t-other{k}"), &sid, at));
    }
    c.store.append(&recs).unwrap();
    let o = c.runner.ontology.snapshot(&c.store).unwrap();
    let found = item_labels::topics(&c.store, &o, "t-long").unwrap();
    assert_eq!(found, vec!["topic:ledger".to_string()]);
}

/// **9. `view show` of a real next turn** shows the first exchange's items with their labels, through the
/// `view.show` RPC as the owner's own terminal asks it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn view_show_of_the_next_turn_shows_the_first_exchange_s_labels() {
    use crate::approval::Surface;
    let jev = FakeJev::start().unwrap();
    jev_says_personal(&jev);
    let r = rig(&jev);
    let c: &Arc<Core> = &r.core;
    let sid = session(c);
    crate::test_support::turn(c, &sid, &format!("FYI, {PRIVATE} today.")).await;
    let (message, _) = message_and_reply(c, &sid);
    assert_eq!(landed(c, &message).await.state, "labelled");
    let next = crate::test_support::turn(c, &sid, "And what is next?").await;
    let shown = crate::tests_m3::rpc_as(
        c,
        crate::tests_m3::surface("sock#1", Surface::Cli),
        theseus_protocol::method::VIEW_SHOW,
        serde_json::json!({"turn_id": next.turn_id}),
    )
    .await
    .unwrap_or_else(|e| panic!("{e:?}"));
    let r: theseus_protocol::view::ViewShowResult = serde_json::from_value(shown).unwrap();
    let lines: Vec<String> = (r.views.iter())
        .flat_map(|v| v.slots.iter())
        .flat_map(|s| s.pieces.iter())
        .flat_map(|p| p.labels.iter().cloned())
        .collect();
    let want = format!("{message}: personal (");
    assert!(
        lines.iter().any(|l| l.starts_with(&want)),
        "no piece of the next turn's view shows the first message's label: {lines:#?}"
    );
    // And the CLI's lines print it under its piece.
    let printed: String = r.views.iter().map(theseus_protocol::view::lines).collect();
    assert!(printed.contains(&format!("jev {want}")), "{printed}");
}

/// **7(b). A reply is asked under half the day's judge allowance alone.** With the day already past half,
/// the turn's input is judged as ever (inputs are reserved exactly as before), and its reply is recorded
/// `refused` on its item: never clean, and nothing spent on it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn past_half_the_day_a_reply_is_refused_and_the_input_is_judged() {
    use crate::judge::spend::{local_day, Stored, META_KEY};
    let jev = FakeJev::start().unwrap();
    jev_says_personal(&jev);
    let r = rig_on(texts(4), Some(&jev), |c| {
        inbound_only(c);
        c.judge.total_secs = 5;
        c.judge.packs.remove(crate::judge::content::PACK);
        c.judge.shadow_limit_usd_per_day = 1.0;
    });
    let c: &Arc<Core> = &r.core;
    let day = Stored {
        day: local_day(theseus_protocol::now_unix_ms()),
        reserved_micros: 600_000,
        spent_micros: 600_000,
    };
    let seed = theseus_store::NewRecord::json(theseus_store::kinds::META, Some(META_KEY), &day);
    c.store.append(&[seed.unwrap()]).unwrap();
    let sid = session(c);
    crate::test_support::turn(c, &sid, &format!("FYI, {PRIVATE} today.")).await;
    let (message, reply) = message_and_reply(c, &sid);
    assert_eq!(
        landed(c, &message).await.state,
        "labelled",
        "the input is judged"
    );
    let v = landed(c, &reply).await;
    assert_eq!(
        (v.state.as_str(), v.line()),
        ("failed", "failed (refused)".into())
    );
}

// ------------------------------------------------------------ round 3

/// **On by default, off when the owner says.** By default a turn's reply is judged; with
/// `[content_based_security] judge_replies = false` it is offered to no judge and reads "not judged then", and
/// its message is judged as ever.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reply_is_judged_by_default_and_not_when_the_owner_turns_it_off() {
    let jev = FakeJev::start().unwrap();
    jev_says_personal(&jev);
    let on = rig(&jev);
    assert!(
        on.core.cfg.content_based_security.judge_replies,
        "the default"
    );
    let sid = session(&on.core);
    crate::test_support::turn(&on.core, &sid, &format!("FYI, {PRIVATE} today.")).await;
    let (_, reply) = message_and_reply(&on.core, &sid);
    assert_eq!(landed(&on.core, &reply).await.line(), "asked, clean");

    let r = rig_on(texts(4), Some(&jev), |c| {
        inbound_only(c);
        c.judge.total_secs = 5;
        c.judge.packs.remove(crate::judge::content::PACK);
        c.content_based_security.judge_replies = false;
    });
    let c: &Arc<Core> = &r.core;
    let sid = session(c);
    crate::test_support::turn(c, &sid, &format!("FYI, {PRIVATE} today.")).await;
    let (message, reply) = message_and_reply(c, &sid);
    assert_eq!(landed(c, &message).await.state, "labelled");
    let submitted = c.runner.judge.content_submitted();
    assert!(
        !submitted.contains(&reply),
        "the reply was offered: {submitted:?}"
    );
    let v = item_labels::of_node(&c.store, None, &reply).unwrap();
    assert_eq!(v.line(), "not judged then");
}

/// **A reply in flight never makes an input wait.** The first turn's reply is held at the judge; the next
/// turn's message is judged and labelled while that reply's ask is still out, by the inputs' own worker.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_input_is_judged_while_a_reply_batch_is_out() {
    let jev = FakeJev::start().unwrap();
    jev_says_personal(&jev);
    // Every reply's request waits for a release that comes only at the end.
    jev.hold_when("Done: answer");
    let r = rig(&jev);
    let c: &Arc<Core> = &r.core;
    let sid = session(c);
    crate::test_support::turn(c, &sid, "What is the plan for today?").await;
    let (_, first_reply) = message_and_reply(c, &sid);
    crate::test_support::turn(c, &sid, &format!("FYI, {PRIVATE} today.")).await;
    let second = (c.store.transcript(&sid).unwrap().into_iter().rev())
        .find(|(_, n)| matches!(n.body, Body::UserMessage { .. }))
        .map(|(_, n)| n.id)
        .expect("the second message");
    let v = landed(c, &second).await;
    assert_eq!(v.state, "labelled", "{v:?}");
    let held = item_labels::of_node(&c.store, None, &first_reply).unwrap();
    assert_eq!(
        held.state, "not_judged",
        "the reply's ask is still out: {held:?}"
    );
    jev.release();
}

// ------------------------------------------------------------ the label's touch (issue 44: a search by label)

/// The touches of `flag`'s root, once `node`'s labels have landed (they ride the same frame).
fn label_touches(c: &Core, flag: crate::content::Flag) -> Vec<exchange::Touch> {
    let root = item_labels::touch_root(flag);
    exchange::newest(&c.store, exchange::Root::Thing(&root), 8).unwrap()
}

/// The exchange `node` is part of.
fn exchange_of(c: &Core, node: &str) -> Exchange {
    let (_, n) = c.store.get_node(node).unwrap().unwrap();
    exchange::exchange(&c.store, n.turn_id.as_deref().unwrap())
        .unwrap()
        .expect("its exchange")
}

/// A message the stand-in calls `personal` writes the label's touch at its exchange's start, in the frame
/// its labels land in: "everything labelled personal" is a timeline, as a topic's is.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_raised_input_label_writes_its_touch_at_the_exchanges_start() {
    let jev = FakeJev::start().unwrap();
    jev_says_personal(&jev);
    let r = rig(&jev);
    let c: &Arc<Core> = &r.core;
    let sid = session(c);
    crate::test_support::turn(c, &sid, &format!("FYI, {PRIVATE} today.")).await;
    let (message, _) = message_and_reply(c, &sid);
    assert_eq!(landed(c, &message).await.state, "labelled");
    let ex = exchange_of(c, &message);
    let t = label_touches(c, crate::content::Flag::Personal);
    assert_eq!(t.len(), 1, "{t:?}");
    assert_eq!(
        (t[0].exchange.as_str(), t[0].session_id.as_str(), t[0].at_ms),
        (ex.turn_id.as_str(), sid.as_str(), ex.start_ms)
    );
    assert!(label_touches(c, crate::content::Flag::Secret).is_empty());
}

/// A reply the stand-in calls an injection writes the label's touch, and its session still carries no
/// label and no `content.labelled` row: the freeze's input is unchanged.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_replys_label_writes_its_touch_and_still_not_the_sessions_label() {
    let jev = FakeJev::start().unwrap();
    jev_says_personal(&jev);
    let yes = Jev::Choice {
        option: "yes".into(),
        confidence: 0.97,
    };
    jev.script_when("Done: answer", None, "injection", yes);
    let r = rig(&jev);
    let c: &Arc<Core> = &r.core;
    let sid = session(c);
    crate::test_support::turn(c, &sid, "What is the plan for today?").await;
    let (_, reply) = message_and_reply(c, &sid);
    assert_eq!(landed(c, &reply).await.state, "labelled");
    let ex = exchange_of(c, &reply);
    let t = label_touches(c, crate::content::Flag::Injection);
    let turns: Vec<&str> = t.iter().map(|t| t.exchange.as_str()).collect();
    assert_eq!(turns, [ex.turn_id.as_str()]);
    assert!(
        flags(c, &sid).labels.is_empty(),
        "the session carries nothing"
    );
    assert!(
        crate::tests_judge::kinds(&c.store, "content.labelled").is_empty(),
        "no row says a session was labelled"
    );
}

/// A label raised on something the turn read (a tool's result) is found on that turn's exchange: neither
/// the message nor the reply holds it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tool_results_label_is_found_on_its_exchange() {
    use crate::provider::Scripted;
    let jev = FakeJev::start().unwrap();
    jev_says_personal(&jev);
    let echo = Scripted::tools(
        "",
        &[(
            "t1",
            "proc_run",
            serde_json::json!({"argv": ["echo", PRIVATE]}),
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
    assert_eq!(landed(c, &result).await.state, "labelled");
    let ex = exchange_of(c, &result);
    let t = label_touches(c, crate::content::Flag::Personal);
    let turns: Vec<&str> = t.iter().map(|t| t.exchange.as_str()).collect();
    assert_eq!(turns, [ex.turn_id.as_str()]);
}
