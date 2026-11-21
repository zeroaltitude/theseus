//! Numbered items (issue 44): "issue 33", "PR 12", "#33", "owner/repo#33",
//! as `item:` terms. They are kept apart from [`super::entities`], which
//! builds the entity field and reads a query: a bare number is no identity
//! by itself, so it never matches one node to another. The core asks for
//! them by `index.items`, scopes the unqualified ones by the place they were
//! said in, and touches them as it touches the other things.
//!
//! | Said | Term |
//! |---|---|
//! | `issue 33`, `Issue #33`, `issues 33` | `item:issue-33` |
//! | `PR 12`, `pr #12`, `pull request 12`, `pull 12` | `item:pr-12` |
//! | `#33` alone | `item:33` |
//! | `octo/lantern#33` | `item:octo/lantern#33` (qualified: it names its repository) |
//!
//! A `#` straight after a word (`C#3`), a URL's fragment, Discord's channel
//! mention (`<#123>`), and a number of more than [`DIGITS`] digits are no
//! item.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use regex::Regex;

/// The most digits an item's number has: more is an id, not an item.
pub const DIGITS: usize = 6;

static KIND_NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"(?i)\b(issues?|prs?|pull[ \t]+requests?|pulls?)[ \t]+#?(\d{{1,{DIGITS}}})\b"
    ))
    .unwrap()
});
/// `#N`, or `owner/repo#N`, after the start or a separator.
static HASH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"(?:^|[\s(\[,;:])((?:[A-Za-z0-9][A-Za-z0-9_.-]*/[A-Za-z0-9][A-Za-z0-9_.-]*)?)#(\d{{1,{DIGITS}}})\b"
    ))
    .unwrap()
});

/// The number as written, without leading zeros.
fn number(s: &str) -> String {
    s.parse::<u64>()
        .map_or_else(|_| s.to_string(), |n| n.to_string())
}

/// Every numbered item in `text`, as `item:` terms, sorted and deduplicated.
pub fn items(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut spans = Vec::new();
    for c in KIND_NUMBER.captures_iter(text) {
        let kind = if c[1].to_ascii_lowercase().starts_with("issue") {
            "issue"
        } else {
            "pr"
        };
        out.insert(format!("item:{kind}-{}", number(&c[2])));
        spans.push(c.get(0).unwrap().range());
    }
    for c in HASH.captures_iter(text) {
        let at = c.get(2).unwrap().start();
        if spans.iter().any(|s| s.contains(&at)) {
            continue;
        }
        let n = number(&c[2]);
        match c.get(1).map(|m| m.as_str()).filter(|r| !r.is_empty()) {
            Some(repo) => out.insert(format!("item:{}#{n}", repo.to_ascii_lowercase())),
            None => out.insert(format!("item:{n}")),
        };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn got(text: &str) -> Vec<String> {
        items(text).into_iter().collect()
    }

    #[test]
    fn an_item_is_named_with_its_kind_when_the_words_give_one() {
        assert_eq!(
            got("see Issue #33, then PR 12 and pull request 7"),
            ["item:issue-33", "item:pr-12", "item:pr-7"]
        );
        assert_eq!(got("issue 033"), ["item:issue-33"]);
    }

    #[test]
    fn a_bare_hash_is_an_unscoped_item_and_a_repository_qualifies_it() {
        assert_eq!(got("fixed in #33"), ["item:33"]);
        assert_eq!(got("(Octo/Lantern#33)"), ["item:octo/lantern#33"]);
    }

    #[test]
    fn what_only_looks_like_an_item_is_none() {
        for text in [
            "C#3 and F#4",
            "https://example.org/page#12",
            "<#123456> is a channel",
            "#1234567 is an id",
            "issue 1234567",
            "33 issues",
        ] {
            assert!(items(text).is_empty(), "{text:?}: {:?}", items(text));
        }
    }

    #[test]
    fn items_never_reach_the_entity_field() {
        let e = super::super::entities("issue 33 and #12 in octo/lantern#4");
        assert!(e.iter().all(|t| !t.starts_with("item:")), "{e:?}");
    }
}
