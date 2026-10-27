//! "Are these the same person" by name alone, with no judgment (theseus-jrffh).
//! A person's normalized key is the name lowercased with its spaces, hyphens,
//! underscores and dots taken out, so `Aaron Solomon`, `aaron-solomon` and
//! `aaronsolomon` are one key. Two people whose keys are equal are one
//! person (`Core::people_same_merge`). A bare first name is never a key of a
//! full name: `aaron` is not `aaronsolomon`.

/// The separators a key leaves out.
const SEPARATORS: [char; 4] = [' ', '-', '_', '.'];

/// A person's normalized key: lowercase, no whitespace, hyphen, underscore
/// or dot, and a leading `@` of a handle-like name dropped.
pub fn name_key(name: &str) -> String {
    name.trim()
        .trim_start_matches('@')
        .chars()
        .filter(|c| !c.is_whitespace() && !SEPARATORS.contains(c))
        .flat_map(char::to_lowercase)
        .collect()
}

/// An unordered pair of person ids as one string: the smaller first, so
/// `(a, b)` and `(b, a)` are the same pair.
pub fn pair_key(a: &str, b: &str) -> String {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    format!("{lo}|{hi}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The owner's four Aarons are one key, and every other row is a
    /// different one.
    #[test]
    fn the_four_aarons_key_together_and_others_do_not() {
        for n in [
            "aaronsolomon",
            "aaron-solomon",
            "aaron solomon",
            "Aaron Solomon",
            "aaron_solomon",
            "Aaron.Solomon",
            "  @Aaron  Solomon ",
        ] {
            assert_eq!(name_key(n), "aaronsolomon", "{n:?}");
        }
        let apart = [
            ("aaron", "aaron solomon"),
            ("Peyton", "Peyton Greenside"),
            ("aaron solomon", "aaron solomons"),
            ("Ann Marie", "Anna Rie"),
        ];
        for (a, b) in apart {
            assert_ne!(name_key(a), name_key(b), "{a:?} / {b:?}");
        }
        assert_eq!(name_key(" - . "), "");
    }

    #[test]
    fn a_pair_has_no_order() {
        assert_eq!(
            pair_key("person:b", "person:a"),
            pair_key("person:a", "person:b")
        );
        assert_eq!(pair_key("person:a", "person:b"), "person:a|person:b");
    }
}
