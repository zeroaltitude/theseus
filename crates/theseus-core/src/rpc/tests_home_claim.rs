//! A home turn that served one thread becomes that thread's memory (theseus-98bz.6), through the
//! core: a message the router runs at home writes a file under one thread's root, and every node
//! of the turn is keyed to that thread, so the lens passes it for that thread's recall and finds it
//! another stream's for the other's. What claims nothing (unthreaded files, two threads' files, a
//! sealed thread, unseen paths), what a split already moved, the undo, the readers (history, a
//! split over a claim), and the switch off.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, Scripted as Jev};
use theseus_memory::threads::{Tie, Verdict};
use theseus_protocol::thread_route::{RouteHow, ThreadAnswerParams, ThreadTag};
use theseus_protocol::threads::{
    ThreadEditParams, ThreadNewParams, ThreadSealParams, ThreadSplitParams, ThreadUnclaimParams,
    WorkingSet,
};
use theseus_protocol::{
    method, Id, Message, Request, SessionKind, TurnSubmitParams, TurnSubmitResult,
};
use tokio::io::{
    duplex, AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines, ReadHalf, WriteHalf,
};

use super::home_claim::{claim_key, ClaimRecord};
use super::thread_flow::Routing;
use super::Core;
use crate::approval::{Answerer, Client, Surface};
use crate::ledger::LedgerRow;
use crate::moved::{moved_of_key, MovedTo};
use crate::node::Body;
use crate::provider::Scripted;
use crate::session::SessionRecord;
use crate::tests_inbound::inbound_only;
use crate::tests_judge::{rig_on, Rig};
use theseus_store::Store as _;

fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    }
}

/// A connection from the CLI to the core.
struct Wire {
    w: WriteHalf<DuplexStream>,
    lines: Lines<BufReader<ReadHalf<DuplexStream>>>,
    next: u64,
}

impl Wire {
    fn open(core: &Arc<Core>) -> Self {
        let (client, server) = duplex(1 << 20);
        let (sr, sw) = tokio::io::split(server);
        tokio::spawn(
            core.clone()
                .serve_connection(sr, sw, Client::new("test", Surface::Cli)),
        );
        let (cr, cw) = tokio::io::split(client);
        Wire {
            w: cw,
            lines: BufReader::new(cr).lines(),
            next: 1,
        }
    }

    async fn send(&mut self, m: &str, params: impl serde::Serialize) -> u64 {
        let id = self.next;
        self.next += 1;
        let mut line = serde_json::to_string(&Request::new(Id::Num(id), m, params)).unwrap();
        line.push('\n');
        self.w.write_all(line.as_bytes()).await.unwrap();
        id
    }

    async fn message(&mut self) -> Message {
        let l = tokio::time::timeout(Duration::from_secs(60), self.lines.next_line())
            .await
            .expect("a message within a minute")
            .unwrap()
            .expect("the connection is open");
        serde_json::from_str(&l).unwrap()
    }

    /// The response to `id`; notifications before it are dropped.
    async fn response(&mut self, id: u64) -> Value {
        loop {
            if let Message::Response(r) = self.message().await {
                if r.id == Id::Num(id) {
                    return serde_json::to_value(r).unwrap();
                }
            }
        }
    }

    /// The next notification of `method`.
    async fn notice(&mut self, method: &str) -> Value {
        loop {
            if let Message::Notification(n) = self.message().await {
                if n.method == method {
                    return n.params;
                }
            }
        }
    }
}

fn params(session: &str, input: &str) -> TurnSubmitParams {
    TurnSubmitParams {
        dir: None,
        carried: false,
        prompt: None,
        session_id: Some(session.into()),
        input: input.into(),
        profile: None,
        provider: None,
        model: None,
        author: None,
        attachments: vec![],
        reply_to: None,
        opened_from: None,
        thread: None,
        speaking: None,
        in_reply_to: None,
        via: None,
        surface: None,
        route_at_ms: None,
        deadline_secs: None,
    }
}

fn result(v: Value) -> TurnSubmitResult {
    serde_json::from_value(v["result"].clone()).unwrap_or_else(|_| panic!("a turn result in {v}"))
}

/// One `turn.submit`: Jev says `home` surely, so it runs in the place's own session.
async fn submit(c: &Arc<Core>, session: &str, input: &str) -> TurnSubmitResult {
    let mut w = Wire::open(c);
    let id = w.send(method::TURN_SUBMIT, params(session, input)).await;
    result(w.response(id).await)
}

fn thread_answer(jev: &FakeJev, slug: &str, p: f64) {
    jev.script(
        "thread",
        Jev::Choice {
            option: slug.into(),
            confidence: p,
        },
    );
}

/// The model's turn: it writes each of `paths` in one response, then answers.
fn writes(paths: &[String]) -> Vec<Scripted> {
    let calls: Vec<(String, &str, Value)> = paths
        .iter()
        .enumerate()
        .map(|(i, p)| {
            (
                format!("w{i}"),
                "fs_write",
                json!({"path": p, "content": "x\n"}),
            )
        })
        .collect();
    let calls: Vec<(&str, &str, Value)> = calls
        .iter()
        .map(|(id, t, v)| (id.as_str(), *t, v.clone()))
        .collect();
    vec![
        Scripted::tools("", &calls),
        Scripted::text("Written."),
        Scripted::text("ok"),
    ]
}

/// A rig whose model writes `paths`, a home session in `work`, and the two threads: `pr-round`
/// (root `tarnlib`) and `sec-fix` (root `secsvc`), roots named bare, under the home's directory.
struct Place {
    r: Rig,
    jev: FakeJev,
    work: tempfile::TempDir,
    home: String,
}

fn place(rel: &[&str], claim: bool) -> Place {
    let jev = FakeJev::start().unwrap();
    let work = tempfile::tempdir().unwrap();
    for d in ["tarnlib/tarn", "secsvc/api", "scratch"] {
        std::fs::create_dir_all(work.path().join(d)).unwrap();
    }
    let paths: Vec<String> = rel
        .iter()
        .map(|p| work.path().join(p).display().to_string())
        .collect();
    let root = work.path().display().to_string();
    let r = rig_on(writes(&paths), Some(&jev), |c| {
        inbound_only(c);
        c.judge.total_secs = 5;
        c.tools.roots = vec![root.clone()];
        c.threads.route_wait_ms = 3000;
        c.threads.ask_wait_secs = 5;
        c.threads.claim_home_turns = claim;
    });
    let c = &r.core;
    for (slug, title, root) in [
        ("pr-round", "pr round", "tarnlib"),
        ("sec-fix", "security fix", "secsvc"),
    ] {
        c.thread_new(
            &ThreadNewParams {
                slug: slug.into(),
                title: Some(title.into()),
                goal: Some(format!("Finish the {title} work")),
                ..Default::default()
            },
            cli(),
        )
        .unwrap();
        c.thread_edit(
            &ThreadEditParams {
                slug: slug.into(),
                working_set: Some(WorkingSet {
                    roots: vec![root.into()],
                    ..Default::default()
                }),
                ..Default::default()
            },
            cli(),
        )
        .unwrap();
    }
    let mut rec = SessionRecord::new(SessionKind::Conversation, None);
    rec.dir = Some(root);
    c.store.put_session(&rec.session_id, &rec).unwrap();
    Place {
        r,
        jev,
        work,
        home: rec.session_id,
    }
}

fn rows(c: &Core, kind: &str) -> Vec<LedgerRow> {
    c.store
        .ledger_tail::<LedgerRow>(5000)
        .unwrap()
        .into_iter()
        .map(|(_, r)| r)
        .filter(|r| r.kind == kind)
        .collect()
}

/// The turn's nodes in `sid`, as (position, id, the thread and split its key holds).
fn turn_nodes(c: &Core, sid: &str, turn: &str) -> Vec<(u64, String, Option<MovedTo>)> {
    c.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .filter(|(_, n)| n.turn_id.as_deref() == Some(turn))
        .map(|(p, n)| {
            let to: Option<Option<MovedTo>> = c.store.get_meta(&moved_of_key(&n.id)).unwrap();
            (p, n.id, to.flatten())
        })
        .collect()
}

fn user_node(c: &Core, sid: &str, turn: &str) -> String {
    c.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .find(|(_, n)| {
            n.turn_id.as_deref() == Some(turn) && matches!(n.body, Body::UserMessage { .. })
        })
        .map(|(_, n)| n.id)
        .expect("the turn's message")
}

fn claims(c: &Core) -> Vec<ClaimRecord> {
    c.store
        .inner()
        .latest_with_prefix(theseus_store::kinds::META, "thread.claim.")
        .unwrap()
        .iter()
        .map(|r| r.decode().unwrap())
        .collect()
}

/// The lens's verdict on `node` of `home` for `slug`'s recall, its own turn's.
fn verdict(c: &Core, slug: &str, home: &str, node: &str) -> Verdict {
    let asker = c.thread_session(slug, cli()).unwrap().session_id;
    let scene = c
        .runner
        .thread_scene(Some(&asker), "what changed in the retry", true)
        .expect("threads exist");
    scene
        .lens([(home, node, 1.0)], &BTreeSet::new())
        .judge(home, node, 1.0, Tie::Unknown)
}

/// The key test. A message Jev cannot place is asked about, and the owner answers `home`: the turn
/// runs in the place's own session and writes `tarnlib/tarn/retry.py`, under pr-round's root alone.
/// Every node of the turn is keyed to pr-round by a `claim_` value, one `thread.claimed` row names
/// it, and the lens passes the turn's message for pr-round's recall and finds it another stream's
/// for sec-fix's.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_home_turn_that_served_a_thread_is_claimed() {
    let p = place(&["tarnlib/tarn/retry.py"], true);
    let (c, home) = (&p.r.core, p.home.as_str());
    // Two contend, so the router asks (theseus-e31an: a lone lean is no
    // contest): the message names pr-round's root, and Jev leans to sec-fix.
    thread_answer(&p.jev, "sec-fix", 0.7);
    let mut w = Wire::open(c);
    let id = w
        .send(
            method::TURN_SUBMIT,
            params(home, "the second review comment on the retry in tarnlib"),
        )
        .await;
    let asked = w.notice("thread.asked").await;
    let mut a = Wire::open(c);
    let aid = a
        .send(
            method::THREAD_ANSWER,
            ThreadAnswerParams {
                ask_id: asked["ask_id"].as_str().unwrap().into(),
                choice: "home".into(),
                author: None,
                discord: None,
            },
        )
        .await;
    assert!(a.response(aid).await["result"]["routed"].is_null());
    let res = result(w.response(id).await);
    assert_eq!(res.session_id, home, "it ran at home");
    assert_eq!(res.thread.as_ref().map(|t| t.how), Some(RouteHow::Asked));
    assert!(p.work.path().join("tarnlib/tarn/retry.py").exists());

    let nodes = turn_nodes(c, home, &res.turn_id);
    assert!(nodes.len() >= 4, "message, call, result, reply: {nodes:?}");
    let claim = &rows(c, "thread.claimed");
    assert_eq!(claim.len(), 1, "{claim:?}");
    let id = claim[0].data["claim"].as_str().unwrap();
    assert!(id.starts_with("claim_"), "{id}");
    assert_eq!(claim[0].data["thread"], "pr-round");
    assert_eq!(claim[0].data["session"], home);
    assert_eq!(claim[0].data["nodes"], nodes.len());
    let path = p
        .work
        .path()
        .join("tarnlib/tarn/retry.py")
        .display()
        .to_string();
    assert_eq!(claim[0].data["paths"], json!([path]));
    for (_, node, to) in &nodes {
        assert_eq!(
            to.as_ref(),
            Some(&MovedTo {
                thread: "pr-round".into(),
                split: id.into(),
            }),
            "{node}"
        );
    }
    let rec: ClaimRecord = c.store.get_meta(&claim_key(id)).unwrap().unwrap();
    assert_eq!(
        (
            rec.thread.as_str(),
            rec.turn.as_str(),
            rec.nodes.len(),
            rec.undone
        ),
        ("pr-round", res.turn_id.as_str(), nodes.len(), false)
    );

    let note = user_node(c, home, &res.turn_id);
    assert!(matches!(verdict(c, "pr-round", home, &note), Verdict::Pass));
    let other = verdict(c, "sec-fix", home, &note);
    assert!(
        !matches!(other, Verdict::Pass),
        "another stream's: {other:?}"
    );
}

/// A home turn that writes nothing threaded, or one file under each thread's root, claims nothing;
/// nor does one whose only thread is sealed, nor one with more paths than its result names.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_home_turn_claims_nothing_that_is_not_one_live_threads() {
    for (case, rel) in [
        ("unthreaded", vec!["scratch/scratch.txt".to_string()]),
        (
            "two threads",
            vec![
                "tarnlib/tarn/a.py".to_string(),
                "secsvc/api/b.go".to_string(),
            ],
        ),
        (
            "unseen paths",
            (0..9).map(|i| format!("tarnlib/tarn/m{i}.py")).collect(),
        ),
    ] {
        let rel: Vec<&str> = rel.iter().map(String::as_str).collect();
        let p = place(&rel, true);
        let c = &p.r.core;
        thread_answer(&p.jev, "home", 0.96);
        let res = submit(c, &p.home, "tidy the tree").await;
        assert_eq!(res.session_id, p.home, "{case}");
        let files = res.files.as_ref().expect("files written");
        assert_eq!(
            files.changed.paths.len() + files.changed.more as usize,
            rel.len(),
            "{case}"
        );
        assert!(rows(c, "thread.claimed").is_empty(), "{case}");
        assert!(claims(c).is_empty(), "{case}");
        assert!(
            turn_nodes(c, &p.home, &res.turn_id)
                .iter()
                .all(|(_, _, to)| to.is_none()),
            "{case}"
        );
    }

    // pr-round sealed: its files are claimed by no one, and sec-fix does not hold them.
    let p = place(&["tarnlib/tarn/retry.py"], true);
    let c = &p.r.core;
    c.thread_seal(
        &ThreadSealParams {
            slug: "pr-round".into(),
            reason: Some("embargoed".into()),
            ..Default::default()
        },
        cli(),
    )
    .unwrap();
    thread_answer(&p.jev, "home", 0.96);
    let res = submit(c, &p.home, "fix the retry").await;
    assert_eq!(res.session_id, p.home);
    assert!(rows(c, "thread.claimed").is_empty());
    assert!(claims(c).is_empty());
}

/// A node a split moved already keeps its split's value; the rest of the turn is claimed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_node_a_split_moved_keeps_its_splits_value() {
    // The switch off for the turn, so the claim is made here, after the key is planted.
    let p = place(&["tarnlib/tarn/retry.py"], false);
    let (c, home) = (&p.r.core, p.home.as_str());
    thread_answer(&p.jev, "home", 0.96);
    let res = submit(c, home, "fix the retry").await;
    let note = user_node(c, home, &res.turn_id);
    let split = MovedTo {
        thread: "sec-fix".into(),
        split: "spl_earlier".into(),
    };
    c.store
        .append(&[theseus_store::NewRecord::json(
            theseus_store::kinds::META,
            Some(&moved_of_key(&note)),
            &Some(&split),
        )
        .unwrap()])
        .unwrap();
    let routing = Routing {
        tag: ThreadTag {
            slug: None,
            title: "home".into(),
            how: RouteHow::Home,
            confidence: 0.96,
            movable: false,
        },
        home: home.into(),
        session: home.into(),
        slug: None,
        bench: None,
    };
    let rec = c.try_claim(&routing, &res).unwrap().expect("claimed");
    for (_, node, to) in turn_nodes(c, home, &res.turn_id) {
        match node == note {
            true => assert_eq!(to.as_ref(), Some(&split)),
            false => assert_eq!(to.map(|m| m.split).as_deref(), Some(rec.id.as_str())),
        }
    }
    assert!(!rec.nodes.contains(&note));
}

/// A split of the whole turn into a new thread takes the claimed nodes over (a claim is weaker
/// than a move), and its unsplit clears them.
fn split_over_the_claim(c: &Core, home: &str, turn: &str) {
    let nodes = turn_nodes(c, home, turn);
    let (first, last) = (nodes[0].0, nodes[nodes.len() - 1].0);
    let split = c
        .thread_split(
            &ThreadSplitParams {
                from: home.into(),
                slug: "retry-notes".into(),
                range: Some([first, last]),
                ..Default::default()
            },
            cli(),
        )
        .unwrap_or_else(|e| panic!("a split over a claim: {}", e.message));
    let taken = turn_nodes(c, home, turn);
    assert!(
        taken
            .iter()
            .all(|(_, _, to)| to.as_ref().map(|m| m.split.as_str()) == Some(split.split.as_str())),
        "{taken:?}"
    );
    c.thread_unsplit(
        &theseus_protocol::threads::ThreadUnsplitParams {
            split: split.split,
            ..Default::default()
        },
        cli(),
    )
    .unwrap_or_else(|e| panic!("{}", e.message));
    assert!(turn_nodes(c, home, turn)
        .iter()
        .all(|(_, _, to)| to.is_none()));
}

/// `thread.unclaim` puts every claimed node back to no thread, so pr-round's lens reads the turn as
/// it reads the place's own conversation; a node a split took after the claim keeps the split's;
/// a second undo, and an unknown claim, are refused with the argument named.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unclaim_restores_the_home_turn_and_leaves_a_later_split() {
    let p = place(&["tarnlib/tarn/retry.py"], true);
    let (c, home) = (&p.r.core, p.home.as_str());
    let base = {
        let n = crate::node::Node::user(home, None, "test", "the place's own note");
        c.store.append(&[n.record().unwrap()]).unwrap();
        n.id
    };
    let before = verdict(c, "pr-round", home, &base);
    assert!(
        !matches!(before, Verdict::Pass),
        "the base is distant: {before:?}"
    );
    thread_answer(&p.jev, "home", 0.96);
    let res = submit(c, home, "fix the retry").await;
    let id = rows(c, "thread.claimed")[0].data["claim"]
        .as_str()
        .unwrap()
        .to_string();
    let note = user_node(c, home, &res.turn_id);
    assert!(matches!(verdict(c, "pr-round", home, &note), Verdict::Pass));

    let nodes = turn_nodes(c, home, &res.turn_id);
    split_over_the_claim(c, home, &res.turn_id);
    // The unsplit cleared every node; key one back to the claim and one to a later split.
    let mine = MovedTo {
        thread: "pr-round".into(),
        split: id.clone(),
    };
    let later = MovedTo {
        thread: "sec-fix".into(),
        split: "spl_later".into(),
    };
    let recs: Vec<_> = nodes
        .iter()
        .enumerate()
        .map(|(i, (_, n, _))| {
            let to = if i == 0 { &later } else { &mine };
            theseus_store::NewRecord::json(
                theseus_store::kinds::META,
                Some(&moved_of_key(n)),
                &Some(to),
            )
            .unwrap()
        })
        .collect();
    c.store.append(&recs).unwrap();

    let undo = |claim: &str| {
        c.thread_unclaim(
            &ThreadUnclaimParams {
                claim: claim.into(),
                ..Default::default()
            },
            cli(),
        )
    };
    let out = undo(&id).unwrap_or_else(|e| panic!("{}", e.message));
    assert_eq!((out.restored as usize, out.kept), (nodes.len() - 1, 1));
    assert_eq!(out.thread, "pr-round");
    for (i, (_, _, to)) in turn_nodes(c, home, &res.turn_id).into_iter().enumerate() {
        assert_eq!(to, (i == 0).then(|| later.clone()));
    }
    let unclaimed = rows(c, "thread.unclaimed");
    assert_eq!(unclaimed.len(), 1);
    assert_eq!(
        (
            unclaimed[0].data["restored"].as_u64(),
            unclaimed[0].data["kept"].as_u64()
        ),
        (Some(nodes.len() as u64 - 1), Some(1))
    );
    let after = verdict(c, "pr-round", home, &note);
    assert_eq!(
        format!("{after:?}"),
        format!("{before:?}"),
        "the verdict before the claim"
    );
    assert!(claims(c).iter().all(|r| r.undone));

    let again = undo(&id).unwrap_err().message;
    assert!(
        again.contains("undone already") && again.contains("`claim`"),
        "{again}"
    );
    let unknown = undo("claim_nothing").unwrap_err().message;
    assert!(unknown.contains("`thread.unclaim`'s `claim`"), "{unknown}");
}

/// A claimed node's history detail names the thread as `claimed_by` and the claim, never
/// `moved_to`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_claimed_nodes_history_says_claimed_by() {
    let p = place(&["tarnlib/tarn/retry.py"], true);
    let (c, home) = (&p.r.core, p.home.as_str());
    thread_answer(&p.jev, "home", 0.96);
    let res = submit(c, home, "fix the retry").await;
    let id = rows(c, "thread.claimed")[0].data["claim"].clone();
    let mut w = Wire::open(c);
    let h = w
        .send(method::SESSION_HISTORY, json!({"session_id": home}))
        .await;
    let v = w.response(h).await;
    let nodes: Vec<&Value> = v["result"]["nodes"]
        .as_array()
        .unwrap_or_else(|| panic!("{v}"))
        .iter()
        .filter(|n| n["turn_id"] == res.turn_id.as_str())
        .collect();
    assert!(!nodes.is_empty());
    for n in nodes {
        assert_eq!(n["detail"]["claimed_by"], "pr-round", "{n}");
        assert_eq!(n["detail"]["claim"], id, "{n}");
        assert!(n["detail"].get("moved_to").is_none(), "{n}");
    }
}

/// With `claim_home_turns = false` a home turn that wrote pr-round's file leaves no key, no record
/// and no row: the behaviour before the switch.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_the_switch_off_nothing_is_claimed() {
    let p = place(&["tarnlib/tarn/retry.py"], false);
    let (c, home) = (&p.r.core, p.home.as_str());
    thread_answer(&p.jev, "home", 0.96);
    let res = submit(c, home, "fix the retry").await;
    assert_eq!(res.session_id, home);
    assert!(res.files.is_some());
    assert!(rows(c, "thread.claimed").is_empty());
    assert!(claims(c).is_empty());
    assert!(turn_nodes(c, home, &res.turn_id)
        .iter()
        .all(|(_, _, to)| to.is_none()));
    // The template with its `[threads]` table and this one key said: the loader takes it.
    let text = crate::Config::EXAMPLE_TOML
        .replace("# [threads]", "[threads]")
        .replace("# claim_home_turns = true", "claim_home_turns = false");
    let off = crate::Config::parse(&text).unwrap().0;
    assert!(!off.threads.claim_home_turns);
    let on = crate::Config::example();
    assert!(on.threads.claim_home_turns);
    assert!(crate::Config::EXAMPLE_TOML.contains("# claim_home_turns = true"));
}

/// The claim's `Routing` for a turn that ran at `home`.
fn at_home(home: &str) -> Routing {
    Routing {
        tag: ThreadTag {
            slug: None,
            title: "home".into(),
            how: RouteHow::Home,
            confidence: 0.96,
            movable: false,
        },
        home: home.into(),
        session: home.into(),
        slug: None,
        bench: None,
    }
}

/// A node an unsplit nulled after the claim is neither restored nor kept (theseus-l4n39): a split
/// over the whole claim, its unsplit, then the unclaim counts nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unclaim_counts_a_nulled_node_as_neither() {
    let p = place(&["tarnlib/tarn/retry.py"], true);
    let (c, home) = (&p.r.core, p.home.as_str());
    thread_answer(&p.jev, "home", 0.96);
    let res = submit(c, home, "fix the retry").await;
    let id = rows(c, "thread.claimed")[0].data["claim"]
        .as_str()
        .unwrap()
        .to_string();
    split_over_the_claim(c, home, &res.turn_id);
    let out = c
        .thread_unclaim(
            &ThreadUnclaimParams {
                claim: id,
                ..Default::default()
            },
            cli(),
        )
        .unwrap_or_else(|e| panic!("{}", e.message));
    assert_eq!((out.restored, out.kept), (0, 0), "{out:?}");
    let row = &rows(c, "thread.unclaimed")[0].data;
    assert_eq!(
        (row["restored"].as_u64(), row["kept"].as_u64()),
        (Some(0), Some(0))
    );
}

/// A home turn longer than a page (64 nodes) is read across pages and claimed whole; one past the
/// most read (1,024) claims nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_long_home_turn_is_read_across_pages() {
    let p = place(&["tarnlib/tarn/retry.py"], false);
    let (c, home) = (&p.r.core, p.home.as_str());
    thread_answer(&p.jev, "home", 0.96);
    let res = submit(c, home, "fix the retry").await;
    let grow = |n: usize| {
        let recs: Vec<_> = (0..n)
            .map(|i| {
                crate::node::Node::user(home, Some(&res.turn_id), "test", &format!("step {i}"))
                    .record()
                    .unwrap()
            })
            .collect();
        c.store.append(&recs).unwrap();
    };
    grow(70);
    let whole = turn_nodes(c, home, &res.turn_id).len();
    assert!(whole > 70, "{whole}");
    let rec = c.try_claim(&at_home(home), &res).unwrap().expect("claimed");
    assert_eq!(
        rec.nodes.len(),
        whole,
        "every node of the turn, across pages"
    );
    grow(1_024);
    assert!(
        c.try_claim(&at_home(home), &res).unwrap().is_none(),
        "past the most read, nothing"
    );
}

/// A split's keys that land while the claim waits are read by it (theseus-l4n39): the claim reads
/// and writes under the home's execution lock, the lock a split's frame holds. A frame holding that
/// lock stages a split's key for the turn's message; the claim, started meanwhile, keeps the
/// split's value and leaves the message out of its record.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_split_landing_beside_the_claim_is_kept() {
    let p = place(&["tarnlib/tarn/retry.py"], false);
    let (c, home) = (&p.r.core, p.home.as_str());
    thread_answer(&p.jev, "home", 0.96);
    let res = submit(c, home, "fix the retry").await;
    let note = user_node(c, home, &res.turn_id);
    let exec = c
        .store
        .get_session::<SessionRecord>(home)
        .unwrap()
        .unwrap()
        .execution_id
        .unwrap();
    let split = MovedTo {
        thread: "sec-fix".into(),
        split: "spl_beside".into(),
    };
    let (held_tx, held_rx) = std::sync::mpsc::channel::<()>();
    let (go_tx, go_rx) = std::sync::mpsc::channel::<()>();
    let rec = std::thread::scope(|s| {
        let (exec, note, split) = (&exec, &note, &split);
        let splitter = s.spawn(move || {
            c.kernel
                .frame(&[exec], |k| {
                    k.stage(&[theseus_store::NewRecord::json(
                        theseus_store::kinds::META,
                        Some(&moved_of_key(note)),
                        &Some(split),
                    )?])?;
                    held_tx.send(()).unwrap();
                    go_rx.recv().unwrap();
                    Ok(())
                })
                .unwrap();
        });
        held_rx.recv().unwrap();
        let claimer = s.spawn(|| c.try_claim(&at_home(home), &res).unwrap());
        // The claim is under way; the split's frame lands after it began.
        std::thread::sleep(Duration::from_millis(300));
        go_tx.send(()).unwrap();
        splitter.join().unwrap();
        claimer.join().unwrap().expect("claimed")
    });
    assert!(!rec.nodes.contains(&note), "{:?}", rec.nodes);
    let to: Option<Option<MovedTo>> = c.store.get_meta(&moved_of_key(&note)).unwrap();
    assert_eq!(to.flatten(), Some(split));
}
