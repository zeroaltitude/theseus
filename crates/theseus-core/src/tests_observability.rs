//! The data rule on every route row and provider call (theseus-a1wvq.11,
//! theseus-9ymz0): `route.decided`'s `cn` word, gate, provider, model and
//! origin; `provider.call`'s origin; the turn's footer words; and the gate's
//! counter. The rig is `tests_dm_rig`'s.

use serde_json::{json, Value};
use theseus_judge::fake::FakeJev;

use crate::content::Flag;
use crate::content_wait::judging::Judging;
use crate::tests_dm_rig::{
    dm, gate_counts, jev, judging_with, rig_telemetry_with, say, telemetry, PRIVATE,
};
use crate::tests_route::{decided, mode, rig, turn};

/// A judge that raises `confidential` on `PRIVATE`'s marker and nothing else.
fn finds_private() -> std::sync::Arc<Judging> {
    Judging::with(|_, text| match text.contains("marker-7c1q") {
        true => vec![Flag::Confidential],
        false => vec![],
    })
}

/// A plain Claude turn whose mode lists no restricted profile gains the
/// provider, model and origin, a `cn` word, and no `data_gate`: no label
/// keeps its session off a restricted model, and it never waited for any.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_plain_turns_route_row_gains_only_where_it_ran() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    let r = rig(Some(&jev), 1, |_| {});
    let res = turn(&r.core, None, "Talk to me about the weather.", None).await;
    let d = &decided(&r.core.store)[0];
    assert_eq!(d["origin"], "us", "{d}");
    assert_eq!(d["model"], res.model.as_str(), "{d}");
    assert_eq!(d["provider"], res.provider.as_str(), "{d}");
    assert_eq!(d["cn"], "allowed", "{d}");
    assert!(d.get("data_gate").is_none(), "{d}");
    let t = res.route.unwrap().data.expect("data");
    assert_eq!(
        (t.origin.as_str(), t.gate, t.cn.as_str()),
        ("us", None, "allowed")
    );
}

fn count_is_one(n: &Value) -> bool {
    n == 1 || n == "1"
}

/// theseus-4h85l: the provider chokepoint counts each refusal on
/// `theseus.route.data_gate`, by `chokepoint` and its why word: a call made for
/// no session, here.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_chokepoint_counts_its_refusal() {
    let (rx, tel) = telemetry().await;
    let jev = jev(&[]);
    let (r, _zai, _d) = crate::tests_dm_rig::rig_telemetry(&jev, tel);
    let provider = r.core.runner.providers["zai"].clone();
    let req = crate::provider::ProviderRequest {
        model: "glm-5.3-flash".into(),
        max_tokens: 16,
        ..Default::default()
    };
    assert!(provider.count_tokens(&req).await.is_err());
    let counts = gate_counts(&r.core, &rx).await;
    assert_eq!(counts.len(), 1, "{counts:?}");
    assert_eq!(
        (&counts[0].0, &counts[0].1),
        (&json!("chokepoint"), &json!("no_session"))
    );
    assert!(count_is_one(&counts[0].2), "{counts:?}");
}

/// Its mirror: the mode's list is restricted first, but the verdict is below
/// the mode's bar, so the decision stays on the base whatever the labels say.
/// The rule cost the turn nothing: no word, no count.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unsure_verdict_that_stays_on_its_base_shows_no_data_gate() {
    let (rx, tel) = telemetry().await;
    let jev = FakeJev::start().unwrap();
    mode(&jev, "trivial", 0.3);
    let (r, _zai, _d) = rig_telemetry_with(&jev, tel, |_c| {
    });
    judging_with(&r, finds_private());
    let res = say(&r.core, &dm(&r.core), PRIVATE, None).await;
    let d = &decided(&r.core.store)[0];
    assert_eq!(d["reason"], "unsure", "{d}");
    assert!(d.get("data_gate").is_none(), "{d}");
    let data = res.route.unwrap().data.expect("data");
    assert_eq!((data.gate.clone(), data.footer()), (None, None));
    assert_eq!(gate_counts(&r.core, &rx).await, vec![]);
}
