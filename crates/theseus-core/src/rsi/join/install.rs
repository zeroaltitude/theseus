//! The install and the notice (theseus-pw1q.9). Only with `[self] install
//! = true` (the owner's standing OK, `false` by default) and only on the
//! owner's machine. The joined merge is checked out in a tree of its own and
//! **built in L1** (theseus-pw1q.21.1: `scripts/build.sh` is the merged
//! branch's code, so it runs below the same floor as its gate, with the tree
//! and its build tree the only writable places and no network). The L0 half
//! then runs no repository code: [`script`] only copies each shipped binary in
//! by copy-then-rename and restarts the daemon, in a transient `systemd-run
//! --user` unit, since the daemon must not kill itself mid-step: the unit
//! outlives the restart it makes.
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
use crate::rsi::jail::Job;
use crate::rsi::tree::{Place, Tree};

/// The binaries an install swaps in (`scripts/build.sh --installed`):
/// theseus-sim is built, never installed (theseus-rwpbg.5).
pub const BINARIES: &[&str] = &["theseusd", "theseus", "theseus-tui", "theseus-index"];
/// The service the restart names (`scripts/user-service.sh`'s default unit).
pub const SERVICE: &str = "theseusd.service";

/// Starts an install of a merge; the unit's name.
pub trait Installer: Send + Sync {
    fn start(&self, p: &Place, merge: &str) -> Result<String>;
}

/// The script the unit runs: the L0 half of an install, which touches no
/// file of the repository but the binaries the L1 build made. Copy-then-
/// rename each into the prefix, then restart. No `cd`, no `scripts/` of the
/// tree, nothing it could have changed.
pub fn script(target: &Path, merge: &str) -> String {
    format!(
        "#!/bin/bash\n# A self-join's install (theseus-pw1q.9) of {merge}: the copy and the \
         restart. The build ran in L1.\nset -euo pipefail\ntarget={target}\n\
         prefix=\"$HOME/.local/bin\"\nfor b in {names}; do\n  \
         cp \"$target/release/$b\" \"$prefix/.$b.new\" && mv -f \"$prefix/.$b.new\" \"$prefix/$b\"\n\
         done\nsystemctl --user restart {SERVICE}\n",
        target = shell(&target.to_string_lossy()),
        names = BINARIES.join(" "),
    )
}

/// The build, in L1: `scripts/build.sh --profile release` in the merged
/// tree, into its build tree, as the install always built it, at nice 19.
fn build(p: &Place, tree: &Tree, merge: &str) -> Result<()> {
    let jail = p
        .jail()?
        .context("an install builds in L1, and this place has none")?;
    std::fs::create_dir_all(p.gates())?;
    let log = p.gates().join(format!("install-{}.log", tree.slug));
    let argv = [
        "nice",
        "-n",
        "19",
        "scripts/build.sh",
        "--profile",
        "release",
    ]
    .map(Into::into)
    .to_vec();
    let code = jail.run_blocking(
        p,
        &Job {
            tree,
            ro: Vec::new(),
            argv,
            env: vec![("THESEUS_COMMIT".into(), merge.into())],
            log: &log,
            limit: BUILD_SILENCE,
        },
    )?;
    if code != Some(0) {
        let text = std::fs::read_to_string(&log).unwrap_or_default();
        let tail: Vec<&str> = text.lines().rev().take(5).collect();
        bail!(
            "the install's build in L1 failed (exit {code:?}); its log, {}, ends: {}",
            log.display(),
            tail.into_iter().rev().collect::<Vec<_>>().join(" | ")
        );
    }
    Ok(())
}

/// The longest an install's build may take.
const BUILD_SILENCE: std::time::Duration = std::time::Duration::from_secs(2 * 3600);

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
        build(p, &t, merge)?;
        let script_path = p.root().join(format!("install-{short}.sh"));
        std::fs::write(&script_path, script(&t.target, merge))?;
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
pub(crate) fn notice(j: &Value, review: Option<&Value>, merge: &str) -> String {
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
    let trigger = format!(
        "self branch {} ({}) joined main as {} and is installed and running",
        s(j, "branch"),
        s(j, "item"),
        &merge[..merge.len().min(12)]
    );
    let head = crate::owner_note::head(
        crate::owner_note::Kind::Fyi,
        "Theseus changed itself",
        &trigger,
    );
    format!(
        "{head}\nWhat: {what}\nWhy: {why}\nReview: {reviewed}\nNumbers: main's gate \
         on the merge {gate}, {secs} s; {tries} try(s)\nCost: ${cost:.4} (build ${build_usd:.4}, \
         review ${review_usd:.4})\nRevert: `theseus self revert {merge}`",
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
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    /// The L0 half copies what the owner's install copies: the list
    /// `scripts/build.sh --installed` prints, in its order.
    #[test]
    fn the_binaries_are_what_build_sh_installs() {
        let build = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/build.sh");
        let out = std::process::Command::new("bash")
            .arg(&build)
            .arg("--installed")
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        let listed: Vec<String> = String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect();
        assert_eq!(listed, BINARIES);
    }

    /// The unit outlives the daemon's restart: a transient user unit runs
    /// the script, which restarts last.
    #[test]
    fn the_install_is_a_transient_user_unit_that_restarts_last() {
        let a = argv("theseus-self-install-abc", Path::new("/s/install-abc.sh"));
        assert_eq!(&a[..2], ["systemd-run", "--user"]);
        assert!(a.contains(&"--unit=theseus-self-install-abc".to_string()));
        let s = script(Path::new("/t/it's/target"), "abc123");
        assert!(s.contains("target='/t/it'\\''s/target'\n"), "{s}");
        assert!(s
            .trim_end()
            .ends_with(&format!("systemctl --user restart {SERVICE}")));
        let swap = s.find("mv -f").unwrap();
        assert!(
            swap < s.find("systemctl").unwrap(),
            "copy-then-rename, then the restart"
        );
    }

    /// The L0 half runs nothing of the repository (theseus-pw1q.21.1): against
    /// a fake build output it copies each binary in, renames it into place and
    /// restarts the service, and the only programs it runs are `cp`, `mv` and
    /// `systemctl`. It names no `cd`, no `scripts/`, no cargo.
    #[test]
    fn the_l0_half_only_copies_and_restarts() {
        let s = script(Path::new("/t/target"), "abc123");
        for forbidden in ["cd ", "scripts/", "cargo", "build.sh", "bash -c", "eval"] {
            assert!(!s.contains(forbidden), "{forbidden:?} in the L0 half:\n{s}");
        }
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let out = root.join("target/release");
        let (home, bin) = (root.join("home"), root.join("bin"));
        for d in [&out, &home.join(".local/bin"), &bin] {
            std::fs::create_dir_all(d).unwrap();
        }
        for b in BINARIES {
            std::fs::write(out.join(b), format!("fake {b}")).unwrap();
            std::fs::write(home.join(".local/bin").join(b), "old").unwrap();
        }
        // A stand-in systemctl that records its arguments.
        let calls = root.join("calls");
        let ctl = bin.join("systemctl");
        std::fs::write(
            &ctl,
            format!("#!/bin/sh\necho \"$@\" >> {}\n", calls.display()),
        )
        .unwrap();
        std::fs::set_permissions(&ctl, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = root.join("install.sh");
        std::fs::write(&path, script(&root.join("target"), "abc123")).unwrap();
        let st = std::process::Command::new("/bin/bash")
            .arg(&path)
            .env_clear()
            .env("HOME", &home)
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .status()
            .unwrap();
        assert!(st.success());
        for b in BINARIES {
            let got = std::fs::read_to_string(home.join(".local/bin").join(b)).unwrap();
            assert_eq!(got, format!("fake {b}"), "{b} is the build's");
            assert!(!home.join(".local/bin").join(format!(".{b}.new")).exists());
        }
        assert_eq!(
            std::fs::read_to_string(&calls).unwrap(),
            format!("--user restart {SERVICE}\n"),
            "the one call: the restart"
        );
    }
}
