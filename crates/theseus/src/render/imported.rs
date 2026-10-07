//! Health's sessions, with an import's apart (theseus-revl): `512 · imported
//! 21,151 · erased 3`. Apart from `render.rs`, whose length the shape budget
//! caps (`scripts/long-files.txt`).

use super::thousands;
use theseus_protocol::import::HealthImported;

/// The words after `sessions`: the owner's own count, then `· imported N`
/// when an import holds sessions and `· erased N` when any were erased. A
/// store with no import says the count alone.
pub fn sessions_words(sessions: u64, imported: &HealthImported) -> String {
    let mut out = sessions.to_string();
    if imported.sessions > 0 {
        out.push_str(&format!(" · imported {}", thousands(imported.sessions)));
    }
    if imported.erased > 0 {
        out.push_str(&format!(" · erased {}", thousands(imported.erased)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(sessions: u64, imported: u64, erased: u64) -> String {
        sessions_words(
            sessions,
            &HealthImported {
                sessions: imported,
                erased,
            },
        )
    }

    #[test]
    fn an_import_and_an_erase_are_named_beside_the_owners_sessions() {
        assert_eq!(words(512, 21_151, 3), "512 · imported 21,151 · erased 3");
        assert_eq!(words(512, 21_151, 0), "512 · imported 21,151");
        assert_eq!(words(4, 0, 300), "4 · erased 300");
    }

    #[test]
    fn a_store_with_no_import_says_its_count_alone() {
        assert_eq!(words(3, 0, 0), "3");
        // The owner's own count is written as it always was.
        assert_eq!(words(1_234, 0, 0), "1234");
    }
}
