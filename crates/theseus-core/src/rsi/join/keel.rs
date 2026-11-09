//! The join's keel check (theseus-pw1q.9): before a self branch joins, its
//! change against main is classified, and anything in the hard keel stops at
//! the owner's card and waits. The classes are one table, [`CLASSES`], each
//! a set of path patterns; beside them three that read the diff itself:
//!
//! - `keel-guard`: main's `scripts/keel-guard.py` (when main has one) finds
//!   an erosion in the range: a test removed or loosened, a budget, cap or
//!   ceiling raised;
//! - `store-format`: a line that adds or removes `MANIFEST_FORMAT` (a
//!   one-way store format bump);
//! - `spend-key`: a config line that adds, removes or changes a spend key
//!   (`*_usd`, a spend limit, a ceiling, the runaway lines).
//!
//! A `"self"` principal never answers the card: the card is a post to the
//! owner's DM, and nothing in the join reads an answer to it. A branch held
//! at the keel is never retried at that head.
//!
//! [`CLASSES`] holds every file main's keel guard holds in its `KEEL_FILES`
//! and `KEEL_PREFIXES`, and `every_file_the_keel_guard_holds_is_in_a_class`
//! reads the guard's own source to prove it. The two lists are not the same
//! check and neither replaces the other: the guard judges a *line* and a
//! branch may ack its finding with a `Keel:` trailer, while a class is a
//! *path* and nothing a branch writes clears it. So a file the guard holds
//! and no class named would reach main behind the branch's own ack, which is
//! what theseus-ozplq found: the data rule's files were in `KEEL_FILES` and
//! in no class. The test is the one source the two keep: a path added to the
//! guard with no class to hold it fails the gate.

use std::path::Path;

use anyhow::Result;

use crate::rsi::tree::Place;

/// A security class of paths: its name, why it is the owner's, and its
/// patterns (`*` within a path segment, `**` across segments).
#[derive(Debug, Clone, Copy)]
pub struct Class {
    pub name: &'static str,
    pub why: &'static str,
    pub paths: &'static [&'static str],
}

/// Every path class the join holds for the owner.
pub const CLASSES: &[Class] = &[
    Class {
        name: "sandbox",
        why: "L1, the boundary every job runs in",
        paths: &[
            "crates/theseus-sandbox/**",
            "crates/theseus-kernel/src/job_l1*",
        ],
    },
    Class {
        name: "policy",
        why: "the gate's postures, the floor, approvals and the place rule",
        paths: &[
            "crates/theseus-core/src/policy.rs",
            "crates/theseus-core/src/policy/**",
            "crates/theseus-core/src/approval.rs",
            "crates/theseus-core/src/places.rs",
            "crates/theseus-core/src/ceiling.rs",
            "crates/theseus-core/src/external.rs",
            "crates/theseus-core/src/toolrun/order.rs",
            "crates/theseus-core/src/sandbox.rs",
            "crates/theseus-core/src/egress.rs",
        ],
    },
    Class {
        name: "aws-iam",
        why: "AWS's roles, sessions, guardrails and stack policies",
        paths: &[
            "infra/aws/**",
            "crates/theseus-aws-guard/**",
            "crates/theseus-core/src/aws/**/*iam*",
            "crates/theseus-core/src/aws/**/*polic*",
            "crates/theseus-core/src/aws/session.rs",
            "crates/theseus-core/src/aws/bootstrap*",
        ],
    },
    Class {
        name: "secrets",
        why: "secrets, the broker that hands them out, and the scrubber",
        paths: &[
            "crates/theseus-core/src/secrets.rs",
            "crates/theseus-core/src/secrets/**",
            "crates/theseus-core/src/broker.rs",
            "crates/theseus-core/src/broker/**",
            "crates/theseus-core/src/scrub.rs",
            "crates/theseus-core/src/scrub/**",
            "scripts/scrub*",
            "scripts/test_scrub_added.py",
        ],
    },
    Class {
        name: "data-rule",
        why: "what a session is labelled, which model may see it, and what leaves the machine",
        paths: &[
            // The provider chokepoint and the turn's floor: the two places
            // the restricted-origin rule is enforced, and the predicate both
            // spend their whole decision on. `content*.rs` below does not
            // reach into `content/`, so the predicate is named outright.
            "crates/theseus-core/src/data_gate.rs",
            "crates/theseus-core/src/turn/data_step.rs",
            "crates/theseus-core/src/content/cn.rs",
            // Jev's pick runs under that floor.
            "crates/theseus-core/src/routing.rs",
            // The classifier, its asks, its facts, its scene, its wait, the
            // operator's removal, its wire shapes and its config table.
            "crates/theseus-core/src/**/content*.rs",
            "crates/theseus-protocol/src/content.rs",
            // What leaves the machine carries no vault value.
            "crates/theseus-core/src/exits.rs",
            // Who is on the other end of a socket or a CLI answer.
            "crates/theseus-core/src/peer*.rs",
            // The web UI's own door: its Host and Origin checks.
            "crates/theseusd/src/web.rs",
        ],
    },
    Class {
        name: "keel",
        why: "the gate, the keel guard and the line ceilings",
        paths: &[
            "scripts/keel-*",
            "scripts/test_keel_guard.py",
            "scripts/gate.sh",
            "scripts/long-files.txt",
            "scripts/shape.sh",
            ".config/nextest.toml",
            "deny.toml",
        ],
    },
    Class {
        name: "self",
        why: "the kill switch, the self budget and this door to main",
        paths: &[
            "crates/theseus-core/src/rsi/mod.rs",
            "crates/theseus-core/src/rsi/switch.rs",
            "crates/theseus-core/src/rsi/budget.rs",
            "crates/theseus-core/src/rsi/tree.rs",
            "crates/theseus-core/src/rsi/join.rs",
            "crates/theseus-core/src/rsi/join/**",
            "crates/theseus-core/src/turn/self_budget_step.rs",
        ],
    },
];

/// The class of the sealed dir's code: `sealed::KEEL_PATHS`.
pub const SEALED: &str = "sealed";
/// A finding of main's keel guard.
pub const KEEL_GUARD: &str = "keel-guard";
/// A `MANIFEST_FORMAT` change.
pub const STORE_FORMAT: &str = "store-format";
/// A spend key's line.
pub const SPEND_KEY: &str = "spend-key";

/// The config files a spend key lives in.
const CONFIG_PATHS: &[&str] = &[
    "crates/theseus-core/src/config.rs",
    "crates/theseus-core/src/config/**",
    "crates/theseus-core/config/**",
    "bench/*.toml",
];
/// What names a spend key on a config line.
const SPEND_WORDS: &[&str] = &[
    "_usd",
    "spend_limit",
    "daily_spend",
    "spend_ceiling",
    "runaway",
];

/// One reason the join waits for the owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub class: String,
    /// The path, or the finding's line.
    pub at: String,
}

/// Whether `path` matches `pat`: `*` within a segment, `**` across them.
pub fn matches(pat: &str, path: &str) -> bool {
    fn seg(p: &[u8], s: &[u8]) -> bool {
        match (p.first(), s.first()) {
            (None, None) => true,
            (Some(b'*'), _) => seg(&p[1..], s) || (!s.is_empty() && seg(p, &s[1..])),
            (Some(a), Some(b)) if a == b => seg(&p[1..], &s[1..]),
            _ => false,
        }
    }
    fn parts(p: &[&str], s: &[&str]) -> bool {
        match (p.first(), s.first()) {
            (None, None) => true,
            (Some(&"**"), _) => parts(&p[1..], s) || (!s.is_empty() && parts(p, &s[1..])),
            (Some(a), Some(b)) if seg(a.as_bytes(), b.as_bytes()) => parts(&p[1..], &s[1..]),
            _ => false,
        }
    }
    let p: Vec<&str> = pat.split('/').collect();
    let s: Vec<&str> = path.split('/').collect();
    parts(&p, &s)
}

/// The keel classes of a change: its paths, and its diff (`-U0`, as git
/// prints it). Pure; the keel guard's findings are [`keel_guard`]'s.
pub fn classify(paths: &[String], diff: &str) -> Vec<Hit> {
    let mut hits = Vec::new();
    for path in paths {
        for c in CLASSES {
            if c.paths.iter().any(|p| matches(p, path)) {
                hits.push(Hit {
                    class: c.name.into(),
                    at: path.clone(),
                });
            }
        }
        if crate::rsi::sealed::KEEL_PATHS
            .iter()
            .any(|k| path == k || (k.ends_with('/') && path.starts_with(k)))
        {
            hits.push(Hit {
                class: SEALED.into(),
                at: path.clone(),
            });
        }
    }
    let mut file = String::new();
    for l in diff.lines() {
        if let Some(f) = l
            .strip_prefix("+++ b/")
            .or_else(|| l.strip_prefix("--- a/"))
        {
            file = f.to_string();
            continue;
        }
        if l.starts_with("+++") || l.starts_with("---") {
            continue;
        }
        let Some(text) = l.strip_prefix('+').or_else(|| l.strip_prefix('-')) else {
            continue;
        };
        let at = || format!("{file}: {}", text.trim());
        if text.contains("MANIFEST_FORMAT") {
            hits.push(Hit {
                class: STORE_FORMAT.into(),
                at: at(),
            });
        }
        let config = CONFIG_PATHS.iter().any(|p| matches(p, &file));
        if config && SPEND_WORDS.iter().any(|w| text.contains(w)) {
            hits.push(Hit {
                class: SPEND_KEY.into(),
                at: at(),
            });
        }
    }
    hits.dedup();
    hits
}

/// Main's keel guard over `base..head` in `dir` (a tree at the head): its
/// findings, none when it passes or main has no guard. A guard that cannot
/// run is a finding: the keel never passes by default.
pub fn keel_guard(p: &Place, dir: &Path, base: &str, head: &str) -> Result<Vec<Hit>> {
    let Some(blob) = p
        .git_ok(
            &p.repo,
            &[
                "rev-parse",
                "-q",
                "--verify",
                &format!("{base}:scripts/keel-guard.py"),
            ],
        )
        .ok()
        .filter(|b| !b.is_empty())
    else {
        return Ok(Vec::new());
    };
    let o = p.git(&p.repo, &["cat-file", "blob", &blob])?;
    std::fs::create_dir_all(p.gates())?;
    let script = p.gates().join(format!("keel-guard-{blob}.py"));
    std::fs::write(&script, &o.stdout)?;
    let mut c = std::process::Command::new("python3");
    c.arg(&script)
        .args([
            "--repo",
            &dir.to_string_lossy(),
            "--base",
            base,
            "--head",
            head,
            "--quiet",
        ])
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    for (k, v) in &p.env {
        c.env(k, v);
    }
    let run = theseus_kernel::children::spawn(
        theseus_kernel::children::Kind::Owned,
        || c.spawn(),
        |c| Some(c.id()),
    )
    .and_then(std::process::Child::wait_with_output);
    let o = match run {
        Ok(o) => o,
        Err(e) => {
            return Ok(vec![Hit {
                class: KEEL_GUARD.into(),
                at: format!("main's keel guard could not run: {e}"),
            }])
        }
    };
    if o.status.success() {
        return Ok(Vec::new());
    }
    let out = String::from_utf8_lossy(&o.stdout);
    let mut hits: Vec<Hit> = out
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(20)
        .map(|l| Hit {
            class: KEEL_GUARD.into(),
            at: l.trim().to_string(),
        })
        .collect();
    if hits.is_empty() {
        hits.push(Hit {
            class: KEEL_GUARD.into(),
            at: format!(
                "main's keel guard failed (exit {:?}): {}",
                o.status.code(),
                String::from_utf8_lossy(&o.stderr).trim()
            ),
        });
    }
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classes(paths: &[&str], diff: &str) -> Vec<String> {
        let paths: Vec<String> = paths.iter().map(|p| (*p).to_string()).collect();
        let mut c: Vec<String> = classify(&paths, diff)
            .into_iter()
            .map(|h| h.class)
            .collect();
        c.dedup();
        c
    }

    #[test]
    fn the_patterns_match_within_and_across_segments() {
        assert!(matches("scripts/keel-*", "scripts/keel-guard.py"));
        assert!(!matches("scripts/keel-*", "scripts/keel/x.py"));
        assert!(matches(
            "crates/theseus-sandbox/**",
            "crates/theseus-sandbox/src/lib.rs"
        ));
        assert!(matches(
            "crates/theseus-core/src/aws/**/*iam*",
            "crates/theseus-core/src/aws/iam.rs"
        ));
        assert!(matches(
            "crates/theseus-core/src/aws/**/*polic*",
            "crates/theseus-core/src/aws/hands/policy.rs"
        ));
        assert!(!matches(
            "crates/theseus-core/src/policy.rs",
            "crates/theseus-core/src/policy.rsx"
        ));
    }

    /// A plain change is no class: it joins after its review.
    #[test]
    fn a_plain_change_is_no_class() {
        let diff = "--- a/crates/theseus-core/src/recall.rs\n+++ b/crates/theseus-core/src/recall.rs\n+let n = 3;\n";
        assert!(classes(
            &["crates/theseus-core/src/recall.rs", "docs/notes/x.md"],
            diff
        )
        .is_empty());
    }

    #[test]
    fn the_sandbox_is_the_owners() {
        assert_eq!(
            classes(&["crates/theseus-sandbox/src/seccomp.rs"], ""),
            ["sandbox"]
        );
    }

    #[test]
    fn policy_and_approvals_are_the_owners() {
        assert_eq!(
            classes(&["crates/theseus-core/src/approval.rs"], ""),
            ["policy"]
        );
        assert_eq!(
            classes(&["crates/theseus-core/src/policy/modes.rs"], ""),
            ["policy"]
        );
    }

    /// Every path the sandbox and policy classes hold, named here apart from
    /// [`CLASSES`], so a path dropped from the table fails a test.
    #[test]
    fn every_sandbox_and_policy_path_is_the_owners() {
        for p in [
            "crates/theseus-sandbox/src/lib.rs",
            "crates/theseus-kernel/src/job_l1.rs",
        ] {
            assert_eq!(classes(&[p], ""), ["sandbox"], "{p}");
        }
        for p in [
            "crates/theseus-core/src/policy.rs",
            "crates/theseus-core/src/policy/modes.rs",
            "crates/theseus-core/src/approval.rs",
            "crates/theseus-core/src/places.rs",
            "crates/theseus-core/src/ceiling.rs",
            "crates/theseus-core/src/external.rs",
            "crates/theseus-core/src/toolrun/order.rs",
            "crates/theseus-core/src/sandbox.rs",
            "crates/theseus-core/src/egress.rs",
        ] {
            assert_eq!(classes(&[p], ""), ["policy"], "{p}");
        }
    }

    #[test]
    fn aws_iam_and_policies_are_the_owners() {
        assert_eq!(classes(&["infra/aws/core.yaml"], ""), ["aws-iam"]);
        assert_eq!(
            classes(&["crates/theseus-core/src/aws/session.rs"], ""),
            ["aws-iam"]
        );
        assert!(classes(&["crates/theseus-core/src/aws/cost.rs"], "").is_empty());
    }

    #[test]
    fn secrets_are_the_owners() {
        assert_eq!(
            classes(&["crates/theseus-core/src/secrets.rs"], ""),
            ["secrets"]
        );
        assert_eq!(
            classes(&["crates/theseus-core/src/scrub/shapes.rs"], ""),
            ["secrets"]
        );
    }

    #[test]
    fn the_gate_the_guard_and_the_ceilings_are_the_owners() {
        for p in [
            "scripts/keel-guard.py",
            "scripts/keel-signers",
            "scripts/gate.sh",
            "scripts/long-files.txt",
        ] {
            assert_eq!(classes(&[p], ""), ["keel"], "{p}");
        }
    }

    /// The data rule's files, each named here apart from [`CLASSES`]: the
    /// chokepoint, the turn's floor, the route's pick, the classifier and its
    /// parts, the exits, the peer checks and the web UI's door
    /// (theseus-ozplq).
    #[test]
    fn the_data_rules_files_are_the_owners() {
        for p in [
            "crates/theseus-core/src/data_gate.rs",
            "crates/theseus-core/src/turn/data_step.rs",
            "crates/theseus-core/src/routing.rs",
            "crates/theseus-core/src/content.rs",
            "crates/theseus-core/src/content/cn.rs",
            "crates/theseus-core/src/content_scene.rs",
            "crates/theseus-core/src/content_wait.rs",
            "crates/theseus-core/src/turn/content_step.rs",
            "crates/theseus-core/src/judge/content.rs",
            "crates/theseus-core/src/fact/content.rs",
            "crates/theseus-core/src/rpc/content.rs",
            "crates/theseus-core/src/config/content_based_security.rs",
            "crates/theseus-protocol/src/content.rs",
            "crates/theseus-core/src/exits.rs",
            "crates/theseus-core/src/peer.rs",
            "crates/theseus-core/src/peer_proc.rs",
            "crates/theseusd/src/web.rs",
        ] {
            assert_eq!(classes(&[p], ""), ["data-rule"], "{p}");
        }
        // The route's config table is the owner's by the guard's own word:
        // `KEEL_FILES` says "NOT config/routing.rs", and no class claims it.
        assert!(classes(&["crates/theseus-core/src/config/routing.rs"], "").is_empty());
        // A name that merely ends in the word is not the rule.
        assert!(classes(&["crates/theseus-core/src/tests_content_flags.rs"], "").is_empty());
    }

    /// The strings of the tuple `name = (` .. `)` in the keel guard's source:
    /// a comment is dropped first, then the tuple ends at the `)` that is
    /// left, whether on its own line or on the opening one.
    ///
    /// `sealed::tests`'s older copy of this ends at the *first* `)` anywhere
    /// in the body, which a comment inside the tuple supplies, and so reads
    /// only the paths above that comment. That is harmless there today (every
    /// `KEEL_PATHS` entry is above it) and is its own row; here it hid four of
    /// the guard's files, `routing.rs` and `turn/data_step.rs` among them, and
    /// this test passed while the gap it is here to catch was open.
    fn guard_tuple(src: &str, name: &str) -> Vec<String> {
        let after = src
            .split_once(&format!("\n{name} = ("))
            .unwrap_or_else(|| panic!("scripts/keel-guard.py has no {name}"))
            .1;
        let mut out = Vec::new();
        for line in after.lines() {
            let code = line.split('#').next().unwrap_or_default();
            let mut rest = code;
            while let Some(i) = rest.find('"') {
                let tail = &rest[i + 1..];
                let Some(j) = tail.find('"') else { break };
                out.push(tail[..j].to_string());
                rest = &tail[j + 1..];
            }
            // The tuple's close, on its own line or on the opening one.
            if code.contains(')') {
                break;
            }
        }
        out
    }

    /// The parser reads every entry of `KEEL_FILES`, not the ones above the
    /// first comment with a `)` in it (`sealed::tests`'s older copy read 13
    /// of 23). The count is checked against a second, line-based count, and
    /// the ten paths that sit below that comment are named.
    #[test]
    fn the_parser_reads_every_entry_of_the_guards_file_list() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let src = std::fs::read_to_string(root.join("scripts/keel-guard.py")).unwrap();
        let files = guard_tuple(&src, "KEEL_FILES");
        let body = src.split_once("\nKEEL_FILES = (").unwrap().1;
        let body = body.split_once("\n)").unwrap().0;
        let by_line = body
            .lines()
            .filter(|l| l.trim_start().starts_with('"'))
            .count();
        assert!(by_line >= 23, "the line count itself is wrong: {by_line}");
        assert_eq!(files.len(), by_line, "{files:?}");
        for p in [
            "crates/theseus-core/src/routing.rs",
            "crates/theseus-core/src/turn/data_step.rs",
            "crates/theseus-core/src/content/cn.rs",
            "crates/theseus-core/src/data_gate.rs",
            "crates/theseus-core/src/exits.rs",
            "crates/theseus-core/src/peer.rs",
            "crates/theseus-core/src/peer_proc.rs",
            "crates/theseusd/src/web.rs",
            "scripts/scrub-added.sh",
            "scripts/test_scrub_added.py",
        ] {
            assert!(files.iter().any(|f| f == p), "{p} not read: {files:?}");
        }
    }

    /// One source for the two lists (theseus-ozplq): every file main's keel
    /// guard holds is in a class, so a path added to `KEEL_FILES` with no
    /// class to hold it fails here rather than reaching main behind a branch's
    /// own `Keel:` ack.
    #[test]
    fn every_file_the_keel_guard_holds_is_in_a_class() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let src = std::fs::read_to_string(root.join("scripts/keel-guard.py")).unwrap();
        let files = guard_tuple(&src, "KEEL_FILES");
        let prefixes = guard_tuple(&src, "KEEL_PREFIXES");
        assert!(!files.is_empty() && !prefixes.is_empty(), "{files:?}");
        let held = |p: &str| !classify(&[p.to_string()], "").is_empty();
        let missing: Vec<&String> = files
            .iter()
            .filter(|p| !held(p))
            .chain(prefixes.iter().filter(|p| !held(&format!("{p}held.rs"))))
            .collect();
        assert!(
            missing.is_empty(),
            "scripts/keel-guard.py holds these, and no keel CLASS does: {missing:?}"
        );
    }

    #[test]
    fn the_sealed_dirs_code_is_the_owners() {
        let c = classes(&["crates/theseus-core/src/rsi/sealed/filter.rs"], "");
        assert_eq!(c, ["sealed"]);
    }

    #[test]
    fn the_exam_set_in_the_repository_is_the_sealed_class_too() {
        let c = classes(&["crates/theseus-exam/exam/exam-v2.toml"], "");
        assert_eq!(c, ["sealed"]);
    }

    #[test]
    fn the_switch_the_budget_and_this_door_are_the_owners() {
        for p in [
            "crates/theseus-core/src/rsi/switch.rs",
            "crates/theseus-core/src/rsi/join/keel.rs",
        ] {
            assert_eq!(classes(&[p], ""), ["self"], "{p}");
        }
    }

    #[test]
    fn a_store_format_change_is_the_owners() {
        let diff = "--- a/crates/theseus-store/src/store.rs\n+++ b/crates/theseus-store/src/store.rs\n-pub const MANIFEST_FORMAT: u32 = 32;\n+pub const MANIFEST_FORMAT: u32 = 33;\n";
        assert_eq!(
            classes(&["crates/theseus-store/src/store.rs"], diff),
            ["store-format"]
        );
    }

    #[test]
    fn a_spend_key_is_the_owners() {
        let diff = "--- a/crates/theseus-core/config/theseus.example.toml\n+++ b/crates/theseus-core/config/theseus.example.toml\n-# day_usd = 30.0\n+# day_usd = 300.0\n";
        assert_eq!(
            classes(&["crates/theseus-core/config/theseus.example.toml"], diff),
            ["spend-key"]
        );
        // The same words outside the config are code, not a key.
        let code = "--- a/crates/theseus-core/src/recall.rs\n+++ b/crates/theseus-core/src/recall.rs\n+let cost_usd = 0.0;\n";
        assert!(classes(&["crates/theseus-core/src/recall.rs"], code).is_empty());
    }
}
