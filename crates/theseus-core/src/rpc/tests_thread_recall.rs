//! Threads in recall (theseus-f2g0) through the whole core: a note from
//! another thread names its thread; the distance withholds what is under the
//! bar; naming a thread waives it; a sealed thread's notes never come in
//! ambiently (100 seeded recalls), nor does one whose seal is proposed; "not
//! needed" is remembered until the thread's content changes; a task and a
//! split inherit the seal; an unseal is the owner's, from a private place;
//! the seal's report lists what had crossed; and `thread.read` of a sealed
//! thread asks the owner first.

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use theseus_protocol::index::{IndexHit, IndexQueryResult, IndexSourceRank};
use theseus_protocol::memory::RecallManifest;
use theseus_protocol::threads::{ThreadEditParams, ThreadNewParams, ThreadSealParams};

use super::Core;
use crate::approval::{Answerer, Surface};
use crate::config::memory::MemoryArm;
use crate::config::MemoryMode;
use crate::node::{Body, Node};
use crate::policy::{Decision, Posture};
use crate::recall::{Ask, AskFuture, Scene};
use crate::tests_recall::{rig_with, session, turn, Rig};

pub(super) fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    }
}

pub(super) fn canary() -> Rig {
    rig_with(MemoryMode::Canary, |c| c.memory.canary_fraction = 1.0)
}

pub(super) fn new_thread(c: &Core, slug: &str, title: &str) {
    c.thread_new(
        &ThreadNewParams {
            slug: slug.into(),
            title: Some(title.into()),
            ..Default::default()
        },
        cli(),
    )
    .unwrap();
}

/// The thread's session, opened and bound.
pub(super) fn thread_session(c: &Core, slug: &str) -> String {
    c.thread_session(slug, cli()).unwrap().session_id
}

/// A note in `sid`.
pub(super) fn say(c: &Core, sid: &str, text: &str) -> String {
    let n = Node::user(sid, None, "test", text);
    c.store.append(&[n.record().unwrap()]).unwrap();
    n.id
}

/// A stand-in index that answers `picks`: (session, the index of its node,
/// the fused score), in order.
pub(super) fn scored(c: &Arc<Core>, picks: Vec<(String, usize, f64)>) -> Ask {
    scored_with(c, picks, Vec::new())
}

/// The same, each hit with the entities its chunk holds and the question's it
/// shares (`entities[i]` for `picks[i]`; none for a hit past the list).
pub(super) fn scored_with(
    c: &Arc<Core>,
    picks: Vec<(String, usize, f64)>,
    entities: Vec<(Vec<String>, Vec<String>)>,
) -> Ask {
    let store = c.store.clone();
    Arc::new(move |_| -> AskFuture {
        let hits = picks
            .iter()
            .enumerate()
            .map(|(i, (sid, k, fused))| {
                let (position, n) = store.session_nodes(sid).unwrap().remove(*k);
                let (held, shared) = entities.get(i).cloned().unwrap_or_default();
                IndexHit {
                    text: crate::recall::text_of(&n),
                    node_id: n.id,
                    chunk: 0,
                    session_id: n.session_id,
                    position,
                    kind: "user_message".into(),
                    origin: "operator".into(),
                    author: None,
                    place: None,
                    tool: None,
                    time_ms: 0,
                    external: false,
                    entities_matched: shared,
                    entities: held,
                    sources: [(
                        "bm25".to_string(),
                        IndexSourceRank {
                            rank: i + 1,
                            score: 1.0,
                        },
                    )]
                    .into(),
                    fused: *fused,
                }
            })
            .collect();
        Box::pin(async move {
            Ok(IndexQueryResult {
                hits,
                indexed_through: 0,
                lag: Default::default(),
                timings: Default::default(),
                skipped: Default::default(),
                weights: Default::default(),
            })
        })
    })
}

/// A turn's recall as the pipeline runs it for `asker` saying `message`
/// (ambient: the distance applies in a threaded session), without a turn.
pub(super) async fn ambient(c: &Arc<Core>, asker: &str, message: &str, ask: Ask) -> RecallManifest {
    let memory = &c.runner.memory;
    memory.set_ask(ask);
    let mut begun = memory.begin(
        message.to_string(),
        None,
        40,
        MemoryArm::Baseline,
        Duration::from_millis(250),
    );
    let answer = begun.answer().await;
    let scene = Scene {
        mode: "live",
        session_id: Some(asker),
        turn_id: None,
        place: c.runner.place_of(asker),
        in_context: Default::default(),
        labeled: Default::default(),
        budget_tokens: None,
        max_items: None,
        science: memory.science_for(MemoryArm::Baseline),
        activation: None,
        threads: c.runner.thread_scene(Some(asker), message, true),
        persona: None,
        category: None,
    };
    memory.manifest(
        &scene,
        &begun,
        answer,
        |s| c.runner.place_of(s),
        |_| Vec::new(),
        true,
    )
}

pub(super) fn nodes_of(m: &RecallManifest) -> Vec<&str> {
    m.admitted.iter().map(|a| a.session_id.as_str()).collect()
}

/// A recalled note from another thread begins its header with the thread's
/// title, the day and the position, as a canary turn writes it; a note from
/// an unthreaded session has no such mark.
#[tokio::test]
async fn a_cross_thread_hit_names_its_thread() {
    let r = canary();
    let c = &r.core;
    new_thread(c, "security-fix", "security fix");
    new_thread(c, "pr-round", "PR round");
    let fix = thread_session(c, "security-fix");
    say(c, &fix, "The embargoed fix ports to the tls-pool crate.");
    let pr = thread_session(c, "pr-round");
    let plain = session(c, None, &["The unthreaded note about the tls-pool crate."]);
    c.runner.memory.set_ask(scored(
        c,
        vec![(fix.clone(), 0, 0.0164), (plain.clone(), 0, 0.0163)],
    ));
    let res = turn(c, &pr, "What about the tls-pool crate?").await;
    assert_eq!(res.recalled, 2);
    let nodes = c.store.session_nodes(&pr).unwrap();
    let items = nodes
        .iter()
        .find_map(|(_, n)| match &n.body {
            Body::Recall { items, .. } => Some(items.clone()),
            _ => None,
        })
        .expect("a Recall node");
    let from_fix = items.iter().find(|i| i.session_id == fix).unwrap();
    let mark = format!(
        "[from thread «security fix», {}, @{}]",
        day_of(c, &from_fix.node_id),
        from_fix.position
    );
    assert!(
        from_fix.header.contains(&mark),
        "{} / {mark}",
        from_fix.header
    );
    let from_plain = items.iter().find(|i| i.session_id == plain).unwrap();
    assert!(
        !from_plain.header.contains("from thread"),
        "{}",
        from_plain.header
    );
}

fn day_of(c: &Core, node: &str) -> String {
    let (_, n) = c.store.get_node(node).unwrap().unwrap();
    let (y, m, d) = crate::wake::civil_from_days((n.created_at_ms / 1000 / 86_400) as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Another thread's note under the bar is withheld (`thread`), one above it
/// is admitted; naming the thread waives the distance for the one under it.
#[tokio::test]
async fn the_bar_withholds_a_far_thread_and_naming_it_waives_that() {
    let r = canary();
    let c = &r.core;
    new_thread(c, "security-fix", "security fix");
    new_thread(c, "pr-round", "PR round");
    let fix = thread_session(c, "security-fix");
    say(c, &fix, "near");
    say(c, &fix, "far");
    let pr = thread_session(c, "pr-round");
    let plain = session(c, None, &["top"]);
    let picks = vec![
        (plain.clone(), 0, 0.040),
        (fix.clone(), 0, 0.035),
        (fix.clone(), 1, 0.020),
    ];
    let m = ambient(c, &pr, "tell me more", scored(c, picks.clone())).await;
    assert_eq!(m.admitted.len(), 2, "{m:?}");
    assert_eq!(m.drops.get("thread"), Some(&1));
    assert_eq!(m.dropped[0].reason, "thread_bar");
    let m = ambient(
        c,
        &pr,
        "tell me more about the security fix",
        scored(c, picks),
    )
    .await;
    assert_eq!(m.admitted.len(), 3, "naming the thread waives the bar");
}

/// Over 100 seeded recalls (the asker, the message and the scores vary), a
/// sealed thread's notes never come in, and the manifest counts them.
#[tokio::test]
async fn a_sealed_threads_notes_never_come_in_ambiently() {
    let r = canary();
    let c = &r.core;
    new_thread(c, "embargo", "embargoed fix");
    new_thread(c, "pr-round", "PR round");
    new_thread(c, "docs", "docs sweep");
    let sealed = thread_session(c, "embargo");
    say(c, &sealed, "the sealed fix details");
    c.thread_seal(
        &ThreadSealParams {
            slug: "embargo".into(),
            reason: Some("embargoed".into()),
            ..Default::default()
        },
        cli(),
    )
    .unwrap();
    let pr = thread_session(c, "pr-round");
    say(c, &pr, "a pr note");
    let docs = thread_session(c, "docs");
    say(c, &docs, "a docs note");
    let free = session(c, None, &["an unthreaded note"]);
    let askers = [pr.clone(), docs.clone(), free.clone()];
    let mut x: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    for i in 0..100 {
        let asker = &askers[(next() % 3) as usize];
        let message = match next() % 3 {
            0 => "what did the embargo say about embargoed fix",
            1 => "pr-round and docs sweep",
            _ => "anything",
        };
        let score = |n: u64| 0.001 + (n % 49) as f64 / 1000.0;
        let picks = vec![
            (sealed.clone(), 0, score(next())),
            (pr.clone(), 0, score(next())),
            (docs.clone(), 0, score(next())),
            (free.clone(), 0, score(next())),
        ];
        let m = ambient(c, asker, message, scored(c, picks)).await;
        assert!(
            !nodes_of(&m).contains(&sealed.as_str()),
            "recall {i} by {asker} brought in a sealed note: {m:?}"
        );
        assert_eq!(m.drops.get("sealed"), Some(&1), "recall {i}: {:?}", m.drops);
    }
    // The sealed thread's own session reads itself.
    let own = ambient(
        c,
        &sealed,
        "the sealed fix",
        scored(c, vec![(sealed.clone(), 0, 0.04)]),
    )
    .await;
    assert_eq!(own.admitted.len() + own.dropped.len(), 1);
    assert!(!own.drops.contains_key("sealed"));
}

/// A proposed seal holds the thread out of other threads' recall until the
/// owner answers; "not needed" releases it and is remembered, one proposal
/// until the thread's content changes.
#[tokio::test]
async fn a_pending_seal_holds_the_thread_out_and_not_needed_is_remembered() {
    let r = canary();
    let c = &r.core;
    new_thread(c, "security-fix", "security fix");
    new_thread(c, "pr-round", "PR round");
    let fix = thread_session(c, "security-fix");
    say(c, &fix, "the fix");
    let pr = thread_session(c, "pr-round");
    let pick = || scored(c, vec![(fix.clone(), 0, 0.04)]);
    let before = ambient(c, &pr, "x", pick()).await;
    assert_eq!(before.admitted.len(), 1);

    assert!(c
        .thread_recommend_seal("security-fix", "it names a CVE")
        .unwrap());
    let held = ambient(c, &pr, "x", pick()).await;
    assert!(held.admitted.is_empty());
    assert_eq!(held.drops.get("sealed"), Some(&1));
    assert!(
        !c.thread_recommend_seal("security-fix", "again").unwrap(),
        "one proposal waits at a time"
    );
    let rows = |kind: &str| {
        c.store
            .ledger_tail::<crate::ledger::LedgerRow>(2000)
            .unwrap()
            .into_iter()
            .map(|(_, r)| r)
            .filter(|r| r.kind == kind)
            .count()
    };
    assert_eq!(rows("thread.seal_proposed"), 1);

    c.thread_seal_dismiss("security-fix", cli()).unwrap();
    assert_eq!(rows("thread.seal_dismissed"), 1, "the label row");
    let after = ambient(c, &pr, "x", pick()).await;
    assert_eq!(after.admitted.len(), 1, "not needed releases the thread");
    assert!(
        !c.thread_recommend_seal("security-fix", "still a CVE")
            .unwrap(),
        "not needed is remembered while the content is as it was"
    );
    c.thread_edit(
        &ThreadEditParams {
            slug: "security-fix".into(),
            card: Some(vec!["the CVE is public now".into()]),
            ..Default::default()
        },
        cli(),
    )
    .unwrap();
    assert!(c
        .thread_recommend_seal("security-fix", "new content")
        .unwrap());
    // A dismissal with nothing proposed is refused.
    c.thread_seal_dismiss("security-fix", cli()).unwrap();
    assert!(c.thread_seal_dismiss("security-fix", cli()).is_err());
}

/// A thread made sealed, one split from a sealed thread, and a task of a
/// sealed thread's session are all held: their notes never come in.
#[tokio::test]
async fn a_split_and_a_task_inherit_the_seal() {
    let r = canary();
    let c = &r.core;
    new_thread(c, "embargo", "embargoed fix");
    new_thread(c, "pr-round", "PR round");
    let sealed = thread_session(c, "embargo");
    say(c, &sealed, "details");
    c.thread_seal(
        &ThreadSealParams {
            slug: "embargo".into(),
            ..Default::default()
        },
        cli(),
    )
    .unwrap();
    // A split.
    let split = c
        .thread_new(
            &ThreadNewParams {
                slug: "embargo-docs".into(),
                split_from: Some("embargo".into()),
                ..Default::default()
            },
            cli(),
        )
        .unwrap();
    assert!(split.thread.sealed.is_some(), "a split starts sealed");
    assert_eq!(split.thread.split_from.as_deref(), Some("embargo"));
    let by_hand = c
        .thread_new(
            &ThreadNewParams {
                slug: "sealed-at-birth".into(),
                sealed: true,
                ..Default::default()
            },
            cli(),
        )
        .unwrap();
    assert!(by_hand.thread.sealed.is_some());
    // A task of the sealed thread's session is the thread's.
    let task = crate::session::SessionRecord::with_id(
        crate::new_id("ses"),
        theseus_protocol::SessionKind::Task,
        None,
    );
    let mut task = task;
    task.task = Some(crate::session::TaskOf {
        parent_session: sealed.clone(),
        parent_execution: "exe_x".into(),
        by: "act_x".into(),
        target: None,
        arrangement: None,
        check: None,
        notify_done: false,
    });
    c.store.put_session(&task.session_id, &task).unwrap();
    say(c, &task.session_id, "the task's finding");
    let pr = thread_session(c, "pr-round");
    let m = ambient(
        c,
        &pr,
        "x",
        scored(c, vec![(task.session_id.clone(), 0, 0.04)]),
    )
    .await;
    assert!(m.admitted.is_empty(), "{m:?}");
    assert_eq!(m.drops.get("sealed"), Some(&1));
    // The rule for a merge.
    let board = c.threads();
    let (a, b) = (
        board.get("embargo").unwrap(),
        board.get("pr-round").unwrap(),
    );
    assert!(crate::threads::merged_seal(b, a).is_some());
    assert!(crate::threads::merged_seal(b, b).is_none());
}

/// An unseal is the owner's, from a private place: a stranger in a shared
/// channel is refused, and the seal stands.
#[tokio::test]
async fn an_unseal_is_refused_from_a_shared_place() {
    let r = canary();
    let c = &r.core;
    new_thread(c, "embargo", "embargoed fix");
    let stranger = Answerer {
        label: "discord:777".into(),
        surface: Surface::Discord,
        discord: Some(theseus_protocol::DiscordOrigin {
            user_id: "777".into(),
            channel_id: "42".into(),
            guild_id: Some("7".into()),
        }),
        agent: None,
    };
    let params = ThreadSealParams {
        slug: "embargo".into(),
        ..Default::default()
    };
    // Nor does a stranger seal: the threads are the owner's.
    assert!(c.thread_seal(&params, stranger.clone()).is_err());
    assert!(c.threads().get("embargo").unwrap().sealed.is_none());
    c.thread_seal(&params, cli()).unwrap();
    assert!(c.thread_unseal("embargo", stranger).is_err());
    assert!(c.threads().get("embargo").unwrap().sealed.is_some());
    let opened = c.thread_unseal("embargo", cli()).unwrap();
    assert!(opened.thread.sealed.is_none());
}

/// A seal reports what had already crossed: the `Recall` nodes of other
/// sessions whose sources are the thread's, grouped by the session that holds
/// them; with `redirect`, a note stands in each.
#[tokio::test]
async fn the_seal_reports_what_had_crossed_and_can_redirect() {
    let r = canary();
    let c = &r.core;
    new_thread(c, "security-fix", "security fix");
    new_thread(c, "pr-round", "PR round");
    let fix = thread_session(c, "security-fix");
    say(c, &fix, "the fix port one");
    say(c, &fix, "the fix port two");
    let pr = thread_session(c, "pr-round");
    c.runner.memory.set_ask(scored(
        c,
        vec![(fix.clone(), 0, 0.04), (fix.clone(), 1, 0.039)],
    ));
    turn(c, &pr, "port").await;
    let sealed = c
        .thread_seal(
            &ThreadSealParams {
                slug: "security-fix".into(),
                reason: Some("embargoed".into()),
                ..Default::default()
            },
            cli(),
        )
        .unwrap();
    assert_eq!(sealed.crossed.len(), 1, "{:?}", sealed.crossed);
    assert_eq!(sealed.crossed[0].session_id, pr);
    assert_eq!(sealed.crossed[0].thread.as_deref(), Some("pr-round"));
    assert_eq!(sealed.crossed[0].facts, 2);
    assert_eq!(
        theseus_protocol::threads::crossed_words(&sealed.crossed),
        ["2 facts were already used in «PR round»"]
    );
    assert!(!sealed.crossed[0].redirected);
    // Again with redirect: sealed already, so the report is empty; unseal and
    // seal with the offer taken.
    c.thread_unseal("security-fix", cli()).unwrap();
    let again = c
        .thread_seal(
            &ThreadSealParams {
                slug: "security-fix".into(),
                redirect: true,
                ..Default::default()
            },
            cli(),
        )
        .unwrap();
    assert!(again.crossed[0].redirected);
    let notes: Vec<String> = c
        .store
        .session_nodes(&pr)
        .unwrap()
        .into_iter()
        .filter_map(|(_, n)| match n.body {
            Body::UserMessage { text, .. } if text.starts_with("Redirect:") => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(notes.len(), 1, "one note in the session that used them");
    assert!(notes[0].contains("security fix") && !notes[0].contains("port one"));
}

/// `thread.read` of a sealed thread waits for the owner; of an open one it
/// does not; of a shared place it is refused.
#[tokio::test]
async fn reading_a_sealed_thread_asks_the_owner_first() {
    let r = canary();
    let c = &r.core;
    new_thread(c, "security-fix", "security fix");
    new_thread(c, "pr-round", "PR round");
    let open = || Decision {
        posture: Posture::Open,
        reason: "open".into(),
        notify: None,
        floor: false,
        granted: None,
    };
    let gate = |thread: &str| {
        crate::thread_read::gate(
            &c.tools,
            &crate::thread_read::ThreadRead,
            &json!({"thread": thread}),
            open(),
        )
        .posture
    };
    assert_eq!(gate("security-fix"), Posture::Open);
    c.thread_seal(
        &ThreadSealParams {
            slug: "security-fix".into(),
            ..Default::default()
        },
        cli(),
    )
    .unwrap();
    assert_eq!(gate("security-fix"), Posture::Approve);
    assert_eq!(gate("security fix"), Posture::Approve, "by its title too");
    assert_eq!(gate("pr-round"), Posture::Open);
    // The read itself: the card, and no notes without a query.
    let sid = session(c, None, &[]);
    let out = crate::thread_read::call(c.clone(), &sid, json!({"thread": "pr-round"}))
        .await
        .unwrap();
    assert!(out.text.contains("PR round") && out.text.contains("Pass a query"));
    assert!(
        crate::thread_read::call(c.clone(), &sid, json!({"thread": "nope"}))
            .await
            .is_err()
    );
}

/// "seal this thread" in a thread's session seals it before the turn runs,
/// and leaves a note in the session; the same words in an unthreaded session,
/// or with more in them, are an ordinary message.
#[tokio::test]
async fn the_owners_explicit_verb_seals_the_thread() {
    use super::thread_seal::is_seal_word;
    for yes in ["/seal", "Seal this thread.", "  seal this thread!  "] {
        assert!(is_seal_word(yes), "{yes}");
    }
    for no in [
        "seal this thread now",
        "please seal it",
        "unseal this thread",
        "",
    ] {
        assert!(!is_seal_word(no), "{no}");
    }
    let r = canary();
    let c = &r.core;
    new_thread(c, "scratch", "scratch pad");
    new_thread(c, "other", "another thread");
    let ordinary =
        super::tests_threads::ask(c.clone(), Some("other"), None, "seal this thread now").await;
    assert!(ordinary["error"].is_null(), "{ordinary}");
    assert!(c.threads().get("other").unwrap().sealed.is_none());
    let said = super::tests_threads::ask(c.clone(), Some("scratch"), None, "/seal").await;
    assert!(said["error"].is_null(), "{said}");
    let sid = said["result"]["session_id"].as_str().unwrap().to_string();
    let t = c.threads().get("scratch").unwrap().clone();
    assert!(
        t.sealed.as_ref().is_some_and(|s| s.reason.contains("word")),
        "{t:?}"
    );
    let noted = c
        .store
        .session_nodes(&sid)
        .unwrap()
        .into_iter()
        .any(|(_, n)| {
            matches!(&n.body, Body::UserMessage { text, .. } if text.starts_with("The owner sealed this thread"))
        });
    assert!(noted, "a note says so before the turn");
    // The same words from a session no thread serves seal nothing.
    let plain = session(c, None, &[]);
    let r2 = super::tests_threads::ask(c.clone(), None, Some(&plain), "seal this thread").await;
    assert!(r2["error"].is_null(), "{r2}");
    assert_eq!(c.threads().all().filter(|t| t.sealed.is_some()).count(), 1);
}

/// `thread.read` with a query gives the thread's card and its top notes,
/// each cited by thread and position.
#[tokio::test]
async fn a_read_with_a_query_cites_the_threads_notes() {
    let r = canary();
    let c = &r.core;
    new_thread(c, "security-fix", "security fix");
    let fix = thread_session(c, "security-fix");
    let id = say(c, &fix, "The staging proxy listens on port 8443.");
    let asker = session(c, None, &[]);
    c.runner
        .memory
        .set_ask(scored(c, vec![(fix.clone(), 0, 0.04)]));
    let out = crate::thread_read::call(
        c.clone(),
        &asker,
        json!({"thread": "Security Fix", "query": "staging proxy port"}),
    )
    .await
    .unwrap();
    assert!(
        out.text
            .contains("Thread «security fix» (security-fix, active)"),
        "{}",
        out.text
    );
    let (position, _) = c.store.get_node(&id).unwrap().unwrap();
    let cite = format!("[thread «security fix», {}, @{position}]", day_of(c, &id));
    assert!(out.text.contains(&cite), "{}", out.text);
    assert!(out.text.contains("port 8443"), "{}", out.text);
    assert_eq!(out.meta["shown"], 1);
}

/// The three seal methods over the protocol, as the CLI sends them.
#[tokio::test]
async fn the_seal_methods_answer_over_the_protocol() {
    use theseus_protocol::{Id, Message, Request};
    use tokio::io::{duplex, AsyncBufReadExt, AsyncWriteExt, BufReader};
    let r = canary();
    let c = r.core.clone();
    new_thread(&c, "tides", "tide gauges");
    let call = |method: &'static str, params: serde_json::Value| {
        let c = c.clone();
        async move {
            let (client, server) = duplex(64 * 1024);
            let (sr, sw) = tokio::io::split(server);
            let srv = tokio::spawn(c.serve_connection(
                sr,
                sw,
                crate::approval::Client::new("cli", Surface::Cli),
            ));
            let (cr, mut cw) = tokio::io::split(client);
            let mut line =
                serde_json::to_string(&Request::new(Id::Num(1), method, params)).unwrap();
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
            out
        }
    };
    let sealed = call("thread.seal", json!({"slug": "tides", "reason": "embargo"})).await;
    assert_eq!(
        sealed["result"]["thread"]["thread"]["sealed"]["reason"], "embargo",
        "{sealed}"
    );
    assert_eq!(sealed["result"]["crossed"], json!([]));
    let opened = call("thread.unseal", json!({"slug": "tides"})).await;
    assert!(opened["result"]["thread"]["sealed"].is_null(), "{opened}");
    let none = call("thread.seal_dismiss", json!({"slug": "tides"})).await;
    assert!(
        none["error"]["message"]
            .as_str()
            .unwrap()
            .contains("no seal is proposed"),
        "{none}"
    );
}
