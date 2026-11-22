//! Where a turn ran, on its route row (theseus-a1wvq.11): `route.decided`'s
//! provider, model and origin. The rig is `tests_route`'s.

use theseus_judge::fake::FakeJev;

use crate::tests_route::{decided, mode, rig, turn};

/// A plain Claude turn's route row names the provider, model and origin it
/// ran on, and nothing of the restricted-origin rule deleted on 2026-11-21:
/// no `cn` word and no `data_gate`, and the turn's result carries no data part.
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
    assert!(d.get("cn").is_none(), "{d}");
    assert!(d.get("data_gate").is_none(), "{d}");
    // The route carries no data part (theseus-a1wvq.11's, retired 2026-11-21).
    assert!(!serde_json::to_string(&res.route.unwrap())
        .unwrap()
        .contains("\"data\""));
}
