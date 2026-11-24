//! The keys this build renamed, and the message a config that still has one gets. The loader rejects unknown keys, so
//! an old name is always an error; this turns that error into the new name instead of "unknown field". There is no
//! alias: a renamed key fails the load until the config says the new one (the owner, 2026-10-17: "more important to
//! be right than to be historically conservative this early").
//!
//! A path is dotted from the document's root; `*` stands for any table's name (`profiles.*` is every profile).

use toml::{Table, Value};

/// Every (old, new) pair, in the order a document is checked. A key retired outright is not here: it is the
/// retired-keys list's (`config/retired_keys.rs`), which loads with a warning, and so are the old names of a
/// key it retired (`[model] keep_warm_hours`, `[threads] keep_warm_hours`).
pub const RENAMED: &[(&str, &str)] = &[
    ("model.max_loops_mode", "model.on_max_loops"),
    ("profiles.*.max_loops_mode", "profiles.*.on_max_loops"),
    ("routing.modes", "routing.classes"),
    ("routing.mode", "routing.rung"),
    ("judge.packs.*.mode", "judge.packs.*.rung"),
    ("route", "route_bench"),
    ("persona.import_files", "persona_import.import_files"),
    ("persona.own_boost", "persona_import.own_boost"),
    ("model.cache_ttl", "cache.ttl"),
    ("watches", "watch_for"),
    ("routing.data.private_topics", "privacy.sensitive_names"),
    ("self.sealed_dir", "self.sealed.dir"),
    ("content.enabled", "content_based_security.session_flags"),
    ("content.injection", "content_based_security.injection"),
    ("content.wait_ms", "content_based_security.wait_ms"),
    ("content.max_pieces", "content_based_security.max_pieces"),
];

/// Every key this build retired outright, and what replaced it. Like a renamed key, one of these fails the load,
/// where the retired-keys list's warns (theseus-gyjyo.8): the `injection` label replaced the external-text hold,
/// and a config that still names it would otherwise believe a mechanism is there that is not.
pub const RETIRED: &[(&str, &str)] = &[
    (
        "policy.external_text",
        "the `injection` label replaced it: reading outside text no longer holds a session, and a session that \
         carries `injection` runs its calls and tells the operator — only `[content_based_security] injection = \
         \"refuse\"` refuses the calls that act until he removes the label",
    ),
    (
        "policy.external_programs",
        "the `injection` label replaced it: no program's output holds a session any more; the content pack asks \
         of every result what it asks of a page, and `injection` tells the operator rather than stopping the \
         session, unless `[content_based_security] injection = \"refuse\"` is set",
    ),
];

/// The profile names Theseus once shipped that fixed an effort, each with the profile that replaces it. Effort is
/// chosen for each message now, so none is shipped; a config that still names one without defining it is told so by
/// the lookup (`Config::profile`), for one release. A profile the config defines under such a name is its own and is
/// never touched.
pub const RENAMED_PROFILES: &[(&str, &str)] = &[
    ("sonnethi", "sonnet"),
    ("opushi", "opus"),
    ("haikuhi", "haiku"),
];

/// The words of a refusal of the profile `name`: the renamed message when `name` is one Theseus once
/// shipped, else the site's own (`own`). Every place a person names a profile that no profile answers to
/// says it through this, so the three old names are told where they went wherever they are typed.
pub fn or_unknown(name: &str, own: impl FnOnce() -> String) -> String {
    renamed_profile(name).unwrap_or_else(own)
}

/// What a lookup of the profile `name` says when it names none and was once shipped, or none.
pub fn renamed_profile(name: &str) -> Option<String> {
    let (old, new) = RENAMED_PROFILES.iter().find(|(old, _)| *old == name)?;
    Some(format!(
        "the profile {old:?} was renamed: Theseus no longer ships a profile that fixes the effort, because it \
         chooses how much effort each message needs by itself. Use the profile {new:?}, or add a profile of your \
         own with effort = \"high\" under [profiles.<name>] to fix it yourself"
    ))
}

/// The message for the first old key `text` has, or none (also for text that is not TOML: its own error stands).
pub fn explain(text: &str) -> Option<String> {
    let doc: Table = text.parse().ok()?;
    let retired = RETIRED.iter().find_map(|(old, why)| {
        let table = found(&doc, &old.split('.').collect::<Vec<_>>())?;
        Some(format!("{} was retired: {why}", show(old, table)))
    });
    retired.or_else(|| renamed_in(&doc))
}

fn renamed_in(doc: &Table) -> Option<String> {
    RENAMED.iter().find_map(|(old, new)| {
        let table = found(doc, &old.split('.').collect::<Vec<_>>())?;
        Some(format!(
            "{} was renamed to {}",
            show(old, table),
            show(new, table)
        ))
    })
}

/// Whether `path` is in `table`, and whether what it names is itself a table.
fn found(table: &Table, path: &[&str]) -> Option<bool> {
    match path {
        [] => None,
        [last] => table.get(*last).map(Value::is_table),
        [first, rest @ ..] if *first == "*" => table
            .values()
            .filter_map(Value::as_table)
            .find_map(|t| found(t, rest)),
        [first, rest @ ..] => found(table.get(*first)?.as_table()?, rest),
    }
}

/// `[model] max_loops_mode` for a key, `[route]` for a section (what was found is a table).
fn show(path: &str, is_table: bool) -> String {
    match path.rsplit_once('.') {
        Some((section, key)) if !is_table => format!("`[{section}] {key}`"),
        _ => format!("`[{path}]`"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;

    /// Each old key alone in a document, and the message it must get: one literal row for each row of `RENAMED`,
    /// so a row dropped from the table fails its case here.
    const CASES: &[(&str, &str)] = &[
        (
            "[model]\nmax_loops_mode = \"end\"\n",
            "`[model] max_loops_mode` was renamed to `[model] on_max_loops`",
        ),
        (
            "[profiles.sonnet]\nmax_loops_mode = \"end\"\n",
            "`[profiles.*] max_loops_mode` was renamed to `[profiles.*] on_max_loops`",
        ),
        (
            "[routing.modes.chat]\nprofiles = [\"sonnet\"]\n",
            "`[routing.modes]` was renamed to `[routing.classes]`",
        ),
        (
            "[routing]\nmode = \"shadow\"\n",
            "`[routing] mode` was renamed to `[routing] rung`",
        ),
        (
            "[judge.packs.\"loop.v2\"]\nmode = \"off\"\n",
            "`[judge.packs.*] mode` was renamed to `[judge.packs.*] rung`",
        ),
        (
            "[route]\nreplay_day_usd = 2.0\n",
            "`[route]` was renamed to `[route_bench]`",
        ),
        (
            "[persona]\nimport_files = [\"SOUL.md\"]\n",
            "`[persona] import_files` was renamed to `[persona_import] import_files`",
        ),
        (
            "[persona]\nown_boost = 1.5\n",
            "`[persona] own_boost` was renamed to `[persona_import] own_boost`",
        ),
        (
            "[model]\ncache_ttl = \"1h\"\n",
            "`[model] cache_ttl` was renamed to `[cache] ttl`",
        ),
        (
            "[watches]\nmax_days = 10\n",
            "`[watches]` was renamed to `[watch_for]`",
        ),
        (
            "[routing.data]\nprivate_topics = [\"Acme\"]\n",
            "`[routing.data] private_topics` was renamed to `[privacy] sensitive_names`",
        ),
        (
            "[self]\nsealed_dir = \"/srv/theseus-sealed\"\n",
            "`[self] sealed_dir` was renamed to `[self.sealed] dir`",
        ),
        (
            "[content]\nenabled = false\n",
            "`[content] enabled` was renamed to `[content_based_security] session_flags`",
        ),
        (
            "[content]\ninjection = \"notify\"\n",
            "`[content] injection` was renamed to `[content_based_security] injection`",
        ),
        (
            "[content]\nwait_ms = 1000\n",
            "`[content] wait_ms` was renamed to `[content_based_security] wait_ms`",
        ),
        (
            "[content]\nmax_pieces = 3\n",
            "`[content] max_pieces` was renamed to `[content_based_security] max_pieces`",
        ),
    ];

    #[test]
    fn every_old_key_fails_the_load_with_its_new_name() {
        for (text, message) in CASES {
            let err = Config::parse(text).expect_err(message);
            assert!(format!("{err:#}").contains(message), "{message}: {err:#}");
        }
    }

    /// An old profile name that no profile of the config has is met with where it went, for each of the three;
    /// a profile the config defines under the name is its own and resolves; any other missing name is only unknown.
    #[test]
    fn an_old_high_effort_profile_name_is_told_it_was_renamed() {
        let c = Config::example();
        for (old, new) in RENAMED_PROFILES {
            let said = format!("{:#}", c.profile(old).expect_err(old));
            assert!(said.contains(&format!("{old:?} was renamed")), "{said}");
            assert!(said.contains(&format!("Use the profile {new:?}")), "{said}");
            assert!(!said.contains("unknown profile"), "{said}");
            assert!(
                c.profile(new).is_ok(),
                "the profile it points at ships: {new}"
            );
        }
        let own = format!(
            "{}\n[profiles.sonnethi]\nprovider = \"anthropic\"\nmodel = \"claude-sonnet-5-5\"\neffort = \"high\"\n",
            Config::EXAMPLE_TOML
        );
        let (c, _) = Config::parse(&own).unwrap();
        assert!(
            c.profile("sonnethi").is_ok(),
            "the config's own profile stands"
        );
        let said = format!("{:#}", Config::example().profile("sonnetlow").unwrap_err());
        assert!(said.starts_with("unknown profile"), "{said}");
    }

    /// A result file of `docs/benchmarks`: a dated run's report or data, and the two summaries built from the
    /// runs (`omnibus`, `index.json`). They record what ran, under the names it ran with.
    fn is_result(name: &str) -> bool {
        name.starts_with(|c: char| c.is_ascii_digit())
            || name.starts_with("omnibus.")
            || name == "index.json"
    }

    /// The files under `dir` (a path relative to the workspace root) that hold `word` as a whole word, skipping
    /// what is built or fetched (`target`, `node_modules`, `.git` and the cockpit's build) and the benchmark
    /// result files (`is_result`), nothing else.
    fn naming(root: &std::path::Path, dir: &str, word: &str, found: &mut Vec<String>) {
        let built = ["target", "node_modules", ".git"];
        let Ok(entries) = std::fs::read_dir(root.join(dir)) else {
            return;
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let at = if dir.is_empty() {
                name.clone()
            } else {
                format!("{dir}/{name}")
            };
            let cockpit_build = ["cockpit/dist", "crates/theseusd/cockpit/dist"];
            if built.contains(&name.as_str())
                || cockpit_build.contains(&at.as_str())
                || (dir == "docs/benchmarks" && is_result(&name))
            {
                continue;
            }
            if e.path().is_dir() {
                naming(root, &at, word, found);
            } else if let Ok(text) = std::fs::read_to_string(e.path()) {
                let whole = |i: usize| {
                    let before = text[..i].chars().next_back();
                    let after = text[i + word.len()..].chars().next();
                    !before.is_some_and(|c| c.is_alphanumeric() || c == '_')
                        && !after.is_some_and(|c| c.is_alphanumeric() || c == '_')
                };
                if text.match_indices(word).any(|(i, _)| whole(i)) {
                    found.push(at);
                }
            }
        }
    }

    /// A config whose `[model] live` names one of the three, with no table of that name, does
    /// not load, and says where the name went (the operator who follows the new template and
    /// deletes the tables is the one this release's message is for).
    #[test]
    fn a_live_profile_that_is_an_old_name_with_no_table_is_told_it_was_renamed() {
        let text = Config::EXAMPLE_TOML.replace("live = \"sonnet\"\n", "live = \"opushi\"\n");
        let said = format!("{:#}", Config::parse(&text).unwrap_err());
        assert!(
            said.contains("\"opushi\" was renamed") && said.contains("Use the profile \"opus\""),
            "{said}"
        );
        let other = Config::EXAMPLE_TOML.replace("live = \"sonnet\"\n", "live = \"nowhere\"\n");
        let said = format!("{:#}", Config::parse(&other).unwrap_err());
        assert!(said.contains("model.live = \"nowhere\" is not"), "{said}");
    }

    /// No file of the tree names a profile that once fixed an effort, bar this one (the table of what was
    /// renamed and its tests) and the benchmark result files of `docs/benchmarks` (`is_result`), which are
    /// records of what ran then.
    #[test]
    fn no_shipped_file_names_a_retired_high_effort_profile() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for (old, _) in RENAMED_PROFILES {
            let mut found = Vec::new();
            naming(&root, "", old, &mut found);
            found.retain(|f| f != "crates/theseus-core/src/config/renamed.rs");
            assert!(found.is_empty(), "{old:?} is still named by: {found:?}");
        }
    }

    #[test]
    fn the_table_and_its_cases_are_the_same_length() {
        assert_eq!(CASES.len(), RENAMED.len());
    }

    /// Moves every renamed key of `old` to its new path, as an operator editing the file would.
    fn migrated(old: &str) -> String {
        fn take(t: &mut Table, path: &[&str]) -> Vec<(Vec<String>, Value)> {
            match path {
                [last] => t.remove(*last).map(|v| (vec![], v)).into_iter().collect(),
                [first, rest @ ..] if *first == "*" => t
                    .iter_mut()
                    .filter_map(|(k, v)| v.as_table_mut().map(|t| (k.clone(), t)))
                    .flat_map(|(k, t)| {
                        take(t, rest).into_iter().map(move |(mut at, v)| {
                            at.insert(0, k.clone());
                            (at, v)
                        })
                    })
                    .collect(),
                [first, rest @ ..] => t
                    .get_mut(*first)
                    .and_then(Value::as_table_mut)
                    .map(|t| take(t, rest))
                    .unwrap_or_default(),
                [] => vec![],
            }
        }
        fn put(t: &mut Table, path: &[String], v: Value) {
            match path {
                [last] => {
                    t.insert(last.clone(), v);
                }
                [first, rest @ ..] => {
                    let entry = t
                        .entry(first.clone())
                        .or_insert_with(|| Value::Table(Table::new()));
                    put(entry.as_table_mut().unwrap(), rest, v);
                }
                [] => {}
            }
        }
        let mut doc: Table = old.parse().unwrap();
        for (old_path, new_path) in RENAMED {
            let from: Vec<&str> = old_path.split('.').collect();
            for (wild, v) in take(&mut doc, &from) {
                // The new path's `*` takes the names the old one matched, in order.
                let mut names = wild.into_iter();
                let to: Vec<String> = new_path
                    .split('.')
                    .map(|p| {
                        if p == "*" {
                            names.next().unwrap()
                        } else {
                            p.to_string()
                        }
                    })
                    .collect();
                put(&mut doc, &to, v);
            }
        }
        fn prune(t: &mut Table) {
            for (_, v) in t.iter_mut() {
                if let Some(t) = v.as_table_mut() {
                    prune(t);
                }
            }
            t.retain(|_, v| v.as_table().is_none_or(|t| !t.is_empty()));
        }
        prune(&mut doc);
        toml::to_string(&doc).unwrap()
    }

    fn resolved(text: &str) -> String {
        toml::to_string(&Config::parse(text).unwrap().0).unwrap()
    }

    /// An old-shaped config with every renamed key in it, moved by the table, is the new-shaped one.
    #[test]
    fn an_old_shaped_config_resolves_to_the_new_shaped_ones_config() {
        let secrets = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n";
        let old = format!(
            "{secrets}\n[model]\nmax_loops_mode = \"end\"\ncache_ttl = \"1h\"\nkeep_warm_hours = 12\n\
             [profiles.sonnet]\nprovider = \"anthropic\"\nmodel = \"claude-sonnet-5-5\"\nmax_loops_mode = \"notify\"\n\
             [routing]\nmode = \"shadow\"\n[routing.modes.chat]\nprofiles = [\"sonnet\"]\n\
             [routing.data]\nprivate_topics = [\"Acme\"]\n\
             [judge.packs.\"loop.v2\"]\nmode = \"off\"\n\
             [route]\nreplay_day_usd = 2.0\n\
             [persona]\nimport_files = [\"SOUL.md\"]\nown_boost = 1.5\n\
             [cache]\nkeep_warm_minutes = 50\n[threads]\nkeep_warm_hours = 2\n\
             [watches]\nmax_days = 10\n\
             [content]\ninjection = \"notify\"\nwait_ms = 1000\nmax_pieces = 3\n"
        );
        let new = format!(
            "{secrets}\n[model]\non_max_loops = \"end\"\n\
             [profiles.sonnet]\nprovider = \"anthropic\"\nmodel = \"claude-sonnet-5-5\"\non_max_loops = \"notify\"\n\
             [routing]\nrung = \"shadow\"\n[routing.classes.chat]\nprofiles = [\"sonnet\"]\n\
             [privacy]\nsensitive_names = [\"Acme\"]\n\
             [judge.packs.\"loop.v2\"]\nrung = \"off\"\n\
             [route_bench]\nreplay_day_usd = 2.0\n\
             [persona_import]\nimport_files = [\"SOUL.md\"]\nown_boost = 1.5\n\
             [cache]\nttl = \"1h\"\nkeep_warm_hours = 12\nkeep_warm_every_minutes = 50\n\
             threads_keep_warm_hours = 2\n\
             [watch_for]\nmax_days = 10\n\
             [content_based_security]\ninjection = \"notify\"\n\
             wait_ms = 1000\nmax_pieces = 3\n"
        );
        assert!(Config::parse(&old).is_err());
        assert_eq!(resolved(&migrated(&old)), resolved(&new));
    }

    /// The owner-shaped fixture, before and after the renames.
    #[test]
    fn the_owner_shaped_fixture_resolves_the_same_before_and_after() {
        let old = include_str!("../../fixtures/owner_shape_old.toml");
        let new = include_str!("../../fixtures/owner_shape.toml");
        assert!(Config::parse(old).is_err());
        assert_eq!(resolved(&migrated(old)), resolved(new));
    }

    #[test]
    fn a_config_with_none_of_them_is_not_explained() {
        assert_eq!(explain("[model]\nmax_loops = 5\n"), None);
        assert_eq!(explain("not toml ["), None);
    }
}
