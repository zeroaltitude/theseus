//! A reply watch through the stand-in's gateway (theseus-rlnbf): a watch on
//! the shared `#lab` for a message from ben. A message from cy fires nothing;
//! ben's fires it, though ben may not drive Theseus there (the binding tells
//! the core before it routes), and the owner's DM gets the notice, not `#lab`.

use std::sync::Arc;

use theseus_core::provider::{FakeProvider, Provider};
use theseus_protocol::{WatchCheck, WatchCreateParams};

use super::{bindings, guild, Rig, ANA_DM, BEN, CY, LAB};

#[tokio::test]
async fn a_reply_from_the_named_person_in_a_shared_channel_reaches_the_owners_dm() {
    let model =
        |_: &std::path::Path, _| -> Arc<dyn Provider> { Arc::new(FakeProvider::scripted(vec![])) };
    let r = Rig::start_tweaked(model, guild(true), &bindings(false), &[], |_| {}).await;
    let home = r.core.home().unwrap();
    let p = WatchCreateParams {
        title: "ben replies".into(),
        check: WatchCheck::Reply {
            place: "#lab".into(),
            from: Some(BEN.to_string()),
            words: None,
        },
        until: String::new(),
        every: None,
        for_at_most: Some("5d".into()),
        then: None,
        follow_up: None,
        session_id: None,
        author: None,
        discord: None,
    };
    let p = theseus_core::watch::reply::resolved(&p, &r.core.runner.place_rule).unwrap();
    let now = r.core.kernel.now_ms();
    let built = theseus_core::watch::spec::build(&p, now, &r.core.cfg.watch_for, None).unwrap();
    let set = r
        .core
        .kernel
        .create_wake(&home.execution_id, None, built.new, "the test")
        .unwrap();
    let ex = r
        .core
        .kernel
        .execution(&home.execution_id)
        .unwrap()
        .unwrap();
    r.core.watch_seed(&ex, &set.wake, now, true);

    r.say((CY, "cy"), Some(LAB), "berth four is free");
    r.say((BEN, "ben"), Some(LAB), "ship leaves at dawn");
    let is_it = |m: &theseus_sim::fake_discord::Msg| m.versions[0].contains("ship leaves at dawn");
    r.until("the notice in ana's DM", || {
        r.posted(ANA_DM).iter().any(is_it)
    })
    .await;
    let post = r.posted(ANA_DM).into_iter().find(is_it).unwrap();
    assert!(
        post.content.starts_with("👁 ben replied in #lab: \""),
        "{}",
        post.content
    );
    assert!(
        !r.posted(LAB).iter().any(is_it),
        "never into the shared channel"
    );
    assert!(
        !post.content.contains("berth four"),
        "cy's message is not the reply"
    );
}
