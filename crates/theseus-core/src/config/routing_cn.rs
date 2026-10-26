//! The load checks for China-origin profiles in `[routing.classes]`
//! (theseus-a1wvq.4, the owner's D2): a restricted profile (origin `cn` or
//! `unknown`) enters a class only through `trivial`'s `cn_profiles`, where it
//! runs for a cleared ask alone, and never through a class's `profiles`.

use anyhow::{bail, Result};

use super::routing::{CHEAPEST, MODES};
use crate::catalog::Catalog;
use crate::Config;

/// The refusals, in words that name the key and what to do. A profile a class
/// names that is not configured is skipped: the defaults name profiles a
/// sparse note may lack, and routing passes over them.
pub(super) fn validate(cfg: &Config, catalog: &Catalog) -> Result<()> {
    let restricted = |p: &str| {
        let origin = cfg.profile_origin(p, catalog);
        (cfg.all_profiles().contains_key(p) && origin.restricted()).then_some(origin)
    };
    drafts_dir(cfg)?;
    for m in MODES {
        let t = cfg.routing.classes.table(m);
        for p in t.profiles.iter().filter(|p| *p != CHEAPEST) {
            let Some(origin) = restricted(p) else {
                continue;
            };
            let way_out = match origin {
                crate::catalog::ModelOrigin::Unknown => format!(
                    "; or if its endpoint is trusted, vouch it with [providers.{}] origin",
                    cfg.all_profiles()[p].provider
                ),
                _ => String::new(),
            };
            bail!(
                "routing.classes.{m}.profiles: {p} is of restricted origin ({}): list it in \
                 routing.classes.trivial.cn_profiles, where it runs only for an open request{way_out}",
                format!("{origin:?}").to_lowercase()
            );
        }
        drafts(cfg, catalog, m, t, &restricted)?;
        if t.cn_profiles.is_empty() {
            if t.cn_after.is_some() {
                bail!("routing.classes.{m}.cn_after: it has no cn_profiles to place");
            }
            continue;
        }
        if m != "trivial" {
            bail!(
                "routing.classes.{m}.cn_profiles: a China-origin model never answers {m} (the \
                 owner's rule: it drafts code that a Claude model reviews; the draft is \
                 `cn_draft`, a later key)"
            );
        }
        for p in &t.cn_profiles {
            if restricted(p).is_none() {
                bail!(
                    "routing.classes.{m}.cn_profiles: {p:?} is not a configured profile of \
                     restricted origin (an unrestricted one belongs in profiles; `{CHEAPEST}` \
                     never names one)"
                );
            }
        }
        if let Some(a) = t.cn_after.as_ref().filter(|a| !t.profiles.contains(a)) {
            bail!("routing.classes.{m}.cn_after: {a:?} is not an entry of routing.classes.{m}.profiles");
        }
    }
    Ok(())
}

/// The classes a China-origin model may draft for (theseus-a1wvq.13).
pub const DRAFTING: [&str; 2] = ["routine_coding", "deep_coding"];

/// Where the draft trees live, when a class drafts: a root of the workspace,
/// so a review's runs there take the tool's posture and the state dir's
/// approve-list hold does not reach them (theseus-iilre).
pub(crate) fn drafts_root(cfg: &Config) -> Option<std::path::PathBuf> {
    DRAFTING
        .iter()
        .any(|m| cfg.routing.classes.table(m).cn_draft.is_some())
        .then(|| theseus_tools::paths::canonical_best_effort(&cfg.drafts_dir()))
}

/// `server.drafts_dir` outside the state dir and every approve-list entry: a
/// draft tree inside either would bring the asking back (theseus-iilre).
fn drafts_dir(cfg: &Config) -> Result<()> {
    let canon = |p: &str| theseus_tools::paths::canonical_best_effort(&super::expand(p));
    let at = canon(&cfg.server.drafts_dir);
    let held = std::iter::once(("server.state_dir", canon(&cfg.server.state_dir))).chain(
        cfg.tools
            .approve_paths
            .iter()
            .map(|a| ("tools.approve_paths", canon(a))),
    );
    for (key, dir) in held {
        if theseus_tools::paths::within(&at, &dir) {
            bail!(
                "server.drafts_dir: {:?} is inside {key} ({}), where the review's runs would ask \
                 every time: keep draft trees outside it",
                cfg.server.drafts_dir,
                dir.display()
            );
        }
    }
    Ok(())
}

/// `cn_draft` and `final` (theseus-a1wvq.13): a restricted profile drafts only
/// a coding class's asks, and only with Claude profiles to review it.
fn drafts(
    cfg: &Config,
    catalog: &Catalog,
    m: &str,
    t: &super::routing::ClassProfiles,
    restricted: &dyn Fn(&str) -> Option<crate::catalog::ModelOrigin>,
) -> Result<()> {
    let Some(d) = &t.cn_draft else {
        if !t.final_review.is_empty() {
            bail!("routing.classes.{m}.final: it reviews a draft, and the class has no cn_draft");
        }
        if t.first_pass.is_some() {
            bail!(
                "routing.classes.{m}.first_pass: it reads a draft, and the class has no cn_draft"
            );
        }
        return Ok(());
    };
    if !DRAFTING.contains(&m) {
        bail!(
            "routing.classes.{m}.cn_draft: a China-origin model drafts only code, in \
             routing.classes.routine_coding or deep_coding"
        );
    }
    if restricted(d).is_none() {
        bail!(
            "routing.classes.{m}.cn_draft: {d:?} is not a configured profile of restricted \
             origin (a Claude profile belongs in profiles, or in final)"
        );
    }
    if t.final_review.is_empty() {
        bail!(
            "routing.classes.{m}.final: cn_draft needs it: name the Claude profiles that \
             review every draft, as final = [\"sonnet\"]"
        );
    }
    if let Some(f) = &t.first_pass {
        if restricted(f).is_none() {
            bail!(
                "routing.classes.{m}.first_pass: {f:?} is not a configured profile of restricted \
                 origin: a Claude profile reviews in final, and the first pass is a cheaper \
                 model's first read"
            );
        }
    }
    if t.first_pass.as_deref() == Some(d.as_str()) {
        bail!(
            "routing.classes.{m}.first_pass: {d:?} is also the class's cn_draft, and the drafting \
             model would review its own draft: name another restricted profile, or none"
        );
    }
    for p in &t.final_review {
        if !claude(cfg, catalog, p) {
            bail!(
                "routing.classes.{m}.final: {p} is not a Claude profile: a Claude model \
                 reviews every draft"
            );
        }
    }
    Ok(())
}

/// A configured profile of US origin whose model is a Claude row of the
/// catalog (one the catalog's own Anthropic rows hold).
pub(crate) fn claude(cfg: &Config, catalog: &Catalog, profile: &str) -> bool {
    cfg.all_profiles().get(profile).is_some_and(|p| {
        cfg.profile_origin(profile, catalog) == crate::catalog::ModelOrigin::Us
            && catalog
                .get(&p.model)
                .is_some_and(|e| e.provider == "anthropic")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The template with `routing` appended to it, loaded as the daemon
    /// loads a config, plus any `extra` (providers and profiles).
    fn load(extra: &str, routing: &str) -> Result<Config, String> {
        let text = format!("{}\n{extra}\n{routing}\n", Config::EXAMPLE_TOML);
        let cfg = Config::from_toml(&text).map_err(|e| e.to_string())?;
        cfg.validate().map(|()| cfg).map_err(|e| format!("{e:#}"))
    }

    /// Providers borrow the template's `zai_api_key` entry: the checks read
    /// endpoints and origins, not keys.
    const DEEPSEEK: &str =
        "[providers.deepseek]\napi_base = \"https://api.deepseek.com/anthropic\"\n\
        api_key_secret = \"zai_api_key\"\n[profiles.dsflash]\nprovider = \"deepseek\"\n\
        model = \"deepseek-flash\"\n";
    const ODD: &str = "[providers.odd]\napi_base = \"https://llm.widgetco.test/v1\"\n\
        api_key_secret = \"zai_api_key\"\n[profiles.oddball]\nprovider = \"odd\"\n\
        model = \"widget-1\"\n";

    #[test]
    fn a_mode_list_never_names_a_restricted_profile() {
        // The defaults, with the template's profiles, load.
        load("", "").unwrap();
        for (class, p) in [("chat", "glm"), ("review", "glm53"), ("trivial", "glm")] {
            let e = load(
                "",
                &format!("[routing.classes.{class}]\nprofiles = [\"{p}\"]"),
            )
            .unwrap_err();
            assert!(
                e.contains(&format!(
                    "routing.classes.{class}.profiles: {p} is of restricted origin (cn)"
                )) && e.contains("routing.classes.trivial.cn_profiles"),
                "{e}"
            );
        }
        let e = load(
            DEEPSEEK,
            "[routing.classes.sophisticated]\nprofiles = [\"dsflash\", \"opus\"]",
        )
        .unwrap_err();
        assert!(
            e.contains("sophisticated.profiles: dsflash is of restricted origin (cn)"),
            "{e}"
        );
        // An unknown endpoint is refused, with the way out named.
        let e = load(ODD, "[routing.classes.chat]\nprofiles = [\"oddball\"]").unwrap_err();
        assert!(
            e.contains("oddball is of restricted origin (unknown)")
                && e.contains("[providers.odd] origin"),
            "{e}"
        );
        // Vouched `us`, it loads; an unconfigured name and `cheapest` load.
        let vouched = ODD.replace("[providers.odd]\n", "[providers.odd]\norigin = \"us\"\n");
        load(&vouched, "[routing.classes.chat]\nprofiles = [\"oddball\"]").unwrap();
        load(
            "",
            "[routing.classes.chat]\nprofiles = [\"nope\", \"cheapest\"]",
        )
        .unwrap();
    }

    #[test]
    fn cn_profiles_are_refused_where_china_models_never_answer() {
        let ok = "[routing.classes.trivial]\nprofiles = [\"haiku\", \"cheapest\"]\ncn_profiles = [\"glm\"]\ncn_after = \"haiku\"";
        let cfg = load("", ok).unwrap();
        assert_eq!(cfg.routing.classes.trivial.cn_profiles, ["glm"]);
        // route.v9's two classes included: a China-origin model on a run_tool
        // turn would read tool output, which waits on theseus-ouqru's screen.
        for m in [
            "run_tool",
            "commentary",
            "quick",
            "chat",
            "sophisticated",
            "review",
            "routine_coding",
            "deep_coding",
        ] {
            let e = load(
                "",
                &format!("[routing.classes.{m}]\ncn_profiles = [\"glm\"]"),
            )
            .unwrap_err();
            assert!(
                e.contains(&format!(
                    "routing.classes.{m}.cn_profiles: a China-origin model never answers {m}"
                )),
                "{e}"
            );
        }
        for (bad, says) in [
            (
                "cn_profiles = [\"nope\"]",
                "\"nope\" is not a configured profile of restricted origin",
            ),
            (
                "cn_profiles = [\"opus\"]",
                "\"opus\" is not a configured profile of restricted origin",
            ),
            (
                "cn_profiles = [\"cheapest\"]",
                "\"cheapest\" is not a configured profile of restricted origin",
            ),
            (
                "cn_profiles = [\"glm\"]\ncn_after = \"fable\"",
                "cn_after: \"fable\" is not an entry",
            ),
            (
                "cn_after = \"haiku\"",
                "trivial.cn_after: it has no cn_profiles",
            ),
        ] {
            let e = load(
                "",
                &format!("[routing.classes.trivial]\nprofiles = [\"haiku\"]\n{bad}"),
            )
            .unwrap_err();
            assert!(e.contains(says), "{bad}: {e}");
        }
        let e = load("", "[routing.classes.chat]\ncn_after = \"haiku\"").unwrap_err();
        assert!(e.contains("chat.cn_after: it has no cn_profiles"), "{e}");
    }

    #[test]
    fn cn_profiles_parse_per_mode() {
        use crate::config::RoutingConfig;
        for m in crate::config::routing::MODES {
            let text = format!(
                "[classes.{m}]\nprofiles = [\"haiku\"]\ncn_profiles = [\"glm\"]\ncn_after = \"haiku\""
            );
            let c: RoutingConfig = toml::from_str(&text).unwrap();
            let t = c.classes.table(m);
            assert_eq!(
                (t.cn_profiles.as_slice(), t.cn_after.as_deref()),
                (["glm".to_string()].as_slice(), Some("haiku")),
                "{m}"
            );
        }
        // A config without them, and the sparse writer's output, are as before.
        let d = toml::to_string(&RoutingConfig::default()).unwrap();
        assert!(!d.contains("cn_"), "{d}");
        let back: RoutingConfig = toml::from_str("").unwrap();
        assert_eq!(back, RoutingConfig::default());
        assert!(
            toml::from_str::<RoutingConfig>("[classes.trivial]\ncn_profile = [\"glm\"]").is_err()
        );
    }
}
