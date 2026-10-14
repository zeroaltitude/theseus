//! The output filter (theseus-pw1q.5): what the scorer gives back is checked
//! against every string of the set it scored, and refused whole when one
//! appears. It runs on the serialized output, so a new field cannot skip it.

use super::check::normalize;
use super::{Scores, SealedError};

/// A string shorter than this is a pattern like `7433`, which an output of
/// counts and hex ids could hold by chance; only a named canary is held
/// whatever its length.
const MIN_NEEDLE: usize = 8;

/// The strings a set's output must never hold.
pub struct Filter {
    needles: Vec<String>,
}

impl Filter {
    /// `strings`: each item's task and check patterns, held when long enough
    /// to be no chance match; `canaries`: the text an item names as one,
    /// held whatever its length.
    pub fn new(
        strings: impl IntoIterator<Item = String>,
        canaries: impl IntoIterator<Item = String>,
    ) -> Self {
        let mut needles: Vec<String> = strings
            .into_iter()
            .map(|s| normalize(&s))
            .filter(|s| s.chars().count() >= MIN_NEEDLE)
            .chain(
                canaries
                    .into_iter()
                    .map(|s| normalize(&s))
                    .filter(|s| !s.is_empty()),
            )
            .collect();
        needles.sort();
        needles.dedup();
        Self { needles }
    }

    /// Whether `text` holds none of the set's strings.
    pub fn allows(&self, text: &str) -> bool {
        let t = normalize(text);
        !self.needles.iter().any(|n| t.contains(n.as_str()))
    }

    /// The scorer's output, serialized, against the set.
    pub fn clean(&self, scores: &Scores) -> Result<(), SealedError> {
        let text = serde_json::to_string(scores).map_err(|_| SealedError::Withheld)?;
        if self.allows(&text) {
            Ok(())
        } else {
            Err(SealedError::Withheld)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_string_of_the_set_is_refused_wherever_it_sits_and_short_ones_are_not_held() {
        let f = Filter::new(
            ["The Zebra Protocol Seven".to_string(), "7433".to_string()],
            ["ab".to_string()],
        );
        assert!(!f.allows("x the  zebra protocol SEVEN y"));
        assert!(
            f.allows("7433 items, 3 failed"),
            "a short pattern is no needle"
        );
        assert!(f.allows("{\"set\":\"s\",\"items\":3}"));
        assert!(
            !f.allows("a canary: AB"),
            "a named canary is held at any length"
        );
    }
}
