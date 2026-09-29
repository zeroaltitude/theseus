//! Secret scrubbing (spec §3.9): tool output is scrubbed before it reaches the
//! model, the store, or a client. Every secret the vault has given is
//! replaced by `[redacted:<name>]` wherever it appears verbatim; well-known
//! token shapes are replaced by `[redacted:<shape>]`.
//!
//! The values are read from the secret board at each scrub, so a value is
//! known here the moment it resolves (theseus-qa0). A turn waits for the
//! board's first round to settle before any tool output passes through here.

use std::sync::Arc;

use crate::secrets::{SecretBoard, SecretState};

#[derive(Default)]
pub struct Scrubber {
    exact: Vec<(String, String)>,
    board: Option<Arc<SecretBoard>>,
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
    /// Scrub every value the board holds ready.
    pub fn from_board(board: Arc<SecretBoard>) -> Self {
        Self {
            exact: Vec::new(),
            board: Some(board),
        }
    }

    #[cfg(test)]
    pub fn with_values(values: Vec<(String, String)>) -> Self {
        Self {
            exact: values,
            board: None,
        }
    }

    pub fn scrub(&self, text: &str) -> (String, u32) {
        let states = self.board.as_ref().map(|b| b.states());
        let mut exact: Vec<(&str, &str)> = self
            .exact
            .iter()
            .map(|(v, n)| (v.as_str(), n.as_str()))
            .collect();
        for (name, s) in states.iter().flat_map(|m| m.iter()) {
            if let SecretState::Ready(v) = s {
                let v = v.expose().trim();
                if v.len() >= 8 {
                    exact.push((v, name));
                }
            }
        }
        // Longest first so a secret containing another is replaced whole.
        exact.sort_by_key(|e| std::cmp::Reverse(e.0.len()));
        let mut out = text.to_string();
        let mut n = 0u32;
        for (v, name) in exact {
            if out.contains(v) {
                n += out.matches(v).count() as u32;
                out = out.replace(v, &format!("[redacted:{name}]"));
            }
        }
        drop(states);
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

    /// A value is scrubbed from the moment the board has it.
    #[test]
    fn values_come_from_the_board_as_they_resolve() {
        use crate::secrets::Secret;
        let board = SecretBoard::new(["db".to_string()], std::time::Instant::now());
        let s = Scrubber::from_board(board.clone());
        assert_eq!(s.scrub("pw=hunter2hunter2").1, 0, "nothing resolved yet");
        board.publish(
            [("db".to_string(), Ok(Secret::new("hunter2hunter2".into())))].into(),
            "fake",
        );
        let (out, n) = s.scrub("pw=hunter2hunter2");
        assert_eq!((out.as_str(), n), ("pw=[redacted:db]", 1));
    }
}
