//! The three things decision 6 needs before it may act (issue 44): the
//! judge's typed answer, evidence from the turn's admitted inputs, and the
//! one-use receipt; and the shadow the new pack starts in. Through the
//! tool's own path, a clean narrowing in his own words is recorded as what
//! it WOULD do and keeps its card in shadow, applies once when its judge
//! acts, and a replay, text from outside, or an agent's process at the
//! terminal keeps the card.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use theseus_judge::band::band;
use theseus_judge::client::Answer;
use theseus_judge::judge::AnswerRecord;
use theseus_memory::gate::Present;
use theseus_memory::recall::Place;
use theseus_store::{kinds, NewRecord};

use super::*;
use crate::approval::{Client, Surface};
use crate::exchange::{Exchange, Opener, OwnerMark, Peer, OWNER};
use crate::peer_proc::Behind;
use crate::task_graph::words::{applies, decide, Ask, Evidence, Operation, Why, Words};
use crate::task_graph::{NewTask, TaskOrigin, TaskRecord, TaskState};
use crate::view_gate::{present, Input, OwnerMarks};

const TASK: &str = "tsk_00000000000000000000000000000091";
const S: &str = "ses_harbour";

// ---- the judge's answer, typed --------------------------------------------

fn answered(choice: &str, p: f64) -> AnswerRecord {
    let options = ["restricts", "pauses", "resumes", "other"];
    let answer = Answer::Choice {
        choice: choice.into(),
        probabilities: options
            .iter()
            .map(|o| {
                (
                    o.to_string(),
                    if *o == choice { p } else { (1.0 - p) / 3.0 },
                )
            })
            .collect(),
        confidence: p,
    };
    let pack = theseus_judge::pack::by_name(crate::judge::task_words::PACK).unwrap();
    let q = pack.questions.iter().find(|q| q.id == "change").unwrap();
    AnswerRecord {
        question: "change".into(),
        def: "change".into(),
        about: None,
        band: band(&answer, q.thresholds),
        no_match: Some("other".into()),
        answer,
    }
}

/// Only `restricts`, `pauses` and `resumes` at the act band are read as
/// themselves; `other`, and any answer under the band, is `unsure`.
#[test]
fn only_a_sure_answer_is_read_as_its_operation() {
    assert_eq!(
        operation_of(&answered("restricts", 0.97)),
        Operation::Restricts
    );
    assert_eq!(operation_of(&answered("pauses", 0.97)), Operation::Pauses);
    assert_eq!(operation_of(&answered("resumes", 0.97)), Operation::Resumes);
    assert_eq!(operation_of(&answered("other", 0.97)), Operation::Unsure);
    assert_eq!(
        operation_of(&answered("restricts", 0.70)),
        Operation::Unsure
    );
    assert_eq!(operation_of(&answered("pauses", 0.50)), Operation::Unsure);
}

// ---- the rule, over a clean case --------------------------------------------

fn task() -> TaskRecord {
    let mut t = NewTask {
        id: TASK.into(),
        title: "Deploy",
        objective: "Deploy the service".into(),
        acceptance: vec!["it answers on /health".into()],
        parent: None,
        deps: vec![],
        session: Some("ses_task".into()),
        origin: TaskOrigin {
            session: S.into(),
            principal: "operator".into(),
            ..Default::default()
        },
        state: TaskState::InProgress,
    }
    .build(1_000);
    t.version = 3;
    t
}

/// The owner alone at the terminal, his message `msg_1` the current ask, a
/// judgment of it naming this task; `operation`, `spent` and `acting` vary.
struct Clean {
    operation: Operation,
    spent: bool,
    acting: bool,
    outside: bool,
}

const SAID: &str = "staging only";

impl Clean {
    fn new(operation: Operation) -> Self {
        Self {
            operation,
            spent: false,
            acting: false,
            outside: false,
        }
    }
}

impl OwnerMarks for Clean {
    fn in_person(&self, _: &str) -> anyhow::Result<Option<bool>> {
        Ok(Some(true))
    }
}

impl Evidence for Clean {
    fn session(&self) -> &str {
        S
    }
    fn is_task(&self) -> bool {
        false
    }
    fn mark(&self, node: &str) -> anyhow::Result<Option<OwnerMark>> {
        Ok(Some(OwnerMark {
            node: node.into(),
            in_person: true,
            owner_handle: None,
            peer: Peer::Person,
            agent: None,
            surface: "cli".into(),
            place: "local".into(),
            arrived_session: S.into(),
            ran_session: S.into(),
            author: None,
            at_ms: 2_000,
        }))
    }
    fn exchange(&self) -> anyhow::Result<Option<Exchange>> {
        Ok(Some(Exchange {
            turn_id: "trn_now".into(),
            session_id: S.into(),
            place: "local".into(),
            private: true,
            opened_by: Opener::Message,
            person: Some(OWNER.into()),
            start_ms: 2_000,
            end_ms: None,
            ask: Some("msg_1".into()),
            reply: None,
            messages: vec!["msg_1".into()],
            stop: None,
            rebuilt: false,
        }))
    }
    fn present(&self, mark: &OwnerMark) -> Present {
        present(&Place::Private, None, Input::Message(&mark.node), self)
    }
    fn text(&self, _: &str) -> anyhow::Result<Option<String>> {
        Ok(Some(SAID.into()))
    }
    fn outside(&self) -> bool {
        self.outside
    }
    fn words(&self, _: &str, _: u64) -> anyhow::Result<Option<Words>> {
        Ok(None)
    }
    fn judged(&self, node: &str) -> Option<JudgedChange> {
        Some(JudgedChange {
            node: node.into(),
            task: TASK.into(),
            base_version: 3,
            operation: self.operation,
        })
    }
    fn spent(&self, _: &str) -> bool {
        self.spent
    }
    fn acting(&self, _: &str) -> bool {
        self.acting
    }
}

fn narrowing(objective: &str) -> Ask<'_> {
    Ask {
        named: TASK,
        version: 3,
        said: Some("msg_1"),
        kind: Some(Kind::Narrows),
        objective: Some(objective),
        ..Ask::default()
    }
}

/// In shadow a clean narrowing in his own words passes every check and keeps
/// its card (`shadow`); when its judge acts it applies; a spent receipt, an
/// `other` (unsure) answer, or outside text keeps the card in either.
#[test]
fn shadow_keeps_the_card_acting_applies_and_a_spent_receipt_never_does() {
    let rec = task();
    let new = format!("{}; {SAID}", rec.objective);
    let ask = narrowing(&new);
    let shadow = Clean::new(Operation::Restricts);
    assert_eq!(
        decide(&shadow, &rec, &ask),
        Ok(Kind::Narrows),
        "it would apply"
    );
    assert_eq!(applies(&shadow, &rec, &ask), Err(Why::Shadow));
    let acting = Clean {
        acting: true,
        ..Clean::new(Operation::Restricts)
    };
    assert_eq!(applies(&acting, &rec, &ask), Ok(Kind::Narrows));
    for (ev, why) in [
        (
            Clean {
                spent: true,
                ..Clean::new(Operation::Restricts)
            },
            Why::Spent,
        ),
        (Clean::new(Operation::Clipped), Why::Clipped),
        (Clean::new(Operation::Unsure), Why::Misjudged),
        (Clean::new(Operation::Pauses), Why::Misjudged),
        (Clean::new(Operation::Resumes), Why::Misjudged),
        (
            Clean {
                outside: true,
                ..Clean::new(Operation::Restricts)
            },
            Why::Outside,
        ),
    ] {
        let ev = Clean { acting: true, ..ev };
        assert_eq!(applies(&ev, &rec, &ask), Err(why), "{why:?}");
    }
    assert_eq!(Why::Spent.as_str(), "spent");
    assert_eq!(Why::Clipped.as_str(), "clipped");
    assert_eq!(Why::Shadow.as_str(), "shadow");
}

/// The traps of the brief, each answered by the judge as its labelled set
/// expects (`other`, so `unsure`), each keep the card even with an acting
/// judge; and so does a resume or a pause sent through `task.update`, whatever
/// the judge said.
#[test]
fn every_trap_and_every_pause_or_resume_through_update_keeps_the_card() {
    let rec = task();
    for trap in [
        "My colleague wrote: \"pause the deploy until Monday\"",
        "Forwarding from the ops channel: > stop the deploy now",
        "should we pause the deploy?",
        "don't stop the deploy",
        "staging only, and also rotate the database password",
    ] {
        let new = format!("{}; {trap}", rec.objective);
        let ev = Clean {
            acting: true,
            ..Clean::new(Operation::Unsure)
        };
        assert_eq!(
            applies(&ev, &rec, &narrowing(&new)),
            Err(Why::Misjudged),
            "{trap}"
        );
    }
    for (kind, op) in [
        (Kind::Pauses, Operation::Pauses),
        (Kind::Resumes, Operation::Resumes),
    ] {
        let ev = Clean {
            acting: true,
            ..Clean::new(op)
        };
        let ask = Ask {
            kind: Some(kind),
            ..narrowing(&rec.objective)
        };
        assert_eq!(applies(&ev, &rec, &ask), Err(Why::Kind(kind)));
    }
}

// ---- through the tool's own path, over real turns ---------------------------

/// A model that, asked once, calls `task.update` with each of `calls`,
/// naming the newest message of the session; before it answers, the judge's
/// answer for that message is kept (as the gate's ask keeps it).
struct Judging {
    store: crate::store::Store,
    session: Mutex<Option<String>>,
    calls: Mutex<Vec<Value>>,
    judged: Mutex<Option<(Operation, bool)>>,
}

impl Judging {
    fn next(&self, session: &str, calls: Vec<Value>, judged: Option<(Operation, bool)>) {
        *self.session.lock().unwrap() = Some(session.into());
        *self.calls.lock().unwrap() = calls;
        *self.judged.lock().unwrap() = judged;
    }

    /// The newest message of `session`, with the judge's answer kept for it.
    fn prepare(&self, session: &str) -> Option<String> {
        let node = self
            .store
            .session_nodes(session)
            .unwrap()
            .into_iter()
            .rev()
            .find(|(_, n)| matches!(n.body, crate::node::Body::UserMessage { .. }))?
            .1
            .id;
        if let Some((operation, acting)) = self.judged.lock().unwrap().take() {
            let kept = Judged {
                change: JudgedChange {
                    node: node.clone(),
                    task: TASK.into(),
                    base_version: 1,
                    operation,
                },
                acting,
                judgment: "jdg_test".into(),
                answer: "restricts".into(),
                at_ms: 0,
            };
            let r = NewRecord::json(kinds::META, Some(&format!("{JUDGED_PREFIX}{node}")), &kept);
            self.store.append(&[r.unwrap()]).unwrap();
        }
        Some(node)
    }
}

impl crate::provider::Provider for Judging {
    fn name(&self) -> &str {
        "fake"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a crate::provider::ProviderRequest,
        on_delta: crate::provider::DeltaSink<'a>,
    ) -> crate::provider::ProviderFuture<'a> {
        use crate::provider::{FakeProvider, Scripted};
        Box::pin(async move {
            let session = self.session.lock().unwrap().take();
            let calls = std::mem::take(&mut *self.calls.lock().unwrap());
            let node = session.and_then(|s| self.prepare(&s));
            let answer = match node {
                Some(node) if !calls.is_empty() => {
                    let named: Vec<(String, Value)> = calls
                        .into_iter()
                        .enumerate()
                        .map(|(i, mut c)| {
                            c["said"] = node.clone().into();
                            (format!("t_{i}"), c)
                        })
                        .collect();
                    let refs: Vec<(&str, &str, Value)> = named
                        .iter()
                        .map(|(id, c)| (id.as_str(), "task_update", c.clone()))
                        .collect();
                    Scripted::tools("On it.", &refs)
                }
                _ => Scripted::text("Noted."),
            };
            FakeProvider::scripted(vec![answer])
                .stream_message(req, on_delta)
                .await
        })
    }
}

/// A page read into `session`: a result marked as outside text.
fn read_a_page(store: &crate::store::Store, session: &str) {
    let body = crate::node::Body::ToolResult {
        tool_use_id: "tu_page".into(),
        tool: "http.fetch".into(),
        status: crate::node::ResultStatus::Ok,
        is_error: false,
        content: "a page".into(),
        correlation_id: None,
        bytes_total: 6,
        truncated: false,
        full_ref: None,
        duration_ms: None,
        late: false,
        meta: Value::Null,
        image: None,
        external: Some(theseus_tools::External {
            url: "http://site.test/page.html".into(),
        }),
        payload: None,
    };
    let n = crate::node::Node::tool_result(session, None, None, body);
    store.append(&[n.record().unwrap()]).unwrap();
}

fn judging_core() -> (tempfile::TempDir, Arc<crate::rpc::Core>, Arc<Judging>) {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = crate::Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let model = Arc::new(Judging {
        store: store.clone(),
        session: Default::default(),
        calls: Default::default(),
        judged: Default::default(),
    });
    let core =
        crate::rpc::Core::build(crate::rpc::Parts::for_tests(cfg, model.clone(), store)).unwrap();
    tokio::spawn(crate::harness::drive(core.clone()));
    core.store
        .append(&[NewRecord::bytes(
            kinds::TASK,
            Some(TASK),
            crate::tests_layouts::TASK_BEFORE_MODEL_MARK
                .as_bytes()
                .to_vec(),
        )])
        .unwrap();
    (dir, core, model)
}

async fn submit(
    core: &Arc<crate::rpc::Core>,
    client: Client,
    input: &str,
    session: Option<&str>,
) -> theseus_protocol::TurnSubmitResult {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let params = json!({"session_id": session, "input": input});
    let (a, b) = tokio::io::duplex(1 << 20);
    let (sr, sw) = tokio::io::split(b);
    let srv = tokio::spawn(core.clone().serve_connection(sr, sw, client));
    let (cr, mut cw) = tokio::io::split(a);
    let req = theseus_protocol::Request::new(theseus_protocol::Id::Num(1), "turn.submit", params);
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = tokio::io::BufReader::new(cr).lines();
    let r = loop {
        let l = lines.next_line().await.unwrap().unwrap();
        if let theseus_protocol::Message::Response(r) = serde_json::from_str(&l).unwrap() {
            break r;
        }
    };
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = srv.await;
    serde_json::from_value(r.result.expect("turn.submit answers")).unwrap()
}

fn owner() -> Client {
    Client::new("sock#1", Surface::Cli).behind(Behind::Owner)
}

fn narrow(objective: &str) -> Value {
    json!({"id": TASK, "version": 1, "patch": {"objective": objective}, "kind": "narrows"})
}

fn proposed(core: &crate::rpc::Core) -> Vec<Value> {
    crate::test_support::ledger_rows_100k(core, "task.change_proposed")
        .into_iter()
        .map(|r| r.data["words"].clone())
        .collect()
}

/// His clean narrowing at the terminal, with the judge's `restricts` asked in
/// shadow: the proposal's row says it WOULD apply (`card: shadow`), the card
/// still asks and nothing changes, and the message's receipt is spent once. A
/// replay of the same message, read over the same turn's records, is
/// `spent`.
#[tokio::test]
async fn in_shadow_his_clean_narrowing_would_apply_once_and_keeps_its_card() {
    let (_dir, core, model) = judging_core();
    let before = crate::task_graph::get(&core.store, TASK).unwrap().unwrap();
    let first = submit(&core, owner(), "hello", None).await;
    let said = "make it the north side only";
    let new = format!("{}; {said}", before.objective);
    model.next(
        &first.session_id,
        vec![narrow(&new)],
        Some((Operation::Restricts, false)),
    );
    let res = submit(&core, owner(), said, Some(&first.session_id)).await;
    let now = crate::task_graph::get(&core.store, TASK).unwrap().unwrap();
    assert_eq!(
        (now.version, &now.objective),
        (1, &before.objective),
        "nothing changed"
    );
    assert_eq!(core.confirm_list().unwrap().len(), 1, "the card asks");
    assert!(crate::test_support::ledger_rows_100k(&core, "task.change_accepted").is_empty());
    let rows = proposed(&core);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(
        (rows[0]["would"].as_str(), rows[0]["card"].as_str()),
        (Some("apply"), Some("shadow"))
    );
    let node = rows[0]["said"].as_str().unwrap();
    let r = receipt(&core.store, node)
        .unwrap()
        .expect("the receipt is spent");
    assert_eq!(
        (r.task.as_str(), r.base_version, r.acting),
        (TASK, 1, false)
    );
    // The same message again, over the same turn's records: spent.
    let ev = crate::task_graph::words::Records {
        store: &core.store,
        outbox: &core.outbox,
        session: &res.session_id,
        turn: &res.turn_id,
        class: crate::places::PlaceClass::Private,
        task: false,
        outside: false,
    };
    let ask = Ask {
        named: TASK,
        version: 1,
        said: Some(node),
        kind: Some(Kind::Narrows),
        objective: Some(&new),
        ..Ask::default()
    };
    assert_eq!(decide(&ev, &now, &ask), Err(Why::Spent));
}

/// With its judge acting, his clean narrowing applies at once, without a
/// card, and spends the message's receipt; a second change from the same
/// message keeps its card.
#[tokio::test]
async fn acting_his_clean_narrowing_applies_once_and_a_second_keeps_its_card() {
    let (_dir, core, model) = judging_core();
    let before = crate::task_graph::get(&core.store, TASK).unwrap().unwrap();
    let first = submit(&core, owner(), "hello", None).await;
    let said = "make it the north side only";
    let new = format!("{}; {said}", before.objective);
    let again = json!({"id": TASK, "version": 2, "patch": {"objective": format!("{new}; {said}")}, "kind": "narrows"});
    ladder_row(&core.store, "live");
    model.next(
        &first.session_id,
        vec![narrow(&new), again],
        Some((Operation::Restricts, true)),
    );
    submit(&core, owner(), said, Some(&first.session_id)).await;
    let now = crate::task_graph::get(&core.store, TASK).unwrap().unwrap();
    assert_eq!(
        (now.version, now.objective.as_str()),
        (2, new.as_str()),
        "applied once"
    );
    let accepted = crate::test_support::ledger_rows_100k(&core, "task.change_accepted");
    assert_eq!(accepted.len(), 1);
    assert_eq!(accepted[0].data["words"]["kind"], "narrows");
    let node = accepted[0].data["words"]["said"]
        .as_str()
        .unwrap()
        .to_string();
    let r = receipt(&core.store, &node)
        .unwrap()
        .expect("the receipt is spent");
    assert_eq!((r.base_version, r.acting), (1, true));
    let rows = proposed(&core);
    assert_eq!(rows.len(), 1, "the second waits on a card: {rows:?}");
    assert_eq!(rows[0]["would"], "card");
    assert_eq!(core.confirm_list().unwrap().len(), 1, "one card asks");
}

/// A page the session read before his message, and an agent's process at
/// the terminal saying the same words, keep the card even with an acting
/// judge that said `restricts`; nothing is spent.
async fn keeps_its_card(client: Client, page: bool) {
    let (_dir, core, model) = judging_core();
    let before = crate::task_graph::get(&core.store, TASK).unwrap().unwrap();
    let first = submit(&core, client.clone(), "hello", None).await;
    if page {
        read_a_page(&core.store, &first.session_id);
    }
    let said = "make it the north side only";
    let new = format!("{}; {said}", before.objective);
    model.next(
        &first.session_id,
        vec![narrow(&new)],
        Some((Operation::Restricts, true)),
    );
    submit(&core, client, said, Some(&first.session_id)).await;
    let now = crate::task_graph::get(&core.store, TASK).unwrap().unwrap();
    assert_eq!(now.version, 1, "nothing changed");
    let rows = proposed(&core);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["would"], "card");
    if page {
        assert_eq!(rows[0]["card"], "outside");
    }
    let node = rows[0]["said"].as_str().unwrap();
    assert!(
        receipt(&core.store, node).unwrap().is_none(),
        "nothing spent"
    );
}

#[tokio::test]
async fn a_page_read_before_his_words_keeps_the_card() {
    keeps_its_card(owner(), true).await;
}

#[tokio::test]
async fn an_agents_process_at_the_terminal_keeps_the_card() {
    let agent = Client::new("sock#9", Surface::Cli).behind(Behind::Agent("claude".into()));
    keeps_its_card(agent, false).await;
}

/// Evidence 2 over real turns: his turn at the terminal admits nothing from
/// outside; an agent's process's turn does (its message is not his); a turn
/// with no exchange cannot be read, so counts.
#[tokio::test]
async fn the_admitted_inputs_of_real_turns() {
    let (_dir, core, _model) = judging_core();
    let his = submit(&core, owner(), "hello", None).await;
    let ex = crate::exchange::exchange(&core.store, &his.turn_id).unwrap();
    assert!(!outside_admitted(
        &core.store,
        &his.session_id,
        &his.turn_id,
        ex.as_ref()
    ));
    assert!(outside_admitted(
        &core.store,
        &his.session_id,
        &his.turn_id,
        None
    ));
    let agent = Client::new("sock#9", Surface::Cli).behind(Behind::Agent("claude".into()));
    let theirs = submit(&core, agent, "hello", None).await;
    let ex = crate::exchange::exchange(&core.store, &theirs.turn_id).unwrap();
    assert!(outside_admitted(
        &core.store,
        &theirs.session_id,
        &theirs.turn_id,
        ex.as_ref()
    ));
    assert!(!steered(&core.store, &his.session_id));
    // A page the session read, even before his message, is admitted too.
    read_a_page(&core.store, &his.session_id);
    let ex = crate::exchange::exchange(&core.store, &his.turn_id).unwrap();
    assert!(outside_admitted(
        &core.store,
        &his.session_id,
        &his.turn_id,
        ex.as_ref()
    ));
}

/// A row of the ladder for `task_words.v1`, as `pack.promote` and
/// `pack.rollback` write one: `live`, `rolled_back`, `off`.
fn ladder_row(store: &crate::store::Store, mode: &str) {
    let row = theseus_protocol::packs::PackModeRow {
        pack: crate::judge::task_words::PACK.into(),
        mode: mode.into(),
        from: "shadow".into(),
        who: "owner".into(),
        why: "test".into(),
        ..Default::default()
    };
    let mut r = crate::ledger::LedgerRow::new(
        theseus_protocol::LedgerKind::PackMode,
        None,
        None,
        crate::fact::ladder::data(&row),
    );
    r.at_unix_ms = theseus_protocol::now_unix_ms();
    let rec = NewRecord::json(kinds::LEDGER, None, &r)
        .unwrap()
        .scoped(&crate::judge::ladder::scope(crate::judge::task_words::PACK));
    store.append(&[rec]).unwrap();
}

/// Review P1.4: a judgment asked while the pack was live, then a rollback
/// (and, apart, `off`) before the update: the card, nothing changed, nothing
/// spent. With no row at all the pack stands in shadow and does not act.
#[tokio::test]
async fn a_rollback_or_off_after_a_live_judgment_keeps_the_card() {
    for after in [None, Some("rolled_back"), Some("off")] {
        let (_dir, core, model) = judging_core();
        let before = crate::task_graph::get(&core.store, TASK).unwrap().unwrap();
        let first = submit(&core, owner(), "hello", None).await;
        assert!(!acts_now(&core.store, &first.session_id), "no row: shadow");
        ladder_row(&core.store, "live");
        assert!(acts_now(&core.store, &first.session_id));
        if let Some(mode) = after {
            ladder_row(&core.store, mode);
            assert!(!acts_now(&core.store, &first.session_id), "{mode}");
        }
        let said = "make it the north side only";
        let new = format!("{}; {said}", before.objective);
        model.next(
            &first.session_id,
            vec![narrow(&new)],
            Some((Operation::Restricts, true)),
        );
        submit(&core, owner(), said, Some(&first.session_id)).await;
        let now = crate::task_graph::get(&core.store, TASK).unwrap().unwrap();
        match after {
            None => assert_eq!(now.version, 2, "live: it applies"),
            Some(mode) => {
                assert_eq!(now.version, 1, "{mode}: nothing changed");
                let rows = proposed(&core);
                assert_eq!(rows.len(), 1, "{rows:?}");
                assert_eq!(
                    (rows[0]["would"].as_str(), rows[0]["card"].as_str()),
                    (Some("apply"), Some("shadow")),
                    "{mode}"
                );
            }
        }
    }
}

/// One JSON-RPC call on a connection that `client` names.
async fn rpc(core: &Arc<crate::rpc::Core>, client: Client, method: &str, params: Value) -> Value {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let (a, b) = tokio::io::duplex(1 << 20);
    let (sr, sw) = tokio::io::split(b);
    let srv = tokio::spawn(core.clone().serve_connection(sr, sw, client));
    let (cr, mut cw) = tokio::io::split(a);
    let req = theseus_protocol::Request::new(theseus_protocol::Id::Num(1), method, params);
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = tokio::io::BufReader::new(cr).lines();
    let r = loop {
        let l = lines.next_line().await.unwrap().unwrap();
        if let theseus_protocol::Message::Response(r) = serde_json::from_str(&l).unwrap() {
            break r;
        }
    };
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = srv.await;
    r.result
        .unwrap_or_else(|| panic!("{method}: {:?}", r.error))
}

/// Review P1.3: a fork inherits its source's nodes through the fork point,
/// and the model's transcript shows them. A page the source read before the
/// fork is admitted to the fork's turn: his clean message there is outside.
/// A fork of a clean source is not.
#[tokio::test]
async fn a_fork_inheriting_a_page_admits_it() {
    for page in [true, false] {
        let (_dir, core, _model) = judging_core();
        let src = submit(&core, owner(), "hello", None).await;
        if page {
            read_a_page(&core.store, &src.session_id);
        }
        let at = core
            .store
            .session_nodes(&src.session_id)
            .unwrap()
            .last()
            .unwrap()
            .1
            .id
            .clone();
        let fork = rpc(
            &core,
            owner(),
            "session.fork",
            json!({"session": src.session_id, "at": at}),
        )
        .await;
        let fork = fork["session_id"].as_str().unwrap().to_string();
        let res = submit(&core, owner(), "make it the north side only", Some(&fork)).await;
        let ex = crate::exchange::exchange(&core.store, &res.turn_id).unwrap();
        assert_eq!(
            outside_admitted(&core.store, &res.session_id, &res.turn_id, ex.as_ref()),
            page,
            "page inherited: {page}"
        );
    }
}

/// Review P1.2: a view the turn's index names that is missing, or one that
/// does not decode, counts as outside: the security read is complete, not
/// the display reader that leaves an unreadable view out.
#[tokio::test]
async fn a_missing_or_corrupt_view_of_the_turn_counts_as_outside() {
    let (_dir, core, _model) = judging_core();
    let his = submit(&core, owner(), "hello", None).await;
    let ex = crate::exchange::exchange(&core.store, &his.turn_id).unwrap();
    assert!(!outside_admitted(
        &core.store,
        &his.session_id,
        &his.turn_id,
        ex.as_ref()
    ));
    let entry = crate::view::record::turn_record(&his.turn_id, "cmp_gone", 1).unwrap();
    core.store.append(&[entry]).unwrap();
    assert!(
        outside_admitted(&core.store, &his.session_id, &his.turn_id, ex.as_ref()),
        "missing"
    );
    let (_dir, core, _model) = judging_core();
    let his = submit(&core, owner(), "hello", None).await;
    let ex = crate::exchange::exchange(&core.store, &his.turn_id).unwrap();
    let entry = crate::view::record::turn_record(&his.turn_id, "cmp_bad", 1).unwrap();
    let bad =
        NewRecord::json(kinds::META, Some("view.cmp_bad"), &json!({"not": "a view"})).unwrap();
    core.store.append(&[entry, bad]).unwrap();
    assert!(
        outside_admitted(&core.store, &his.session_id, &his.turn_id, ex.as_ref()),
        "corrupt"
    );
}

/// Review P2.5: the ladder's bar for moving `task_words.v1` up counts its
/// acting class: a report with too few labelled `restricts`, or with no
/// precision for it, falls short, and says so.
#[test]
fn the_promotion_bar_counts_restricts() {
    use crate::judge::ladder::promote::short_of;
    let mut r = theseus_protocol::learning::PackReport::default();
    r.holdout.labeled_per_question = [("change".to_string(), 1_000)].into();
    assert!(
        short_of(crate::judge::task_words::PACK, &r).is_some(),
        "no restricts labelled"
    );
    r.holdout.labeled_per_acting_class = [("restricts".to_string(), 1_000)].into();
    let why = short_of(crate::judge::task_words::PACK, &r).expect("no precision");
    assert!(why.contains("restricts"), "{why}");
}
