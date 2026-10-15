//! A promise to follow up, found in a reply (theseus-slvl.9): "I'll check
//! back after CI". The matcher is deterministic and small, one table, so a
//! reply's promise is counted the same way every week: no model reads it.
//!
//! A reply is read with its fenced code and quoted lines left out, in lower
//! case with its apostrophes straight. Only phrases that look ahead are in
//! the table; "I checked" and "after CI failed" are not promises.

/// The phrases that promise a follow-up, in lower case. A reply holding one
/// of them says it will come back; the turn that said it should have set a
/// wake (or started a task) for it.
pub const PHRASES: &[&str] = &[
    "i'll check",
    "i will check",
    "i'll let you know",
    "i will let you know",
    "i'll follow up",
    "i will follow up",
    "i'll circle back",
    "i'll report back",
    "i'll get back to you",
    "i'll come back to",
    "i'll ping you",
    "i'll keep an eye",
    "i'll monitor",
    "i'll watch for",
    "i'll look again",
    "i'll revisit",
    "i'll update you",
    "i'll tell you when",
    "i'll tell you once",
    "i'll say when",
    "after ci passes",
    "after ci finishes",
    "after ci completes",
    "once ci passes",
    "once ci finishes",
    "once the build finishes",
    "once it finishes, i'll",
    "when it finishes, i'll",
    "i'll be back",
    "i'll check back",
];

/// The text as the matcher reads it: fenced blocks and quoted lines gone,
/// lower case, apostrophes straight, runs of space one.
fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut fenced = false;
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced || t.starts_with('>') {
            continue;
        }
        out.push_str(line);
        out.push(' ');
    }
    out.to_lowercase()
        .replace(['\u{2019}', '\u{2018}'], "'")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The first phrase of the table that `text` promises with, if any.
pub fn promised(text: &str) -> Option<&'static str> {
    let t = plain(text);
    PHRASES.iter().copied().find(|p| t.contains(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reply_that_looks_ahead_is_a_promise() {
        for (text, phrase) in [
            ("Pushed. I'll check back after CI.", "i'll check"),
            (
                "Done. I\u{2019}ll let you know when it lands.",
                "i'll let you know",
            ),
            (
                "Merged it.\n\nI will follow up tomorrow morning.",
                "i will follow up",
            ),
            (
                "Tests are running; after CI passes the release goes out.",
                "after ci passes",
            ),
            ("Ok. i'll   circle\nback on Friday", "i'll circle back"),
        ] {
            assert_eq!(promised(text), Some(phrase), "{text}");
        }
    }

    #[test]
    fn a_reply_that_does_not_look_ahead_is_not() {
        for text in [
            "I checked the build and it passed.",
            "It failed after CI ran twice.",
            "You'll want to check the logs.",
            "```\necho \"I'll check\"\n```\nThat is the script.",
            "> I'll let you know\nThat is what the old message said.",
            "",
        ] {
            assert_eq!(promised(text), None, "{text:?}");
        }
    }

    #[test]
    fn every_phrase_is_lower_case_and_straight() {
        for p in PHRASES {
            assert_eq!(*p, p.to_lowercase(), "{p}");
            assert!(!p.contains('\u{2019}'), "{p}");
            assert!(promised(p).is_some(), "{p} finds itself");
        }
    }
}
