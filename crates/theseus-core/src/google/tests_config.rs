//! With no `[google]` section nothing Google exists.

use super::tests::*;
use crate::Config;

#[tokio::test]
async fn no_google_section_registers_nothing_and_reads_nothing() {
    let cfg = Config::example();
    assert!(cfg.google.is_empty());
    let dir = tempfile::tempdir().unwrap();
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let model = std::sync::Arc::new(crate::provider::FakeProvider::scripted(vec![
        crate::provider::Scripted::text("Hi."),
    ]));
    let core = crate::Core::build(crate::rpc::Parts::for_tests(cfg, model.clone(), store)).unwrap();
    assert!(core.tools.google.is_none());
    for t in core.tools.registry.all() {
        let name = t.name();
        assert!(
            ![
                "google.",
                "gmail.",
                "drive.",
                "sheets.",
                "slides.",
                "calendar."
            ]
            .iter()
            .any(|p| name.starts_with(p)),
            "{name} is registered with no [google]"
        );
    }
    // No phase, no secret read: the startup log has none.
    core.google_after_serving();
    assert!(!core.startup_log.has("google.check"));
    // A plain turn's request has no Google in it: no tool, no word.
    let sid = session(&core);
    crate::test_support::turn(&core, &sid, "hello").await;
    let req = serde_json::to_string(&model.requests()[0].body())
        .unwrap()
        .to_lowercase();
    for word in ["google", "gmail", "calendar_events", "drive_"] {
        assert!(!req.contains(word), "{word} is in a plain turn's request");
    }
    // Every provider is behind the data floor even so (theseus-a1wvq.3: a
    // restricted model is refused whatever the config names), and the plain
    // turn above went through it to its unrestricted model.
    let providers: std::collections::BTreeMap<
        String,
        std::sync::Arc<dyn crate::provider::Provider>,
    > = core.runner.providers.clone().into_iter().collect();
    let guarded = crate::data_gate::guard(
        providers.clone(),
        &core.cfg,
        &core.runner.catalog,
        &core.store,
        &Default::default(),
    );
    for (name, p) in &providers {
        assert!(
            !std::sync::Arc::ptr_eq(p, &guarded[name]),
            "{name} was not wrapped"
        );
    }
    assert_eq!(model.requests().len(), 1, "the plain turn was sent");
}
