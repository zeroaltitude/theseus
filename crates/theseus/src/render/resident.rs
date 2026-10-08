//! Health's line for the daemon's own memory (theseus-9lxe): its resident
//! set, the heap in use and held, the last trim, and the largest caches.
//! Apart from `render.rs`, whose length the shape budget caps
//! (`scripts/long-files.txt`).

use theseus_protocol::resident::ResidentHealth;

use super::{plural, Tag};

fn mib(b: u64) -> String {
    format!("{:.0} MiB", b as f64 / (1024.0 * 1024.0))
}

/// `memory: 106 MiB resident; heap 61 MiB in use of 154 MiB held; …`, then
/// a line naming each cache by size. Nothing from a daemon before it.
pub fn resident_lines(r: Option<&ResidentHealth>) -> Vec<(Tag, String)> {
    let Some(r) = r else { return Vec::new() };
    let heap = r.heap.as_ref().map_or(String::new(), |h| {
        format!(
            "; heap {} in use of {} held",
            mib(h.in_use_bytes),
            mib(h.held_bytes)
        )
    });
    let trim = r.last_trim.as_ref().map_or(String::new(), |t| {
        format!(
            "; {} so far, the last {} to {} in {:.1} ms",
            plural(r.trims, "trim", "trims"),
            mib(t.rss_before_bytes),
            mib(t.rss_after_bytes),
            t.ms
        )
    });
    let caches: Vec<String> = r
        .caches
        .iter()
        .map(|c| {
            let size = match (c.bytes, c.cap_bytes) {
                (0, 0) => "empty".to_string(),
                (b, 0) => format!("{}{}", if c.estimated { "~" } else { "" }, mib(b)),
                (0, cap) => format!("bound {}", mib(cap)),
                (b, cap) => format!("{} of {}", mib(b), mib(cap)),
            };
            format!("{} {size}", c.name)
        })
        .collect();
    let mut out = vec![(
        Tag::Plain,
        format!("memory: {} resident{heap}{trim}", mib(r.rss_bytes)),
    )];
    if !caches.is_empty() {
        out.push((Tag::Plain, format!("  caches: {}", caches.join(", "))));
    }
    out
}
