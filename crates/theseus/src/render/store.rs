//! Health's lines for the store and the last crash (Review 2's R4 and consideration 1):
//! the records list reads skipped, with what repairs them, and the newest crash a start
//! found. Apart from `render.rs`, whose length the shape budget caps
//! (`scripts/long-files.txt`).

use super::{fmt_time, plural};

/// `store: N records skipped …` when list reads skipped records whose reads
/// are refused (R4, theseus-15g): which, and what repairs it; nothing while
/// none are.
pub fn store_reads_line(s: &theseus_protocol::StoreStatus) -> Option<String> {
    (s.refused_records > 0).then(|| {
        let shown: Vec<String> = s.refused_positions.iter().map(u64::to_string).collect();
        let more = if (shown.len() as u64) < s.refused_records {
            ", …"
        } else {
            ""
        };
        format!(
            "store: {} skipped by list reads, their frame corrupt (positions {}{more}); {}",
            plural(s.refused_records, "record", "records"),
            shown.join(", "),
            s.repair.as_deref().unwrap_or("restore from a copy")
        )
    })
}

/// `crash: …`, the newest crash a start found (Review 2's consideration 1):
/// when, where it panicked, and its file, which holds the message.
pub fn crash_line(c: &theseus_protocol::CrashStatus) -> String {
    const ENDED: &str = ", which ended the last run";
    let ended = if c.this_start { ENDED } else { "" };
    format!(
        "crash: {} at {} (thread {}, pid {}, {}){ended}; {}",
        fmt_time(c.at_unix_ms),
        c.location,
        c.thread,
        c.pid,
        c.version,
        c.file
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The records list reads skipped (R4, theseus-15g), with what repairs
    /// them, and nothing while none are; the newest crash, said as what ended
    /// the last run when this start found it.
    #[test]
    fn health_counts_refused_reads_and_names_the_last_crash() {
        use theseus_protocol::{CrashStatus, StoreStatus};
        assert!(store_reads_line(&StoreStatus::default()).is_none());
        let line = store_reads_line(&StoreStatus {
            refused_records: 3,
            refused_positions: vec![17, 18],
            repair: Some("stop the daemon, then run `theseusd restore --repair`".into()),
        })
        .unwrap();
        assert_eq!(
            line,
            "store: 3 records skipped by list reads, their frame corrupt (positions 17, 18, …); \
             stop the daemon, then run `theseusd restore --repair`"
        );
        let mut c = CrashStatus {
            at_unix_ms: 1_790_000_000_000,
            pid: 4242,
            version: "0.0.1".into(),
            thread: "tokio-runtime-worker".into(),
            location: "crates/theseus-core/src/html.rs:120:9".into(),
            file: "/s/crashes/crash-1790000000000-4242.json".into(),
            this_start: true,
        };
        let line = crash_line(&c);
        assert!(
            line.ends_with(
                "at crates/theseus-core/src/html.rs:120:9 (thread tokio-runtime-worker, pid \
                 4242, 0.0.1), which ended the last run; /s/crashes/crash-1790000000000-4242.json"
            ),
            "{line}"
        );
        c.this_start = false;
        assert!(!crash_line(&c).contains("ended the last run"));
    }
}
