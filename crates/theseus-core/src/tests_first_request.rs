//! The first request's budget in tokens, as sent (theseus-to0xw): the measure is the census's own number, it
//! counts every kind of place, it holds each kind to its budget, and a tool a place loads is named in
//! `first_request::ALWAYS_LOADED` with its reason ("deferred by default").

use std::sync::Arc;

use theseus_protocol::census::PLACES;

use crate::first_request::{allowed, budget, ALWAYS_LOADED};
use crate::provider::FakeProvider;
use crate::store::Store;
use crate::tools_defer::PlaceKind;
use crate::{Config, Core};

/// The owner's config in shape (theseus-ykc7o): the installed config's sections with no secret of its own.
pub(crate) const OWNER_SHAPE: &str = include_str!("../fixtures/owner_shape.toml");

fn rig() -> (Arc<Core>, tempfile::TempDir) {
    // The daemon's own defaults, as the bench's scratch daemon runs them (the template's every section adds
    // AWS, GitHub and MCP tools).
    rig_of("[secrets]\nanthropic_api_key = \"env:THESEUS_TEST_KEY\"\n")
}

/// The fixture's personas' files, copied from `fixtures/personas` into `dir/.theseus/personas`: `/@PERSONAS@` is
/// that directory. A shared place's request names each withheld file by its path, so the path's length is part of
/// the count: a copy under the rig's own directory (about 42 characters, as the owner's are about 55) keeps the
/// count the same wherever the repository is checked out.
pub(crate) fn with_personas(toml: &str, dir: &std::path::Path) -> String {
    let to = dir.join(".theseus/personas");
    std::fs::create_dir_all(&to).unwrap();
    let from = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/personas");
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
    }
    toml.replace("/@PERSONAS@", to.to_str().unwrap())
}

/// The owner shape as it was before its personas: the tables from `[context]` to `[aws.accounts`.
pub(crate) fn without_personas(toml: &str) -> String {
    let (head, rest) = toml.split_once("\n[context]").unwrap();
    let (_, tail) = rest.split_once("\n[aws.accounts").unwrap();
    format!("{head}\n[aws.accounts{tail}")
}

pub(crate) fn rig_of(toml: &str) -> (Arc<Core>, tempfile::TempDir) {
    rig_built(toml, false)
}

/// [`rig_of`], for a daemon started from a settings file, as every real one is: it has the tools that
/// only such a daemon registers (the settings', the looks', the profiles').
pub(crate) fn rig_started(toml: &str) -> (Arc<Core>, tempfile::TempDir) {
    rig_built(toml, true)
}

fn rig_built(toml: &str, started: bool) -> (Arc<Core>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = Config::parse(&with_personas(toml, dir.path())).unwrap().0;
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.config_file = started.then(|| dir.path().join("theseus.toml"));
    cfg.tools.projects_dir = Some(root.canonicalize().unwrap().to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(vec![]));
    let mut parts = crate::rpc::Parts::for_tests(cfg, fake, store);
    // Tests build with deferral off; a daemon has it on.
    parts.cfg.tools.defer.enabled = true;
    let core = Core::build(parts).unwrap();
    (core, dir)
}

/// The measure is what `theseus census` prints as the request's total, for every kind of place it counts.
#[test]
fn the_measure_is_the_censuss_own_total() {
    let (core, _dir) = rig();
    for place in PLACES {
        let (census, _, _) = core
            .context_census(&theseus_protocol::census::ContextCensusParams {
                session_id: None,
                place: Some(place.to_string()),
                exact: false,
            })
            .unwrap();
        let first = core.first_request(place).unwrap();
        assert_eq!(first.tokens, census.total_tokens, "{place}");
        assert!(first.tokens > census.system_tokens, "{place}: tools count");
    }
}

/// A kind of place loads its own families: a DM's request is not the CLI's (the census once counted
/// every fresh session as the CLI's, whatever `--place` said).
#[test]
fn each_kind_of_place_is_counted_with_its_own_families() {
    let (core, _dir) = rig();
    let loaded = |p: &str| core.first_request(p).unwrap().loaded;
    // The DM defers the tasks and keeps the reminders (theseus-rwpbg.3, T5).
    assert!(!loaded("dm").contains(&"task_create".to_string()));
    assert!(loaded("dm").contains(&"wake_at".to_string()));
    assert!(loaded("dm").contains(&"http_fetch".to_string()));
    assert!(!loaded("cli").contains(&"task_create".to_string()));
    assert!(!loaded("task").contains(&"context_history".to_string()));
    assert!(loaded("headless").contains(&"term_open".to_string()));
    assert!(
        !loaded("channel").contains(&"fs_read".to_string()),
        "a channel is offered the public tools"
    );
}

/// Headroom said beside a figure, and a warning within a twentieth of the ceiling (as `line-pressure.py` does for
/// clippy's): the test holds the ceiling, this line lets a branch see it coming.
fn pressure(tokens: u64, max: u64) -> String {
    let room = max.saturating_sub(tokens);
    let warn = if room * 20 <= max {
        "  WARNING: within 5% of its budget"
    } else {
        ""
    };
    format!(", {room} tokens of room{warn}")
}

/// The owner's word of 2026-11-22 on these budgets: "2000-4000 tokens budget is fine -- these aren't rules but
/// general guidelines". A first request is held to its place's budget plus this headroom; the budgets themselves
/// (`first_request`, which the tools' demotion reads) are unchanged.
pub(crate) const HEADROOM: u64 = 4_000;

/// A place's budget as a guideline: the budget and the owner's headroom.
pub(crate) fn guideline(place: &str) -> Option<u64> {
    budget(place).map(|b| b + HEADROOM)
}

/// Every kind of place is within its budget as a guideline with headroom (cli and task 8,000 and 4,000 more, the
/// DM 15,000 and 4,000 more: the owner's word of 2026-11-22), and the DM's is the larger.
#[test]
fn every_kind_of_place_is_within_its_budget() {
    let (core, _dir) = rig();
    for place in PLACES {
        let Some(max) = guideline(place) else {
            assert_eq!(place, "harness");
            continue;
        };
        let tokens = core.first_request(place).unwrap().tokens;
        eprintln!(
            "first request, {place}: {tokens} tokens (budget {max}){}",
            pressure(tokens, max)
        );
        assert!(
            tokens <= max,
            "{place}: {tokens} tokens over its budget of {max}"
        );
    }
    // And on the owner's config in shape (theseus-ykc7o: install #30 read the owner's cli at 8,991 tokens while
    // the defaults read 7,516): every kind of place within its budget there too.
    let (owner, _dir) = rig_of(OWNER_SHAPE);
    for place in PLACES {
        let Some(max) = guideline(place) else {
            continue;
        };
        let tokens = owner.first_request(place).unwrap().tokens;
        eprintln!(
            "first request, the owner's shape, {place}: {tokens} tokens (budget {max}){}",
            pressure(tokens, max)
        );
        assert!(
            tokens <= max,
            "the owner's shape, {place}: {tokens} tokens over its budget of {max}"
        );
    }
    // The same on a daemon started from a settings file, as every real one is: it has the settings', the looks'
    // and the profiles' tools, which every place defers by default, so the budgets are the owner's own.
    let (started, _sdir) = rig_started(OWNER_SHAPE);
    for place in PLACES {
        let Some(max) = guideline(place) else {
            continue;
        };
        let tokens = started.first_request(place).unwrap().tokens;
        eprintln!("first request, the owner's shape, started from a file, {place}: {tokens} tokens (budget {max})");
        assert!(
            tokens <= max,
            "started from a file, {place}: {tokens} tokens over its budget of {max}"
        );
    }
    assert_eq!(budget("cli"), Some(9_500));
    assert_eq!(budget("task"), Some(9_500));
    assert_eq!(budget("dm"), Some(15_000));
    assert!(budget("channel") <= Some(15_000) && budget("headless") <= Some(15_000));
}

/// Deferred by default: a tool a place loads in its first request is named in the allowlist for that place, and
/// an entry that no place loads, on the defaults or the owner's shape, is stale.
#[test]
fn a_loaded_tool_is_named_in_the_allowlist_with_its_reason() {
    // On the defaults and on the owner's config in shape (its language servers, its web search key).
    let (core, _dir) = rig();
    let (owner, _odir) = rig_of(OWNER_SHAPE);
    // And a daemon started from a settings file, which has the looks and the profiles: a tool that only
    // such a daemon registers is named here too if a place ever loads it (issue 28).
    let (started, _sdir) = rig_started(OWNER_SHAPE);
    let mut seen = std::collections::BTreeSet::new();
    for core in [&core, &owner, &started] {
        for kind in PlaceKind::ALL {
            let place = kind.as_str();
            for tool in core.first_request(place).unwrap().loaded {
                assert!(
                    allowed(place, &tool),
                    "{tool} is loaded in the {place} place's first request and is not in \
                     first_request::ALWAYS_LOADED: defer it, or name it there with the ledger's evidence \
                     (use in about 1 of 5 sessions of that place)"
                );
                seen.insert((place, tool));
            }
        }
    }
    for (tool, places, reason) in ALWAYS_LOADED {
        assert!(!reason.is_empty(), "{tool}: a reason");
        for p in *places {
            assert!(
                seen.contains(&(*p, tool.to_string())),
                "{tool} is listed for {p} and that place does not load it: drop the entry"
            );
        }
    }
}

/// The line that names the profiles (issue 37) rides in the turn's tail, which the census does not
/// read, so its cost is counted here the census's own way (its characters at the model's rate for
/// text) and added to every kind of place's first request on the owner's shape: no place is put
/// over its budget by it (a guideline with headroom: the owner's word of 2026-11-22), and no budget moves.
#[test]
fn the_profile_line_fits_every_places_room() {
    let (owner, _dir) = rig_of(OWNER_SHAPE);
    let names = owner.cfg.all_profiles().keys().map(String::as_str);
    let line =
        crate::profile_tool::names::line(names).expect("the owner's shape has several profiles");
    for place in PLACES {
        let Some(max) = guideline(place) else {
            continue;
        };
        let first = owner.first_request(place).unwrap().tokens;
        let rates = crate::catalog::TokenRates::of(
            &owner.cfg.profile(&owner.cfg.model.live).unwrap().model,
        );
        let cost = (line.len() as f64 / rates.text).ceil() as u64;
        eprintln!(
            "first request, the owner's shape, {place}, with the profile line ({cost} tokens): {}{}",
            first + cost,
            pressure(first + cost, max)
        );
        assert!(
            first + cost <= max,
            "the owner's shape, {place}: {} tokens with the profile line, over its budget of {max}",
            first + cost
        );
    }
}
