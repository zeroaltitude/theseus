//! `theseus catalog`'s lines on the config's `[catalog]` tables
//! (theseus-vwar): the catalog is the code's, and a config's table changes
//! some of its figures on purpose, adds a model, or copies the code's row.

use serde_json::Value;
use theseus_protocol::{CatalogListResult, Usage};

use super::{fmt_price, fmt_tokens};

/// `theseus catalog`'s table: a header and a row per model, with the
/// five prices (theseus-4v1z): input, output, cache read, and the cache's
/// write at its 5-minute and its 1-hour lifetime.
pub fn catalog_table_lines(l: &CatalogListResult) -> Vec<String> {
    let mut out = vec![format!(
        "{:<24} {:<10} {:>9} {:>8} {:>7} {:>7} {:>7} {:>7} {:>7}  {:<9} profiles",
        "model",
        "provider",
        "window",
        "max out",
        "$in",
        "$out",
        "$c.rd",
        "$c.wr",
        "$c.1h",
        "thinking"
    )];
    for m in &l.models {
        let e = &m.entry;
        let num = |k: &str| e.get(k).and_then(Value::as_f64).unwrap_or(0.0);
        out.push(format!(
            "{:<24} {:<10} {:>9} {:>8} {:>7} {:>7} {:>7} {:>7} {:>7}  {:<9} {}",
            m.model,
            e.get("provider").and_then(Value::as_str).unwrap_or("?"),
            fmt_tokens(num("context_window") as u64),
            fmt_tokens(num("max_output_tokens") as u64),
            fmt_price(num("input_per_mtok")),
            fmt_price(num("output_per_mtok")),
            fmt_price(num("cache_read_per_mtok")),
            fmt_price(num("cache_write_per_mtok")),
            fmt_price(num("cache_write_1h_per_mtok")),
            e.get("thinking").and_then(Value::as_str).unwrap_or("?"),
            m.profiles.join(",")
        ));
    }
    out
}

/// Health's cache writes, with the 1-hour ones within them when there are
/// any, as plain numbers like the tokens line's others: `1200 (1h 300)`.
pub fn cache_write_words(u: &Usage) -> String {
    match u.cache_creation_1h_input_tokens {
        0 => u.cache_creation_input_tokens.to_string(),
        h => format!("{} (1h {h})", u.cache_creation_input_tokens),
    }
}

/// One line per model the config has a `[catalog]` table for: each field it
/// changes, with the code's value beside it; or that it adds a model the
/// code lacks; or that it copies the code's figures and changes nothing.
pub fn catalog_config_lines(l: &CatalogListResult) -> Vec<String> {
    l.models
        .iter()
        .filter_map(|m| {
            let config = m.config.as_ref()?.as_object()?;
            let Some(code) = m.code.as_ref().and_then(Value::as_object) else {
                return Some(format!(
                    "{}: the config's [catalog] table adds it; the code has no such model",
                    m.model
                ));
            };
            // Its source alone changes nothing: it says only where the
            // figures came from.
            let changes: Vec<String> = config
                .iter()
                .filter(|(k, v)| k.as_str() != "source" && code.get(k.as_str()) != Some(v))
                .map(|(k, v)| match code.get(k.as_str()) {
                    Some(was) => format!("{k} {v} (the code's {was})"),
                    None => format!("{k} {v}"),
                })
                .collect();
            Some(match changes.is_empty() {
                true => format!(
                    "{}: the config's [catalog] table copies the code's figures and changes \
                     nothing, but would hold them past the code's next fix: leave it out",
                    m.model
                ),
                false => format!(
                    "{}: the config's [catalog] table sets {}",
                    m.model,
                    changes.join(", ")
                ),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use theseus_protocol::CatalogModel;

    fn model(name: &str, entry: Value) -> CatalogModel {
        serde_json::from_value(json!({"model": name, "entry": entry, "profiles": ["main"]}))
            .unwrap()
    }

    #[test]
    fn the_table_has_a_column_for_the_one_hour_write_price() {
        let l = CatalogListResult {
            version: "2026-01-01".into(),
            models: vec![model(
                "orbit-5",
                json!({"provider": "orbit", "context_window": 1000000, "max_output_tokens": 64000,
                    "input_per_mtok": 3.0, "output_per_mtok": 15.0, "cache_read_per_mtok": 0.3,
                    "cache_write_per_mtok": 3.75, "cache_write_1h_per_mtok": 6.0,
                    "thinking": "adaptive"}),
            )],
        };
        let t = catalog_table_lines(&l);
        assert!(t[0].contains("$c.wr   $c.1h  thinking"), "{}", t[0]);
        assert_eq!(
            t[1],
            "orbit-5                  orbit             1M      64K       3      15     0.3    \
             3.75       6  adaptive  main"
        );
    }

    #[test]
    fn the_tokens_line_shows_one_hour_writes_within_the_cache_writes() {
        let mut u = Usage {
            cache_creation_input_tokens: 1200,
            ..Usage::default()
        };
        assert_eq!(cache_write_words(&u), "1200");
        u.cache_creation_1h_input_tokens = 300;
        assert_eq!(cache_write_words(&u), "1200 (1h 300)");
    }
}
