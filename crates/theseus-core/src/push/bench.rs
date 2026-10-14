//! The push's micro-bench (theseus-hkx30): what one committed frame costs
//! the applier thread, both boards and the fan-out to the watchers, over a
//! recorded stream.
//!
//! The stream is recorded from a scratch core running real turns on a
//! scripted model (plain turns, and turns with an in-process tool call, over
//! a few dozen conversations), every frame the kernel and the store commit as
//! the observer saw it, until it holds `frames`. It is replayed into a push
//! whose boards start empty, `watchers` `work.watch` connections registered
//! (queues whose writers drain them, so each frame is serialized once and
//! queued to each), and each frame's `apply` is timed. The numbers are the
//! applier's, per frame: the lifecycle report holds their p95 to 20 µs.

use std::sync::Arc;
use std::time::Instant;

use anyhow::{bail, Result};
use serde_json::json;
use theseus_protocol::SessionKind;
use tokio::sync::mpsc;

use super::{Frame, Push, WorkScope};
use crate::bus::EventSink;
use crate::provider::{FakeProvider, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

/// Conversations the stream is spread over.
const SESSIONS: usize = 40;

/// What replaying a stream cost the applier, per frame, in microseconds.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub watchers: usize,
    pub frames: usize,
    /// Every frame's time, in the order replayed.
    pub samples_us: Vec<f64>,
}

/// A scratch core on a scripted model: turns alternate plain and with one
/// in-process tool call, so the stream holds both shapes.
fn scratch(dir: &std::path::Path, turns: usize) -> Result<Arc<Core>> {
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.to_string_lossy().into_owned();
    let work = dir.join("work");
    std::fs::create_dir_all(&work)?;
    cfg.tools.projects_dir = Some(work.canonicalize()?.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = crate::policy::Posture::Open;
    let mut script = Vec::new();
    for i in 0..turns {
        if i % 2 == 1 {
            script.push(Scripted::tools(
                "",
                &[("t1", "text_diff", json!({"a": "one\n", "b": "two\n"}))],
            ));
        }
        script.push(Scripted::text("done"));
    }
    let store = Store::open_unsynced(&dir.join("store"))?;
    let parts = crate::rpc::Parts {
        providers: [(
            cfg.model.provider.clone(),
            Arc::new(FakeProvider::scripted(script)) as Arc<dyn crate::provider::Provider>,
        )]
        .into_iter()
        .collect(),
        cfg,
        store,
        secrets: crate::secrets::SecretBoard::empty(),
        startup_log: Arc::default(),
        telemetry: Some(crate::telemetry::Telemetry::disabled()),
        scrubber: Arc::new(crate::scrub::Scrubber::default()),
        launcher: Arc::new(crate::toolrun::InlineLauncher),
        config_gate: crate::config_gate::ConfigGate::file("bench"),
        toollets: vec![],
        cpu_cores: None,
    };
    Core::build(parts)
}

/// Record at least `frames` frames of real turns.
async fn record(core: &Arc<Core>, frames: usize) -> Result<Vec<Frame>> {
    let (tx, mut rx) = mpsc::unbounded_channel::<Option<Frame>>();
    let o = super::observer(tx);
    if !core.kernel.observe(o.clone()) || !core.store.observe(o) {
        bail!("an observer is installed already");
    }
    let mut sessions = Vec::new();
    for _ in 0..SESSIONS {
        let r = SessionRecord::new(SessionKind::Conversation, None);
        core.store.put_session(&r.session_id, &r)?;
        sessions.push(r.session_id);
    }
    let mut out: Vec<Frame> = Vec::new();
    let mut turn = 0;
    while out.len() < frames {
        let sid = &sessions[turn % SESSIONS];
        let rec: SessionRecord = core
            .store
            .get_session(sid)?
            .ok_or_else(|| anyhow::anyhow!("no session {sid}"))?;
        let (live, _) = core.live_profile();
        let target = core.runner.resolve_target(&live, None, None, None)?;
        core.runner
            .run(TurnRequest {
                prompt: None,
                session: rec,
                input: Some(format!("turn {turn}")),
                target,
                sink: EventSink::new(core.bus.clone(), sid, None),
                author: "bench".into(),
                recompile: None,
                attachments: vec![],
                arrived: None,
                reply_to: None,
            })
            .await?;
        turn += 1;
        while let Ok(Some(f)) = rx.try_recv() {
            out.push(f);
        }
    }
    out.truncate(frames);
    Ok(out)
}

/// Replay `stream` into a push whose boards start empty, with `watchers`
/// work watchers, timing each frame.
fn replay(
    core: &Arc<Core>,
    rt: &tokio::runtime::Handle,
    stream: &[Frame],
    watchers: usize,
) -> Row {
    let push = Push::default();
    let mut drains = Vec::new();
    for i in 0..watchers {
        let (tx, mut rx) = crate::outbound::channel_capped(Arc::default(), usize::MAX);
        core.bus
            .watch_work(&format!("bench#{i}"), tx, WorkScope::default(), false);
        // The connection's writer: it takes each line off the queue.
        drains.push(rt.spawn(async move {
            while rx.recv().await.is_some() {
                rx.written();
            }
        }));
    }
    let mut samples_us = Vec::with_capacity(stream.len());
    for f in stream {
        let f = f.clone();
        let t = Instant::now();
        push.apply(core, f);
        samples_us.push(t.elapsed().as_secs_f64() * 1e6);
    }
    for i in 0..watchers {
        core.bus.drop_conn(&format!("bench#{i}"));
    }
    for d in drains {
        let _ = rt.block_on(d);
    }
    eprintln!("DBG board {} us/frame, work {} us/frame", super::DBG_BOARD.swap(0, std::sync::atomic::Ordering::Relaxed) / 1000 / stream.len() as u64, super::DBG_WORK.swap(0, std::sync::atomic::Ordering::Relaxed) / 1000 / stream.len() as u64);
    Row {
        watchers,
        frames: stream.len(),
        samples_us,
    }
}

/// Record `frames` frames, then replay them once for each count of watchers.
pub fn run(frames: usize, watchers: &[usize]) -> Result<Vec<Row>> {
    let dir = std::env::temp_dir().join(format!(
        "theseus-push-bench-{}-{}",
        std::process::id(),
        theseus_protocol::now_unix_ms()
    ));
    std::fs::create_dir_all(&dir)?;
    let watchers = watchers.to_vec();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let (core, stream) = rt.block_on(async {
        let turns = frames / 3 + SESSIONS;
        let core = scratch(&dir, turns)?;
        let stream = record(&core, frames).await?;
        anyhow::Ok((core, stream))
    })?;
    let out = rt.block_on(async {
        // The replay is the applier thread's work: a blocking thread, as
        // production's is.
        let c = core.clone();
        let handle = tokio::runtime::Handle::current();
        tokio::task::spawn_blocking(move || {
            watchers
                .iter()
                .map(|w| replay(&c, &handle, &stream, *w))
                .collect::<Vec<Row>>()
        })
        .await
        .map_err(anyhow::Error::from)
    })?;
    drop(core);
    drop(rt);
    let _ = std::fs::remove_dir_all(&dir);
    Ok(out)
}
