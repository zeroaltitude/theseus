//! How a reply is labelled where a surface shows who spoke (theseus-slvl.3):
//! "Theseus", with the model it ran on as metadata. The persona's own name
//! and emoji replace the literal when persona data joins (`persona_for`);
//! until then every surface shows the one name, through here.

/// Who a reply is from.
pub const REPLY_NAME: &str = "Theseus";

/// A reply's speaker with its model in parentheses, as `theseus history`
/// prints it: `Theseus (sonnet-5-5)`. A model's `claude-` prefix is the
/// vendor's, and a reply's line need not carry it. A reply that names no
/// model is `Theseus` alone.
pub fn reply_label(model: &str) -> String {
    let short = model.strip_prefix("claude-").unwrap_or(model);
    match short.is_empty() {
        true => REPLY_NAME.to_string(),
        false => format!("{REPLY_NAME} ({short})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reply_is_theseus_with_its_model_as_metadata() {
        assert_eq!(reply_label("claude-sonnet-5-5"), "Theseus (sonnet-5-5)");
        assert_eq!(reply_label("glm-5.3-flash"), "Theseus (glm-5.3-flash)");
        assert_eq!(reply_label(""), "Theseus");
    }
}
