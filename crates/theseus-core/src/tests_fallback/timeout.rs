//! A call cut at its whole-call bound goes to the model's fallback, as a
//! refusal does (theseus-3cf5c): once, never a chain, and only that phase.

use super::*;
use crate::provider::{ProviderError, TimeoutPhase};

fn cut(phase: TimeoutPhase) -> Scripted {
    Scripted::Fail(ProviderError::Timeout {
        phase,
        elapsed_ms: 1_800_000,
    })
}

fn read_call() -> Scripted {
    Scripted::tools(
        "Opening the archive.",
        &[("toolu_read", "fs_read", json!({"path": "harbor.txt"}))],
    )
}

#[tokio::test]
async fn a_whole_call_timeout_goes_to_the_fallback_and_finishes() {
    let r = rig(vec![
        cut(TimeoutPhase::Total),
        Scripted::text("The archive says four."),
    ]);
    let sid = bound_session(&r.core);
    let res = turn(&r.core, &sid, None).await;
    assert_eq!(res.output, "The archive says four.", "{res:?}");
    assert_eq!(res.stop_reason, "no_tool_calls", "{res:?}");
    assert_eq!(res.model, "claude-sonnet-5");
    assert_eq!(
        res.fallback,
        Some(TurnFallback {
            from: "claude-sonnet-5-5".into(),
            to: "claude-sonnet-5".into(),
            category: Some("timeout".into()),
            answered: true,
            answers: 1,
            refusals: 1,
            latched: true,
        })
    );
    let reqs = r.fake.requests();
    assert_eq!(
        reqs.iter().map(|q| q.model.as_str()).collect::<Vec<_>>(),
        ["claude-sonnet-5-5", "claude-sonnet-5"]
    );
    let switch = rows(&r.core, &sid, "provider.fallback");
    assert_eq!(
        switch,
        [
            json!({"from": "claude-sonnet-5-5", "to": "claude-sonnet-5", "category": "timeout",
                "loop": 0, "refused": null, "n": 1, "latched": true})
        ]
    );
    assert!(rows(&r.core, &sid, "provider.refusal").is_empty());
}

#[tokio::test]
async fn a_timeout_on_the_fallback_too_ends_the_turn_and_never_chains() {
    let r = rig_with(
        vec![
            cut(TimeoutPhase::Total),
            cut(TimeoutPhase::Total),
            Scripted::text("never sent"),
        ],
        |c| {
            c.catalog.insert(
                "claude-sonnet-5".into(),
                crate::catalog::CatalogRow {
                    refusal_fallback_model: Some("claude-opus-4-8".into()),
                    ..Default::default()
                },
            );
        },
    );
    let sid = bound_session(&r.core);
    let res = try_turn(&r.core, &sid, None).await;
    let models: Vec<String> = r.fake.requests().iter().map(|q| q.model.clone()).collect();
    assert_eq!(models, ["claude-sonnet-5-5", "claude-sonnet-5"], "{res:?}");
    assert!(res.is_err(), "the turn fails: {res:?}");
    assert_eq!(rows(&r.core, &sid, "provider.fallback").len(), 1);
    assert_eq!(rows(&r.core, &sid, "turn.failed").len(), 1);
}

#[tokio::test]
async fn no_fallback_named_the_one_same_model_retry_stands() {
    // Opus 5.5 names none.
    let r = rig(vec![cut(TimeoutPhase::Total), Scripted::text("Four.")]);
    let sid = bound_session(&r.core);
    let res = turn(&r.core, &sid, Some("claude-opus-5-5")).await;
    assert_eq!(
        (res.output.as_str(), res.model.as_str()),
        ("Four.", "claude-opus-5-5")
    );
    assert!(res.fallback.is_none());
    let models: Vec<String> = r.fake.requests().iter().map(|q| q.model.clone()).collect();
    assert_eq!(models, ["claude-opus-5-5", "claude-opus-5-5"]);
    assert!(rows(&r.core, &sid, "provider.fallback").is_empty());
}

#[tokio::test]
async fn a_stream_idle_cut_does_not_switch_models() {
    let r = rig(vec![cut(TimeoutPhase::StreamIdle), Scripted::text("Four.")]);
    let sid = bound_session(&r.core);
    let res = turn(&r.core, &sid, None).await;
    assert!(res.fallback.is_none(), "{res:?}");
    let models: Vec<String> = r.fake.requests().iter().map(|q| q.model.clone()).collect();
    assert_eq!(models, ["claude-sonnet-5-5", "claude-sonnet-5-5"]);
    assert!(rows(&r.core, &sid, "provider.fallback").is_empty());
}

#[tokio::test]
async fn a_timeout_fallbacks_requests_keep_the_whole_transcript() {
    // Loop 0 calls a tool; loop 1 is cut and goes to the fallback; loop 2
    // calls again there; loop 3 answers. Nothing was refused, so nothing is
    // left out of the render (the kept render's own rule is
    // `compiler::tests_kept::a_timeout_fallback_keeps_the_kept_render`).
    let r = rig(vec![
        read_call(),
        cut(TimeoutPhase::Total),
        read_call(),
        Scripted::text("Four."),
    ]);
    let sid = bound_session(&r.core);
    let res = turn(&r.core, &sid, None).await;
    assert!(res.output.ends_with("Four."), "{}", res.output);
    assert_eq!(res.loops, 4, "{res:?}");
    // The first call's answer and its result are in the fallback's requests.
    let reqs = r.fake.requests();
    let sent = serde_json::to_string(&reqs[3].messages).unwrap();
    assert!(
        sent.contains("toolu_read") && sent.contains("Opening the archive."),
        "{sent}"
    );
}
