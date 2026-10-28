//! The self card (theseus-slvl.8): what the tools note says of a place's
//! powers, per place, fixed for a config.

use std::sync::Arc;
use std::time::Instant;

use crate::ceiling::PlaceView;
use crate::places::{BoundPlace, PlaceClass, PlaceRule};
use crate::secrets::SecretBoard;
use crate::self_card::card;
use crate::toolrun::{build_runtime, InlineLauncher, ToolRuntime};
use crate::Config;

fn config(tweak: impl FnOnce(&mut Config)) -> Config {
    let mut cfg = Config::example();
    cfg.voice.enabled = true;
    tweak(&mut cfg);
    cfg
}

fn runtime(cfg: &Config) -> ToolRuntime {
    let board = SecretBoard::new(cfg.secrets.keys().cloned(), Instant::now());
    build_runtime(cfg, None, Arc::default(), Arc::new(InlineLauncher), board).unwrap()
}

fn private(voice: bool) -> PlaceView {
    PlaceView {
        voice,
        draft: None,
        ..PlaceClass::Private.into()
    }
}

const VOICE: &str = "You can join a voice channel the bindings name: the person types /join in a private place to invite you.";
const WAKES: &str = "You can set a wake to follow up later and run background tasks.";
const PAST: &str = "You can look up past conversations.";

#[test]
fn a_private_place_with_voice_bound_says_all_four() {
    let cfg = config(|_| {});
    let said = card(&cfg, &runtime(&cfg), private(true));
    let port = cfg.web.port;
    assert_eq!(
        said,
        format!(
            "{VOICE} {WAKES} {PAST} The person can see everything you do in the cockpit at http://127.0.0.1:{port}."
        )
    );
}

#[test]
fn without_a_bound_voice_channel_it_does_not_offer_join() {
    let cfg = config(|_| {});
    let said = card(&cfg, &runtime(&cfg), private(false));
    assert!(!said.contains("/join") && said.starts_with(WAKES), "{said}");
    // `[voice]` off says nothing of it either, whatever is bound.
    let off = config(|c| c.voice.enabled = false);
    assert!(!card(&off, &runtime(&off), private(true)).contains("/join"));
}

#[test]
fn with_the_web_ui_off_it_names_no_cockpit() {
    let cfg = config(|c| c.web.enabled = false);
    let said = card(&cfg, &runtime(&cfg), private(true));
    assert_eq!(said, format!("{VOICE} {WAKES} {PAST}"));
}

#[test]
fn a_shared_place_hears_only_what_it_may_use() {
    let cfg = config(|_| {});
    let rt = runtime(&cfg);
    let view = PlaceView {
        voice: false,
        draft: None,
        ..PlaceClass::Shared.into()
    };
    assert_eq!(card(&cfg, &rt, view), WAKES);
}

#[test]
fn an_ipv6_loopback_is_bracketed() {
    let cfg = config(|c| c.web.bind = "::1".into());
    let said = card(&cfg, &runtime(&cfg), private(false));
    assert!(
        said.ends_with(&format!("at http://[::1]:{}.", cfg.web.port)),
        "{said}"
    );
}

/// Two sessions of a config read the same bytes, and a config change changes
/// them: the card never carries a request's value.
#[test]
fn the_card_is_fixed_for_a_config_and_follows_it() {
    let cfg = config(|_| {});
    let (a, b) = (runtime(&cfg), runtime(&cfg));
    assert_eq!(card(&cfg, &a, private(true)), card(&cfg, &b, private(true)));
    let moved = config(|c| c.web.port = cfg.web.port.wrapping_add(1));
    assert_ne!(
        card(&cfg, &a, private(true)),
        card(&moved, &runtime(&moved), private(true))
    );
}

/// `/join` is offered on a Discord place of the owner's, when the file binds
/// a voice channel: never on the CLI's or web UI's place, a shared one, or
/// before the binding has said.
#[test]
fn the_place_rule_offers_join_on_a_private_discord_place_only_once_voice_is_bound() {
    let cfg = config(|_| {});
    let rule = PlaceRule::default();
    rule.bind(vec![
        BoundPlace {
            target: "discord:dm:200000000000000001".into(),
            name: "DM @owner".into(),
            private: true,
            ..Default::default()
        },
        BoundPlace {
            target: "discord:channel:300000000000000001".into(),
            name: "#lobby".into(),
            guild: Some("100000000000000001".into()),
            ..Default::default()
        },
    ]);
    let dm = Some("discord:dm:200000000000000001");
    let lobby = Some("discord:channel:300000000000000001");
    assert!(!rule.place(&cfg, dm).voice, "not bound yet");
    rule.bind_voice(true);
    assert!(rule.place(&cfg, dm).voice);
    assert!(!rule.place(&cfg, lobby).voice, "a shared place");
    assert!(!rule.place(&cfg, None).voice, "the CLI has no /join");
    rule.bind_voice(false);
    assert!(!rule.place(&cfg, dm).voice);
}
