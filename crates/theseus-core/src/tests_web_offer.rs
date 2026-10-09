//! `web.search` is offered only when its key's `[secrets]` entry exists
//! (theseus-4o4c); `http.fetch` always is.

use std::sync::Arc;
use std::time::Instant;

use crate::secrets::SecretBoard;
use crate::toolrun::{build_runtime, InlineLauncher, ToolRuntime};
use crate::Config;

/// The template's config, with no key entry for the search unless a test adds one.
fn keyless() -> Config {
    let mut cfg = Config::example();
    cfg.secrets.remove("brave_api_key");
    cfg
}

fn runtime(tweak: impl FnOnce(&mut Config)) -> ToolRuntime {
    let mut cfg = keyless();
    tweak(&mut cfg);
    let board = SecretBoard::new(cfg.secrets.keys().cloned(), Instant::now());
    build_runtime(&cfg, None, Arc::default(), Arc::new(InlineLauncher), board).unwrap()
}

fn wire_names(rt: &ToolRuntime) -> Vec<String> {
    rt.definitions()
        .iter()
        .map(|d| d["name"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn without_a_key_entry_web_search_is_not_offered_and_health_says_why() {
    let rt = runtime(|_| {});
    assert!(rt.registry.get("http.fetch").is_some());
    assert!(rt.registry.get("web.search").is_none());
    assert!(!wire_names(&rt).iter().any(|n| n.contains("search")));
    assert!(wire_names(&rt).contains(&"http_fetch".to_string()));
    assert_eq!(
        rt.not_offered,
        ["web.search: not offered: no [secrets] entry brave_api_key; add it and restart theseusd to offer it"]
    );
    // No grant for a tool that is not there.
    assert!(rt.broker.status().iter().all(|g| g.to != "web.search"));
}

#[test]
fn an_entry_offers_it_even_when_it_cannot_be_read() {
    let rt = runtime(|c| {
        c.secrets.insert(
            "brave_api_key".into(),
            "op://invented vault/none/notesPlain".into(),
        );
    });
    assert!(rt.registry.get("web.search").is_some());
    assert!(wire_names(&rt).contains(&"web_search".to_string()));
    assert!(rt.not_offered.is_empty());
    assert!(rt.broker.status().iter().any(|g| g.to == "web.search"));
}

#[test]
fn a_policy_naming_the_absent_tool_still_loads() {
    let mut cfg = keyless();
    cfg.policy
        .tools
        .insert("web.search".into(), crate::policy::Posture::Approve);
    cfg.validate().unwrap();
    let rt = runtime(|c| {
        c.policy
            .tools
            .insert("web.search".into(), crate::policy::Posture::Approve);
    });
    assert!(rt.registry.get("web.search").is_none());
}

/// Every path that tells the model its tools: the place-narrowed list for a
/// private and a shared place, and the system prompt's tools paragraph.
#[test]
fn the_model_hears_of_web_search_only_when_it_is_keyed() {
    use crate::places::PlaceClass;
    let heard = |rt: &ToolRuntime| {
        let mut said = Vec::new();
        for class in [PlaceClass::Private, PlaceClass::Shared] {
            let defs = rt.definitions_for(class.into());
            let note = rt.system_note_for(class.into());
            said.push((
                defs.iter().any(|d| d["name"] == "web_search"),
                note.contains("web_search") || note.contains("web.search"),
            ));
        }
        said
    };
    let unkeyed = runtime(|_| {});
    assert_eq!(heard(&unkeyed), [(false, false), (false, false)]);
    let keyed = runtime(|c| {
        c.secrets
            .insert("brave_api_key".into(), "env:INVENTED_SEARCH_KEY".into());
    });
    let said = heard(&keyed);
    assert!(said.iter().all(|(def, _)| *def), "{said:?}");
}
