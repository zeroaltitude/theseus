//! The mint step's pure parts on the miner's own synthetic ledger (the
//! owner's example: a land stretch with a clean arm, a rerere-resolved arm
//! and a rerere-left hand-back): the kind rule's table, the spec, the keyed
//! split, the writer's brief, and the data rule.

use super::brief::{brief, occurrence_id, split};
use super::kind::{choose, Choice, Kind, Needs};
use super::spec::{flow_spec, placeholder_name};
use super::validate::isolated;
use crate::rsi::exams::SplitKey;
use crate::rsi::mine::fixture::{Ledger, REPO};
use crate::rsi::mine::{mine, Motif, Params, SlotKind};

/// The land motif of the owner's example.
pub(crate) fn land() -> Motif {
    let mut l = Ledger::new();
    l.owners_example(REPO);
    mine(&l.runs, &l.vocab, &Params::default(), 10_000_000)
        .motifs
        .into_iter()
        .find(|m| m.steps.iter().any(|s| s.id == "merge"))
        .expect("the land motif")
}

fn keep(s: &str) -> String {
    s.to_string()
}

fn all() -> Vec<String> {
    ["flow", "mcp", "rust"].map(String::from).to_vec()
}

#[test]
fn the_kind_rule_in_its_order() {
    let m = land();
    // 1. every step an allowed tool: a flow, whatever the flow's steps reach.
    let n = Needs::of(&m);
    assert_eq!(n, Needs::default(), "the land stretch needs nothing new");
    assert_eq!(choose(&n, &all()), Choice::Mint(Kind::Flow));
    // 2. the tool itself needs the network or a secret: an owner's card.
    let net = Needs {
        other_tools: true,
        network: true,
        ..Needs::default()
    };
    assert!(matches!(choose(&net, &all()), Choice::OwnerCard { why } if why.contains("network")));
    let secret = Needs {
        other_tools: true,
        secret: true,
        ..Needs::default()
    };
    assert!(matches!(choose(&secret, &all()), Choice::OwnerCard { why } if why.contains("secret")));
    let both = Needs {
        other_tools: true,
        network: true,
        secret: true,
        ..Needs::default()
    };
    assert!(matches!(choose(&both, &all()), Choice::OwnerCard { why } if why.contains("and")));
    // 3. computation, reading only, no network, no secret: MCP.
    let compute = Needs {
        computation: true,
        ..Needs::default()
    };
    assert_eq!(choose(&compute, &all()), Choice::Mint(Kind::Mcp));
    // 4. internals, a write, a graduation: Rust.
    for needs in [
        Needs {
            internals: true,
            ..Needs::default()
        },
        Needs {
            computation: true,
            writes: true,
            ..Needs::default()
        },
        Needs {
            graduation: true,
            ..Needs::default()
        },
        Needs {
            other_tools: true,
            ..Needs::default()
        },
    ] {
        assert_eq!(
            choose(&needs, &all()),
            Choice::Mint(Kind::Rust),
            "{needs:?}"
        );
    }
    // `[self.mint] kinds` limits the choice: the default is flows alone.
    let only_flow = vec![keep("flow")];
    assert_eq!(choose(&compute, &only_flow), Choice::NotAllowed(Kind::Mcp));
    assert_eq!(choose(&n, &only_flow), Choice::Mint(Kind::Flow));
    assert_eq!(choose(&n, &[]), Choice::NotAllowed(Kind::Flow));
    // A network or secret card is not a mint whatever the list allows.
    assert!(matches!(choose(&net, &only_flow), Choice::OwnerCard { .. }));
}

#[test]
fn a_motifs_needs_are_read_from_its_steps() {
    let mut m = land();
    m.steps[0].shape.tool = "http.fetch".into();
    let n = Needs::of(&m);
    assert!(n.other_tools && n.network && !n.secret);
    m.steps[0].shape.tool = "aws.call".into();
    let n = Needs::of(&m);
    assert!(n.network && n.secret);
    m.steps[0].shape.tool = "fs.edit".into();
    let n = Needs::of(&m);
    assert!(n.writes && !n.network);
}

#[test]
fn the_land_motif_makes_a_valid_flow_spec_with_a_placeholder_name() {
    let m = land();
    let name = placeholder_name(&m);
    let spec = flow_spec(&m, &name, &|s| s.to_string()).unwrap().spec;
    assert_eq!(spec.name, name);
    assert!(name.starts_with("mint_") && name.len() == 17);
    assert_eq!(spec.scope.repos, [REPO]);
    assert!(spec
        .digest
        .as_deref()
        .is_some_and(|d| d.starts_with("sha256:")));
    // The spec round-trips through R6's own parse and digest check.
    let again = crate::flow::spec::Spec::parse(&spec.frozen_bytes()).unwrap();
    assert_eq!(again.digest, spec.digest);
    let ids: Vec<&str> = spec.steps.iter().map(|s| s.id.as_str()).collect();
    for want in ["fetch", "merge", "commit", "push"] {
        assert!(ids.contains(&want), "{ids:?}");
    }
    let merge = spec.steps.iter().find(|s| s.id == "merge").unwrap();
    // The conflict arm goes on to the commit; the left one hands back.
    assert!(
        merge
            .on
            .values()
            .any(|t| t.starts_with("handback:") || t.starts_with("goto:")),
        "{:?}",
        merge.on
    );
    assert!(spec.exits.contains_key("ok"));
    assert!(spec.evidence.contains_key("occurrences"));
}

#[test]
fn a_constant_the_scrubber_would_change_becomes_an_input_and_is_kept_nowhere() {
    let mut m = land();
    // The test command's crate is a constant in the example; make it a value
    // the scrubber would change.
    let secret = format!("tok{}{}", "en-", "abcdef0123");
    let step = m
        .steps
        .iter_mut()
        .find(|s| s.id.starts_with("cargo_nextest_run"))
        .unwrap();
    let slot = step
        .slots
        .iter_mut()
        .find(|s| s.kind == SlotKind::Constant)
        .expect("a constant slot");
    slot.constant = Some(secret.clone());
    let scrub = |s: &str| s.replace(&secret, "[scrubbed]");
    let spec = flow_spec(&m, "land_it", &scrub).unwrap().spec;
    let json = serde_json::to_string(&spec).unwrap();
    assert!(!json.contains(&secret), "{json}");
    assert!(
        spec.inputs
            .keys()
            .any(|k| k.starts_with("cargo_nextest_run")),
        "{:?}",
        spec.inputs.keys()
    );
    // The same constant, left alone by the scrubber, stays.
    let kept = flow_spec(&m, "land_it", &|s| s.to_string()).unwrap().spec;
    assert!(serde_json::to_string(&kept).unwrap().contains(&secret));
}

#[test]
fn a_bench_motif_has_no_repository_so_it_is_no_flow() {
    let mut m = land();
    m.bench_born = true;
    m.scope = "bench:terminal".into();
    let e = flow_spec(&m, "x", &|s| s.to_string()).unwrap_err();
    assert!(e.contains("not a repository"), "{e}");
}

#[test]
fn the_split_is_even_stable_and_keyed() {
    let m = land();
    let (a, b) = (SplitKey([1; 32]), SplitKey([2; 32]));
    let h = split(&a, &m.digest, &m.occurrences);
    assert_eq!(h.build.len() + h.check.len(), m.occurrences.len());
    assert!(h.check.len() >= h.build.len() && h.check.len() - h.build.len() <= 1);
    let again = split(&a, &m.digest, &m.occurrences);
    let ids = |v: &[&crate::rsi::mine::Occurrence]| {
        v.iter().map(|o| occurrence_id(o)).collect::<Vec<_>>()
    };
    assert_eq!(ids(&h.check), ids(&again.check));
    let other = split(&b, &m.digest, &m.occurrences);
    assert_ne!(
        ids(&h.check),
        ids(&other.check),
        "another key splits another way"
    );
    for o in &h.build {
        assert!(!h.check.iter().any(|c| occurrence_id(c) == occurrence_id(o)));
    }
}

#[test]
fn the_brief_holds_no_check_half_id() {
    let m = land();
    let key = SplitKey([9; 32]);
    let halves = split(&key, &m.digest, &m.occurrences);
    assert!(halves.check.len() >= 3 && !halves.build.is_empty());
    let text = brief(&m.spec(), Kind::Flow, &halves.build);
    // The brief names every build-half id, so the check below is not vacuous...
    for o in &halves.build {
        assert!(text.contains(&occurrence_id(o)), "{text}");
    }
    // ... and none of the check half's: not the whole id, not its turn.
    for o in &halves.check {
        assert!(!text.contains(&occurrence_id(o)), "{text}");
        assert!(!text.contains(&o.turn), "{text}");
    }
    assert!(isolated(&text, &halves.check));
    // The asserted rule fails a brief made from every occurrence.
    let everyone: Vec<_> = m.occurrences.iter().collect();
    let leaky = brief(&m.spec(), Kind::Flow, &everyone);
    assert!(!isolated(&leaky, &halves.check));
    // The brief says what the writer does and never persuades.
    assert!(text.contains("kind: flow") && text.contains("contract"));
    assert!(text.contains("never says when to prefer it"));
}

#[test]
fn the_check_half_lives_beside_the_tool_as_open_data() {
    let m = land();
    let halves = split(&SplitKey([3; 32]), &m.digest, &m.occurrences);
    let dir = tempfile::tempdir().unwrap();
    super::brief::write_check(dir.path(), "mint_x", &halves.check).unwrap();
    assert!(dir.path().join("mint_x/check/occurrences.json").is_file());
    let back = super::brief::read_check(dir.path(), "mint_x").unwrap();
    assert_eq!(back.len(), halves.check.len());
    assert_eq!(occurrence_id(&back[0]), occurrence_id(halves.check[0]));
}

const CN_PROXY: &str = r#"
[secrets]
anthropic_api_key = "env:ANTHROPIC_API_KEY"
cnkey = "env:CN_KEY"

[providers.cnproxy]
api_base = "https://api.deepseek.com/anthropic"
api_key_secret = "cnkey"
origin = "cn"

[profiles.assistant]
provider = "cnproxy"
model = "claude-sonnet-5-5"
"#;

#[test]
fn a_private_motif_refuses_a_china_origin_profile() {
    let (cfg, _) = crate::Config::parse(CN_PROXY).unwrap_or_else(|e| panic!("{e:#}"));
    let catalog = crate::catalog::Catalog::builtin();
    // The profile's name and its model's name say nothing: a Claude model id on
    // a China-origin endpoint. Only the origin refuses it.
    let e = super::data_rule(&cfg, &catalog, "assistant", true).unwrap_err();
    assert!(e.contains("restricted") || e.contains("Cn"), "{e}");
    // The built-in China-origin profile, by origin as well.
    assert!(super::data_rule(&cfg, &catalog, "glm53", true).is_err());
    // An unknown profile is of unknown origin: failing closed.
    assert!(super::data_rule(&cfg, &catalog, "nope", true).is_err());
    // The live profile (a Claude model) is fine, and a bench-born motif may
    // use any model.
    let live = cfg.model.live.clone();
    assert!(
        super::data_rule(&cfg, &catalog, &live, true).is_ok(),
        "{live}"
    );
    assert!(super::data_rule(&cfg, &catalog, "assistant", false).is_ok());
    assert!(super::data_rule(&cfg, &catalog, "glm53", false).is_ok());
}

/// A private motif is written only on a model of US origin (theseus-xs5ah):
/// a profile on a host vouched `eu` is refused, though it is not restricted.
#[test]
fn a_private_motif_refuses_an_eu_origin_profile() {
    let eu = CN_PROXY
        .replace("api.deepseek.com", "api.example.eu")
        .replace("origin = \"cn\"", "origin = \"eu\"");
    let (cfg, _) = crate::Config::parse(&eu).unwrap_or_else(|e| panic!("{e:#}"));
    let catalog = crate::catalog::Catalog::builtin();
    let origin = cfg.profile_origin("assistant", &catalog);
    assert_eq!(origin, crate::catalog::ModelOrigin::Eu);
    let e = super::data_rule(&cfg, &catalog, "assistant", true).unwrap_err();
    assert!(e.contains("Eu"), "{e}");
    assert!(super::data_rule(&cfg, &catalog, "assistant", false).is_ok());
}
