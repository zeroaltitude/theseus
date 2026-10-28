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
        ],
    },
    Class {
        name: "keel",
        why: "the gate, the keel guard and the line ceilings",
        paths: &[
            "scripts/keel-*",
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
