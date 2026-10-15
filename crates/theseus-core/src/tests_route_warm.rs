//! Routing's cold estimate reads the prefix library's warmth (theseus-rr15):
//! a switch above `cold_switch_tokens` is held back because its target would
//! write the whole request cold; when the library holds the session's third
//! layer warm on the target's model, only the rest is written, and the switch
//! goes at once.

use theseus_judge::fake::FakeJev;

use crate::tests_route::{mode, rig, turn};

fn project(c: &mut crate::Config, dir: &std::path::Path) {
    let f = dir.join("harbour.md");
    std::fs::write(&f, format!("The harbour log. {}", "wave ".repeat(6_000))).unwrap();
    c.context.files = vec![f.to_string_lossy().into_owned().into()];
    c.routing.cold_switch_tokens = 3_000;
}

fn header(core: &crate::Core) -> String {
    core.store
        .ledger_tail::<crate::ledger::LedgerRow>(10_000)
        .unwrap()
        .into_iter()
        .find(|(_, r)| r.is_kind("prefix.picked"))
        .map(|(_, r)| r.data["header"].as_str().unwrap().to_string())
        .unwrap()
}

/// Cold on the target's model, the switch waits for a second turn; with the
/// project's header warm there, it goes at the first.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_warm_library_header_on_the_targets_model_counts_as_cached() {
    let files = tempfile::tempdir().unwrap();
    let jev = FakeJev::start().unwrap();
    mode(&jev, "sophisticated", 0.95);
    let r = rig(Some(&jev), 4, |c| project(c, files.path()));
    let one = turn(
        &r.core,
        None,
        "Design the write-ahead log's segment format.",
        None,
    )
    .await;
    assert_eq!(one.profile, "sonnet");
    assert_eq!(
        one.route.as_ref().unwrap().reason,
        "cache_hold",
        "cold on opus"
    );
    let (live, _) = r.core.live_profile();
    let opus = r
        .core
        .runner
        .resolve_target(&live, Some("opus"), None, None)
        .unwrap();
    let now = theseus_protocol::now_unix_ms();
    r.core
        .runner
        .prefix
        .touch(&header(&r.core), &opus.model, 3_600_000, now);
    let two = turn(&r.core, None, "Design the index's checkpoint format.", None).await;
    assert_eq!(
        (
            two.profile.as_str(),
            two.route.as_ref().unwrap().reason.as_str()
        ),
        ("opus", "verdict"),
        "the project's header is warm on opus: the rest is under the bar"
    );
}
