//! What the core's tests share: a scripted turn on a test core, the ledger's
//! rows of a kind (each helper named for the window it reads), and a poll.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;
use theseus_protocol::{SessionKind, TurnSubmitResult};
use theseus_store::{kinds, NewRecord};

use crate::bus::EventSink;
use crate::ledger::LedgerRow;
use crate::session::SessionRecord;
use crate::turn::TurnRequest;
use crate::Core;

/// `sid` carries a `flag` label, as a Google result's write site leaves it
/// (source `google.call`), without a turn that read one.
pub(crate) fn label_session(core: &Core, sid: &str, flag: crate::content::Flag) {
    let mut f = crate::content::ContentFlags::default();
    f.raise(crate::content::deterministic::label(
        flag,
        "nod_test",
        crate::content::ItemKind::ToolResult,
        "google.call".into(),
        1,
    ));
    core.store
        .update_session(sid, |rec| {
            rec.data = Some(Box::new(f));
            Ok(vec![])
        })
        .unwrap();
}

pub(crate) async fn turn(core: &Arc<Core>, sid: &str, input: &str) -> TurnSubmitResult {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = EventSink::new(core.bus.clone(), sid, None);
    core.runner
        .run(TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
        .unwrap()
}

pub(crate) async fn turn_in(
    core: &Arc<Core>,
    session: Option<&str>,
    input: &str,
) -> TurnSubmitResult {
    let rec = match session {
        Some(id) => core
            .store
            .get_session::<SessionRecord>(id)
            .unwrap()
            .unwrap(),
        None => {
            let r = SessionRecord::new(SessionKind::Conversation, None);
            core.store.put_session(&r.session_id, &r).unwrap();
            r
        }
    };
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = EventSink::new(core.bus.clone(), &rec.session_id, None);
    core.runner
        .run(TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
        .unwrap()
}

pub(crate) async fn try_turn(
    core: &Arc<Core>,
    sid: &str,
    input: &str,
) -> anyhow::Result<TurnSubmitResult> {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None)?;
    let sink = EventSink::new(core.bus.clone(), sid, None);
    core.runner
        .run(TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
}

pub(crate) fn rows(core: &Core, kind: &str) -> Vec<Value> {
    core.store
        .ledger_tail::<crate::ledger::LedgerRow>(10_000)
        .unwrap()
        .into_iter()
        .map(|(_, r)| r)
        .filter(|r| r.kind == kind)
        .map(|r| r.data)
        .collect()
}

pub(crate) fn ledger_rows_2k(core: &Core, kind: &str) -> Vec<LedgerRow> {
    core.store
        .ledger_tail::<LedgerRow>(2000)
        .unwrap()
        .into_iter()
        .map(|(_, r)| r)
        .filter(|r| r.kind == kind)
        .collect()
}

pub(crate) fn rows_100k(core: &Core, kind: &str) -> Vec<Value> {
    core.store
        .ledger_tail::<crate::ledger::LedgerRow>(100_000)
        .unwrap()
        .into_iter()
        .filter(|(_, r)| r.is_kind(kind))
        .map(|(_, r)| r.data)
        .collect()
}

pub(crate) fn ledger_rows_100k(core: &Core, kind: &str) -> Vec<LedgerRow> {
    core.store
        .ledger_tail::<LedgerRow>(100_000)
        .unwrap()
        .into_iter()
        .map(|(_, r)| r)
        .filter(|r| r.kind == kind)
        .collect()
}

/// The ledger rows of `kind` in the session, oldest first.
pub(crate) fn session_rows_1k(core: &Core, sid: &str, kind: &str) -> Vec<Value> {
    let rows: Vec<(u64, crate::ledger::LedgerRow)> = core.store.ledger_tail(1000).unwrap();
    rows.into_iter()
        .filter(|(_, r)| r.kind == kind && r.session_id.as_deref() == Some(sid))
        .map(|(_, r)| r.data)
        .collect()
}

/// The session's rows of `kind`, scoped to its recalls.
pub(crate) fn recall_rows(core: &Core, sid: &str, kind: &str) -> Vec<LedgerRow> {
    core.store
        .scope_after(&crate::fact::recall::scope(sid), 0)
        .unwrap()
        .iter()
        .map(|r| r.decode::<LedgerRow>().unwrap())
        .filter(|r| r.kind == kind)
        .collect()
}

/// The ledger rows of `kind`, oldest first.
pub(crate) fn rows_2k(core: &Core, kind: &str) -> Vec<Value> {
    let rows: Vec<(u64, crate::ledger::LedgerRow)> = core.store.ledger_tail(2000).unwrap();
    rows.into_iter()
        .filter(|(_, r)| r.kind == kind)
        .map(|(_, r)| r.data)
        .collect()
}

pub(crate) fn ledger_rows_5k(core: &Core, kind: &str) -> Vec<crate::ledger::LedgerRow> {
    let rows: Vec<(u64, crate::ledger::LedgerRow)> = core.store.ledger_tail(5000).unwrap();
    rows.into_iter()
        .map(|(_, r)| r)
        .filter(|r| r.kind == kind)
        .collect()
}

pub(crate) async fn until_within(what: &str, secs: u64, mut f: impl FnMut() -> bool) {
    let t0 = Instant::now();
    while !f() {
        assert!(
            t0.elapsed() < Duration::from_secs(secs),
            "no {what} in {secs} s"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Until `f` holds, with no clock: for a wait on the very event the next line reads, where a
/// slow host and a lost event differ only in how long it takes. A hang ends at nextest's
/// `slow-timeout` kill (`.config/nextest.toml`), never by a deadline of the test's own. A line
/// every 30 s of wall time says what is awaited, so a hang names itself in nextest's captured
/// output.
pub(crate) async fn until_event(what: &str, mut f: impl FnMut() -> bool) {
    let t0 = Instant::now();
    let mut said = 0;
    while !f() {
        if t0.elapsed() >= Duration::from_secs(30 * (said + 1)) {
            said += 1;
            eprintln!("waiting for {what} ({} s)", t0.elapsed().as_secs());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

pub(crate) async fn until(what: &str, mut f: impl FnMut() -> bool) {
    let t0 = Instant::now();
    while !f() {
        assert!(t0.elapsed() < Duration::from_secs(20), "no {what} in 20 s");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

pub(crate) async fn until_within_10ms(what: &str, secs: u64, mut f: impl FnMut() -> bool) {
    let t0 = Instant::now();
    while !f() {
        assert!(
            t0.elapsed() < Duration::from_secs(secs),
            "no {what} in {secs} s"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Until the push board has applied every frame committed so far (theseus-fpvn8).
/// The board applies frames on a thread of its own, in the order their commits
/// returned, so a turn's return does not say its frames are on the board. One
/// more frame is committed, the session's own record unchanged, and the board's
/// feed waited on until it reaches that frame's position: frames queue in commit
/// order, so every frame committed before it is applied by then. The push must
/// be seeded, so that it watches.
pub(crate) async fn board_caught_up(core: &Arc<Core>, session_id: &str) {
    let rec: SessionRecord = core.store.get_session(session_id).unwrap().unwrap();
    let at = core
        .store
        .append(&[NewRecord::json(kinds::SESSION, Some(session_id), &rec).unwrap()])
        .unwrap()
        .into_iter()
        .max()
        .unwrap();
    let mut feed = core.push.feed();
    tokio::time::timeout(Duration::from_secs(20), feed.wait_for(|p| *p >= at))
        .await
        .expect("the board caught up within 20 s")
        .expect("the board's feed is open");
}

/// The owner at their own terminal: who the owner's acts are judged for.
pub(crate) fn owner_cli() -> crate::approval::Answerer {
    crate::approval::Answerer {
        label: "the CLI".into(),
        surface: crate::approval::Surface::Cli,
        discord: None,
        agent: None,
    }
}

/// A person in a shared channel nobody bound as private.
pub(crate) fn stranger() -> crate::approval::Answerer {
    crate::approval::Answerer {
        label: "discord:777".into(),
        surface: crate::approval::Surface::Discord,
        discord: Some(theseus_protocol::DiscordOrigin {
            user_id: "777".into(),
            channel_id: "9001".into(),
            guild_id: Some("7".into()),
        }),
        agent: None,
    }
}
