//! A core with a one-hour cache, for the tests that need one (`tests_persona_layer`). These rigs were
//! `tests_keep_warm`'s, whose read went on 2026-11-22 (issue 44: no saved request is sent again).

use std::sync::Arc;

use theseus_protocol::SessionKind;

use crate::bus::EventSink;
use crate::provider::FakeProvider;
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

/// The template with a 1-hour `[cache] ttl`, so every profile that says none
/// (the live one, sonnet, among them) caches for an hour, and every request
/// asks the provider to cache for an hour.
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
