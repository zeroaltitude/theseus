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
//!
//! The copy goes where the service looks, and only where the loop can write
//! (issue 20): [`layout`] reads where the service's program is. Where the
//! service runs a root-owned build the install copies nothing and restarts
//! nothing ([`Started::NotInstalled`]; `verify` tells the owner the one step).
//! Where it installs, the unit leaves a marker naming the merge before the
//! restart, and the new daemon checks its own commit against it (`verify`).

use std::path::Path;

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

use super::git;
use super::layout::{self, Layout};
use super::verify;
use crate::fact::rsi_install::SelfInstallVerified;
use crate::fact::rsi_join::SelfInstalled;
use crate::rpc::Core;
use crate::rsi::jail::Job;
use crate::rsi::tree::{self, Place, Tree};

/// The binaries an install swaps in (`scripts/build.sh --installed`):
/// theseus-sim is built, never installed (theseus-rwpbg.5).
pub const BINARIES: &[&str] = &["theseusd", "theseus", "theseus-tui", "theseus-index"];
/// The service the restart names (`scripts/user-service.sh`'s default unit).
pub const SERVICE: &str = "theseusd.service";

/// What starting an install came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Started {
    /// The unit that copies the programs in and restarts the service; its name.
    Unit(String),
    /// Nothing was copied and nothing restarted.
    NotInstalled(NotInstalled),
}

/// Why a build was not installed, and the one step that installs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotInstalled {
    /// Why, as the owner reads it.
    pub why: String,
    /// Where the built programs wait.
    pub target: std::path::PathBuf,
    /// The one command that installs them.
    pub step: String,
}

/// Starts an install of a merge.
pub trait Installer: Send + Sync {
    fn start(&self, p: &Place, merge: &str) -> Result<Started>;
}

/// The script the unit runs: the L0 half of an install, which touches no
/// file of the repository but the binaries the L1 build made. Copy-then-
/// rename each into `prefix` (where the service's program is), leave the
/// marker that names the merge (`verify`), then restart. No `cd`, no
/// `scripts/` of the tree, nothing it could have changed.
pub fn script(target: &Path, prefix: &Path, marker: &Path, merge: &str) -> String {
    format!(
        "#!/bin/bash\n# A self-join's install (theseus-pw1q.9) of {merge}: the copy and the \
         restart. The build ran in L1.\nset -euo pipefail\ntarget={target}\n\
         prefix={prefix}\nmarker={marker}\nfor b in {names}; do\n  \
         cp \"$target/release/$b\" \"$prefix/.$b.new\" && mv -f \"$prefix/.$b.new\" \"$prefix/$b\"\n\
         done\nprintf '%s\\n' {merge} > \"$marker.new\" && mv -f \"$marker.new\" \"$marker\"\n\
         systemctl --user restart {SERVICE}\n",
        target = shell(&target.to_string_lossy()),
        prefix = shell(&prefix.to_string_lossy()),
        marker = shell(&marker.to_string_lossy()),
        merge = shell(merge),
        names = BINARIES.join(" "),
    )
}

/// The build, in L1: `scripts/build.sh --profile release` in the merged
/// tree, into its build tree, as the install always built it, at nice 19.
pub(super) fn build(p: &Place, tree: &Tree, merge: &str) -> Result<()> {
    let jail = p
        .jail()?
        .context("an install builds in L1, and this place has none")?;
    // The merge's own lock files, whose crates the owner has just agreed to, are fetched the same
    // way the merge's gate had them (`deps`).
    let _packages = crate::rsi::deps::provide_blocking(p, tree)?;
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

/// The build an install runs in its tree: [`build`] in the machine's own,
/// a stand-in in a test's (the tree's own lifecycle is the same either way).
pub type Build = dyn Fn(&Place, &Tree, &str) -> Result<()> + Send + Sync;

/// The install's tree, and its build tree's path: a detached worktree at
/// `merge`, built in L1 by `build`, and **closed again as soon as the build
/// tree holds the binaries**, so a join leaves no tree open.
///
/// The close is this function's own, as every other detached tree's is
/// (`join::git`'s doc: each "is removed by `rsi::tree::close`"). The
/// install's was the one exception, and nothing else could stand in for it:
/// the tree is named for the merge (`install-<short>`), not for a backlog
/// item, so the pipeline's close pass — which walks the task records
/// (`rsi::pipeline::queues`'s `closed`) — can never own it, while
/// `rsi::capacity::trees_open` counts every directory under `self/trees/`.
/// One left behind therefore held the single pipeline slot for good
/// (2026-11-12: 102 MB open 15h42m, every later start on `hold=slots`).
/// Widening the reaper instead would have had it race this very build,
/// which may run for [`BUILD_SILENCE`].
///
/// The build tree stays — the unit's L0 half ([`script`]) reads `release/`
/// out of it after this returns and across the restart — and a build tree is
/// no open tree, since only `self/trees/` is counted.
///
/// A failed build closes the tree too: its log is written to the gates dir,
/// not into the tree, so there is nothing left in there to read.
pub fn built_tree(p: &Place, merge: &str, build: &Build) -> Result<std::path::PathBuf> {
    let short = &merge[..merge.len().min(12)];
    let t = git::detached(p, "install", short, merge)?;
    let built = build(p, &t, merge);
    if let Err(e) = tree::close_keeping_target(p, &t) {
        tracing::warn!(
            tree = %t.dir.display(),
            error = %format!("{e:#}"),
            "self-install: the tree was not closed"
        );
    }
    if built.is_err() {
        // A build that failed leaves nothing the unit will read, and gigabytes of build
        // output: finished with its tree, so reaped now (issue 6, the install's reap).
        reap_target(p, merge);
    }
    built?;
    Ok(t.target)
}

/// The build tree of the install of `merge`, where the unit reads `release/` from.
pub fn target_of(p: &Place, merge: &str) -> std::path::PathBuf {
    let short = &merge[..merge.len().min(12)];
    p.targets().join(git::slug_of("install", short))
}

/// Remove the install's build tree (about 2 GB). Called only where the install is provably
/// finished with it: its build failed, or the new daemon has written `self.installed`, which
/// it writes after the unit copied the binaries in and restarted. Regenerable, so deleted
/// outright. Whether one was removed.
pub fn reap_target(p: &Place, merge: &str) -> bool {
    let target = target_of(p, merge);
    if !target.exists() {
        return false;
    }
    match crate::rsi::tree::remove_all(&target) {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!(
                tree = %target.display(),
                error = %format!("{e:#}"),
                "self-install: the build tree was not reaped"
            );
            false
        }
    }
}

/// The machine's own: the layout it reads, a tree at the merge, the script, and its unit.
pub struct Systemd;

impl Installer for Systemd {
    fn start(&self, p: &Place, merge: &str) -> Result<Started> {
        start_with(p, merge, &layout::Machine, &build, &run_unit)
    }
}

/// An install of `merge` on the machine `host` describes: the layout is read first, the build
/// is made in either case (it is what the owner's step installs), and the unit is started by
/// `run` only where the loop can write the service's directory.
pub(super) fn start_with(
    p: &Place,
    merge: &str,
    host: &dyn layout::Host,
    build: &Build,
    run: &dyn Fn(&[String]) -> Result<()>,
) -> Result<Started> {
    let layout = layout::read(host);
    let target = built_tree(p, merge, build)?;
    let dir = match layout {
        Layout::User { dir } => dir,
        Layout::Rooted { program } => {
            return Ok(Started::NotInstalled(NotInstalled {
                why: format!(
                    "this machine's service runs {}, a build kept in a directory only an \
                     administrator can write, which Theseus cannot replace",
                    program.display()
                ),
                step: layout::step(&program, &target, &p.repo, merge),
                target,
            }));
        }
        Layout::Unknown { why } => {
            return Ok(Started::NotInstalled(NotInstalled {
                why: format!(
                    "Theseus could not tell where this machine's service keeps its program \
                     ({why}), so it does not know where the new build would go"
                ),
                step: layout::unknown_step(&target),
                target,
            }));
        }
    };
    let short = &merge[..merge.len().min(12)];
    let script_path = p.root().join(format!("install-{short}.sh"));
    std::fs::write(
        &script_path,
        script(&target, &dir, &verify::marker_of(p), merge),
    )?;
    let unit = format!("theseus-self-install-{short}");
    run(&argv(&unit, &script_path))?;
    Ok(Started::Unit(unit))
}

/// Hand `argv` (a `systemd-run`) to the machine.
fn run_unit(a: &[String]) -> Result<()> {
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
    Ok(())
}

impl Core {
    /// Once serving: the install's check against the marker the unit left, then the notice for
    /// the join this build installs, if it has none yet (the module's doc). Nothing with the mode
    /// off.
    pub fn self_installed_after_serving(&self) -> Result<Option<String>> {
        // An install that finished in an earlier run left its build tree: clear it.
        self.reap_installed_targets();
        self.self_installed_running(crate::build().commit.as_deref())
    }

    /// The same, for a build that names `running` as its commit: a marker naming another commit
    /// is a mismatch, told once; one naming this commit is checked off with the notice.
    pub fn self_installed_running(&self, running: Option<&str>) -> Result<Option<String>> {
        if let Some(merge) = self.self_install_mismatch(running)? {
            return Ok(Some(merge));
        }
        match running {
            Some(c) => self.self_installed_as(c),
            None => Ok(None),
        }
    }

    /// The same, for a build of `commit`: the merge noticed, if any. A marker that names `commit`
    /// is the install's own: the build is the one installed, which the row says (`verified`), and
    /// the marker is spent.
    pub fn self_installed_as(&self, commit: &str) -> Result<Option<String>> {
        if self.cfg.self_improve.mode != theseus_protocol::rsi::SelfMode::Act {
            return Ok(None);
        }
        // An install into the root-owned directory is "installed and running" only once the new daemon has
        // confirmed itself (`root_install::verify`), which calls this again.
        if self.root_install_pending_for(commit) {
            return Ok(None);
        }
        let verified = self.self_install_marked(commit);
        let told = self.self_installed_row(commit, verified)?;
        if verified {
            self.self_install_spent();
        }
        Ok(told)
    }

    fn self_installed_row(&self, commit: &str, verified: bool) -> Result<Option<String>> {
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
        let checked = SelfInstallVerified {
            merge: commit.into(),
            branch,
        };
        if verified {
            frame.push(rec.row(&checked)?);
        }
        self.store.append(&frame)?;
        self.outbox.posted(&post);
        rec.announce(&fact);
        if verified {
            rec.announce(&checked);
        }
        self.reap_installed_targets();
        Ok(Some(commit.into()))
    }

    /// Reap the build tree of every install whose `self.installed` row is written: the new
    /// daemon wrote it after the unit copied the binaries in, so nothing reads that tree
    /// again. Run when the daemon serves and as each notice is written, so an install that
    /// finished earlier and left its tree is cleared too. How many were removed.
    pub fn reap_installed_targets(&self) -> usize {
        let Some(repo) = self.cfg.self_improve.repo.clone() else {
            return 0;
        };
        let place = self.tools.self_pr.place(&repo);
        let Ok(Some(rows)) = super::rows_of(&self.store, &["self.installed"]) else {
            return 0;
        };
        rows.iter()
            .filter_map(|r| r.data["merge"].as_str())
            .filter(|merge| reap_target(&place, merge))
            .count()
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
         on the merge {gate}, {secs} s; {tries} try(s){lines}\nCost: ${cost:.4} (build ${build_usd:.4}, \
         review ${review_usd:.4})\nRevert: `theseus self revert {merge}`",
        what = s(j, "what"),
        why = s(j, "why"),
        gate = s(j, "gate"),
        secs = j["numbers"]["gate_secs"].as_u64().unwrap_or(0),
        tries = j["numbers"]["tries"].as_u64().unwrap_or(1),
        // The change in lines, from the counter STATS.md uses (issue 45), when the join had it.
        lines = j["numbers"]["lines"]
            .as_str()
            .map(|l| format!("\nLines: {l}"))
            .unwrap_or_default(),
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
        let s = script(
            Path::new("/t/it's/target"),
            Path::new("/p/bin"),
            Path::new("/s/install-pending"),
            "abc123",
        );
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
        let s = script(
            Path::new("/t/target"),
            Path::new("/p/bin"),
            Path::new("/s/install-pending"),
            "abc123",
        );
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
        let marker = root.join("install-pending");
        std::fs::write(
            &path,
            script(
                &root.join("target"),
                &home.join(".local/bin"),
                &marker,
                "abc123",
            ),
        )
        .unwrap();
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
        assert_eq!(
            std::fs::read_to_string(&marker).unwrap(),
            "abc123\n",
            "the marker names the merge the restart was for"
        );
    }
}
