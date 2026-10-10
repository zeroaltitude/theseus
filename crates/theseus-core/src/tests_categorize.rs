//! `categorize.v1` in shadow and the parked-task invariant (M5 step 28b;
//! design §3, "28b"), against the fake Jev: the `exchange_end` trigger;
//! candidates from the kinds table; accept and reject write labels and an
//! operator membership; a refused accept writes neither; a shared place's
//! session and a task are never judged; a slow or failing Jev changes no
//! turn; and parked detection on scripted states.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, FakeMode, Scripted as Jev};
use theseus_kernel::{Execution, PendingWake, Stopped, Wake};
use theseus_ontology::{Category, CategoryId, MemberList, Membership, Ontology, Origin, Record};
use theseus_protocol::{
    DiscordOrigin, OntologyCategoryAddParams, OntologyProposalAcceptParams,
    OntologyProposalRejectParams, OntologyProposalsParams, SessionKind, TurnSubmitResult,
};

use crate::approval::{Answerer, Surface};
use crate::bus::EventSink;
use crate::config::{JudgePackConfig, PackMode};
use crate::judge::categorize::{self, due, is_human, Mark, Trigger, EVERY, QUIET_MS};
use crate::ledger::LedgerRow;
use crate::node::{Node, Origin as NodeOrigin};
use crate::places::BoundPlace;
use crate::provider::{FakeProvider, Scripted};
use crate::rpc::{Core, Parts};
use crate::secrets::{Secret, SecretBoard};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::Config;

const OWNER: u64 = 271_828_182_845_904_523;
const ALICE: u64 = 222_222_222_222_222_222;
/// A guild channel the bindings file binds, shared.
const LAB: u64 = 314_159_265_358_979_323;

// ---------------------------------------------------------------- the mark

/// Two judgments of one session settled in one sink frame, the newer
/// first: the frame carries the newer mark alone, so the store's mark never
/// moves back (theseus-xkbs).
#[test]
fn a_frame_carries_each_sessions_newest_mark() {
    let mark = |judgment: &str, through: u64| Mark {
        judgment: judgment.into(),
        through,
        through_ms: 0,
        at_ms: 0,
    };
    let kept = categorize::newest_each(vec![
        ("ses_a".into(), mark("jdg_new", 20)),
        ("ses_b".into(), mark("jdg_b", 5)),
        ("ses_a".into(), mark("jdg_old", 10)),
    ]);
    assert_eq!(
        kept,
        vec![
            ("ses_a".to_string(), mark("jdg_new", 20)),
            ("ses_b".to_string(), mark("jdg_b", 5)),
        ]
    );
}

// ---------------------------------------------------------------- the trigger

/// A node of `origin` at minute `min`.
fn at(min: u64, n: Node) -> Node {
    let mut n = n;
    n.created_at_ms = 1_000_000_000 + min * 60_000;
    n
}

fn human(min: u64) -> Node {
    at(
        min,
        Node::user("ses_t", Some("turn_t"), "op", "the tide is high"),
    )
}

fn reply(min: u64) -> Node {
    at(
        min,
        Node::relayed("ses_t", Some("turn_t"), NodeOrigin::Agent, "model", "ok"),
    )
}

/// A message the model reads as the user's that no operator typed: a
/// wake's note, a task's report.
fn relayed(min: u64) -> Node {
    at(
        min,
        Node::relayed(
            "ses_t",
            None,
            NodeOrigin::Harness,
            "harness",
            "a wake came due",
        ),
    )
}

fn numbered(nodes: Vec<Node>) -> Vec<(u64, Node)> {
    nodes
        .into_iter()
        .enumerate()
        .map(|(i, n)| (i as u64 + 1, n))
        .collect()
}

/// Ten human messages since the last judgment bring one; nine do not, and
/// neither do the harness's messages, a task's report, or the replies.
#[test]
fn the_exchange_end_trigger_counts_human_messages_since_the_last() {
    let mut nodes = Vec::new();
    for i in 0..EVERY as u64 - 1 {
        nodes.push(human(i));
        nodes.push(reply(i));
        nodes.push(relayed(i));
    }
    assert_eq!(
        due(&numbered(nodes.clone()), None),
        None,
        "nine human messages"
    );
    assert_eq!(
        nodes.iter().filter(|n| !is_human(n)).count(),
        2 * (EVERY - 1),
        "the replies and the relayed messages are not human"
    );
    nodes.push(human(9));
    assert_eq!(due(&numbered(nodes), None), Some(Trigger::Count));
}

/// The first exchange end after 30 minutes' quiet brings one when anything
/// new arrived: the gap before the exchange's first human message (or the
/// mark's message, when the exchange is the first thing after it); a
/// session's first message is never quiet.
#[test]
fn the_exchange_end_trigger_reads_thirty_minutes_quiet() {
    let quiet = QUIET_MS / 60_000;
    // A reply, then a burst of two messages after the quiet.
    let after = numbered(vec![
        reply(0),
        human(quiet),
        human(quiet + 1),
        reply(quiet + 2),
    ]);
    assert_eq!(due(&after, None), Some(Trigger::Quiet));
    let after = numbered(vec![reply(0), human(quiet - 1), reply(quiet)]);
    assert_eq!(due(&after, None), None, "29 minutes is not quiet");
    // A session's first message: nothing before it.
    assert_eq!(due(&numbered(vec![human(0), reply(1)]), None), None);
    // Right after a judgment, the mark's message is what came before.
    let mark = Mark {
        judgment: "jdg_x".into(),
        through: 0,
        through_ms: 1_000_000_000,
        at_ms: 0,
    };
    let after = numbered(vec![human(quiet), reply(quiet + 1)]);
    assert_eq!(due(&after, Some(&mark)), Some(Trigger::Quiet));
    // Nothing new since the mark: nothing to judge, however quiet.
    assert_eq!(due(&numbered(vec![reply(quiet * 3)]), Some(&mark)), None);
}

// ---------------------------------------------------------- the candidates

fn topic(name: &str, desc: &str) -> Record {
    Record::Category(Category {
        id: CategoryId::new("topic", name).unwrap(),
        name: name.into(),
        parent: None,
        description: desc.into(),
        added_by: "the CLI".into(),
        retired_ms: None,
        handles: Vec::new(),
        merged_into: None,
    })
}

/// The candidates are the categories of the kinds the operator assigns (the
/// kinds table): topics, never a given kind's channel or person, and never
/// one named as the pack's own options. Past 50, the session's own topics
/// come first, then those with a description, each by name.
#[test]
fn candidates_come_from_the_kinds_table() {
    let mut o = Ontology::seeded();
    o.put(
        Record::Category(Category {
            id: CategoryId::new("channel", "314").unwrap(),
            name: "lab".into(),
            parent: None,
            description: "the lab channel".into(),
            added_by: "transport".into(),
            retired_ms: None,
            handles: Vec::new(),
            merged_into: None,
        }),
        Origin::Transport,
    )
    .unwrap();
    o.put(
        topic("harbor", "tides, moorings, the harbour master"),
        Origin::Operator,
    )
    .unwrap();
    o.put(topic("garden", "irrigation and planting"), Origin::Operator)
        .unwrap();
    o.put(
        topic("none", "a topic named as an option"),
        Origin::Operator,
    )
    .unwrap();
    let (c, m, left) = categorize::candidates(&o, "ses_a");
    let ids: Vec<&str> = c.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, ["garden", "harbor"], "topics alone, by name");
    assert_eq!(
        c[1].description,
        "harbor: tides, moorings, the harbour master"
    );
    assert!(m.is_empty());
    assert_eq!(left, 0);
    // Sixty topics; the session is in `zz-last`, and five have no description.
    for i in 0..60 {
        let desc = if i < 5 { "" } else { "a described topic" };
        o.put(topic(&format!("t{i:02}"), desc), Origin::Operator)
            .unwrap();
    }
    o.put(topic("zz-last", "the session's own"), Origin::Operator)
        .unwrap();
    o.put(
        Record::Members(MemberList {
            session: "ses_a".into(),
            kind: "topic".into(),
            members: vec![Membership::operator(
                CategoryId::new("topic", "zz-last").unwrap(),
                5,
            )],
        }),
        Origin::Operator,
    )
    .unwrap();
    let (c, m, left) = categorize::candidates(&o, "ses_a");
    assert_eq!(c.len(), categorize::CANDIDATES);
    assert_eq!(left, 63 - categorize::CANDIDATES, "63 topics, 50 offered");
    assert_eq!(c[0].id, "zz-last", "the session's own first");
    assert!(
        c.iter()
            .all(|t| !["t00", "t01", "t02", "t03", "t04"].contains(&t.id.as_str())),
        "the undescribed ones are left out first"
    );
    assert_eq!(m.len(), 1);
    assert_eq!(
        (m[0].id.as_str(), m[0].title.as_str()),
        ("zz-last", "zz-last")
    );
}

// ------------------------------------------------------- through the core

pub(crate) struct Rig {
    pub(crate) core: Arc<Core>,
    pub(crate) fake: Arc<FakeProvider>,
    _dir: tempfile::TempDir,
}

fn board() -> Arc<SecretBoard> {
    let b = SecretBoard::new(["jev_api_key".to_string()], Instant::now());
    b.publish(
        BTreeMap::from([(
            "jev_api_key".to_string(),
            Ok(Secret::new("jev-test-key-0123456789".into())),
        )]),
        "test",
    );
    b
}

fn texts(n: usize) -> Vec<Scripted> {
    (0..n)
        .map(|i| Scripted::text(&format!("Noted {i}.")))
        .collect()
}

/// A core with the judge on (against `jev`), every other wired pack off
/// (`loop.v1`, and the gate's, inbound's and compile's points) so each
/// judgment Jev sees is categorize's, the owner named, the owner's DM and a
/// shared channel bound, and the topics `harbor` and `garden` declared.
pub(crate) fn rig(jev: Option<&FakeJev>, turns: usize, tweak: impl FnOnce(&mut Config)) -> Rig {
    rig_topics(
        jev,
        turns,
        tweak,
        &[
            ("harbor", "Tides, moorings, the harbour master."),
            ("garden", "Irrigation, planting plans, seasonal chores."),
        ],
    )
}

/// [`rig`], with `topics` declared (none: an empty ontology).
fn rig_topics(
    jev: Option<&FakeJev>,
    turns: usize,
    tweak: impl FnOnce(&mut Config),
    topics: &[(&str, &str)],
) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.roots = vec![];
    cfg.places.owner = Some(vec![format!("discord:{OWNER}")]);
    if let Some(j) = jev {
        cfg.judge.enabled = true;
        cfg.judge.api_base = j.base();
        cfg.judge.connect_secs = 1;
        cfg.judge.total_secs = 1;
    }
    for (pack, _) in crate::judge::WIRED {
        if *pack != categorize::PACK {
            cfg.judge.packs.insert(
                (*pack).into(),
                JudgePackConfig {
                    mode: Some(PackMode::Off),
                    sample: None,
                    notices: None,
                },
            );
        }
    }
    tweak(&mut cfg);
    cfg.validate().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(texts(turns)));
    let mut p = Parts::for_tests(cfg, fake.clone(), store);
    p.secrets = board();
    let core = Core::build(p).unwrap();
    core.bind_places(vec![
        BoundPlace {
            target: format!("discord:dm:{OWNER}"),
            name: "DM @wren".into(),
            private: false,
            ..Default::default()
        },
        BoundPlace {
            target: format!("discord:channel:{LAB}"),
            name: "#lab".into(),
            private: false,
            ..Default::default()
        },
    ]);
    for &(name, desc) in topics {
        core.ontology_category_add(
            &OntologyCategoryAddParams {
                name: name.into(),
                description: Some(desc.into()),
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    }
    Rig {
        core,
        fake,
        _dir: dir,
    }
}

/// A conversation, posting to `place` when given (none: the CLI's).
pub(crate) fn session(core: &Core, place: Option<&str>) -> String {
    let r = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&r.session_id, &r).unwrap();
    if let Some(p) = place {
        core.outbox.bind_place(p, &r.session_id).unwrap();
    }
    r.session_id
}

async fn turn(core: &Arc<Core>, sid: &str, input: &str) -> TurnSubmitResult {
    let rec = core
        .store
        .get_session::<SessionRecord>(sid)
        .unwrap()
        .unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = EventSink::new(core.bus.clone(), sid, None);
    core.runner
        .run(TurnRequest {
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "op".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            prompt: None,
        })
        .await
        .unwrap()
}

/// `n` messages about moorings, one turn each; the last turn's result.
pub(crate) async fn moorings(core: &Arc<Core>, sid: &str, n: usize) -> TurnSubmitResult {
    let mut last = None;
    for i in 0..n {
        last = Some(
            turn(
                core,
                sid,
                &format!("Mooring line {i}: is it chafing at low tide?"),
            )
            .await,
        );
    }
    last.unwrap()
}

/// The `judge:categorize` scope's rows of `kind`.
fn rows(store: &Store, kind: &str) -> Vec<(theseus_store::Record, LedgerRow)> {
    store
        .scope_after("judge:categorize", 0)
        .unwrap()
        .into_iter()
        .map(|r| {
            let row: LedgerRow = r.decode().unwrap();
            (r, row)
        })
        .filter(|(_, row)| row.kind == kind)
        .collect()
}

/// Wait (on the runtime's timer) until `n` categorize judgments are recorded.
pub(crate) async fn until_judged(
    store: &Store,
    n: usize,
) -> Vec<(theseus_store::Record, LedgerRow)> {
    let t0 = Instant::now();
    loop {
        let r = rows(store, "judge.call");
        if r.len() >= n {
            return r;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "{} of {n} judgments recorded",
            r.len()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

pub(crate) fn harbor_at(jev: &FakeJev, option: &str, confidence: f64) {
    jev.script(
        "topic",
        Jev::Choice {
            option: option.into(),
            confidence,
        },
    );
}

/// The owner on Discord in `channel`, or someone else there.
fn on_discord(user: u64, channel: u64) -> Answerer {
    Answerer {
        label: format!("discord:{user}"),
        surface: Surface::Discord,
        discord: Some(DiscordOrigin {
            user_id: user.to_string(),
            channel_id: channel.to_string(),
            guild_id: Some("27182818".to_string()),
        }),
    }
}

/// Ten messages about moorings in a CLI session bring one `categorize.v1`
/// judgment in shadow, after the tenth: its row keyed and scoped, its state
/// the title and the ten messages without the memberships, the two topics
/// its options; and `ontology.proposals` lists `harbor` with its confidence.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ten_human_messages_bring_one_judgment_and_its_proposal() {
    let jev = FakeJev::start().unwrap();
    harbor_at(&jev, "harbor", 0.93);
    let r = rig(Some(&jev), 11, |_| {});
    let sid = session(&r.core, None);
    moorings(&r.core, &sid, EVERY - 1).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(rows(&r.core.store, "judge.call").is_empty(), "nine: none");
    assert_eq!(jev.connections(), 0);
    let res = moorings(&r.core, &sid, 1).await;
    let judged = until_judged(&r.core.store, 1).await;
    let (rec, row) = &judged[0];
    let d = &row.data;
    assert_eq!(rec.key.as_deref(), d["id"].as_str(), "keyed by its id");
    assert_eq!(row.session_id.as_deref(), Some(sid.as_str()));
    assert_eq!(
        (d["pack"].as_str(), d["mode"].as_str(), d["point"].as_str()),
        (Some("categorize.v1"), Some("shadow"), Some("exchange_end"))
    );
    assert_eq!(d["context"]["trigger"], "count");
    assert_eq!(d["context"]["turn"], json!(res.turn_id));
    assert_eq!(d["context"]["candidates"], 2);
    let mark: Mark = r
        .core
        .store
        .get_meta(&format!("{}{sid}", categorize::MARK_PREFIX))
        .unwrap()
        .unwrap();
    assert_eq!(
        mark.judgment,
        d["id"].as_str().unwrap(),
        "the mark names it"
    );
    let digest = d["state"]["sha256"].as_str().unwrap();
    let state: Value =
        serde_json::from_slice(&std::fs::read(r.core.store.blobs().path(digest)).unwrap()).unwrap();
    let msgs = state["recent_human_messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 10);
    assert_eq!(msgs[9], "Mooring line 0: is it chafing at low tide?");
    assert!(state.get("current_memberships").is_none());
    // Jev was offered the two topics, new_topic, and none.
    let seen = jev.seen();
    assert_eq!(seen.len(), 1);
    let body = serde_json::to_string(&seen[0].body).unwrap();
    for id in ["harbor", "garden", "new_topic", "none"] {
        assert!(body.contains(&format!("\"{id}\"")), "{id} offered: {body}");
    }
    let p = r
        .core
        .ontology_proposals(&OntologyProposalsParams::default())
        .unwrap();
    assert_eq!(p.proposals.len(), 1);
    let pr = &p.proposals[0];
    assert_eq!(
        (pr.topic.as_deref(), pr.topic_name.as_deref(), pr.new_topic),
        (Some("topic:harbor"), Some("harbor"), false)
    );
    assert!((pr.confidence - 0.93).abs() < 1e-6, "{}", pr.confidence);
    assert_eq!(pr.band, "act");
    // An eleventh message brings no second judgment: nine more are needed.
    moorings(&r.core, &sid, 1).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(rows(&r.core.store, "judge.call").len(), 1);
    let h = r.core.health().judge.unwrap();
    assert!(
        h.packs.contains(&"categorize.v1: shadow".to_string()),
        "{:?}",
        h.packs
    );
}

/// The session records the point's decisions have read, once a decision
/// has run since `before` (on the runtime's timer).
async fn read_since(core: &Core, before: u64) -> u64 {
    let t0 = Instant::now();
    loop {
        let now = core.runner.judge.categorize_records_read();
        if now > before {
            // A decision reads once; let a second (there is none) show.
            tokio::time::sleep(Duration::from_millis(100)).await;
            return core.runner.judge.categorize_records_read() - before;
        }
        assert!(t0.elapsed() < Duration::from_secs(20), "no decision ran");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn mark_of(core: &Core, sid: &str) -> Mark {
    core.store
        .get_meta(&format!("{}{sid}", categorize::MARK_PREFIX))
        .unwrap()
        .unwrap()
}

/// On an empty ontology a due exchange end still judges (theseus-ext.12):
/// the Choice is `new_topic` and `none`, the mark moves, and the answer is
/// a proposal whose accept names the ontology's first topic. After the
/// mark, a decision reads only the records after it, and a quiet judgment's
/// input is the human messages since it, never the session's whole history
/// (theseus-gky0).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_empty_ontology_still_judges_and_reads_only_after_the_mark() {
    let jev = FakeJev::start().unwrap();
    harbor_at(&jev, "new_topic", 0.9);
    let r = rig_topics(Some(&jev), 20, |_| {}, &[]);
    let sid = session(&r.core, None);
    moorings(&r.core, &sid, EVERY).await;
    let judged = until_judged(&r.core.store, 1).await;
    let d = &judged[0].1.data;
    assert_eq!(d["context"]["candidates"], 0);
    let body = serde_json::to_string(&jev.seen()[0].body).unwrap();
    for id in ["new_topic", "none"] {
        assert!(body.contains(&format!("\"{id}\"")), "{id} offered: {body}");
    }
    assert!(!body.contains("\"harbor\""), "no topic is declared");
    let mark = mark_of(&r.core, &sid);
    assert_eq!(mark.judgment, d["id"].as_str().unwrap(), "the mark moved");

    // The mark moved past the tenth message's reply, and its message 31
    // minutes back: the next message is quiet, and its judgment reads the
    // records after the mark, the one message among them, and nothing from
    // before.
    let (last, _) = r.core.store.session_nodes(&sid).unwrap().pop().unwrap();
    let back = Mark {
        through: last,
        through_ms: mark.through_ms - QUIET_MS - 60_000,
        ..mark.clone()
    };
    let mark = back.clone();
    r.core
        .store
        .put_meta(&format!("{}{sid}", categorize::MARK_PREFIX), &back)
        .unwrap();
    let before = r.core.runner.judge.categorize_records_read();
    turn(&r.core, &sid, "Fender placement on the north pontoon.").await;
    let judged = until_judged(&r.core.store, 2).await;
    let read = read_since(&r.core, before).await;
    let after_mark = r.core.store.scope_after(&sid, mark.through).unwrap().len() as u64;
    let all = r.core.store.scope_after(&sid, 0).unwrap().len() as u64;
    assert!(
        read > 0 && read <= after_mark && after_mark < all,
        "read {read}: {after_mark} after the mark, {all} in all"
    );
    let d = &judged[1].1.data;
    assert_eq!(d["context"]["trigger"], "quiet");
    let state: Value = serde_json::from_slice(
        &std::fs::read(
            r.core
                .store
                .blobs()
                .path(d["state"]["sha256"].as_str().unwrap()),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        state["recent_human_messages"],
        json!(["Fender placement on the north pontoon."]),
        "the messages since the mark alone"
    );

    // An exchange end that is not due reads only what follows the new mark.
    let mark = mark_of(&r.core, &sid);
    let before = r.core.runner.judge.categorize_records_read();
    turn(&r.core, &sid, "And the stern line?").await;
    let read = read_since(&r.core, before).await;
    let after_mark = r.core.store.scope_after(&sid, mark.through).unwrap().len() as u64;
    assert!(
        read > 0 && read <= after_mark,
        "read {read} of {after_mark}"
    );
    assert_eq!(rows(&r.core.store, "judge.call").len(), 2, "not due");

    // The proposal is new_topic, and its accept makes the first topic.
    let p = r
        .core
        .ontology_proposals(&OntologyProposalsParams::default())
        .unwrap()
        .proposals;
    let newest = p.iter().find(|p| p.session_id == sid).unwrap();
    assert!(newest.new_topic && newest.topic.is_none());
    let done = r
        .core
        .ontology_proposal_accept(
            &OntologyProposalAcceptParams {
                judgment: newest.judgment.clone(),
                topic: Some("garden".into()),
                description: Some("Moorings, as it happens.".into()),
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    assert_eq!(done.topic.as_deref(), Some("topic:garden"));
    let o = r.core.runner.ontology.snapshot(&r.core.store).unwrap();
    let (c, _, _) = categorize::candidates(&o, &sid);
    assert_eq!(c.len(), 1, "the ontology's first topic");
}

/// Three sessions of ten messages about moorings, judged `harbor`,
/// `new_topic`, and `garden`; their proposals listed newest first. Returns
/// the rig, the fake, and each session with its judgment.
async fn three_proposals() -> (Rig, FakeJev, [(String, String); 3]) {
    let jev = FakeJev::start().unwrap();
    harbor_at(&jev, "harbor", 0.91);
    let r = rig(Some(&jev), 30, |_| {});
    let a = session(&r.core, None);
    moorings(&r.core, &a, EVERY).await;
    until_judged(&r.core.store, 1).await;
    harbor_at(&jev, "new_topic", 0.88);
    let b = session(&r.core, None);
    moorings(&r.core, &b, EVERY).await;
    until_judged(&r.core.store, 2).await;
    harbor_at(&jev, "garden", 0.7);
    let c = session(&r.core, None);
    moorings(&r.core, &c, EVERY).await;
    until_judged(&r.core.store, 3).await;
    let all = r
        .core
        .ontology_proposals(&OntologyProposalsParams::default())
        .unwrap()
        .proposals;
    assert_eq!(all.len(), 3);
    let sessions: Vec<&str> = all.iter().map(|p| p.session_id.as_str()).collect();
    assert_eq!(
        sessions,
        [c.as_str(), b.as_str(), a.as_str()],
        "newest first"
    );
    let of = |s: &str| -> String {
        all.iter()
            .find(|p| p.session_id == s)
            .unwrap()
            .judgment
            .clone()
    };

    let (ja, jb, jc) = (of(&a), of(&b), of(&c));
    (r, jev, [(a, ja), (b, jb), (c, jc)])
}

/// Accept writes the operator's membership and the judgment's label in one
/// frame, and leaves the proposals; a second answer is refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accept_writes_an_operator_membership_and_its_label_in_one_frame() {
    let (r, _jev, [(a, ja), _, _]) = three_proposals().await;
    let of = |_: &str| ja.clone();
    // Accept harbor: the membership and the label, one frame.
    let before = r.core.store.stats().unwrap().frames_appended;
    let done = r
        .core
        .ontology_proposal_accept(
            &OntologyProposalAcceptParams {
                judgment: of(&a),
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    assert_eq!(
        r.core.store.stats().unwrap().frames_appended - before,
        1,
        "one frame"
    );
    assert_eq!(done.topic.as_deref(), Some("topic:harbor"));
    let m = &done.memberships[0];
    assert_eq!(
        (m.category.as_str(), m.origin.as_str()),
        ("topic:harbor", "operator")
    );
    let labels = rows(&r.core.store, "judge.label");
    assert_eq!(labels.len(), 1);
    let (rec, l) = &labels[0];
    assert_eq!(rec.key.as_deref(), Some(done.label_id.as_str()));
    assert!(done.label_id.starts_with("lbl_"));
    assert_eq!(
        (
            l.data["judgment"].as_str(),
            l.data["label"].as_str(),
            l.data["source"].as_str(),
            l.data["question"].as_str(),
            l.data["topic"].as_str()
        ),
        (
            Some(of(&a).as_str()),
            Some("accepted"),
            Some("operator"),
            Some("topic"),
            Some("topic:harbor")
        )
    );
    assert_eq!(l.session_id.as_deref(), Some(a.as_str()));
    // Answered: it is gone from the proposals, and a second answer refused.
    let left = r
        .core
        .ontology_proposals(&OntologyProposalsParams::default())
        .unwrap()
        .proposals;
    assert!(left.iter().all(|p| p.session_id != a));
    assert!(r
        .core
        .ontology_proposal_reject(
            &OntologyProposalRejectParams {
                judgment: of(&a),
                ..Default::default()
            },
            "the CLI"
        )
        .unwrap_err()
        .to_string()
        .contains("answered already"));
}

/// A `new_topic` accept needs the topic named, and makes it in the frame
/// that writes the membership and the label; a reject writes its label
/// alone; neither leaves a proposal.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_new_topic_accept_makes_its_topic_and_a_reject_writes_its_label_alone() {
    let (r, _jev, [_, (b, jb), (c, jc)]) = three_proposals().await;
    let of = |s: &str| if s == b { jb.clone() } else { jc.clone() };
    // new_topic: --topic is needed, and names a topic made in the frame.
    let need = r.core.ontology_proposal_accept(
        &OntologyProposalAcceptParams {
            judgment: of(&b),
            ..Default::default()
        },
        "the CLI",
    );
    assert!(need.unwrap_err().to_string().contains("--topic"));
    let before = r.core.store.stats().unwrap().frames_appended;
    let done = r
        .core
        .ontology_proposal_accept(
            &OntologyProposalAcceptParams {
                judgment: of(&b),
                topic: Some("moorings".into()),
                description: Some("Mooring lines and their chafe.".into()),
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    assert_eq!(
        r.core.store.stats().unwrap().frames_appended - before,
        1,
        "one frame"
    );
    assert_eq!(done.topic.as_deref(), Some("topic:moorings"));
    let o = r.core.runner.ontology.snapshot(&r.core.store).unwrap();
    let made = o
        .category(&CategoryId::new("topic", "moorings").unwrap())
        .unwrap();
    assert_eq!(made.description, "Mooring lines and their chafe.");

    // Reject: its label, and no membership.
    let done = r
        .core
        .ontology_proposal_reject(
            &OntologyProposalRejectParams {
                judgment: of(&c),
                note: Some("about moorings, not the garden".into()),
                ..Default::default()
            },
            "the CLI",
        )
        .unwrap();
    assert_eq!(done.label, "rejected");
    assert!(done.memberships.is_empty());
    let labels = rows(&r.core.store, "judge.label");
    assert_eq!(labels.len(), 2);
    assert_eq!(labels[1].1.data["label"], "rejected");
    assert_eq!(labels[1].1.data["note"], "about moorings, not the garden");
    let left = r
        .core
        .ontology_proposals(&OntologyProposalsParams::default())
        .unwrap()
        .proposals;
    assert_eq!(left.len(), 1, "the first session's, unanswered here");
    assert!(left.iter().all(|p| p.session_id != b && p.session_id != c));
}

/// An accept from someone other than the owner, or from a shared place, is
/// refused through `judge_act` and writes neither a membership nor a label;
/// the proposal stays.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_accept_writes_nothing() {
    let jev = FakeJev::start().unwrap();
    harbor_at(&jev, "harbor", 0.93);
    let r = rig(Some(&jev), 10, |_| {});
    let sid = session(&r.core, None);
    moorings(&r.core, &sid, EVERY).await;
    until_judged(&r.core.store, 1).await;
    let judgment = r
        .core
        .ontology_proposals(&OntologyProposalsParams::default())
        .unwrap()
        .proposals[0]
        .judgment
        .clone();
    for who in [on_discord(ALICE, LAB), on_discord(OWNER, LAB)] {
        let e = r
            .core
            .ontology_proposal_accept(
                &OntologyProposalAcceptParams {
                    judgment: judgment.clone(),
                    ..Default::default()
                },
                who.clone(),
            )
            .unwrap_err();
        assert!(
            e.downcast_ref::<crate::approval::Refusal>().is_some(),
            "{}: {e:#}",
            who.label
        );
        let e = r.core.ontology_proposal_reject(
            &OntologyProposalRejectParams {
                judgment: judgment.clone(),
                ..Default::default()
            },
            who,
        );
        assert!(e.is_err());
    }
    assert!(rows(&r.core.store, "judge.label").is_empty(), "no label");
    let o = r.core.runner.ontology.snapshot(&r.core.store).unwrap();
    assert!(o.memberships(&sid).is_empty(), "no membership");
    assert_eq!(
        r.core
            .ontology_proposals(&OntologyProposalsParams::default())
            .unwrap()
            .proposals
            .len(),
        1,
        "the proposal stays"
    );
}

/// A shared place's session is never judged (its compile walk reads no
/// interpreted membership), however many messages arrive; nor is a task
/// (its one human message is its brief), at the very exchange end that
/// judges the same conversation.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_shared_place_and_a_task_are_never_judged() {
    let jev = FakeJev::start().unwrap();
    harbor_at(&jev, "harbor", 0.93);
    let r = rig(Some(&jev), 12, |_| {});
    let shared = session(&r.core, Some(&format!("channel:{LAB}")));
    moorings(&r.core, &shared, EVERY + 2).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(rows(&r.core.store, "judge.call").is_empty());
    assert_eq!(jev.connections(), 0);

    // The same exchange end, due, handed over as a task's and as a
    // conversation's: only the conversation's is judged.
    let off = rig(Some(&jev), 10, |c| {
        c.judge.packs.insert(
            "categorize.v1".into(),
            JudgePackConfig {
                mode: Some(PackMode::Off),
                sample: None,
                notices: None,
            },
        );
    });
    let sid = session(&off.core, None);
    let res = moorings(&off.core, &sid, EVERY).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        rows(&off.core.store, "judge.call").is_empty(),
        "its pack is off"
    );
    let mut cfg = off.core.cfg.judge.clone();
    cfg.packs.remove("categorize.v1");
    let svc = crate::judge::JudgeService::with_parts(
        cfg,
        off.core.store.clone(),
        board(),
        Arc::new(crate::scrub::Scrubber::default()),
        Duration::from_millis(20),
        crate::catalog::judge_prices(),
    );
    svc.attach(&off.core);
    svc.after_turn(&res, true);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        rows(&off.core.store, "judge.call").is_empty(),
        "a task's: none"
    );
    assert_eq!(jev.connections(), 0);
    svc.after_turn(&res, false);
    until_judged(&off.core.store, 1).await;
}

/// The tenth turn, the one that brings a judgment, is the same with the
/// judge off as against a Jev that is down, slow, or malformed: its request
/// bytes, its result, its frames, and its time (well under the slow Jev's).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_slow_or_failing_jev_changes_no_turn() {
    let base = rig(None, EVERY, |_| {});
    let sid = session(&base.core, None);
    moorings(&base.core, &sid, EVERY - 1).await;
    let want = moorings(&base.core, &sid, 1).await;
    let want_req = serde_json::to_value(base.fake.requests().last().unwrap()).unwrap();
    let frames = |r: &TurnSubmitResult| r.trace.as_ref().unwrap().attrs["frames"].as_u64();
    for (mode, class) in [
        (FakeMode::Down, "network"),
        (FakeMode::Slow(Duration::from_secs(10)), "timeout"),
        (FakeMode::Malformed, "malformed"),
    ] {
        let jev = FakeJev::start().unwrap();
        jev.set_mode(mode.clone());
        let r = rig(Some(&jev), EVERY, |c| c.judge.total_secs = 5);
        let sid = session(&r.core, None);
        moorings(&r.core, &sid, EVERY - 1).await;
        let t0 = Instant::now();
        let res = moorings(&r.core, &sid, 1).await;
        let took = t0.elapsed();
        assert!(
            took < Duration::from_secs(3),
            "{mode:?}: the turn took {took:?}"
        );
        assert_eq!(
            (&res.output, &res.stop_reason, res.loops, res.cost_usd),
            (&want.output, &want.stop_reason, want.loops, want.cost_usd),
            "{mode:?}"
        );
        assert_eq!(frames(&res), frames(&want), "{mode:?}: the turn's frames");
        assert!(frames(&res).is_some_and(|f| f <= 5), "{mode:?}");
        let req = serde_json::to_value(r.fake.requests().last().unwrap()).unwrap();
        assert_eq!(req, want_req, "{mode:?}: the turn's request");
        let rows = until_judged(&r.core.store, 1).await;
        assert_eq!(rows[0].1.data["outcome"]["class"], class, "{mode:?}");
        assert!(r
            .core
            .ontology_proposals(&OntologyProposalsParams::default())
            .unwrap()
            .proposals
            .is_empty());
    }
}

// --------------------------------------------------------- parked tasks

const H: u64 = 3_600_000;
const NOW: u64 = 100 * H;

fn task(state: &str, wake: Option<Wake>) -> Execution {
    let mut e: Execution = serde_json::from_value(json!({
        "id": "exe_dock", "schema": 2, "session_id": "ses_0000dock01", "kind": "task",
        "state": state, "authority": {"principal": "operator"},
        "budget": theseus_kernel::Budget::new(1_000_000),
        "turns": 1, "interrupted": 0, "created_at_ms": 0, "updated_at_ms": NOW - 2 * H,
    }))
    .unwrap();
    e.wake = wake;
    e
}

fn blocked_by(e: &Execution, asked: Option<u64>) -> Option<&'static str> {
    crate::parked::blocker(e, |_| asked, NOW).map(|(b, _, _)| b)
}

/// Parked detection on scripted states: a task that can progress by itself
/// (a turn, a queue place, a job, a due time, its own wake, a report or a
/// resume for the driver, a question younger than a day) is not listed;
/// one waiting on input, stopped, on a day-old question, blocked, or on
/// nothing is, with its blocker. A conversation is never a parked task.
#[test]
fn parked_tasks_are_detected_on_scripted_states() {
    use theseus_kernel::Wake::*;
    let fresh = Some(NOW - 23 * H);
    let stale = Some(NOW - 25 * H);
    let can = [
        task("running", None),
        task("queued", None),
        task(
            "waiting",
            Some(Actions {
                correlation_ids: vec!["act_job".into()],
            }),
        ),
        task("waiting", Some(DueAt { at_ms: NOW + H })),
        task(
            "waiting",
            Some(Execution {
                execution_id: "exe_other".into(),
            }),
        ),
    ];
    for e in &can {
        assert_eq!(blocked_by(e, None), None, "{:?}", e.state);
    }
    let confirm = task(
        "waiting",
        Some(Confirm {
            confirm_id: "act_ask".into(),
        }),
    );
    assert_eq!(blocked_by(&confirm, fresh), None, "a question 23 h old");
    assert_eq!(blocked_by(&confirm, stale), Some("approval"));
    let (_, detail, since) = crate::parked::blocker(&confirm, |_| stale, NOW).unwrap();
    assert_eq!(detail, "an approval unanswered for 25 h");
    assert_eq!(since, NOW - 25 * H);
    let budget = task(
        "waiting",
        Some(Budget {
            correlation_id: "act_reset".into(),
        }),
    );
    assert_eq!(blocked_by(&budget, fresh), None);
    assert_eq!(blocked_by(&budget, stale), Some("budget"));
    // Unreadable, a question's age is the execution's last change (2 h).
    assert_eq!(blocked_by(&confirm, None), None);

    let mut input = task("waiting", Some(Input));
    assert_eq!(blocked_by(&input, None), Some("input"));
    // Its own pending wake (37b) continues it: not parked.
    input.wakes.push(PendingWake {
        id: "wak_1".into(),
        due_at_ms: NOW + H,
        note: "check the tide".into(),
        set_at_ms: 0,
        by: "act_w".into(),
        target: None,
        repeat: None,
        occurrence: 0,
    });
    assert_eq!(blocked_by(&input, None), None, "its own wake");
    let mut stopped = task("waiting", Some(Input));
    stopped.stopped = Some(Stopped {
        by: "the CLI".into(),
        at_ms: 0,
        turn: 1,
    });
    assert_eq!(blocked_by(&stopped, None), Some("stopped"));
    let mut resumes = task("waiting", Some(Input));
    resumes.resume_pending = true;
    assert_eq!(blocked_by(&resumes, None), None, "the driver resumes it");
    let mut blocked = task("blocked", None);
    blocked.ended_reason = Some("its tool is gone".into());
    assert_eq!(blocked_by(&blocked, None), Some("blocked"));
    assert_eq!(blocked_by(&task("waiting", None), None), Some("nothing"));
    assert_eq!(blocked_by(&task("complete", None), None), None, "ended");
    let mut convo = task("waiting", Some(Input));
    convo.kind = theseus_kernel::SessionKind::Conversation;
    assert_eq!(blocked_by(&convo, None), None, "a conversation");

    let listed = crate::parked::parked(
        &[can[0].clone(), confirm, stopped],
        |_| stale,
        |_| Some("Sweep the dock".into()),
        NOW,
    );
    let got: Vec<(&str, &str)> = listed
        .iter()
        .map(|p| (p.blocker.as_str(), p.short.as_str()))
        .collect();
    assert_eq!(
        got,
        [("approval", "dock01"), ("stopped", "dock01")],
        "oldest first"
    );
    assert_eq!(listed[0].title.as_deref(), Some("Sweep the dock"));
}

/// Health's `tasks.parked` reads the open executions: none on a store with
/// no task, and a conversation parked on input is not listed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn health_lists_no_parked_task_for_a_conversation() {
    let r = rig(None, 1, |_| {});
    let sid = session(&r.core, None);
    turn(&r.core, &sid, "hello").await;
    assert_eq!(r.core.health().tasks.unwrap().parked, vec![]);
}
