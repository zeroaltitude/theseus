//! "Are these the same person" by name alone, with no judgment (theseus-jrffh).
//! A person's normalized key is the name lowercased with its spaces, hyphens,
//! underscores and dots taken out, so `Bram Teller`, `bram-teller` and
//! `bramteller` are one key. Two people whose keys are equal are one
//! person (`Core::people_same_merge`). A bare first name is never a key of a
//! full name: `bram` is not `bramteller`.

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

    /// The four Brams are one key, and every other row is a different one.
    #[test]
    fn the_four_brams_key_together_and_others_do_not() {
        for n in [
            "bramteller",
            "bram-teller",
            "bram teller",
            "Bram Teller",
            "bram_teller",
            "Bram.Teller",
            "  @Bram  Teller ",
        ] {
            assert_eq!(name_key(n), "bramteller", "{n:?}");
        }
        let apart = [
            ("bram", "bram teller"),
            ("Juno", "Juno Pike"),
            ("bram teller", "bram tellers"),
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
