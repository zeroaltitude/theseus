//! A config cut to what differs from the defaults (theseus-vwar, review item
//! 15). Every key has a safe default, so a note that holds only what differs
//! loads as it is. `theseusd config --sparse` prints one from the loaded
//! note: the switch from a note that is the whole template, pasted, to a
//! note of the deployment's own values.

use anyhow::{ensure, Context, Result};

use super::Config;

/// A sparse note's first line.
pub const SPARSE_HEADER: &str = "# Theseus config: only what differs from the defaults \
     (`theseusd example-config` documents every key).\n";

/// `text`, a config document, as a note that holds only what differs from
/// the defaults: the secrets' references as they are, every value that
/// differs from its default, and nothing else. A key or table stays only
/// when the config would change without it, so a value equal to its
/// default goes, and so do a retired key that nothing reads and a
/// `[catalog]` table that changes no figure of the code's row. Tables and
/// keys come in name order, as `toml` keeps them. The note is checked to
/// load to the same config.
pub fn sparse_note(text: &str) -> Result<String> {
    let (cfg, _) = Config::parse(text)?;
    let want = acting(&cfg)?;
    let mut doc: toml::Table = text.parse().context("parsing config TOML")?;
    prune(&mut doc, &mut Vec::new(), &want);
    let note = format!("{SPARSE_HEADER}{}", toml::to_string_pretty(&doc)?);
    let (again, _) = Config::parse(&note).context("the sparse note does not load")?;
    ensure!(
        acting(&again)? == want,
        "the sparse note loads to another config"
    );
    Ok(note)
}

/// The config as it acts, to compare two by: every setting it prints, and
/// the catalog as the code's rows with the config's tables over them.
fn acting(cfg: &Config) -> Result<toml::Value> {
    let mut v = toml::Value::try_from(cfg)?;
    let catalog = crate::catalog::Catalog::with_overrides(&cfg.catalog);
    if let Some(t) = v.as_table_mut() {
        t.insert("catalog".into(), toml::Value::try_from(catalog)?);
    }
    Ok(v)
}

/// Leave out each key of the table at `path` whose absence changes nothing,
/// and look inside each table that must stay.
fn prune(doc: &mut toml::Table, path: &mut Vec<String>, want: &toml::Value) {
    let keys: Vec<String> = table(doc, path)
        .map(|t| t.keys().cloned().collect())
        .unwrap_or_default();
    for k in keys {
        let mut trial = doc.clone();
        if let Some(t) = table_mut(&mut trial, path) {
            t.remove(&k);
        }
        if loads_as(&trial, want) {
            *doc = trial;
            continue;
        }
        path.push(k);
        prune(doc, path, want);
        path.pop();
    }
}

/// Whether `doc` loads to the config `want` is.
fn loads_as(doc: &toml::Table, want: &toml::Value) -> bool {
    toml::to_string(doc)
        .ok()
        .and_then(|text| Config::parse(&text).ok())
        .and_then(|(cfg, _)| acting(&cfg).ok())
        .is_some_and(|v| v == *want)
}

fn table<'a>(doc: &'a toml::Table, path: &[String]) -> Option<&'a toml::Table> {
    path.iter().try_fold(doc, |t, k| t.get(k)?.as_table())
}

fn table_mut<'a>(doc: &'a mut toml::Table, path: &[String]) -> Option<&'a mut toml::Table> {
    path.iter()
        .try_fold(doc, |t, k| t.get_mut(k)?.as_table_mut())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRETS: &str = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n";

    /// Load, print sparse, load again: the same config, and a sparse note is
    /// its own sparse note.
    fn round_trip(text: &str) -> String {
        let (cfg, _) = Config::parse(text).unwrap();
        let note = sparse_note(text).unwrap();
        let (again, _) = Config::parse(&note).unwrap();
        assert_eq!(acting(&again).unwrap(), acting(&cfg).unwrap(), "{note}");
        assert_eq!(sparse_note(&note).unwrap(), note, "a sparse note is sparse");
        note
    }

    /// Every key has a safe default: a note of only `[secrets]` loads, with
    /// no warning, and is its own sparse note.
    #[test]
    fn a_note_of_only_secrets_loads_and_is_already_sparse() {
        let (cfg, warnings) = Config::parse(SECRETS).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(cfg.model.model, "claude-sonnet-5-5");
        assert_eq!(round_trip(SECRETS), format!("{SPARSE_HEADER}{SECRETS}"));
    }

    /// A value equal to its default goes, and one that differs stays, with
    /// its table; a retired key goes, and its warning with it.
    #[test]
    fn a_default_value_goes_and_a_different_one_stays() {
        let text = format!(
            "{SECRETS}\n[kernel]\nadmission_ceiling = 8\nspend_limit_usd = 25.0\n\
             default_budget = 1000000\n\n[web]\nenabled = true\nport = 7433\n\n\
             [discord]\nenabled = false\n"
        );
        let (_, warnings) = Config::parse(&text).unwrap();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        let note = round_trip(&text);
        assert_eq!(
            note,
            format!(
                "{SPARSE_HEADER}[discord]\nenabled = false\n\n\
                 [kernel]\nspend_limit_usd = 25.0\n\n{SECRETS}"
            )
        );
        let (_, warnings) = Config::parse(&note).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    /// The whole template, pasted as a note, loads, and its sparse note keeps
    /// every secret's reference as it is, every value the template sets
    /// apart from its default, and nothing else.
    #[test]
    fn the_whole_template_as_a_note_loads_and_cuts_to_what_differs() {
        let whole = Config::EXAMPLE_TOML;
        Config::parse(whole).unwrap();
        let note = round_trip(whole);
        let doc: toml::Table = whole.parse().unwrap();
        let cut: toml::Table = note.parse().unwrap();
        assert_eq!(cut["secrets"], doc["secrets"], "the references as they are");
        // The template sets these apart from their defaults.
        assert_eq!(cut["narrative"], toml::Value::Boolean(true));
        assert_eq!(cut["model"]["live"].as_str(), Some("sonnet"));
        assert!(cut["profiles"].get("glm").is_some());
        assert!(cut["providers"].get("zai").is_some());
        // And these are its defaults.
        assert!(cut.get("server").is_none() && cut.get("index").is_none());
        assert!(cut["model"].get("max_loops").is_none());
        let lines = |t: &str| t.lines().filter(|l| !l.trim().is_empty()).count();
        assert!(
            lines(&note) * 3 < lines(whole),
            "{} of {} lines:\n{note}",
            lines(&note),
            lines(whole)
        );
    }

    /// A note pasted from the template before theseus-vwar carries the code's
    /// prices, one `[catalog]` table per built-in model with its five prices,
    /// as the template printed them. It loads as it is, to the config the
    /// template alone loads to, and its sparse note drops every table; a
    /// table one price apart stays, with that price alone.
    #[test]
    fn the_old_templates_price_tables_load_unchanged_and_the_sparse_note_drops_them() {
        use crate::catalog::Catalog;
        let code = Catalog::builtin();
        let table = |id: &str, out: f64| {
            let e = code.get(id).unwrap();
            format!(
                "[catalog.\"{id}\"]\ninput_per_mtok = {:?}\noutput_per_mtok = {out:?}\n\
                 cache_read_per_mtok = {:?}\ncache_write_per_mtok = {:?}\n\
                 cache_write_1h_per_mtok = {:?}\n\n",
                e.input_per_mtok,
                e.cache_read_per_mtok,
                e.cache_write_per_mtok,
                e.cache_write_1h_per_mtok
            )
        };
        let tables: String = code
            .entries
            .iter()
            .map(|(id, e)| table(id, e.output_per_mtok))
            .collect();
        let pasted = format!("{}\n{tables}", Config::EXAMPLE_TOML);
        let (cfg, _) = Config::parse(&pasted).unwrap();
        assert_eq!(cfg.catalog.len(), code.entries.len());
        let (bare, _) = Config::parse(Config::EXAMPLE_TOML).unwrap();
        assert_eq!(
            acting(&cfg).unwrap(),
            acting(&bare).unwrap(),
            "the copies change nothing"
        );
        let note = round_trip(&pasted);
        assert_eq!(note, sparse_note(Config::EXAMPLE_TOML).unwrap());
        assert!(!note.contains("[catalog"), "{note}");

        // One price apart, that table stays, with that price alone.
        let id = "claude-sonnet-5-5";
        let out = code.get(id).unwrap().output_per_mtok;
        let apart = pasted.replacen(&table(id, out), &table(id, out + 1.0), 1);
        assert_ne!(apart, pasted);
        let cut: toml::Table = round_trip(&apart).parse().unwrap();
        let catalog = cut["catalog"].as_table().unwrap();
        assert_eq!(catalog.keys().collect::<Vec<_>>(), [id]);
        assert_eq!(
            catalog[id].as_table().unwrap().clone(),
            toml::Table::from_iter([(
                "output_per_mtok".to_string(),
                toml::Value::Float(out + 1.0)
            )])
        );
    }
}
