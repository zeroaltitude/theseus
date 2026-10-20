//! Deferred tools in the CLI (theseus-7m7w.2): `theseus tools`' list of the
//! families each kind of place loads or defers, with their use and misses,
//! and health's one line. Apart from `render.rs`, whose length the shape
//! budget caps (`scripts/long-files.txt`).

use theseus_protocol::defer::{DeferHealth, DeferListResult};

use super::{plural, Tag};

/// Health's `tools:` line. Nothing from a daemon before it.
pub fn defer_lines(d: Option<&DeferHealth>) -> Vec<(Tag, String)> {
    let Some(d) = d else { return Vec::new() };
    if !d.enabled {
        return vec![(
            Tag::Plain,
            "tools: every tool loads ([tools.defer] enabled = false)".into(),
        )];
    }
    let mut line = format!(
        "tools: families load per place ({} reaches the rest)",
        match d.mechanism.as_str() {
            "search" => "the provider's tool search",
            _ => "tools_find",
        }
    );
    if !d.warmed {
        line.push_str("; the ledger's use is not read yet, defaults only");
    } else {
        line.push_str(&format!(
            "; {} · {} searched, {} missed",
            plural(d.turns as u64, "turn", "turns"),
            d.searched,
            d.missed
        ));
        if let Some(f) = &d.top_miss {
            line.push_str(&format!(" (most: {f})"));
        }
    }
    let tag = if d.missed > 0 { Tag::Warn } else { Tag::Plain };
    vec![(tag, line)]
}

/// `theseus tools`' families per place: each kind of place, each family with
/// whether it loads and why, and its use, searches and misses in the window.
/// `verbose` names each family's tools.
pub fn defer_list(d: &DeferListResult, verbose: bool) -> Vec<String> {
    if !d.enabled {
        return vec!["tool families: every tool loads ([tools.defer] enabled = false)".into()];
    }
    let reach = match d.mechanism.as_str() {
        "search" => "the model searches for a deferred tool (the provider's tool search)",
        _ => "the model asks tools_find; its families load at once, for the turn's next request",
    };
    let mut out = vec![
        format!(
            "tool families per place (a family loads at {:.0}% of a place kind's turns over {} days, hides unused for {}; {reach}{}):",
            d.load_share * 100.0,
            d.window_days,
            d.hide_after_days,
            if d.warmed { "" } else { "; the ledger is not read yet" },
        ),
    ];
    for p in &d.places {
        out.push(format!(
            "  {} ({}) · tools ≈{} tokens a request, of {} whole",
            p.place,
            plural(p.turns as u64, "turn", "turns"),
            p.tokens_sent,
            p.tokens_all
        ));
        for f in &p.families {
            let state = if f.loaded { "loaded  " } else { "deferred" };
            let mut line = format!(
                "    {:<10} {state} {:<26} {:>2} tools ≈{} tokens · used in {} turns",
                f.family,
                f.why,
                f.tools.len(),
                f.tokens,
                f.used_turns
            );
            if f.searched > 0 {
                line.push_str(&format!(" · {} searches", f.searched));
            }
            if f.missed > 0 {
                line.push_str(&format!(" · {} missed", f.missed));
            }
            if f.loaded && f.unused_turns > 0 && f.used_turns == 0 {
                line.push_str(" · loaded, never used");
            }
            out.push(line);
            if verbose {
                out.push(format!("        {}", f.tools.join(", ")));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::defer::{DeferFamily, DeferPlace};

    fn family(name: &str, loaded: bool, why: &str, used: u32, missed: u32) -> DeferFamily {
        DeferFamily {
            family: name.into(),
            tools: vec![format!("{name}_one"), format!("{name}_two")],
            loaded,
            why: why.into(),
            used_turns: used,
            missed,
            ..Default::default()
        }
    }

    #[test]
    fn health_says_how_the_rest_is_reached_and_warns_of_misses() {
        assert!(defer_lines(None).is_empty());
        let d = DeferHealth {
            enabled: true,
            mechanism: "search".into(),
            warmed: true,
            turns: 40,
            searched: 3,
            missed: 2,
            top_miss: Some("aws".into()),
        };
        let l = defer_lines(Some(&d));
        assert_eq!(l[0].0, Tag::Warn);
        assert!(
            l[0].1.contains("tool search") && l[0].1.contains("2 missed (most: aws)"),
            "{}",
            l[0].1
        );
        let off = DeferHealth::default();
        assert!(defer_lines(Some(&off))[0].1.contains("enabled = false"));
        let cold = DeferHealth {
            enabled: true,
            mechanism: "find".into(),
            ..Default::default()
        };
        assert!(defer_lines(Some(&cold))[0].1.contains("tools_find"));
        assert!(defer_lines(Some(&cold))[0].1.contains("defaults only"));
    }

    #[test]
    fn the_list_names_each_family_per_place_with_its_use_and_misses() {
        let d = DeferListResult {
            enabled: true,
            mechanism: "find".into(),
            window_days: 14,
            hide_after_days: 14,
            load_share: 0.1,
            warmed: true,
            places: vec![DeferPlace {
                place: "dm".into(),
                turns: 25,
                tokens_all: 4000,
                tokens_sent: 900,
                families: vec![
                    family("files", true, "always loaded", 20, 0),
                    family("aws", false, "deferred", 0, 3),
                ],
            }],
        };
        let out = defer_list(&d, false).join("\n");
        assert!(
            out.contains("dm (25 turns) · tools ≈900 tokens a request, of 4000 whole"),
            "{out}"
        );
        assert!(out.contains("files      loaded   "), "{out}");
        assert!(
            out.contains("aws        deferred") && out.contains("3 missed"),
            "{out}"
        );
        assert!(!out.contains("aws_one"));
        let verbose = defer_list(&d, true).join("\n");
        assert!(verbose.contains("aws_one, aws_two"), "{verbose}");
        let off = DeferListResult::default();
        assert!(defer_list(&off, false)[0].contains("every tool loads"));
    }
}
