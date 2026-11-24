//! Rendering copies only: mask complete display strings before any clipping.
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use theseus_core::exits::Exits;

/// Arbitrary tool input, copied only to describe a call, never to execute it.
pub(crate) fn strings(exits: &Exits, value: &mut Value) {
    match value {
        Value::String(text) => *text = exits.text(text).0.into_owned(),
        Value::Array(items) => items.iter_mut().for_each(|v| strings(exits, v)),
        Value::Object(items) => items.values_mut().for_each(|v| strings(exits, v)),
        _ => {}
    }
}

/// Text-bearing protocol and Discord fields. Control identities, option values,
/// nonces and authentication tokens are not display text and stay unchanged.
pub(crate) fn json(exits: &Exits, value: &mut Value) {
    match value {
        Value::Array(items) => items.iter_mut().for_each(|v| json(exits, v)),
        Value::Object(items) => {
            for (key, v) in items {
                if matches!(
                    key.as_str(),
                    "header"
                        | "excerpt"
                        | "content"
                        | "text"
                        | "title"
                        | "label"
                        | "description"
                        | "placeholder"
                        | "name"
                        | "summary"
                        | "prompt"
                        | "detail"
                        | "said"
                        | "current"
                        | "last_line"
                        | "why"
                        | "note"
                        | "from"
                        | "source"
                        | "reason"
                        | "error"
                        | "words"
                        | "input"
                        | "choices"
                        | "plain"
                        | "preview"
                ) {
                    strings(exits, v);
                } else {
                    json(exits, v);
                }
            }
        }
        _ => {}
    }
}

/// Never fall back to the raw value if a display copy cannot be decoded.
pub(crate) fn copy<T: Serialize + DeserializeOwned>(exits: &Exits, value: &T) -> Option<T> {
    let mut v = serde_json::to_value(value).ok()?;
    json(exits, &mut v);
    serde_json::from_value(v).ok()
}

pub(crate) fn face(exits: &Exits, value: &super::sched::Face) -> super::sched::Face {
    let mut f = value.clone();
    f.title = exits
        .text(&f.title)
        .0
        .lines()
        .find(|s| !s.trim().is_empty())
        .unwrap_or("")
        .trim()
        .to_string();
    f.from = f.from.map(|s| exits.text(&s).0.into_owned());
    f.every = f.every.map(|s| exits.text(&s).0.into_owned());
    if let super::sched::Life::Cancelled(_, by) = &mut f.life {
        *by = by.as_ref().map(|s| exits.text(s).0.into_owned());
    }
    f.routine = f
        .routine
        .and_then(|(r, counts)| copy(exits, &r).map(|r| (r, counts)));
    f
}

pub(crate) fn future(
    exits: &Exits,
    mut p: super::horizon::FutureParts,
) -> super::horizon::FutureParts {
    p.coming = copy(exits, &p.coming).unwrap_or_default();
    p.routines = copy(exits, &p.routines).unwrap_or_default();
    p.projects = copy(exits, &p.projects).unwrap_or_default();
    for name in p.names.values_mut() {
        *name = exits.text(name).0.into_owned();
    }
    p
}

/// A display request must be masked before its tool summary or preview is cut.
pub(crate) fn request(exits: &Exits, req: &mut theseus_protocol::ConfirmRequest) {
    strings(exits, &mut req.input);
    req.reason = exits.text(&req.reason).0.into_owned();
    req.resource = req.resource.as_ref().map(|s| exits.text(s).0.into_owned());
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn display_text_is_masked_but_control_identities_are_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let core = crate::runtime::tests::core_with(
            dir.path(),
            theseus_core::secrets::SecretBoard::empty(),
            |_| {},
        );
        let value = "vault-control-4c8e1f7a92b3";
        core.secrets.hold(
            "control_token",
            theseus_core::secrets::Secret::new(value.into()),
        );
        let mut data = serde_json::json!({"content": value, "nonce": value,
            "components": [{"custom_id": value, "label": value, "options": [{"label": value, "value": value}]}]});
        json(&core.exits, &mut data);
        assert_eq!(data["content"], "[redacted:control_token]");
        assert_eq!(data["components"][0]["label"], "[redacted:control_token]");
        assert_eq!(
            data["components"][0]["options"][0]["label"],
            "[redacted:control_token]"
        );
        assert_eq!(data["nonce"], value);
        assert_eq!(data["components"][0]["custom_id"], value);
        assert_eq!(data["components"][0]["options"][0]["value"], value);
    }
}

#[cfg(test)]
mod exit_tests_r4 {
    const VALUE: &str = "vault-token-4c8e1f7a92b3";
    #[tokio::test]
    async fn citation_copy_masks_header_before_footnote_cut() {
        let dir = tempfile::tempdir().unwrap();
        let core = crate::runtime::tests::core_with(
            dir.path(),
            theseus_core::secrets::SecretBoard::empty(),
            |_| {},
        );
        core.secrets.hold(
            "display_token",
            theseus_core::secrets::Secret::new(VALUE.into()),
        );
        let cite = theseus_protocol::cite::Citation {
            header: format!("{}{}", "x".repeat(195), VALUE),
            ..Default::default()
        };
        let display = super::copy(&core.exits, &cite).unwrap();
        let text = crate::render::cite::footnotes(&[&display]).unwrap();
        assert!(!text.contains("vau"), "{text}");
        assert!(text.contains("[red"), "{text}");
    }
}
