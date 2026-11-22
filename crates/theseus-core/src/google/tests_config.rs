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
    assert_eq!(model.requests().len(), 1, "the plain turn was sent");
}
