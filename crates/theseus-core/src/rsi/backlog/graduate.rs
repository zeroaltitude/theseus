//! Graduation (theseus-pw1q.13): a loaded extension the owner relies on
//! becomes a planck of the ship. The nightly backlog reads it as one more
//! cause: a loaded, unrevoked extension called at least `[self]
//! graduate_min_calls` times in `graduate_window_days` days since it was
//! loaded, with no failure, and not graduated before, is a candidate
//! `graduate ext-<name>` with its evidence (the counts, the manifest's digest
//! and its declared tests). From there it is an item like any other: ranked,
//! started under the `"self"` principal, built by `self.propose_change`,
//! joined by self-join.
//!
//! - **Once per digest.** The cause key names the digest, and an item that
//!   ever carried the key (open, done, failed or dropped) keeps the
//!   extension from being filed again for it. A new version is a new digest
//!   and counts its own calls.
//! - **A failure, or a read that could not see them all, files nothing.** A
//!   call that ended in an error result, a timeout or a crash counts (see
//!   `extend::usage`), and so does a read that stopped at its row cap.
//! - **The keel.** An extension that asked for network is held for the
//!   owner (`security`, `keel.rs`): its item is `waiting_human` and never
//!   starts. The brief says no port widens what the extension had.
//! - **The brief is data.** The source's path, the tools' schemas and the
//!   declared tests are quoted as JSON strings and values, with what done
//!   is. It carries no instruction beyond the task's own.
//!
//! **The revoke** after the join is the join's notice step
//! ([`Core::extension_revoke_graduated`]): it asks `rsi::gate`, takes only
//! an extension this module filed for that digest, and writes the ordinary
//! `extend.revoked` row with the reason `graduated`, by `self`, citing the
//! merge. Its frozen copy stays on disk, as after the owner's revoke.

use theseus_protocol::extend::ExtendUsage;

use super::read::Obs;
use super::state::State;
use crate::extend::load::{Loaded, LoadedSet};
use crate::extend::{manifest_key, Manifest};
use crate::fact::rsi_graduate::GraduateProposed;
use crate::rpc::Core;

/// A graduate cause's key starts with this.
pub const PREFIX: &str = "graduate.ext";
const DAY_MS: u64 = 86_400_000;
/// The most of a tool's schema or a test's field the brief quotes.
const QUOTE_CHARS: usize = 2_000;

/// What a graduation is: the counts it was filed on and the port's brief.
#[derive(Debug, Clone, PartialEq)]
pub struct Graduation {
    pub extension: String,
    pub digest: String,
    pub calls: u64,
    pub failures: u64,
    pub window_days: u32,
    /// The item's objective, with `{item}` where its id goes.
    pub brief: String,
}

impl Graduation {
    /// The row that files it.
    pub fn row_of(&self, item: &str, keel: Option<String>) -> GraduateProposed {
        GraduateProposed {
            item: item.into(),
            extension: self.extension.clone(),
            digest: self.digest.clone(),
            calls: self.calls,
            failures: self.failures,
            window_days: self.window_days,
            keel,
        }
    }
}

/// The cause key of an extension's version: `graduate.ext:<name>:<digest>`,
/// and `graduate.ext.net:` when it asked for network (the keel mark's key).
pub fn key_of(name: &str, digest: &str, network: bool) -> String {
    let d = &digest[..digest.len().min(12)];
    match network {
        true => format!("{PREFIX}.net:{name}:{d}"),
        false => format!("{PREFIX}:{name}:{d}"),
    }
}

/// The rule: enough calls, no failure, and every row seen.
pub fn fires(u: &ExtendUsage, min_calls: u32) -> bool {
    u.calls >= u64::from(min_calls.max(1)) && u.failures == 0 && !u.truncated
}

/// A string as a JSON string, cut to [`QUOTE_CHARS`].
fn quoted(s: &str) -> String {
    let cut: String = s.chars().take(QUOTE_CHARS).collect();
    serde_json::Value::String(cut).to_string()
}

/// A value as compact JSON, cut to [`QUOTE_CHARS`] characters.
fn compact(v: &serde_json::Value) -> String {
    let s = v.to_string();
    match s.char_indices().nth(QUOTE_CHARS) {
        Some((i, _)) => format!("{} …[cut]", &s[..i]),
        None => s,
    }
}

/// The port's brief for `l`, from its manifest when the store has it.
fn brief_of(l: &Loaded, m: Option<&Manifest>, u: &ExtendUsage) -> String {
    let mut out = format!(
        "Port the loaded extension ext-{name} into core.\n\n\
         Evidence (the ledger's tool-call rows, {days} days since it was loaded): {calls} calls, \
         {fails} failures, p50 {p50} ms, p95 {p95} ms.\n\
         Digest: {digest}\n\
         Frozen source (read-only): {frozen}\n\
         Command: {command}\n\
         Description: {desc}\n",
        name = l.name,
        days = u.window_days,
        calls = u.calls,
        fails = u.failures,
        p50 = u.p50_ms,
        p95 = u.p95_ms,
        digest = l.digest,
        frozen = l.frozen,
        command = compact(&serde_json::json!(l.command)),
        desc = quoted(&l.description),
    );
    match l.capabilities.network.as_slice() {
        [] => out.push_str("Grant: no network, no secret.\n"),
        hosts => out.push_str(&format!(
            "Grant: network to {} (a port with network is the owner's to decide), no secret.\n",
            hosts.join(", ")
        )),
    }
    out.push_str("\nTools (name, input schema):\n");
    match m {
        Some(m) if !m.tools.is_empty() => {
            for t in &m.tools {
                out.push_str(&format!("- {} {}\n", t.name, compact(&t.input_schema)));
            }
        }
        _ => {
            for t in &l.tools {
                out.push_str(&format!("- {t} (its schema: read the frozen source)\n"));
            }
        }
    }
    out.push_str("\nDeclared tests (tool, arguments, expected):\n");
    match m {
        Some(m) if !m.tests.is_empty() => {
            for t in &m.tests {
                let expect = match &t.spec.expect {
                    crate::extend::Expect::Contains(s) => format!("contains {}", quoted(s)),
                    crate::extend::Expect::Equals(s) => format!("equals {}", quoted(s)),
                };
                out.push_str(&format!(
                    "- {} {} {expect}\n",
                    t.spec.tool,
                    compact(&t.spec.arguments)
                ));
            }
        }
        _ => out.push_str("- none declared\n"),
    }
    out.push_str(
        "\nDone is:\n\
         1. A core tool for each tool above, with the same name and input schema, that answers \
         as the frozen source does.\n\
         2. Each declared test as a Rust test beside the tool, which fails without the port.\n\
         3. Every gate passes. The extension stays loaded until the join; the join's notice step \
         then revokes it with the reason graduated, and the owner can undo that with one word.\n\
         4. The port widens nothing the extension had: no network beyond the grant above, no \
         secret, no change to a policy, a permission or a limit. A port that needs one stops at \
         the owner.\n\n\
         Call self.propose_change { item: \"{item}\", issue: <the issue>, why: <one sentence> }.",
    );
    out
}

impl Core {
    /// Revoke the extension `name` at `digest` because its port joined core
    /// (`merge` is the join's commit): the `extend.revoked` row says
    /// `graduated`. Only while `rsi::gate` allows, and only for an
    /// extension this module filed a port for, at that digest: any other
    /// revoke is the owner's (`extension.revoke`).
    pub fn extension_revoke_graduated(
        &self,
        name: &str,
        digest: &str,
        merge: &str,
    ) -> anyhow::Result<theseus_protocol::extend::ExtensionRevokeResult> {
        let g = super::gate(&self.self_ctx());
        if g != super::Gate::Allowed {
            anyhow::bail!("self steps are {}: nothing was revoked", g.name());
        }
        let filed = {
            let st = State::read(&self.store)?;
            let set = LoadedSet::read(&self.store)?;
            set.loaded.get(name).is_some_and(|l| {
                let key = key_of(name, digest, !l.capabilities.network.is_empty());
                st.items.values().any(|m| m.cause == key)
            })
        };
        if !filed {
            anyhow::bail!("no port was filed for {name} at {digest}: only its owner revokes it");
        }
        self.revoke_loaded(
            name,
            Some(digest),
            ("self", &format!("join {merge}")),
            Some("graduated"),
            || Ok(()),
        )
    }

    /// The night's graduation candidates, as observations of their causes. A
    /// read that fails files nothing (and says so): the rest of the night
    /// goes on.
    pub(super) fn graduate_obs(&self, st: &State, now_ms: u64) -> Vec<Obs> {
        let cfg = &self.cfg.self_improve;
        let set = match LoadedSet::read(&self.store) {
            Ok(s) if !s.loaded.is_empty() => s,
            Ok(_) => return vec![],
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "self backlog: the loaded extensions were not read");
                return vec![];
            }
        };
        let calls = match self.extension_calls(now_ms, cfg.graduate_window_days) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "self backlog: extension use was not read");
                return vec![];
            }
        };
        let from = now_ms.saturating_sub(u64::from(cfg.graduate_window_days) * DAY_MS);
        let mut out = Vec::new();
        for l in set.loaded.values() {
            let key = key_of(&l.name, &l.digest, !l.capabilities.network.is_empty());
            if st.items.values().any(|m| m.cause == key) {
                continue;
            }
            let u = calls.usage(&l.name, from.max(l.acked_at_ms), cfg.graduate_window_days);
            if !fires(&u, cfg.graduate_min_calls) {
                continue;
            }
            let manifest = self
                .store
                .get_meta::<Manifest>(&manifest_key(&l.name, &l.digest))
                .ok()
                .flatten();
            let title = format!("graduate ext-{}", l.name);
            let g = Graduation {
                extension: l.name.clone(),
                digest: l.digest.clone(),
                calls: u.calls,
                failures: u.failures,
                window_days: u.window_days,
                brief: brief_of(l, manifest.as_ref(), &u),
            };
            let at = |what: &str, note: String| Obs {
                key: key.clone(),
                title: title.clone(),
                source: "extension".into(),
                id: format!("{what}:{}:{}", l.name, l.digest),
                note,
                at_ms: now_ms,
                usd: 0.0,
                person: false,
                task_failed: false,
                work: 3,
                graduate: None,
                pattern: None,
            };
            out.push(Obs {
                graduate: Some(g),
                ..at("graduate", u.line())
            });
            let (passed, declared) = manifest
                .as_ref()
                .map_or((0, 0), |m| (m.passed(), m.tests.len()));
            out.push(at(
                "manifest",
                format!(
                    "digest {}, {passed} of {declared} declared tests passed in the trial",
                    l.digest
                ),
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(calls: u64, failures: u64, truncated: bool) -> ExtendUsage {
        ExtendUsage {
            window_days: 14,
            calls,
            failures,
            p50_ms: 41,
            p95_ms: 90,
            truncated,
        }
    }

    #[test]
    fn the_rule_fires_at_the_minimum_with_no_failure_and_a_whole_read() {
        assert!(fires(&usage(20, 0, false), 20));
        assert!(!fires(&usage(19, 0, false), 20), "one call short");
        assert!(!fires(&usage(500, 1, false), 20), "one failure");
        assert!(!fires(&usage(500, 0, true), 20), "a read that missed rows");
        assert!(fires(&usage(1, 0, false), 0), "a minimum of 0 is 1");
    }

    #[test]
    fn a_key_names_the_version_and_the_keel_mark_follows_the_network() {
        let d = "3f2a1c9e0b7d5a4c8e";
        assert_eq!(key_of("wc", d, false), "graduate.ext:wc:3f2a1c9e0b7d");
        assert_eq!(key_of("wc", d, true), "graduate.ext.net:wc:3f2a1c9e0b7d");
        assert_ne!(key_of("wc", d, false), key_of("wc", "ffff", false));
    }
}
