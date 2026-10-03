//! The content words of a text, for the families' definitions only: a
//! `paraphrase` item's task and gold share none, and a `scale` item's
//! near-duplicates share some (`item.rs`). Lower case, less a stop list;
//! under both of two tokenizers (runs of `[a-z0-9]` that may hold `_ . - /`
//! inside, and a split on every non-alphanumeric), so a word either would
//! match counts; stemmed crudely; single characters dropped.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use regex::Regex;

/// The stop list.
const STOP: &str = "a an the is are was were be to of in on at for and or it its this that what which \
                    when how do does did i we you my our with from by as now not no so if then than \
                    there here into out up down just only give me us your";

fn stop() -> &'static BTreeSet<&'static str> {
    static S: OnceLock<BTreeSet<&'static str>> = OnceLock::new();
    S.get_or_init(|| STOP.split_whitespace().collect())
}

fn dotted_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[a-z0-9][a-z0-9_.\-/]*[a-z0-9]|[a-z0-9]").expect("a valid regex"))
}

/// A crude English stemmer: plurals, `-ed`, and `-ing`.
pub fn stem(w: &str) -> String {
    let n = w.len();
    let cut = |k: usize| w[..n - k].to_string();
    if n > 4 && w.ends_with("ies") {
        format!("{}y", cut(3))
    } else if n > 5 && w.ends_with("ing") {
        cut(3)
    } else if n > 4
        && ["ed", "ses", "xes", "zes", "ches", "shes"]
            .iter()
            .any(|e| w.ends_with(e))
    {
        cut(2)
    } else if n > 3 && w.ends_with('s') && !w.ends_with("ss") {
        cut(1)
    } else {
        w.to_string()
    }
}

/// A text's content words, stemmed, under both tokenizers, less single
/// characters.
pub fn content_stems(s: &str) -> BTreeSet<String> {
    let lower = s.to_lowercase();
    let stop = stop();
    let dotted = dotted_re().find_iter(&lower).map(|m| m.as_str());
    let split = lower.split(|c: char| !c.is_alphanumeric());
    dotted
        .chain(split)
        .filter(|w| w.chars().count() > 1 && !stop.contains(w))
        .map(stem)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stemmer_cuts_plurals_ed_and_ing() {
        assert_eq!(stem("jobs"), "job");
        assert_eq!(stem("batches"), "batch");
        assert_eq!(stem("handles"), "handle");
        assert_eq!(stem("pruned"), "prun");
        assert_eq!(stem("running"), "runn");
        assert_eq!(stem("replies"), "reply");
        assert_eq!(stem("class"), "class");
        assert_eq!(stem("bus"), "bus");
    }

    #[test]
    fn content_words_come_from_both_tokenizers_less_stops_and_single_characters() {
        let got: Vec<String> = content_stems("Copy ~/.theseus/store, not bindings.toml, a b.")
            .into_iter()
            .collect();
        assert_eq!(
            got,
            [
                "binding",
                "bindings.toml",
                "copy",
                "store",
                "theseu",
                "theseus/store",
                "toml"
            ]
        );
    }
}
