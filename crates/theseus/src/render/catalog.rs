//! `theseus catalog`'s lines on the config's `[catalog]` tables
//! (theseus-vwar): the catalog is the code's, and a config's table changes
//! some of its figures on purpose, adds a model, or copies the code's row.

use serde_json::Value;
use theseus_protocol::CatalogListResult;

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
