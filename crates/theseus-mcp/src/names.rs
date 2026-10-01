//! An MCP tool's two names (design §2.1, for 36b's board):
//!
//! - the canonical name, `mcp:<server>/<tool>`, which the gate resolves
//!   (`MCP_PREFIX`, `[policy.mcp]`);
//! - the wire name a model sees, `mcp__<server>__<tool>`: letters, digits,
//!   `_` and `-` only, at most 64 characters (the providers' limit).
//!
//! A wire name that had to change (a character replaced, or cut to fit)
//! ends in `_` and six hex digits of a digest of the canonical name, so two
//! tools that differ only in what was replaced or cut stay apart.
//! [`wire_names`] makes a whole list unique, as a board needs.

use std::collections::HashMap;

/// The providers' limit on a tool's name.
pub const MAX_WIRE: usize = 64;

pub fn canonical(server: &str, tool: &str) -> String {
    format!("mcp:{server}/{tool}")
}

/// The wire name of one tool, on its own.
pub fn wire_name(server: &str, tool: &str) -> String {
    let exact = format!("mcp__{server}__{tool}");
    let clean = format!("mcp__{}__{}", clean(server), clean(tool));
    if clean == exact && clean.len() <= MAX_WIRE {
        clean
    } else {
        with_digest(&clean, &canonical(server, tool))
    }
}

/// The wire names of a board's tools, in the same order, every one unique.
/// Any two that would collide (`a__b` + `c` and `a` + `b__c` both make
/// `mcp__a__b__c`) both take their digests.
pub fn wire_names(tools: &[(&str, &str)]) -> Vec<String> {
    let mut names: Vec<String> = tools.iter().map(|(s, t)| wire_name(s, t)).collect();
    let mut seen: HashMap<String, usize> = HashMap::new();
    for n in &names {
        *seen.entry(n.clone()).or_default() += 1;
    }
    for (n, (s, t)) in names.iter_mut().zip(tools) {
        if seen[n.as_str()] > 1 {
            *n = with_digest(n, &canonical(s, t));
        }
    }
    names
}

fn clean(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn with_digest(name: &str, key: &str) -> String {
    let digest = format!("{:06x}", fnv1a(key.as_bytes()) & 0xff_ffff);
    // `name` is ASCII here, so a byte cut is a character cut.
    let keep = name.len().min(MAX_WIRE - 1 - digest.len());
    format!("{}_{digest}", &name[..keep])
}

/// FNV-1a, 64 bits: stable across platforms and releases, unlike std's
/// hasher, so a stored wire name never changes under a rebuild.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_names_pass_and_changed_ones_take_a_digest() {
        assert_eq!(canonical("github", "get_issue"), "mcp:github/get_issue");
        assert_eq!(wire_name("github", "get_issue"), "mcp__github__get_issue");
        let dotted = wire_name("github", "get.issue");
        assert!(dotted.starts_with("mcp__github__get_issue_") && dotted.len() == 29);
        assert_ne!(dotted, wire_name("github", "get issue"));
        let long = wire_name("docs", &"x".repeat(100));
        assert_eq!(long.len(), MAX_WIRE);
        assert_ne!(long, wire_name("docs", &"x".repeat(101)));
        assert!(long
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'));
        // Stable: the same digest every build.
        assert_eq!(
            fnv1a(b"mcp:github/get.issue"),
            fnv1a(b"mcp:github/get.issue")
        );
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
    }

    #[test]
    fn a_board_list_is_unique() {
        let names = wire_names(&[("a__b", "c"), ("a", "b__c"), ("fake", "echo")]);
        assert_eq!(names[2], "mcp__fake__echo");
        assert_ne!(names[0], names[1]);
        assert!(names[0].starts_with("mcp__a__b__c_") && names[1].starts_with("mcp__a__b__c_"));
    }
}
