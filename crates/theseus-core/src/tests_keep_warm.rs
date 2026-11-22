//! The keep-warm read's money rules, on the path a job wait's read takes
//! (`TurnRunner::keep_warm_send`, theseus-ezeg, theseus-7m7w.3): past the day
//! ceiling it is refused before any call and said once; past the hard
//! multiple of a notifying limit it is refused before any call; and a turn's
//! end drops the room a task wait recorded. (The reads BETWEEN turns went
//! with the transcript, issue 44.) The core rigs here are shared by other
//! tests of a one-hour cache.

use std::sync::Arc;

use theseus_protocol::SessionKind;

use crate::bus::EventSink;
use crate::keep_warm::{Built, Outcome, MAX_TOKENS};
use crate::provider::{FakeProvider, ProviderRequest};
use crate::session::SessionRecord;
use crate::store::Store;
pub(crate) use crate::test_support::ledger_rows_5k as rows;
use crate::turn::TurnRequest;
use crate::{Config, Core};

/// The template with a 1-hour `[cache] ttl`, so every profile that says none
/// (the live one, sonnet, among them) caches for an hour, and every request
/// is worth keeping warm.
pub(crate) fn one_hour() -> Config {
    let text = Config::EXAMPLE_TOML.replace("\n[cache]\n", "\n[cache]\nttl = \"1h\"\n");
    let mut c = Config::parse(&text).unwrap().0;
    c.cache.keep_warm_min_tokens = 0;
    c
}

pub(crate) fn build(dir: &std::path::Path, c: Config) -> (Arc<Core>, Arc<FakeProvider>) {
    let fake = Arc::new(FakeProvider::default());
    (build_with(dir, c, fake.clone()), fake)
}

pub(crate) fn build_with(dir: &std::path::Path, c: Config, fake: Arc<FakeProvider>) -> Arc<Core> {
    let mut c = c;
    c.server.state_dir = dir.to_string_lossy().into_owned();
    c.tools.roots = vec![];
    let store = Store::open(&dir.join("store")).unwrap();
    Core::build(crate::rpc::Parts::for_tests(c, fake, store)).unwrap()
}

/// A conversation, posting to `place` when given (unbound: the CLI's).
pub(crate) fn session(core: &Core, kind: SessionKind, place: Option<&str>) -> String {
    let r = SessionRecord::new(kind, None);
    core.store.put_session(&r.session_id, &r).unwrap();
    if let Some(p) = place {
        core.outbox.bind_place(p, &r.session_id).unwrap();
    }
    r.session_id
}

/// A person's message and its turn, as `turn.submit` makes them.
pub(crate) async fn turn(core: &Arc<Core>, sid: &str, input: &str) {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    core.runner
        .run(TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink: EventSink::new(core.bus.clone(), sid, None),
            author: "test".into(),
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
        .unwrap();
}

/// The requests that were keep-warm reads: the smallest output.
pub(crate) fn reads(fake: &FakeProvider) -> Vec<ProviderRequest> {
    fake.requests()
        .into_iter()
        .filter(|r| r.max_tokens == MAX_TOKENS)
        .collect()
}

/// The read a wait makes of `sid`'s turn: its last request again at the
/// smallest output, and the session's execution.
fn wait_read(
    core: &Core,
    fake: &FakeProvider,
    sid: &str,
) -> (Built, Option<theseus_kernel::Execution>) {
    let mut request = fake.requests().last().cloned().expect("the turn's request");
    request.max_tokens = MAX_TOKENS;
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let execution_id = rec.execution_id.expect("an execution");
    let execution = core.kernel.execution(&execution_id).unwrap();
    let built = Built {
        request,
        target,
        execution_id,
        tokens: 1,
        during_wait: true,
    };
    (built, execution)
}

/// Past the day ceiling a wait's read is refused before any call: the day's
/// `spend.ceiling` row says it once, and nothing is booked.
#[tokio::test]
async fn a_wait_read_past_the_daily_ceiling_is_refused_and_said_once() {
    let dir = tempfile::tempdir().unwrap();
    // The ceiling's stop mode (theseus-9e02): under the default, notify, it warns and refuses nothing.
    let mut c = one_hour();
    c.kernel.daily_spend_ceiling_mode = crate::config::DailyCeilingMode::Stop;
    let (core, fake) = build(dir.path(), c);
    let sid = session(&core, SessionKind::Conversation, None);
    turn(&core, &sid, "before the ceiling").await;
    core.kernel.day_ceiling().set_limit(1);
    let (built, execution) = wait_read(&core, &fake, &sid);
    let got = core.runner.keep_warm_send(&sid, built, execution).await;
    assert!(
        matches!(&got, Outcome::Stopped(why) if why.contains("daily ceiling")),
        "{got:?}"
    );
    assert!(reads(&fake).is_empty(), "no call past the ceiling");
    assert_eq!(rows(&core, "spend.ceiling").len(), 1);
    assert!(rows(&core, "keep_warm").is_empty());
}

/// A limit that notifies lets a read go past the session's limit, but not
/// past its hard multiple (theseus-jse4): a session already at ten times its
/// limit is refused before any call.
#[tokio::test]
async fn a_wait_read_past_the_hard_multiple_of_a_notifying_limit_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = one_hour();
    c.kernel.spend_limit_usd = 1.0;
    c.kernel.spend_limit_mode = crate::config::SpendLimitMode::Notify;
    let (core, fake) = build(dir.path(), c);
    let sid = session(&core, SessionKind::Conversation, None);
    turn(&core, &sid, "before the stop").await;
    let rec: SessionRecord = core.store.get_session(&sid).unwrap().unwrap();
    let exec = rec.execution_id.unwrap();
    core.kernel
        .book_spend(
            &exec,
            10_000_000,
            theseus_protocol::LedgerKind::KeepWarm,
            serde_json::json!({}),
        )
        .unwrap();
    let (built, execution) = wait_read(&core, &fake, &sid);
    let got = core.runner.keep_warm_send(&sid, built, execution).await;
    assert!(
        matches!(&got, Outcome::Stopped(why) if why.contains("hard stop")),
        "{got:?}"
    );
    assert!(reads(&fake).is_empty(), "no call past the hard stop");
}

/// A turn's end drops the room it recorded for a task wait, so a session's
/// entry does not outlive its turn (the stack review's N10).
#[test]
fn a_turns_end_forgets_the_room_a_wait_recorded() {
    let k = crate::keep_warm::KeepWarm::default();
    k.set_room("ses_room", 40_000, 200_000);
    assert_eq!(k.room("ses_room"), Some((40_000, 200_000)));
    k.turn_ended("ses_room");
    assert_eq!(k.room("ses_room"), None);
}
