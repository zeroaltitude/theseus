//! A message that names a thread by its title (theseus-4qdh8).
//!
//! The pre-router scores a thread from what a message names of its working
//! set, and a title is no part of that: "for the word census of the sample
//! docs and for the harbour log notes" named two threads and scored both 0.0,
//! so nothing contended and the which-thread question was never asked. A
//! thread is named by its title when every content word of the title is in
//! the message. Pure: no store, no clock.

/// Words that carry no title: they join or point, and never name.
const STOP: &[&str] = &[
    "a", "an", "and", "at", "by", "for", "from", "in", "of", "on", "or", "the", "to", "with",
];

/// What a message that names a title adds to the thread's score: above the
/// bar to contend (`thread_route::CONTEND`, 0.25), so two titles named make
/// two contenders and an ask, and below the bar to route on its own
/// (`CONFIRM`, 0.60), since a title is a topic and not an address.
pub const TITLE: f64 = 0.35;

/// A word folded for comparing: lower case, a plural's `s` off.
fn fold(w: &str) -> String {
    let w = w.to_lowercase();
    match w.len() > 3 && w.ends_with('s') && !w.ends_with("ss") {
        true => w[..w.len() - 1].to_string(),
        false => w,
    }
}

fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty() && !STOP.contains(&w.to_lowercase().as_str()))
        .map(fold)
        .collect()
}

/// [`TITLE`] when `text` holds every content word of `title`, and more
/// than one of them: a lone word is a topic many threads share, never a name.
pub fn title_hit(text: &str, title: &str) -> Option<f64> {
    let mut title = words(title);
    // Order is no matter here: the words are a set.
    title.sort();
    title.dedup();
    let said = words(text);
    (title.len() > 1 && title.iter().all(|w| said.contains(w))).then_some(TITLE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_title_named_as_a_phrase_or_by_its_words_is_a_hit() {
        let t = "Harbour log notes";
        assert_eq!(title_hit("add it to the harbour log notes", t), Some(TITLE));
        let t = "Docs word census";
        let said = "the word census of the sample docs";
        assert_eq!(title_hit(said, t), Some(TITLE));
    }

    #[test]
    fn a_titles_repeated_word_counts_once() {
        // One word said twice is still one word, never a name.
        assert_eq!(title_hit("log log log", "Log log"), None);
        // A word that comes back after another counts once, and the title still hits.
        assert_eq!(title_hit("the log notes", "Log notes log"), Some(TITLE));
    }

    #[test]
    fn plurals_case_and_hyphens_fold() {
        let t = "Docs word census";
        assert!(title_hit("DOC word-census, please", t).is_some());
        assert!(title_hit("the harbour-log note", "Harbour log notes").is_some());
    }

    #[test]
    fn a_lone_common_word_is_no_hit() {
        let t = "Docs word census";
        assert_eq!(title_hit("a census of the disk", t), None);
        assert_eq!(title_hit("count the words in the docs", t), None);
        assert_eq!(title_hit("census", "Census"), None);
        assert_eq!(title_hit("the", "The of"), None);
    }
}
