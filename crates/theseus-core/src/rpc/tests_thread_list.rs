//! `thread.list` and `thread.show` after threads were retired (2026-11-22, issue 44): they read
//! the stored records, and they are still the owner's alone (`judge_act(Act::Thread)`), so a
//! person in a shared place is refused them. The shared-place case keeps the act-gate half of the
//! deleted `tests_threads.rs::a_shared_place_can_neither_address_list_nor_bind_a_thread`.

use std::sync::Arc;

use theseus_protocol::threads::{ThreadListParams, ThreadNewParams, ThreadState};

use super::tests::test_core;
use super::Core;
use crate::approval::{Answerer, Surface};
use crate::places::{BoundPlace, PlaceClass};

const LAB: u64 = 314_159_265_358_979_323;
const STRANGER: u64 = 777;

fn core() -> Arc<Core> {
    test_core("the tide is out")
}

fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    }
}

/// A person in a shared channel of the binding's.
fn stranger() -> Answerer {
    Answerer {
        label: format!("discord:{STRANGER}"),
        surface: Surface::Discord,
        discord: Some(theseus_protocol::DiscordOrigin {
            user_id: STRANGER.to_string(),
            channel_id: LAB.to_string(),
            guild_id: Some("7".into()),
        }),
        agent: None,
    }
}

/// A stored thread record, written by the fixture writer (no method reaches it).
fn stored(core: &Core, slug: &str) {
    core.thread_new(
        &ThreadNewParams {
            slug: slug.into(),
            title: Some(format!("Title of {slug}")),
            ..Default::default()
        },
        cli(),
    )
    .unwrap();
}

#[test]
fn list_and_show_read_the_stored_records() {
    let c = core();
    stored(&c, "tides");
    stored(&c, "kelp");
    let listed = c.thread_list(&ThreadListParams::default(), cli()).unwrap();
    assert_eq!(listed.total, 2);
    let slugs: Vec<&str> = listed
        .threads
        .iter()
        .map(|r| r.thread.slug.as_str())
        .collect();
    assert!(
        slugs.contains(&"tides") && slugs.contains(&"kelp"),
        "{slugs:?}"
    );
    let shown = c.thread_show("tides", cli()).unwrap();
    assert_eq!(shown.thread.title, "Title of tides");
    assert_eq!(shown.thread.state, ThreadState::Active);
    let missing = c.thread_show("nothing-here", cli()).unwrap_err();
    assert_eq!(
        missing.code,
        theseus_protocol::error_code::NOT_FOUND,
        "{}",
        missing.message
    );
}

#[test]
fn a_shared_place_can_neither_list_nor_show_a_thread() {
    use theseus_protocol::error_code::REFUSED;
    let c = core();
    c.bind_places(vec![BoundPlace {
        target: format!("discord:channel:{LAB}"),
        name: "#lab".into(),
        private: false,
        ..Default::default()
    }]);
    stored(&c, "tides");
    let refused = |e: super::server::RpcFailure| assert_eq!(e.code, REFUSED, "{}", e.message);
    refused(
        c.thread_list(&ThreadListParams::default(), stranger())
            .unwrap_err(),
    );
    refused(c.thread_show("tides", stranger()).unwrap_err());
    refused(c.thread_session("tides", stranger()).unwrap_err());
    assert!(
        c.outbox.place_session("thread:tides").unwrap().is_none(),
        "no place was bound"
    );
    assert_eq!(c.threads().get("tides").unwrap().state, ThreadState::Active);
    // The owner's own DM, bound private, may.
    let class = |t: &str| c.runner.place_rule.class(&c.cfg, Some(t));
    assert_eq!(class("thread:tides"), PlaceClass::Private);
    assert_eq!(class("discord:thread:tides"), PlaceClass::Private);
    assert_eq!(class(&format!("discord:channel:{LAB}")), PlaceClass::Shared);
    let board = c.threads();
    assert!(board.live(PlaceClass::Shared).is_empty());
    assert_eq!(board.live(PlaceClass::Private).len(), 1);
}
