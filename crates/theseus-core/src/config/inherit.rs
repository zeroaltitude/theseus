//! One rule for a profile's settings: a field a profile leaves unset takes `[model]`'s (theseus-rwpbg.7).
//! `[model]` is the defaults every profile inherits, plus `live` and the implicit `default` profile.

use std::collections::BTreeMap;

use super::ProfileConfig;

/// The fields `[model]` and a profile share, bar `model`, which a profile always names.
const SHARED: [&str; 10] = [
    "provider",
    "max_output_tokens",
    "system",
    "effort",
    "thinking_display",
    "max_loops",
    "max_loops_mode",
    "refusal_fallbacks",
    "cache_ttl",
    "keep_warm_hours",
];

/// The profiles of the document `doc`, each with the `[model]` values it leaves unset written in.
pub(super) fn profiles(
    doc: &toml::Table,
) -> Result<BTreeMap<String, ProfileConfig>, toml::de::Error> {
    let model = doc.get("model").and_then(toml::Value::as_table);
    let mut profiles = doc
        .get("profiles")
        .cloned()
        .unwrap_or_else(|| toml::Value::Table(Default::default()));
    for p in profiles
        .as_table_mut()
        .into_iter()
        .flat_map(|t| t.iter_mut().map(|(_, v)| v))
    {
        let (Some(p), Some(model)) = (p.as_table_mut(), model) else {
            continue;
        };
        for key in SHARED {
            if let (false, Some(v)) = (p.contains_key(key), model.get(key)) {
                p.insert(key.into(), v.clone());
            }
        }
    }
    profiles.try_into()
}
