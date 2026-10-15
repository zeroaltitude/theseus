//! Components V2 (theseus-2j2ka): the builder and the validator for the
//! messages that carry layout, not a `content` string: the live task board and
//! the needs-you card. Pure: values in, values out, so every shape is tested
//! without Discord.
//!
//! A V2 message has the `IS_COMPONENTS_V2` flag, no `content`, and its
//! components alone: text displays, sections, separators, containers and
//! action rows. Discord counts every component, nested ones included, against
//! one limit of 40, and every text display's characters against one of 4,000
//! (the investigators' second pass, §4.3). `validate` holds a message to both,
//! to unique `custom_id`s, and to the shapes a request is refused for, so a
//! builder that goes wrong fails a test here and not a create in a DM.

use serde_json::{json, Value};

/// Discord's `IS_COMPONENTS_V2` message flag (1 << 15).
pub const IS_COMPONENTS_V2: u64 = 1 << 15;
/// Every component of a message, nested ones included.
pub const MAX_COMPONENTS: usize = 40;
/// Every text display's characters, summed over the message.
pub const MAX_TEXT: usize = 4000;
/// A `custom_id` is at most this many characters.
pub const ID_LIMIT: usize = 100;
/// A button's label.
const LABEL_LIMIT: usize = 80;
/// A select menu's options.
const OPTIONS_LIMIT: usize = 25;

const ACTION_ROW: u64 = 1;
const BUTTON: u64 = 2;
const STRING_SELECT: u64 = 3;
const SECTION: u64 = 9;
const TEXT_DISPLAY: u64 = 10;
const SEPARATOR: u64 = 14;
const CONTAINER: u64 = 17;

/// A button's look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    Primary,
    Secondary,
    Success,
    Danger,
}

impl Look {
    fn code(self) -> u64 {
        match self {
            Look::Primary => 1,
            Look::Secondary => 2,
            Look::Success => 3,
            Look::Danger => 4,
        }
    }
}

/// A markdown text display.
pub fn text(content: impl Into<String>) -> Value {
    json!({"type": TEXT_DISPLAY, "content": content.into()})
}

/// A line between two parts.
pub fn separator() -> Value {
    json!({"type": SEPARATOR, "divider": true, "spacing": 1})
}

/// A button that sends its `custom_id` back.
pub fn button(custom_id: impl Into<String>, label: &str, look: Look) -> Value {
    json!({"type": BUTTON, "style": look.code(), "label": clip(label, LABEL_LIMIT),
           "custom_id": custom_id.into()})
}

/// A button that opens a link (style 5): no `custom_id`, so a press is never
/// sent back.
pub fn link_button(url: &str, label: &str) -> Value {
    json!({"type": BUTTON, "style": 5, "label": clip(label, LABEL_LIMIT), "url": url})
}

/// A row of up to five buttons.
pub fn row(buttons: Vec<Value>) -> Value {
    json!({"type": ACTION_ROW, "components": buttons})
}

/// A row holding one select menu: `(label, value)` options, at most 25.
pub fn select(custom_id: &str, placeholder: &str, options: &[(String, String)]) -> Value {
    let options: Vec<Value> = options
        .iter()
        .take(OPTIONS_LIMIT)
        .map(|(l, v)| json!({"label": clip(l, LABEL_LIMIT), "value": clip(v, ID_LIMIT)}))
        .collect();
    row(vec![json!({
        "type": STRING_SELECT, "custom_id": custom_id,
        "placeholder": clip(placeholder, 100), "options": options,
        "min_values": 1, "max_values": 1,
    })])
}

/// Up to three text displays beside one accessory (a button).
pub fn section(texts: Vec<Value>, accessory: Value) -> Value {
    json!({"type": SECTION, "components": texts, "accessory": accessory})
}

/// A box around `children`, with a color bar down its side when `accent` is set.
pub fn container(accent: Option<u32>, children: Vec<Value>) -> Value {
    let mut c = json!({"type": CONTAINER, "components": children});
    if let Some(a) = accent {
        c["accent_color"] = json!(a);
    }
    c
}

fn clip(s: &str, max: usize) -> String {
    super::clip(s, max)
}

/// Every component in `components`, nested ones included: what Discord's
/// limit of 40 counts. A section's accessory counts too.
pub fn count(components: &[Value]) -> usize {
    components
        .iter()
        .map(|c| {
            1 + c["components"].as_array().map_or(0, |k| count(k))
                + usize::from(c.get("accessory").is_some_and(|a| a.is_object()))
        })
        .sum()
}

/// Every text display's characters.
pub fn text_len(components: &[Value]) -> usize {
    components
        .iter()
        .map(|c| {
            let own = if c["type"] == TEXT_DISPLAY {
                c["content"].as_str().map_or(0, |s| s.chars().count())
            } else {
                0
            };
            own + c["components"].as_array().map_or(0, |k| text_len(k))
        })
        .sum()
}

/// Every text display's content, in order: what a person reads.
#[cfg(test)]
pub fn texts(components: &[Value]) -> Vec<String> {
    let mut out = Vec::new();
    for c in components {
        if c["type"] == TEXT_DISPLAY {
            if let Some(s) = c["content"].as_str() {
                out.push(s.to_string());
            }
        }
        if let Some(k) = c["components"].as_array() {
            out.extend(texts(k));
        }
    }
    out
}

/// Every `custom_id` in `components`, in order, accessories and selects included.
pub fn custom_ids(components: &[Value]) -> Vec<String> {
    let mut out = Vec::new();
    for c in components {
        if let Some(id) = c["custom_id"].as_str() {
            out.push(id.to_string());
        }
        if let Some(a) = c.get("accessory") {
            out.extend(custom_ids(std::slice::from_ref(a)));
        }
        if let Some(k) = c["components"].as_array() {
            out.extend(custom_ids(k));
        }
    }
    out
}

/// Whether `components` fit a message and are shaped as Discord takes them.
/// The first reason they do not, in words.
pub fn validate(components: &[Value]) -> Result<(), String> {
    let n = count(components);
    if n > MAX_COMPONENTS {
        return Err(format!("{n} components; at most {MAX_COMPONENTS}"));
    }
    let t = text_len(components);
    if t > MAX_TEXT {
        return Err(format!("{t} characters of text; at most {MAX_TEXT}"));
    }
    let ids = custom_ids(components);
    let mut seen = std::collections::HashSet::new();
    for id in &ids {
        if id.is_empty() || id.chars().count() > ID_LIMIT {
            return Err(format!(
                "custom_id {id:?} is empty or over {ID_LIMIT} characters"
            ));
        }
        if !seen.insert(id) {
            return Err(format!("custom_id {id:?} is used twice"));
        }
    }
    shapes(components, true)
}

/// The shape rules: what may hold what.
fn shapes(components: &[Value], top: bool) -> Result<(), String> {
    for c in components {
        let kind = c["type"].as_u64().unwrap_or(0);
        let kids = c["components"].as_array().map_or(0, Vec::len);
        match kind {
            ACTION_ROW => {
                let buttons = c["components"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|k| k["type"] == BUTTON)
                    .count();
                let selects = kids - buttons;
                if kids == 0 || buttons > 5 || selects > 1 || (selects == 1 && buttons > 0) {
                    return Err(format!(
                        "an action row holds {buttons} buttons and {selects} selects"
                    ));
                }
            }
            SECTION => {
                if !(1..=3).contains(&kids) || !c["accessory"].is_object() {
                    return Err(format!(
                        "a section holds {kids} texts and needs an accessory"
                    ));
                }
                if c["components"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|k| k["type"] != TEXT_DISPLAY)
                {
                    return Err("a section holds text displays only".into());
                }
            }
            CONTAINER => {
                if !top {
                    return Err("a container is not nested".into());
                }
            }
            TEXT_DISPLAY => {
                if c["content"].as_str().is_none_or(|s| s.trim().is_empty()) {
                    return Err("a text display is empty".into());
                }
            }
            BUTTON => {
                let link = c["style"] == 5;
                let has_id = c["custom_id"].is_string();
                let has_url = c["url"].is_string();
                if link == has_id || link != has_url {
                    return Err("a button has a custom_id, or a url when it is a link".into());
                }
            }
            SEPARATOR | STRING_SELECT => {}
            other => {
                return Err(format!(
                    "component type {other} is not one this binding builds"
                ))
            }
        }
        if let Some(k) = c["components"].as_array() {
            shapes(k, false)?;
        }
    }
    Ok(())
}

/// The request body for a V2 message: the flag, no content, the components.
/// `silent` sets `SUPPRESS_NOTIFICATIONS` too.
pub fn body(components: &[Value], silent: bool) -> Value {
    let mut flags = IS_COMPONENTS_V2;
    if silent {
        flags |= crate::policy::SUPPRESS_NOTIFICATIONS;
    }
    json!({"flags": flags, "components": components})
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn card() -> Vec<Value> {
        vec![container(
            Some(0xE5484D),
            vec![
                text("**Needs you**"),
                separator(),
                section(
                    vec![text("a question")],
                    button("x:1", "Answer", Look::Primary),
                ),
                row(vec![
                    button("a:1", "Approve", Look::Success),
                    button("a:2", "Decline", Look::Danger),
                ]),
            ],
        )]
    }

    #[test]
    fn a_container_counts_every_component_inside_it_the_accessory_too() {
        // container 1 + text 1 + separator 1 + section 1 (+ its text 1 + its
        // accessory 1) + row 1 + two buttons 2.
        assert_eq!(count(&card()), 9);
        assert_eq!(
            text_len(&card()),
            "**Needs you**".chars().count() + "a question".chars().count()
        );
        assert_eq!(validate(&card()), Ok(()));
    }

    #[test]
    fn the_limits_are_forty_components_and_four_thousand_characters() {
        let many: Vec<Value> = (0..41).map(|i| text(format!("line {i}"))).collect();
        assert!(validate(&many).unwrap_err().contains("41 components"));
        assert_eq!(validate(&many[..40]), Ok(()));
        let long = vec![text("x".repeat(MAX_TEXT + 1))];
        assert!(validate(&long).unwrap_err().contains("4001 characters"));
        assert_eq!(validate(&[text("x".repeat(MAX_TEXT))]), Ok(()));
    }

    #[test]
    fn a_repeated_custom_id_is_refused_wherever_it_hides() {
        let twice = vec![
            section(vec![text("a")], button("same", "One", Look::Primary)),
            row(vec![button("same", "Two", Look::Primary)]),
        ];
        assert!(validate(&twice).unwrap_err().contains("used twice"));
    }

    #[test]
    fn the_shapes_discord_refuses_are_refused_here() {
        let six: Vec<Value> = (0..6)
            .map(|i| button(format!("b{i}"), "b", Look::Secondary))
            .collect();
        assert!(validate(&[row(six)]).is_err());
        assert!(validate(&[row(vec![])]).is_err());
        assert!(validate(&[section(vec![], button("b", "b", Look::Primary))]).is_err());
        assert!(validate(&[container(None, vec![container(None, vec![text("x")])])]).is_err());
        assert!(validate(&[text("  ")]).is_err());
        assert!(validate(&[row(vec![json!({"type": 2, "style": 1, "label": "x"})])]).is_err());
    }

    #[test]
    fn the_body_has_the_flag_no_content_and_the_silent_bit_only_when_asked() {
        let b = body(&card(), false);
        assert_eq!(b["flags"], IS_COMPONENTS_V2);
        assert!(b.get("content").is_none());
        let s = body(&card(), true);
        assert_eq!(s["flags"], IS_COMPONENTS_V2 | (1 << 12));
    }

    proptest! {
        /// Whatever it is given, the builders' output counts the same way the
        /// validator does, and a message under both limits validates.
        #[test]
        fn a_message_under_the_limits_validates(n in 0usize..30, len in 1usize..100) {
            let parts: Vec<Value> = (0..n).map(|i| text(format!("{i}{}", "x".repeat(len)))).collect();
            prop_assert_eq!(count(&parts), n);
            prop_assert_eq!(validate(&parts).is_ok(), text_len(&parts) <= MAX_TEXT);
        }
    }
}
