//! The keys this build renamed, and the message a config that still has one gets. The loader rejects unknown keys, so
//! an old name is always an error; this turns that error into the new name instead of "unknown field". There is no
//! alias: a renamed key fails the load until the config says the new one (the owner, 2026-10-17: "more important to
//! be right than to be historically conservative this early").
//!
//! A path is dotted from the document's root; `*` stands for any table's name (`profiles.*` is every profile).

use toml::{Table, Value};

/// Every (old, new) pair, in the order a document is checked. A key retired outright is not here: it is the
/// retired-keys list's (`config.rs`), which loads with a warning.
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
    ("model.keep_warm_hours", "cache.keep_warm_hours"),
    ("cache.keep_warm_minutes", "cache.keep_warm_every_minutes"),
    ("threads.keep_warm_hours", "cache.threads_keep_warm_hours"),
    ("watches", "watch_for"),
    ("routing.data.private_topics", "privacy.sensitive_names"),
    ("self.sealed_dir", "self.sealed.dir"),
    ("content.enabled", "content_based_security.session_flags"),
    ("content.injection", "content_based_security.injection"),
    ("content.wait_ms", "content_based_security.wait_ms"),
    ("content.max_pieces", "content_based_security.max_pieces"),
];

/// The message for the first old key `text` has, or none (also for text that is not TOML: its own error stands).
pub fn explain(text: &str) -> Option<String> {
    let doc: Table = text.parse().ok()?;
    RENAMED.iter().find_map(|(old, new)| {
        let table = found(&doc, &old.split('.').collect::<Vec<_>>())?;
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
            "[judge.packs.\"loop.v1\"]\nmode = \"off\"\n",
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
            "[model]\nkeep_warm_hours = 12\n",
            "`[model] keep_warm_hours` was renamed to `[cache] keep_warm_hours`",
        ),
        (
            "[cache]\nkeep_warm_minutes = 50\n",
            "`[cache] keep_warm_minutes` was renamed to `[cache] keep_warm_every_minutes`",
        ),
        (
            "[threads]\nkeep_warm_hours = 2\n",
            "`[threads] keep_warm_hours` was renamed to `[cache] threads_keep_warm_hours`",
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
             [judge.packs.\"loop.v1\"]\nmode = \"off\"\n\
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
             [judge.packs.\"loop.v1\"]\nrung = \"off\"\n\
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
