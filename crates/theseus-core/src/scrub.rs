//! Secret scrubbing (spec §3.9): tool output is scrubbed before it reaches the
//! model, the store, or a client. Every secret resolved at startup is replaced
//! by `[redacted:<name>]` wherever it appears verbatim; well-known token
//! shapes are replaced by `[redacted:<shape>]`.

use crate::secrets::Secrets;

#[derive(Default)]
pub struct Scrubber {
    exact: Vec<(String, String)>,
}

/// Token shapes worth catching even when the value was never resolved here.
const SHAPES: &[(&str, &str)] = &[
    ("sk-ant-", "anthropic_key"),
    ("ghp_", "github_token"),
    ("github_pat_", "github_token"),
    ("gho_", "github_token"),
    ("ops_", "op_service_account"),
    ("xoxb-", "slack_token"),
    ("xoxp-", "slack_token"),
];

impl Scrubber {
    pub fn from_secrets(s: &Secrets) -> Self {
        let mut exact = Vec::new();
        for name in s.names() {
            if let Some(v) = s.get(&name) {
                let v = v.expose().trim().to_string();
                if v.len() >= 8 {
                    exact.push((v, name.clone()));
                }
            }
        }
        // Longest first so a secret containing another is replaced whole.
        exact.sort_by_key(|e| std::cmp::Reverse(e.0.len()));
        Self { exact }
    }

    pub fn with_values(values: Vec<(String, String)>) -> Self {
        Self { exact: values }
    }

    pub fn scrub(&self, text: &str) -> (String, u32) {
        let mut out = text.to_string();
        let mut n = 0u32;
        for (v, name) in &self.exact {
            if out.contains(v.as_str()) {
                n += out.matches(v.as_str()).count() as u32;
                out = out.replace(v.as_str(), &format!("[redacted:{name}]"));
            }
        }
        for (prefix, shape) in SHAPES {
            while let Some(i) = out.find(prefix) {
                let end = out[i..]
                    .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
                    .map(|e| i + e)
                    .unwrap_or(out.len());
                if end - i < prefix.len() + 8 {
                    break; // too short to be a token; stop scanning this shape
                }
                out.replace_range(i..end, &format!("[redacted:{shape}]"));
                n += 1;
            }
        }
        (out, n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_values_and_token_shapes_are_redacted() {
        let s = Scrubber::with_values(vec![("hunter2hunter2".into(), "db_password".into())]);
        let (out, n) = s.scrub("pw=hunter2hunter2 key=sk-ant-api03-abcdefghijklmnop end ghp_short");
        assert_eq!(n, 2, "{out}");
        assert!(out.contains("[redacted:db_password]"));
        assert!(out.contains("[redacted:anthropic_key] end"));
        assert!(out.contains("ghp_short"), "too short to be a token");
    }
}
