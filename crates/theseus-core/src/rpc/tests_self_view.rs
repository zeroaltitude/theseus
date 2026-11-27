//! The self loop's own sessions on the view (issue 44, the cut): a backlog builder's session, opened by the
//! night's start (`rsi::backlog::start`), and a reviewer's, opened by `CheckTurn`, are ordinary sessions of
//! the principal `self` whose inputs carry no owner's mark, so who is present is unknown. A turn always sees
//! what its own session said at its own place, whoever is present: a second turn after a restart holds the
//! objective and the first answer; nothing is summarised. A builder reads at the widest, so another session
//! at its place is shown to it (issue 53); a reviewer keeps to its own place.

use std::sync::{Arc, Mutex};

use serde_json::Value;
use theseus_protocol::rsi::{SelfMode, SelfResumeParams};

use super::tests_self_backlog::{seed_day, tick};
use super::{Core, Parts};
use crate::approval::Answerer;
use crate::bus::EventSink;
use crate::provider::{
    DeltaSink, FakeProvider, Provider, ProviderFuture, ProviderRequest, Scripted,
};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;

const FIRST: &str = "FIRST-SELF: the cause is a retry that never backs off.";
const AGAIN: &str = "AGAIN: go on from where you were";
const UNRELATED: &str = "UNRELATED-WORDS: the spare key is under the third bollard";

/// A stand-in model that keeps every request: `AGAIN` is answered "Going on.", anything else `FIRST`.
#[derive(Default)]
struct Model {
    requests: Mutex<Vec<ProviderRequest>>,
}

impl Provider for Model {
    fn name(&self) -> &str {
        "fake"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            self.requests.lock().unwrap().push(req.clone());
            let sent = serde_json::to_string(&req.messages).unwrap_or_default();
            let last = req
                .messages
                .last()
                .map(|m| m.to_string())
                .unwrap_or_default();
            let answer = match () {
                _ if last.contains(AGAIN) => "Going on.",
                _ if sent.contains(UNRELATED) => "Noted.",
                _ => FIRST,
            };
            let f = FakeProvider::scripted(vec![Scripted::text(answer)]);
            f.stream_message(req, on_delta).await
        })
    }
}

impl Model {
    /// The newest turn's request (it offers tools) whose last message says `what`, as sent.
    fn asked(&self, what: &str) -> String {
        let all = self.requests.lock().unwrap();
        let q = (all.iter().rev())
            .find(|q| {
                !q.tools.is_empty()
                    && q.messages
                        .last()
                        .is_some_and(|m| m.to_string().contains(what))
            })
            .unwrap_or_else(|| panic!("no request asked {what:?}"));
        serde_json::to_string(&q.messages).unwrap()
    }

    /// Requests that offer no tools (a summary writer's) holding `what`.
    fn untooled(&self, what: &str) -> usize {
        (self.requests.lock().unwrap().iter())
            .filter(|q| {
                q.tools.is_empty() && serde_json::to_string(q).unwrap_or_default().contains(what)
            })
            .count()
    }
}

/// The settings of a core whose self loop acts, every place on the view.
fn config() -> (crate::Config, Store) {
    let base = super::tests::test_core("unused");
    let mut cfg = (*base.cfg).clone();
    cfg.self_improve.mode = SelfMode::Act;
    cfg.server.disk_warn_mb = 0;
    cfg.self_improve.backlog_starts = Some(1);
    cfg.self_improve.pipeline_builds = 1;
    cfg.self_improve.backlog_max_open = 20;
    let repo = crate::rsi::tests_tree::fixture();
    cfg.self_improve.repo = Some(repo.place.repo.display().to_string());
    std::mem::forget(repo);
    (cfg, base.store.clone())
}

/// A core over `store` (a second one over the same store is a restart).
fn core_over(cfg: &crate::Config, store: &Store, model: &Arc<Model>) -> Arc<Core> {
    let core = Core::build(Parts::for_tests(cfg.clone(), model.clone(), store.clone())).unwrap();
    core.tools
        .self_pr
        .set_env(crate::rsi::tests_tree::hermetic());
    core
}

/// A turn of `session` as the self loop runs one: no arrival mark.
async fn self_turn(c: &Arc<Core>, session: &str, input: &str) {
    let rec: SessionRecord = c.store.get_session(session).unwrap().unwrap();
    let (live, _) = c.live_profile();
    let target = c.runner.target_for_session(&rec, &live).unwrap();
    let sink = EventSink::new(c.bus.clone(), session, None);
    c.runner
        .run(TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "self:backlog".into(),
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
        .unwrap();
}

/// An ordinary session working in `dir`, with no arrival mark.
fn plain_in(c: &Core, dir: Option<&str>) -> String {
    let mut rec = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    rec.dir = dir.map(str::to_string);
    c.store.put_session(&rec.session_id, &rec).unwrap();
    rec.session_id
}

fn strategy(c: &Core, session: &str) -> String {
    let rec: SessionRecord = c.store.get_session(session).unwrap().unwrap();
    let id = rec.compilation_id.expect("a compilation");
    c.store.get_compilation(&id).unwrap().unwrap().strategy
}

/// The key test: a builder's session as the night opens it; its first turn reads the item's objective;
/// after a restart its second turn holds that objective and its first answer, and, read at the widest, what
/// an ordinary session said in the same tree; no summary is written for it.
#[tokio::test]
async fn a_builders_second_turn_after_a_restart_holds_its_objective_and_first_answer() {
    let (cfg, store) = config();
    let model = Arc::new(Model::default());
    let c = core_over(&cfg, &store, &model);
    c.self_resume(&SelfResumeParams::default(), &Answerer::from("the CLI"))
        .unwrap();
    seed_day(&c, theseus_protocol::now_unix_ms());
    let night = c.backlog_night(tick()).unwrap();
    let item = night.started[0].item.clone();
    let rec = crate::task_graph::get(&c.store, &item).unwrap().unwrap();
    let session = rec.session.clone().unwrap();
    assert_eq!(
        c.tools.self_pr.role(&session),
        Some(crate::rsi::roles::Role::Builder),
        "the night's start records its builder's role"
    );
    let objective = rec.objective.clone();
    let head: String = objective.chars().take(40).collect();
    self_turn(&c, &session, &objective).await;
    assert!(model.asked(&head).contains(&head));
    let dir: Option<String> = c
        .store
        .get_session::<SessionRecord>(&session)
        .unwrap()
        .unwrap()
        .dir;
    let other = plain_in(&c, dir.as_deref());
    self_turn(&c, &other, UNRELATED).await;
    // A restart: a second core over the same store, which holds no presence of the first.
    let c2 = core_over(&cfg, &store, &model);
    self_turn(&c2, &session, AGAIN).await;
    let sent = model.asked(AGAIN);
    assert!(sent.contains(&head), "its objective: {sent}");
    assert!(sent.contains(FIRST), "its first answer: {sent}");
    assert!(
        sent.contains("third bollard"),
        "another session's words, at the widest: {sent}"
    );
    assert_eq!(strategy(&c2, &session), "view");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert_eq!(
        model.untooled("FIRST-SELF"),
        0,
        "no summary of a self session"
    );
}

/// A reviewer's session as `CheckTurn` opens it: its first request holds the claim it reviews; a second turn
/// after a restart holds that claim and its first answer.
#[tokio::test]
async fn a_reviewers_second_turn_after_a_restart_holds_its_claim_and_first_answer() {
    use crate::rsi::join::review::{Ask, CheckTurn, Reviewer};
    let (cfg, store) = config();
    let model = Arc::new(Model::default());
    let c = core_over(&cfg, &store, &model);
    let scratch = crate::scratch::dir("review");
    let (live, _) = c.live_profile();
    let ask = Ask {
        branch: "self/retry-backoff".into(),
        head: "abc123".into(),
        item: "tsk_review_item".into(),
        diff: "+ backoff(attempt)".into(),
        report: "REPORT-UNDER-REVIEW: the retry now backs off".into(),
        evidence: "three failed calls in a night".into(),
        proof: String::new(),
        target: c.runner.resolve_target(&live, None, None, None).unwrap(),
        built_ms: 1,
        scratch: scratch.to_string_lossy().into_owned(),
        repo: String::new(),
        offer_fix: None,
        fixed: None,
    };
    let review = CheckTurn.review(&c, &ask).await.unwrap();
    let session = review.session.expect("its session");
    assert!(model
        .asked("REPORT-UNDER-REVIEW")
        .contains("REPORT-UNDER-REVIEW"));
    let c2 = core_over(&cfg, &store, &model);
    self_turn(&c2, &session, AGAIN).await;
    let sent = model.asked(AGAIN);
    assert!(sent.contains("REPORT-UNDER-REVIEW"), "its claim: {sent}");
    assert!(sent.contains(FIRST), "its first answer: {sent}");
    let _: Value = serde_json::from_str(&sent).unwrap();
}
