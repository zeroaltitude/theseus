//! The operator's sensitive names (`[privacy] sensitive_names`), and the one
//! way to find them in a text (theseus-oejh3). Route.v6's `data` question, the
//! start's check of the packs' text and the floor that keeps a hit off models
//! of restricted origin all ask this matcher, so a name means one thing.
//!
//! A word is a maximal run of Unicode alphanumerics, compared lower-cased. A
//! name matches the same words in a row in the text, whatever separators lie
//! between them: `Big Co` matches "big,\nco", `Acme` matches "Acme's" and
//! "acme-internal" and not "acmeology". A name with no alphanumerics matches
//! nothing.

/// A text's words, lower-cased.
fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.chars().flat_map(char::to_lowercase).collect())
        .collect()
}

/// The names' words, made once for a caller that checks many texts.
#[derive(Debug, Clone, Default)]
pub struct Names {
    /// Each configured name and its words (a name with none is left out).
    names: Vec<(String, Vec<String>)>,
}

impl Names {
    pub fn new(names: &[String]) -> Self {
        let names = names
            .iter()
            .map(|n| (n.clone(), words(n)))
            .filter(|(_, w)| !w.is_empty())
            .collect();
        Self { names }
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// The configured names that occur in `text`, in the list's order.
    pub fn hits(&self, text: &str) -> Vec<String> {
        if self.names.is_empty() {
            return Vec::new();
        }
        let text = words(text);
        self.names
            .iter()
            .filter(|(_, w)| text.windows(w.len()).any(|t| t == w.as_slice()))
            .map(|(n, _)| n.clone())
            .collect()
    }
}

/// The configured names that occur in `text`.
pub fn hits(text: &str, names: &[String]) -> Vec<String> {
    Names::new(names).hits(text)
}

/// The embedded packs whose text names a configured name, with how many
/// names hit: the start's check (theseus-oejh3). Never the names themselves.
pub fn packs_naming(names: &Names) -> Vec<(&'static str, usize)> {
    crate::pack::EMBEDDED
        .iter()
        .map(|(pack, text)| (*pack, names.hits(text).len()))
        .filter(|(_, n)| *n > 0)
        .collect()
}

/// SHA-256 of four old organisation names, lower-cased. The repository is
/// public, so they are held only as digests (theseus-a9zg9, theseus-cuv2o).
/// Other crates' tests read it through the `fake` feature.
#[cfg(any(test, feature = "fake"))]
pub const OLD: [&str; 4] = [
    "51571bf8b06e07629bde6177d7d08ebd8393cf247593f412f9200850afbe9456",
    "7380ccbf122a29113b7c0563493540d14337ed6d9d9c69048b49e619896a60a0",
    "3a60e130bb5cfaae5fa51696b3310767b62e32543a3bb00aa488c34a56ab3a09",
    "dd9f9bdb898bd41a9a3aca4626ff6503e14fee152fd7d08de9734b302287154f",
];

/// Whether `text` has a word (or a hyphen part of one) whose digest is in
/// `OLD`. `clean` remembers the parts already digested, so a scan over many
/// files hashes each distinct part once.
#[cfg(test)]
pub(crate) fn names_an_old_name(text: &str, clean: &mut std::collections::HashSet<String>) -> bool {
    use sha2::{Digest, Sha256};
    text.split(|c: char| !(c.is_alphanumeric() || c == '-'))
        .any(|word| {
            std::iter::once(word).chain(word.split('-')).any(|part| {
                if part.is_empty() || clean.contains(part) {
                    return false;
                }
                let digest = format!("{:x}", Sha256::digest(part.to_lowercase()));
                let old = OLD.contains(&digest.as_str());
                if !old {
                    clean.insert(part.to_string());
                }
                old
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(text: &str, names: &[&str]) -> Vec<String> {
        let names: Vec<String> = names.iter().map(|n| (*n).to_string()).collect();
        hits(text, &names)
    }

    /// The owner's list, as the template has it.
    const OWNER: [&str; 4] = [
        "examplecorp",
        "examplecorpinc",
        "example-repo",
        "example-utils",
    ];

    #[test]
    fn sensitive_name_matching() {
        let acme = ["Acme"];
        let table: &[(&str, &[&str], &[&str])] = &[
            ("ACME plans", &acme, &["Acme"]),
            ("acme plans", &acme, &["Acme"]),
            ("Acme's plan", &acme, &["Acme"]),
            ("see acme-internal", &acme, &["Acme"]),
            ("see acme_internal", &acme, &["Acme"]),
            ("acmeology", &acme, &[]),
            ("an acme", &acme, &["Acme"]),
            ("Big\nCo", &["Big Co"], &["Big Co"]),
            ("Big, Co", &["Big Co"], &["Big Co"]),
            ("Big Fine Co", &["Big Co"], &[]),
            ("Co Big", &["Big Co"], &[]),
            (
                "société  générale",
                &["Société Générale"],
                &["Société Générale"],
            ),
            (
                "SOCIÉTÉ GÉNÉRALE",
                &["Société Générale"],
                &["Société Générale"],
            ),
            ("anything", &[], &[]),
            ("a - b -- c", &["--", "-"], &[]),
            // The owner's list (theseus-oejh3's join): a hyphenated name matches
            // as a unit, so the bare word "example" does not.
            ("ExampleCorp Biosciences", &OWNER, &["examplecorp"]),
            ("ask the example-repo team", &OWNER, &["example-repo"]),
            ("example-utils 0.4", &OWNER, &["example-utils"]),
            (
                "someone@examplecorpinc.example.test",
                &OWNER,
                &["examplecorpinc"],
            ),
            ("examplecorps", &OWNER, &[]),
            ("an example fires", &OWNER, &[]),
            (
                "Widgetco and Acme",
                &["Acme", "Widgetco", "Nobody"],
                &["Acme", "Widgetco"],
            ),
        ];
        for (text, names, want) in table {
            assert_eq!(found(text, names), *want, "{text:?} against {names:?}");
        }
    }

    #[test]
    fn a_pack_that_holds_a_name_is_listed_by_its_name_and_count() {
        let (pack, text) = crate::pack::EMBEDDED[0];
        let word = text
            .split(|c: char| !c.is_alphanumeric())
            .find(|w| w.len() > 6)
            .unwrap();
        let names = Names::new(&[word.to_string(), "Examplecorp".into()]);
        let listed = packs_naming(&names);
        assert!(listed.contains(&(pack, 1)), "{listed:?}");
        assert!(packs_naming(&Names::new(&[])).is_empty());
    }
}
