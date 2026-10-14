//! The install and the notice (theseus-pw1q.9). Only with `[self] install
//! = true` (the owner's standing OK, `false` by default) and only on the
//! owner's machine: the joined merge is checked out in a tree of its own,
//! built with `scripts/build.sh` as the install does, each shipped binary
//! copied in by copy-then-rename, and the daemon restarted. All of it runs
//! in a transient `systemd-run --user` unit, since the daemon must not kill
//! itself mid-step: the unit outlives the restart it makes.
//!
//! The new daemon, once serving ([`Core::self_installed_after_serving`]),
//! finds the `self.joined` row whose merge is the commit it was built from
//! and that has no `self.installed` row, and writes `self.installed` and the
//! notice to the owner's DM in one frame: what changed, why, the numbers,
//! the cost, and the revert. So the notice fires once, never twice.

use std::path::Path;

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

use super::git;
use crate::fact::rsi_join::SelfInstalled;
use crate::rpc::Core;
use crate::rsi::tree::Place;

/// The binaries an install swaps in (`scripts/build.sh --shipped`).
pub const BINARIES: &[&str] = &[
    "theseusd",
    "theseus",
    "theseus-tui",
    "theseus-sim",
    "theseus-index",
];
/// The service the restart names (`scripts/user-service.sh`'s default unit).
pub const SERVICE: &str = "theseusd.service";

/// Starts an install of a merge; the unit's name.
pub trait Installer: Send + Sync {
    fn start(&self, p: &Place, merge: &str) -> Result<String>;
}

/// The script the unit runs, in `tree`, building into `target`.
pub fn script(tree: &Path, target: &Path, merge: &str) -> String {
    format!(
        "#!/bin/bash\n# A self-join's install (theseus-pw1q.9) of {merge}.\nset -euo pipefail\n\
         cd {tree}\nexport CARGO_TARGET_DIR={target} THESEUS_COMMIT={merge}\n\
         scripts/build.sh --profile release-thin\nprefix=\"$HOME/.local/bin\"\n\
         for b in $(scripts/build.sh --shipped); do\n  \
         cp \"$CARGO_TARGET_DIR/release-thin/$b\" \"$prefix/.$b.new\" && mv -f \"$prefix/.$b.new\" \"$prefix/$b\"\n\
         done\nsystemctl --user restart {SERVICE}\n",
        tree = shell(&tree.to_string_lossy()),
        target = shell(&target.to_string_lossy()),
    )
}

fn shell(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The unit's argv for `script`.
pub fn argv(unit: &str, script: &Path) -> Vec<String> {
    vec![
        "systemd-run".into(),
        "--user".into(),
        "--collect".into(),
        "--quiet".into(),
        format!("--unit={unit}"),
        "--property=Nice=10".into(),
        "/bin/bash".into(),
        script.to_string_lossy().into_owned(),
    ]
}

/// The machine's own: a tree at the merge, the script, and its unit.
pub struct Systemd;

impl Installer for Systemd {
    fn start(&self, p: &Place, merge: &str) -> Result<String> {
        let short = &merge[..merge.len().min(12)];
        let t = git::detached(p, "install", short, merge)?;
        let script_path = p.root().join(format!("install-{short}.sh"));
        std::fs::write(&script_path, script(&t.dir, &t.target, merge))?;
        let unit = format!("theseus-self-install-{short}");
        let a = argv(&unit, &script_path);
        let mut c = std::process::Command::new(&a[0]);
        c.args(&a[1..])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped());
        let o = theseus_kernel::children::spawn(
            theseus_kernel::children::Kind::Owned,
            || c.spawn(),
            |c| Some(c.id()),
        )
        .and_then(std::process::Child::wait_with_output)
        .context("systemd-run could not start")?;
        if !o.status.success() {
            bail!(
                "systemd-run refused: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            );
        }
        Ok(unit)
    }
}

impl Core {
    /// Once serving: the notice for the join this build installs, if it has
    /// none yet (the module's doc). Nothing with the mode off.
    pub fn self_installed_after_serving(&self) -> Result<Option<String>> {
        match crate::build().commit {
            Some(c) => self.self_installed_as(&c),
            None => Ok(None),
        }
    }

    /// The same, for a build of `commit`: the merge noticed, if any.
    pub fn self_installed_as(&self, commit: &str) -> Result<Option<String>> {
        if self.cfg.self_improve.mode != theseus_protocol::rsi::SelfMode::Act {
            return Ok(None);
        }
        let Some(rows) = super::rows_of(
            &self.store,
            &["self.joined", "self.installed", "self.review.verdict"],
        )?
        else {
            anyhow::bail!("the ledger's index is still being built");
        };
        fn data_of<'r>(
            rows: &'r [crate::ledger::LedgerRow],
            kind: &'r str,
        ) -> impl Iterator<Item = &'r Value> + 'r {
            rows.iter().filter(move |r| r.kind == kind).map(|r| &r.data)
        }
        let of = |kind: &'static str| data_of(&rows, kind);
        if of("self.installed").any(|d| d["merge"] == commit) {
            return Ok(None);
        }
        let Some(j) = of("self.joined").filter(|d| d["merge"] == commit).last() else {
            return Ok(None);
        };
        let branch = j["branch"].as_str().unwrap_or("").to_string();
        let review = of("self.review.verdict")
            .filter(|d| d["branch"] == branch.as_str() && d["verdict"] == "accept")
            .last();
        let fact = SelfInstalled {
            merge: commit.into(),
            binaries: BINARIES.iter().map(|b| (*b).to_string()).collect(),
            version: crate::VERSION.into(),
            branch: branch.clone(),
        };
        let text = notice(j, review, commit);
        let (post, mut frame) = self
            .outbox
            .stage_to_operator(None, json!({"kind": "self_join", "text": text}))?;
        let rec = self.rec(None);
        frame.push(rec.row(&fact)?);
        self.store.append(&frame)?;
        self.outbox.posted(&post);
        rec.announce(&fact);
        Ok(Some(commit.into()))
    }
}

/// The notice: what changed, why, the numbers, the cost, and the revert.
fn notice(j: &Value, review: Option<&Value>, merge: &str) -> String {
    let s = |v: &Value, k: &str| v[k].as_str().unwrap_or("?").to_string();
    let reviewed = review.map_or_else(
        || "no review row found".to_string(),
        |r| {
            format!(
                "accepted by {} (planted revert: {}){}",
                s(r, "reviewer"),
                s(r, "planted_revert"),
                r["reasons"]
                    .as_array()
                    .and_then(|a| a.first())
                    .and_then(Value::as_str)
                    .map(|x| format!(": {x}"))
                    .unwrap_or_default()
            )
        },
    );
    let review_usd = review
        .and_then(|r| r["numbers"]["cost_usd"].as_f64())
        .unwrap_or(0.0);
    let build_usd = j["cost_usd"].as_f64().unwrap_or(0.0);
    format!(
        "Theseus changed itself: self branch {branch} ({item}) joined main as {short} and is \
         installed and running.\nWhat: {what}\nWhy: {why}\nReview: {reviewed}\nNumbers: main's gate \
         on the merge {gate}, {secs} s; {tries} try(s)\nCost: ${cost:.4} (build ${build_usd:.4}, \
         review ${review_usd:.4})\nRevert: `theseus self revert {merge}`",
        branch = s(j, "branch"),
        item = s(j, "item"),
        short = &merge[..merge.len().min(12)],
        what = s(j, "what"),
        why = s(j, "why"),
        gate = s(j, "gate"),
        secs = j["numbers"]["gate_secs"].as_u64().unwrap_or(0),
        tries = j["numbers"]["tries"].as_u64().unwrap_or(1),
        cost = build_usd + review_usd,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The unit outlives the daemon's restart: a transient user unit runs
    /// the script, which builds as the install does and restarts last.
    #[test]
    fn the_install_is_a_transient_user_unit_that_restarts_last() {
        let a = argv("theseus-self-install-abc", Path::new("/s/install-abc.sh"));
        assert_eq!(&a[..2], ["systemd-run", "--user"]);
        assert!(a.contains(&"--unit=theseus-self-install-abc".to_string()));
        let s = script(Path::new("/t/it's"), Path::new("/t/target"), "abc123");
        assert!(s.contains("cd '/t/it'\\''s'\n"), "{s}");
        assert!(s.contains("scripts/build.sh --profile release-thin\n"));
        assert!(s.contains("THESEUS_COMMIT=abc123"));
        assert!(s
            .trim_end()
            .ends_with(&format!("systemctl --user restart {SERVICE}")));
        let swap = s.find("mv -f").unwrap();
        assert!(
            swap < s.find("systemctl").unwrap(),
            "copy-then-rename, then the restart"
        );
    }
}
