//! A home claim stored by an earlier build, through the core (theseus-98bz.6). Until 2026-11-21 a
//! turn the router ran at a place's home whose files all fell in one thread's working set was
//! claimed for that thread; the owner removed routing by a guess that day, and the claim with it.
//! Now a home turn that writes a thread's file claims nothing, and a claim already in a store
//! still reads (the history names it, the lens reads its nodes as the thread's) and is undone by
//! `thread.unclaim`, beside a split over it.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use theseus_judge::fake::FakeJev;
use theseus_memory::threads::{Tie, Verdict};
use theseus_protocol::threads::{
    ThreadEditParams, ThreadNewParams, ThreadSplitParams, ThreadUnclaimParams, WorkingSet,
};
use theseus_protocol::{
    method, Id, Message, Request, SessionKind, TurnSubmitParams, TurnSubmitResult,
};
use tokio::io::{
    duplex, AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines, ReadHalf, WriteHalf,
};

use super::home_claim::{claim_key, ClaimRecord};
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

/// One `turn.submit`: it names no thread, so it runs in the place's own session.
async fn submit(c: &Arc<Core>, session: &str, input: &str) -> TurnSubmitResult {
    let mut w = Wire::open(c);
    let id = w.send(method::TURN_SUBMIT, params(session, input)).await;
    result(w.response(id).await)
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
    _jev: FakeJev,
    work: tempfile::TempDir,
    home: String,
}

fn place(rel: &[&str]) -> Place {
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
        _jev: jev,
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

/// A claim as an earlier build wrote it for `turn`'s nodes in `home`: its record, and each node keyed
/// to `thread` by the claim's value. Its id.
fn plant_claim(c: &Core, home: &str, turn: &str, thread: &str) -> String {
    let id = crate::new_id(crate::moved::CLAIM);
    let nodes: Vec<String> = turn_nodes(c, home, turn)
        .into_iter()
        .map(|(_, n, _)| n)
        .collect();
    assert!(!nodes.is_empty(), "the turn wrote nodes");
    let rec = ClaimRecord {
        id: id.clone(),
        thread: thread.into(),
        session: home.into(),
        turn: turn.into(),
        nodes: nodes.clone(),
        paths: vec!["tarnlib/tarn/retry.py".into()],
        at_ms: 1,
        undone: false,
    };
    let to = MovedTo {
        thread: thread.into(),
        split: id.clone(),
    };
    let mut recs = vec![theseus_store::NewRecord::json(
        theseus_store::kinds::META,
        Some(&claim_key(&id)),
        &rec,
    )
    .unwrap()];
    for n in &nodes {
        recs.push(
            theseus_store::NewRecord::json(
                theseus_store::kinds::META,
                Some(&moved_of_key(n)),
                &Some(&to),
            )
            .unwrap(),
        );
    }
    c.store.append(&recs).unwrap();
    id
}

/// A message with no address runs at home and writes `tarnlib/tarn/retry.py`, under pr-round's root
/// alone: nothing claims it now. No key, no record, no `thread.claimed` row; the lens reads its
/// message as the place's own conversation, distant for pr-round.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_home_turn_that_wrote_a_threads_file_claims_nothing() {
    let p = place(&["tarnlib/tarn/retry.py"]);
    let (c, home) = (&p.r.core, p.home.as_str());
    let res = submit(c, home, "fix the retry").await;
    assert_eq!(res.session_id, home);
    assert!(res.files.is_some());
    assert!(p.work.path().join("tarnlib/tarn/retry.py").exists());
    assert!(rows(c, "thread.claimed").is_empty());
    assert!(claims(c).is_empty());
    assert!(turn_nodes(c, home, &res.turn_id)
        .iter()
        .all(|(_, _, to)| to.is_none()));
    let note = user_node(c, home, &res.turn_id);
    assert!(!matches!(
        verdict(c, "pr-round", home, &note),
        Verdict::Pass
    ));
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

/// `thread.unclaim` puts every node of a stored claim back to no thread, so pr-round's lens reads the turn as
/// it reads the place's own conversation; a node a split took after the claim keeps the split's;
/// a second undo, and an unknown claim, are refused with the argument named.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unclaim_restores_the_home_turn_and_leaves_a_later_split() {
    let p = place(&["tarnlib/tarn/retry.py"]);
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
    let res = submit(c, home, "fix the retry").await;
    let id = plant_claim(c, home, &res.turn_id, "pr-round");
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

/// A stored claim's node: its history detail names the thread as `claimed_by` and the claim, never
/// `moved_to`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_claimed_nodes_history_says_claimed_by() {
    let p = place(&["tarnlib/tarn/retry.py"]);
    let (c, home) = (&p.r.core, p.home.as_str());
    let res = submit(c, home, "fix the retry").await;
    let id = json!(plant_claim(c, home, &res.turn_id, "pr-round"));
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

/// A node an unsplit nulled after the claim is neither restored nor kept (theseus-l4n39): a split
/// over the whole claim, its unsplit, then the unclaim counts nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unclaim_counts_a_nulled_node_as_neither() {
    let p = place(&["tarnlib/tarn/retry.py"]);
    let (c, home) = (&p.r.core, p.home.as_str());
    let res = submit(c, home, "fix the retry").await;
    let id = plant_claim(c, home, &res.turn_id, "pr-round");
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
