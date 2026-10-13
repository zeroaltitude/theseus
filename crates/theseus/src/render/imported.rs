//! Health's sessions, with an import's apart (theseus-revl): `512 · imported
//! 21,151 · erased 3`. Apart from `render.rs`, whose length the shape budget
//! caps (`scripts/long-files.txt`).

use super::{push, thousands, Line, Tag};
use theseus_protocol::import::HealthImported;
use theseus_protocol::import_sync::HealthSync;

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

/// The OpenClaw sync's line (theseus-pjmj7): when the last run ended and what
/// it did, then the next run, and a warning line under it for a failed or a
/// missed night.
/// `sync: last ran 2026-10-12 03:41 (3 new, 12 appended, 148 messages, 1 refused, people $0.31); next 2026-10-13 03:40`
pub fn sync_line(s: &HealthSync) -> String {
    use super::time::fmt_date;
    format!(
        "sync: last ran {} ({} new, {} appended, {} messages, {} refused, people ${:.2}); next {}",
        fmt_date(s.at_ms),
        s.sessions_new,
        s.sessions_appended,
        thousands(s.messages),
        s.rejected,
        s.people_usd,
        fmt_date(s.next_ms),
    )
}

/// Health's sync lines: nothing before a run.
pub fn push_sync(o: &mut Vec<Line>, s: Option<&HealthSync>) {
    let Some(s) = s else { return };
    push(o, Tag::Plain, &sync_line(s));
    if let Some(w) = &s.warn {
        push(o, Tag::Warn, &format!("⚠ import sync: {w}"));
    }
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
                sync: None,
            },
        )
    }

    /// The sync's line and its warning, pinned to a fixed zone.
    #[test]
    fn the_sync_line_names_the_run_and_warns_of_a_failed_night() {
        crate::render::time::pin_for_tests();
        let mut s = HealthSync {
            at_ms: 1_790_000_000_000,
            sessions_new: 3,
            sessions_appended: 12,
            messages: 1480,
            rejected: 1,
            people_usd: 0.314,
            next_ms: 1_790_086_400_000,
            ..HealthSync::default()
        };
        assert_eq!(
            sync_line(&s),
            "sync: last ran 2026-09-21 07:13 (3 new, 12 appended, 1,480 messages, 1 refused, \
             people $0.31); next 2026-09-22 07:13"
        );
        let mut o = Vec::new();
        push_sync(&mut o, Some(&s));
        assert_eq!(o.len(), 1);
        s.warn = Some("the last sync failed: no python3".into());
        let mut o = Vec::new();
        push_sync(&mut o, Some(&s));
        assert_eq!(o.len(), 2);
        assert_eq!(o[1].text, "⚠ import sync: the last sync failed: no python3");
        assert!(matches!(o[1].tag, Tag::Warn));
        let mut none = Vec::new();
        push_sync(&mut none, None);
        assert!(none.is_empty());
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
