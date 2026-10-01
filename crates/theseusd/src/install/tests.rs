//! `theseusd install` in a temp root, with fake accounts (theseus-7hh):
//! the plans as golden text, `--apply` then `--check`, a second `--apply`
//! that changes nothing, the layout's owners and modes read back, the
//! refusals, and `--remove`, with and without `--purge-state`.
//!
//! The golden files were written by hand from the design (M4 §2.9), so
//! they check the code rather than echo it. On a difference, the test
//! writes what this build renders under the temp dir, to diff against.

use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::{Path, PathBuf};

use theseus_store::Store as _;

use super::host::{self, Fake, Host, User};
use super::layout::{real, unit_arg, unit_env};
use super::*;

const OPERATOR: &str = "ada";
const SOURCE: &str = "op://Example/theseus-config/notesPlain";
const STAMP: &str = "20261001T120000Z";

/// A temp dir: `root/` is the machine, `build/theseusd` the binary,
/// `old/` an old daemon's state dir, and `ada/` the operator's files.
struct Rig {
    tmp: tempfile::TempDir,
    host: Fake,
    env: Env,
    g: Globals,
}

fn vars(kv: &[(&str, &str)]) -> std::collections::BTreeMap<String, String> {
    kv.iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn args(f: impl FnOnce(&mut InstallArgs)) -> InstallArgs {
    let mut a = InstallArgs::default();
    f(&mut a);
    a
}

fn separate(f: impl FnOnce(&mut InstallArgs)) -> InstallArgs {
    args(|a| {
        a.separate = true;
        f(a);
    })
}

impl Rig {
    fn new(env: impl FnOnce(&Path) -> Env, g: Globals) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("root")).unwrap();
        let env = env(tmp.path());
        Self {
            tmp,
            host: Fake::new(OPERATOR, 1000),
            env,
            g,
        }
    }

    /// `sudo theseusd install --separate …`, by ada.
    fn separate() -> Self {
        let r = Self::new(
            |t| Env {
                root: t.join("root"),
                exe: t.join("build/theseusd"),
                cwd: "/home/ada".into(),
                euid: 0,
                ruid: 0,
                vars: vars(&[
                    ("SUDO_UID", "1000"),
                    ("SUDO_USER", OPERATOR),
                    ("HOME", "/root"),
                    (
                        "PATH",
                        "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
                    ),
                ]),
                stamp: STAMP.into(),
            },
            Globals {
                config: SOURCE.into(),
                ..Default::default()
            },
        );
        std::fs::create_dir(r.tmp.path().join("build")).unwrap();
        std::fs::write(&r.env.exe, b"#!/bin/false\nthe binary, as a test has it\n").unwrap();
        r
    }

    /// `theseusd install --user`, by ada, from her shell.
    fn user() -> Self {
        Self::new(
            |t| Env {
                root: t.join("root"),
                exe: "/opt/theseus/bin/theseusd".into(),
                cwd: "/home/ada".into(),
                euid: 1000,
                ruid: 1000,
                vars: vars(&[
                    ("HOME", "/home/ada"),
                    ("PATH", "/home/ada/.local/bin:/usr/bin:/bin"),
                    ("LANG", "C.UTF-8"),
                ]),
                stamp: STAMP.into(),
            },
            Globals {
                config: SOURCE.into(),
                op_token_file: Some("~/.config/theseus/op-token".into()),
                ..Default::default()
            },
        )
    }

    /// The exit status and the output, the temp dir shown as `$TMP`.
    fn run(&mut self, a: &InstallArgs) -> Result<(i32, String)> {
        let mut out = vec![];
        let code = run_with(a, &self.g, &self.env, &mut self.host, &mut out)?;
        Ok((code, self.clean(&String::from_utf8_lossy(&out))))
    }

    fn ok(&mut self, a: &InstallArgs) -> String {
        match self.run(a) {
            Ok((0, out)) => out,
            Ok((code, out)) => panic!("exit {code}:\n{out}"),
            Err(e) => panic!("{e:#}"),
        }
    }

    fn err(&mut self, a: &InstallArgs) -> String {
        match self.run(a) {
            Ok((code, out)) => panic!("succeeded (exit {code}):\n{out}"),
            Err(e) => self.clean(&format!("{e:#}")),
        }
    }

    fn clean(&self, s: &str) -> String {
        s.replace(&self.tmp.path().display().to_string(), "$TMP")
    }

    fn at(&self, p: &str) -> PathBuf {
        real(&self.env.root, Path::new(p))
    }

    fn mode(&self, p: &str) -> u32 {
        std::fs::symlink_metadata(self.at(p))
            .unwrap()
            .permissions()
            .mode()
            & 0o7777
    }

    fn owner(&self, p: &str) -> String {
        let (uid, gid) = self.host.owner(&self.at(p)).unwrap();
        let u = self
            .host
            .user_by_uid(uid)
            .unwrap()
            .map_or(uid.to_string(), |u| u.name);
        let g = self
            .host
            .group_by_gid(gid)
            .unwrap()
            .map_or(gid.to_string(), |g| g.name);
        format!("{u}:{g}")
    }

    /// A stopped daemon's state dir: a store with a record, a spool with a
    /// result, the config's copy, and the bindings.
    fn old_state(&self) -> PathBuf {
        let old = self.tmp.path().join("old");
        let s = theseus_store::WalStore::open(&old.join("store"), Default::default()).unwrap();
        s.append(&[theseus_store::NewRecord::json(
            theseus_store::kinds::SESSION,
            Some("s1"),
            &"one",
        )
        .unwrap()])
            .unwrap();
        s.checkpoint().unwrap();
        drop(s);
        std::fs::create_dir_all(old.join("spool")).unwrap();
        std::fs::write(old.join("spool/job-1.result"), b"a job's result\n").unwrap();
        std::fs::write(
            old.join("config.last-good.toml"),
            b"# the note's last good copy\n",
        )
        .unwrap();
        std::fs::write(old.join("bindings.toml"), b"# bindings\n").unwrap();
        old
    }

    fn user_named(&self, name: &str) -> Option<User> {
        self.host.user(name).unwrap()
    }

    fn members(&self, group: &str) -> Option<Vec<String>> {
        self.host.group(group).unwrap().map(|g| g.members)
    }
}

/// Every entry under `top`: its path, mode, and bytes (a file's).
fn tree(top: &Path) -> Vec<(String, u32, Option<Vec<u8>>)> {
    let mut all = vec![];
    let mut stack = vec![top.to_path_buf()];
    while let Some(p) = stack.pop() {
        for e in std::fs::read_dir(&p).unwrap() {
            let e = e.unwrap();
            let m = std::fs::symlink_metadata(e.path()).unwrap();
            let rel = e.path().strip_prefix(top).unwrap().display().to_string();
            let mode = m.permissions().mode() & 0o7777;
            if m.is_dir() {
                stack.push(e.path());
                all.push((rel, mode, None));
            } else if m.is_file() {
                all.push((rel, mode, Some(std::fs::read(e.path()).unwrap())));
            } else {
                all.push((rel, mode, None));
            }
        }
    }
    all.sort();
    all
}

fn same(name: &str, got: &str, want: &str) {
    if got != want {
        let dir = std::env::temp_dir().join("theseus-install-golden");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, got).unwrap();
        panic!(
            "{name}: this build renders something else, written to {}",
            path.display()
        );
    }
}

/// Every path the separated layout makes.
const LAYOUT: [&str; 11] = [
    "/var/lib/theseus",
    "/etc/theseus",
    "/etc/theseus/op-token",
    "/etc/theseus/theseus.toml",
    "/usr/local/lib/theseus",
    "/usr/local/lib/theseus/theseusd",
    "/etc/systemd/system/theseusd.service",
    "/etc/systemd/user/theseus-job-host.service",
    "/etc/tmpfiles.d/theseus.conf",
    "/run/theseus",
    "/home/ada/.local/state/theseus",
];

#[test]
fn install_with_no_mode_lists_the_modes_and_what_each_needs() {
    let mut r = Rig::user();
    assert_eq!(r.ok(&args(|_| {})), OVERVIEW);
    assert!(OVERVIEW.contains("--user") && OVERVIEW.contains("Needs nothing but you"));
    assert!(OVERVIEW.contains("--separate") && OVERVIEW.contains("Needs root: run it with sudo"));
    let e = r.err(&args(|a| a.apply = true));
    assert!(e.contains("name a mode first"), "{e}");
}

#[test]
fn the_user_plan_is_golden_and_changes_nothing() {
    let mut r = Rig::user();
    let out = r.ok(&args(|a| a.user = true));
    same("user.plan", &out, include_str!("golden/user.plan"));
    assert_eq!(tree(&r.env.root), vec![], "a plan wrote");
}

#[test]
fn the_separate_plan_is_golden_and_changes_nothing() {
    let mut r = Rig::separate();
    let out = r.ok(&separate(|_| {}));
    same("separate.plan", &out, include_str!("golden/separate.plan"));
    assert_eq!(tree(&r.env.root), vec![], "a plan wrote");
    assert!(
        r.host.calls.is_empty(),
        "a plan changed an account: {:?}",
        r.host.calls
    );
}

#[test]
fn the_separate_plan_with_a_config_file_and_a_migration_is_golden() {
    let mut r = Rig::separate();
    let old = r.old_state();
    let before = tree(&old);
    let ada = r.tmp.path().join("ada");
    std::fs::create_dir(&ada).unwrap();
    std::fs::write(
        ada.join("theseus.toml"),
        b"[server]\nstate_dir = \"~/.theseus\"\n",
    )
    .unwrap();
    r.g.config = ada.join("theseus.toml").display().to_string();
    r.g.op_token_file = Some(ada.join("op-token").display().to_string());
    let out = r.ok(&separate(|a| a.migrate_state = Some(old.clone())));
    same(
        "separate-migrate.plan",
        &out,
        include_str!("golden/separate-migrate.plan"),
    );
    assert_eq!(tree(&r.env.root), vec![], "a plan wrote");
    assert_eq!(tree(&old), before, "a plan touched the old state");
}

#[test]
fn the_separate_remove_plan_after_an_apply_is_golden() {
    let mut r = Rig::separate();
    r.ok(&separate(|a| a.apply = true));
    let out = r.ok(&separate(|a| a.remove = true));
    same(
        "separate-remove.plan",
        &out,
        include_str!("golden/separate-remove.plan"),
    );
}

#[test]
fn a_separate_apply_checks_clean_and_a_second_apply_changes_nothing() {
    let mut r = Rig::separate();
    let old = r.old_state();
    let before_old = tree(&old);
    let apply = separate(|a| {
        a.apply = true;
        a.migrate_state = Some(old.clone());
    });
    let log = r.ok(&apply);
    assert!(log.contains("Done: 13 change(s)."), "{log}");
    for line in [
        "created group theseus-ops",
        "created user theseus (system, its own group, home /var/lib/theseus, shell /usr/sbin/nologin)",
        "added ada to group theseus-ops",
        "created dir /var/lib/theseus (theseus:theseus 0700)",
        "created /etc/theseus/op-token empty (theseus:theseus 0600)",
        "installed /usr/local/lib/theseus/theseusd from $TMP/build/theseusd (root:root 0755)",
        "wrote /etc/systemd/system/theseusd.service (root:root 0644)",
        "copied $TMP/old/store to /var/lib/theseus/store",
        "$TMP/old was only read: nothing in it changed",
    ] {
        assert!(log.contains(line), "{line:?} missing from the log:\n{log}");
    }
    let check = separate(|a| {
        a.check = true;
        a.migrate_state = Some(old.clone());
    });
    assert_eq!(
        r.ok(&check),
        "theseusd install --separate: the machine matches the layout.\n"
    );
    let (calls, owners, files) = (
        r.host.calls.clone(),
        r.host.owners.clone(),
        tree(&r.env.root),
    );
    let again = r.ok(&apply);
    assert!(
        again.contains("Nothing to do: the machine matches the layout."),
        "{again}"
    );
    assert_eq!(r.host.calls, calls, "a second apply changed an account");
    assert_eq!(r.host.owners, owners, "a second apply changed an owner");
    assert_eq!(tree(&r.env.root), files, "a second apply changed a file");
    let plan = r.ok(&separate(|a| a.migrate_state = Some(old.clone())));
    assert!(
        plan.contains("Nothing to do: the machine matches the layout."),
        "{plan}"
    );
    assert_eq!(tree(&old), before_old, "the old state dir changed");
}

#[test]
fn the_layout_reads_back_with_its_owners_and_modes() {
    let mut r = Rig::separate();
    let old = r.old_state();
    r.ok(&separate(|a| {
        a.apply = true;
        a.migrate_state = Some(old.clone());
    }));
    for (path, owner, mode) in [
        ("/var/lib/theseus", "theseus:theseus", 0o700),
        ("/etc/theseus", "root:theseus", 0o750),
        ("/etc/theseus/op-token", "theseus:theseus", 0o600),
        ("/usr/local/lib/theseus", "root:root", 0o755),
        ("/usr/local/lib/theseus/theseusd", "root:root", 0o755),
        ("/etc/systemd/system/theseusd.service", "root:root", 0o644),
        (
            "/etc/systemd/user/theseus-job-host.service",
            "root:root",
            0o644,
        ),
        ("/etc/tmpfiles.d/theseus.conf", "root:root", 0o644),
        ("/run/theseus", "theseus:theseus-ops", 0o750),
        ("/var/lib/theseus/store", "theseus:theseus", 0o700),
        (
            "/var/lib/theseus/store/MANIFEST.json",
            "theseus:theseus",
            0o600,
        ),
        ("/var/lib/theseus/store/wal", "theseus:theseus", 0o700),
        (
            "/var/lib/theseus/spool/job-1.result",
            "theseus:theseus",
            0o600,
        ),
        (
            "/var/lib/theseus/config.last-good.toml",
            "theseus:theseus",
            0o600,
        ),
        ("/var/lib/theseus/bindings.toml", "theseus:theseus", 0o600),
        ("/etc/systemd", "root:root", 0o755),
    ] {
        assert_eq!(
            (r.owner(path), r.mode(path)),
            (owner.to_string(), mode),
            "{path}"
        );
    }
    assert_eq!(
        std::fs::metadata(r.at("/etc/theseus/op-token"))
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        std::fs::read(r.at("/usr/local/lib/theseus/theseusd")).unwrap(),
        std::fs::read(&r.env.exe).unwrap()
    );
    let u = r.user_named("theseus").unwrap();
    assert_eq!(
        (u.home.as_path(), u.shell.as_path()),
        (
            Path::new("/var/lib/theseus"),
            Path::new("/usr/sbin/nologin")
        )
    );
    assert!(u.uid < 1000, "not a system user: {u:?}");
    assert_eq!(r.members("theseus-ops"), Some(vec![OPERATOR.to_string()]));
    // The copy is a store this build opens, with its record.
    let s =
        theseus_store::WalStore::open(&r.at("/var/lib/theseus/store"), Default::default()).unwrap();
    let rec = s
        .latest_by_key(theseus_store::kinds::SESSION, "s1")
        .unwrap()
        .unwrap();
    assert_eq!(rec.decode::<String>().unwrap(), "one");
    // The tmpfiles line makes /run/theseus as the layout has it.
    assert!(
        std::fs::read_to_string(r.at("/etc/tmpfiles.d/theseus.conf"))
            .unwrap()
            .contains("d /run/theseus 0750 theseus theseus-ops -")
    );
}

#[test]
fn check_names_a_wrong_mode_and_a_wrong_owner_and_apply_fixes_them() {
    let mut r = Rig::separate();
    r.ok(&separate(|a| a.apply = true));
    std::fs::set_permissions(
        r.at("/var/lib/theseus"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    r.host
        .owners
        .insert(r.at("/etc/theseus/op-token"), (1000, 1000));
    std::fs::write(
        r.at("/etc/tmpfiles.d/theseus.conf"),
        "d /run/theseus 0777 root root -\n",
    )
    .unwrap();
    let (code, out) = r.run(&separate(|a| a.check = true)).unwrap();
    assert_eq!(code, 1, "{out}");
    for line in [
        "theseusd install --separate: 3 difference(s) from the layout:",
        "  fix     dir    /var/lib/theseus theseus:theseus 0700\n                 mode 0755 -> 0700",
        "  fix     file   /etc/theseus/op-token theseus:theseus 0600 (empty)\n                 owner ada:ada -> theseus:theseus",
        "content: line 1 is `d /run/theseus 0777 root root -`, want `# Written by `theseusd install --separate`.`",
    ] {
        assert!(out.contains(line), "{line:?} missing from:\n{out}");
    }
    let log = r.ok(&separate(|a| a.apply = true));
    assert!(
        log.contains("fixed /var/lib/theseus: mode 0755 -> 0700"),
        "{log}"
    );
    assert!(
        log.contains("rewrote /etc/tmpfiles.d/theseus.conf"),
        "{log}"
    );
    assert_eq!(
        r.ok(&separate(|a| a.check = true)),
        "theseusd install --separate: the machine matches the layout.\n"
    );
}

#[test]
fn separate_refuses_to_apply_or_check_without_root_and_says_to_use_sudo() {
    // The real effective uid, as `run` reads it.
    let euid = host::euid().unwrap();
    if euid == 0 {
        eprintln!("skipped: this test runs as root, and must not");
        return;
    }
    let mut r = Rig::separate();
    // Ada, without sudo: the effective uid is the real one this test has.
    (r.env.euid, r.env.ruid) = (euid, 1000);
    for (a, says) in [
        (
            separate(|a| a.apply = true),
            "sudo $TMP/build/theseusd install --separate --apply",
        ),
        (
            separate(|a| a.check = true),
            "sudo $TMP/build/theseusd install --separate --check",
        ),
        (
            separate(|a| {
                a.remove = true;
                a.apply = true;
            }),
            "sudo $TMP/build/theseusd install --separate --remove --apply",
        ),
    ] {
        let e = r.err(&a);
        assert!(
            e.starts_with("`--separate` needs root: run it with sudo"),
            "{e}"
        );
        assert!(e.contains(says), "{says:?} missing from: {e}");
    }
    assert_eq!(tree(&r.env.root), vec![]);
    assert!(r.host.calls.is_empty());
    // The plan only reads, so it needs no root: the review reads it first.
    r.ok(&separate(|_| {}));
}

#[test]
fn a_plan_without_root_says_what_it_cannot_see() {
    if host::euid().unwrap() == 0 {
        eprintln!("skipped: root sees everything");
        return;
    }
    let mut r = Rig::separate();
    r.ok(&separate(|a| a.apply = true));
    let closed = std::fs::Permissions::from_mode(0o000);
    std::fs::set_permissions(r.at("/etc/theseus"), closed).unwrap();
    let planned = r.run(&separate(|_| {}));
    std::fs::set_permissions(r.at("/etc/theseus"), std::fs::Permissions::from_mode(0o750)).unwrap();
    let (code, out) = planned.unwrap();
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("  unseen  file   /etc/theseus/op-token theseus:theseus 0600 (empty)\n                 not visible without root ("),
        "{out}"
    );
    assert!(
        out.contains("1 not visible without root: run the plan with sudo to see them."),
        "{out}"
    );
}

#[test]
fn user_refuses_to_run_as_root() {
    let mut r = Rig::user();
    r.env.euid = 0;
    let e = r.err(&args(|a| a.user = true));
    assert!(e.contains("run it as yourself, not as root"), "{e}");
}

#[test]
fn a_migration_is_refused_while_a_daemon_answers_in_the_old_state_dir() {
    for at in ["theseus.sock", "spool/notify.sock"] {
        let mut r = Rig::separate();
        let old = r.old_state();
        let sock = old.join(at);
        let _daemon = std::os::unix::net::UnixListener::bind(&sock).unwrap();
        let (code, out) = r
            .run(&separate(|a| a.migrate_state = Some(old.clone())))
            .unwrap();
        assert_eq!(code, 1, "{out}");
        assert!(
            out.contains(&format!(
                "refused: a daemon answers on $TMP/old/{at}: stop it first"
            )),
            "{out}"
        );
        let e = r.err(&separate(|a| {
            a.apply = true;
            a.migrate_state = Some(old.clone());
        }));
        assert!(e.contains("nothing was changed: 1 refused"), "{e}");
        assert_eq!(tree(&r.env.root), vec![]);
        assert!(r.host.calls.is_empty());
    }
}

#[test]
fn a_socket_nothing_answers_on_is_not_a_daemon() {
    let mut r = Rig::separate();
    let old = r.old_state();
    drop(std::os::unix::net::UnixListener::bind(old.join("theseus.sock")).unwrap());
    let log = r.ok(&separate(|a| {
        a.apply = true;
        a.migrate_state = Some(old.clone());
    }));
    assert!(log.contains("copied $TMP/old/store"), "{log}");
}

#[test]
fn a_migration_is_refused_for_a_store_newer_than_this_build() {
    let mut r = Rig::separate();
    let old = r.old_state();
    std::fs::write(
        old.join("store/MANIFEST.json"),
        r#"{"format": 99, "engine": "redb"}"#,
    )
    .unwrap();
    let (code, out) = r
        .run(&separate(|a| a.migrate_state = Some(old.clone())))
        .unwrap();
    assert_eq!(code, 1);
    assert!(
        out.contains("is format 99") && out.contains("install the newer theseusd"),
        "{out}"
    );
}

#[test]
fn a_migration_never_overwrites_a_different_store_and_copies_nothing_through_a_symlink() {
    let mut r = Rig::separate();
    let old = r.old_state();
    r.ok(&separate(|a| {
        a.apply = true;
        a.migrate_state = Some(old.clone());
    }));
    std::fs::write(old.join("bindings.toml"), b"# bindings, changed since\n").unwrap();
    let (code, out) = r
        .run(&separate(|a| a.migrate_state = Some(old.clone())))
        .unwrap();
    assert_eq!(code, 1);
    assert!(
        out.contains("/var/lib/theseus/bindings.toml already holds something else: the installer never overwrites a store"),
        "{out}"
    );

    let mut r = Rig::separate();
    let old = r.old_state();
    std::fs::write(r.tmp.path().join("secret"), b"not the operator's to copy\n").unwrap();
    std::os::unix::fs::symlink(r.tmp.path().join("secret"), old.join("spool/planted")).unwrap();
    let (code, out) = r
        .run(&separate(|a| a.migrate_state = Some(old.clone())))
        .unwrap();
    assert_eq!(code, 1);
    assert!(
        out.contains("$TMP/old/spool/planted is not a regular file or a directory (a symlink"),
        "{out}"
    );
}

#[test]
fn remove_after_an_apply_leaves_no_layout_and_a_second_remove_changes_nothing() {
    let mut r = Rig::separate();
    r.ok(&separate(|a| a.apply = true));
    let log = r.ok(&separate(|a| {
        a.remove = true;
        a.apply = true;
    }));
    for line in [
        "removed /etc/systemd/system/theseusd.service",
        "removed dir /run/theseus",
        "removed /usr/local/lib/theseus/theseusd",
        "removed dir /usr/local/lib/theseus",
        "removed /etc/theseus/op-token",
        "removed dir /etc/theseus",
        "removed dir /var/lib/theseus",
        "removed ada from group theseus-ops",
        "removed user theseus",
        "removed group theseus-ops",
    ] {
        assert!(log.contains(line), "{line:?} missing from the log:\n{log}");
    }
    for p in LAYOUT {
        assert!(!r.at(p).exists(), "{p} is still there");
    }
    assert_eq!(r.user_named("theseus"), None);
    assert_eq!(r.members("theseus"), None);
    assert_eq!(r.members("theseus-ops"), None);
    let check = separate(|a| {
        a.remove = true;
        a.check = true;
    });
    assert_eq!(
        r.ok(&check),
        "theseusd install --separate --remove: the machine matches the layout.\n"
    );
    let calls = r.host.calls.clone();
    let again = r.ok(&separate(|a| {
        a.remove = true;
        a.apply = true;
    }));
    assert!(
        again.contains("Nothing to do: the machine matches the layout."),
        "{again}"
    );
    assert_eq!(r.host.calls, calls);
}

#[test]
fn remove_is_refused_while_the_daemon_answers() {
    let mut r = Rig::separate();
    r.ok(&separate(|a| a.apply = true));
    let _daemon =
        std::os::unix::net::UnixListener::bind(r.at("/run/theseus/theseus.sock")).unwrap();
    let e = r.err(&separate(|a| {
        a.remove = true;
        a.apply = true;
    }));
    assert!(e.contains("nothing was changed: 1 refused"), "{e}");
    assert!(r.at("/etc/systemd/system/theseusd.service").exists());
}

#[test]
fn remove_keeps_a_state_dir_that_holds_a_store_and_makes_it_roots() {
    let mut r = Rig::separate();
    let old = r.old_state();
    r.ok(&separate(|a| {
        a.apply = true;
        a.migrate_state = Some(old.clone());
    }));
    let state = tree(&r.at("/var/lib/theseus"));
    let log = r.ok(&separate(|a| {
        a.remove = true;
        a.apply = true;
    }));
    assert!(
        log.contains("kept /var/lib/theseus (it holds bindings.toml, config.last-good.toml, spool, store: the installer never deletes a store (--purge-state moves it aside))"),
        "{log}"
    );
    assert_eq!(
        tree(&r.at("/var/lib/theseus")),
        state,
        "the kept store changed"
    );
    assert_eq!(
        (r.owner("/var/lib/theseus"), r.mode("/var/lib/theseus")),
        ("root:root".into(), 0o700)
    );
    assert_eq!(r.user_named("theseus"), None);
    let again = r.ok(&separate(|a| {
        a.remove = true;
        a.apply = true;
    }));
    assert!(again.contains("Nothing to do"), "{again}");
}

#[test]
fn purge_state_moves_the_state_dir_aside_and_deletes_nothing() {
    let mut r = Rig::separate();
    let old = r.old_state();
    r.ok(&separate(|a| {
        a.apply = true;
        a.migrate_state = Some(old.clone());
    }));
    let state = tree(&r.at("/var/lib/theseus"));
    let plan = r.ok(&separate(|a| {
        a.remove = true;
        a.purge_state = true;
    }));
    assert!(
        plan.contains("  move    dir    /var/lib/theseus\n                 to /var/lib/theseus.removed-20261001T120000Z"),
        "{plan}"
    );
    let log = r.ok(&separate(|a| {
        a.remove = true;
        a.purge_state = true;
        a.apply = true;
    }));
    assert!(log.contains("moved /var/lib/theseus aside to /var/lib/theseus.removed-20261001T120000Z (root:root 0700); nothing in it was deleted"), "{log}");
    assert!(!r.at("/var/lib/theseus").exists());
    let aside = "/var/lib/theseus.removed-20261001T120000Z";
    assert_eq!(tree(&r.at(aside)), state, "something in the store was lost");
    assert_eq!((r.owner(aside), r.mode(aside)), ("root:root".into(), 0o700));
}

#[test]
fn remove_keeps_a_token_and_makes_it_roots() {
    let mut r = Rig::separate();
    r.ok(&separate(|a| a.apply = true));
    std::fs::write(r.at("/etc/theseus/op-token"), b"ops_a-token-for-a-test\n").unwrap();
    let log = r.ok(&separate(|a| {
        a.remove = true;
        a.apply = true;
    }));
    assert!(log.contains("kept /etc/theseus/op-token (it holds a token: delete it yourself once no daemon needs it)"), "{log}");
    assert!(
        log.contains("kept /etc/theseus (it holds op-token)"),
        "{log}"
    );
    assert_eq!(
        std::fs::read(r.at("/etc/theseus/op-token")).unwrap(),
        b"ops_a-token-for-a-test\n"
    );
    assert_eq!(
        (
            r.owner("/etc/theseus/op-token"),
            r.mode("/etc/theseus/op-token")
        ),
        ("root:root".into(), 0o600)
    );
    assert_eq!(
        (r.owner("/etc/theseus"), r.mode("/etc/theseus")),
        ("root:root".into(), 0o700)
    );
    let again = r.ok(&separate(|a| {
        a.remove = true;
        a.apply = true;
    }));
    assert!(again.contains("Nothing to do"), "{again}");
    // An --apply never writes over a token.
    let mut r = Rig::separate();
    r.ok(&separate(|a| a.apply = true));
    std::fs::write(r.at("/etc/theseus/op-token"), b"ops_another\n").unwrap();
    r.ok(&separate(|a| a.apply = true));
    assert_eq!(
        std::fs::read(r.at("/etc/theseus/op-token")).unwrap(),
        b"ops_another\n"
    );
}

#[test]
fn what_the_installer_did_not_make_it_neither_adopts_nor_removes() {
    let mut r = Rig::separate();
    r.host.users.push(User {
        name: "theseus".into(),
        uid: 1001,
        gid: 1001,
        home: "/home/theseus".into(),
        shell: "/bin/bash".into(),
    });
    r.host.groups.push(host::Group {
        name: "theseus".into(),
        gid: 1001,
        members: vec![],
    });
    let (code, out) = r.run(&separate(|_| {})).unwrap();
    assert_eq!(code, 1);
    assert!(
        out.contains(
            "refused: a user theseus exists with home /home/theseus: not one this installer made"
        ),
        "{out}"
    );
    let e = r.err(&separate(|a| a.apply = true));
    assert!(e.contains("nothing was changed"), "{e}");
    let plan = r.ok(&separate(|a| a.remove = true));
    assert!(
        plan.contains(
            "  keep    user   theseus\n                 stays: home /home/theseus, shell /bin/bash"
        ),
        "{plan}"
    );
    assert!(
        plan.contains(
            "  keep    group  theseus\n                 stays: it is user theseus's own group"
        ),
        "{plan}"
    );

    // A unit someone else wrote over the installer's stays.
    let mut r = Rig::separate();
    r.ok(&separate(|a| a.apply = true));
    std::fs::write(
        r.at("/etc/systemd/system/theseusd.service"),
        "[Unit]\n# mine\n",
    )
    .unwrap();
    let log = r.ok(&separate(|a| {
        a.remove = true;
        a.apply = true;
    }));
    assert!(
        r.at("/etc/systemd/system/theseusd.service").exists(),
        "{log}"
    );
}

#[test]
fn a_user_apply_checks_clean_a_second_changes_nothing_and_remove_undoes_it() {
    let mut r = Rig::user();
    let unit = "/home/ada/.config/systemd/user/theseusd.service";
    let log = r.ok(&args(|a| {
        a.user = true;
        a.apply = true;
    }));
    assert!(
        log.contains(&format!("wrote {unit} (ada:ada 0644)")),
        "{log}"
    );
    assert!(
        log.contains("created dir /home/ada/.config/systemd/user (ada:ada 0755), a parent"),
        "{log}"
    );
    assert_eq!((r.owner(unit), r.mode(unit)), ("ada:ada".into(), 0o644));
    let text = std::fs::read_to_string(r.at(unit)).unwrap();
    assert!(
        text.contains("\nDelegate=yes\n") && text.contains("\nKillSignal=SIGINT\n"),
        "{text}"
    );
    assert_eq!(
        r.ok(&args(|a| {
            a.user = true;
            a.check = true;
        })),
        "theseusd install --user: the machine matches the layout.\n"
    );
    let files = tree(&r.env.root);
    let again = r.ok(&args(|a| {
        a.user = true;
        a.apply = true;
    }));
    assert!(again.contains("Nothing to do"), "{again}");
    assert_eq!(tree(&r.env.root), files);
    // A different config source is a difference, named by its line.
    r.g.config = "op://Example/another/notesPlain".into();
    let (code, out) = r
        .run(&args(|a| {
            a.user = true;
            a.check = true;
        }))
        .unwrap();
    assert_eq!(code, 1);
    assert!(out.contains("content: line 8 is `ExecStart=/opt/theseus/bin/theseusd --config op://Example/theseus-config/notesPlain"), "{out}");
    let log = r.ok(&args(|a| {
        a.user = true;
        a.remove = true;
        a.apply = true;
    }));
    assert!(log.contains(&format!("removed {unit}")), "{log}");
    assert!(!r.at(unit).exists());
}

#[test]
fn a_user_unit_without_a_token_file_says_how_to_give_it_one() {
    let mut r = Rig::user();
    r.g.op_token_file = None;
    r.g.state_dir = Some("~/scratch/state".into());
    let out = r.ok(&args(|a| a.user = true));
    assert!(out.contains("ExecStart=/opt/theseus/bin/theseusd --config op://Example/theseus-config/notesPlain --state-dir /home/ada/scratch/state\n"), "{out}");
    assert!(
        out.contains(
            "re-run with --op-token-file <file> (mode 0600). A unit never holds the token itself."
        ),
        "{out}"
    );
}

#[test]
fn a_unit_quotes_what_systemd_would_split_or_expand() {
    assert_eq!(
        unit_arg("op://V/item/notesPlain").unwrap(),
        "op://V/item/notesPlain"
    );
    assert_eq!(unit_arg("/a b/theseusd").unwrap(), "\"/a b/theseusd\"");
    assert_eq!(unit_arg("100%").unwrap(), "100%%");
    assert_eq!(unit_arg("$HOME").unwrap(), "$$HOME");
    assert_eq!(unit_arg("a\"b").unwrap(), "\"a\\\"b\"");
    assert_eq!(unit_arg("").unwrap(), "\"\"");
    assert!(unit_arg("a\nb").is_err());
    assert_eq!(
        unit_env("PATH", "/x:%y").unwrap(),
        "Environment=\"PATH=/x:%%y\""
    );
    assert!(unit_env("X", "a\rb").is_err());
}

#[test]
fn the_stamp_is_utc_and_the_uid_is_the_status_lines() {
    assert_eq!(utc_stamp(0), "19700101T000000Z");
    assert_eq!(utc_stamp(951_782_400 + 3661), "20000229T010101Z");
    let status = "Name:\ttheseusd\nUid:\t1000\t0\t0\t0\nGid:\t1000\t1000\t1000\t1000\n";
    assert_eq!(host::uid_field(status, 0).unwrap(), 1000);
    assert_eq!(host::uid_field(status, 1).unwrap(), 0);
    assert!(host::uid_field("Name:\tx\n", 1).is_err());
}

#[test]
fn passwd_and_group_parse_as_the_system_writes_them() {
    let users = host::parse_passwd("root:x:0:0:root:/root:/bin/bash\n# a comment\ntheseus:x:998:998:Theseus daemon:/var/lib/theseus:/usr/sbin/nologin\nbroken\n");
    assert_eq!(users.len(), 2);
    assert_eq!(users[1].home, Path::new("/var/lib/theseus"));
    let groups = host::parse_group("theseus-ops:x:997:ada,bo\nempty:x:996:\n");
    assert_eq!(groups[0].members, vec!["ada".to_string(), "bo".to_string()]);
    assert!(groups[1].members.is_empty());
}

#[test]
fn the_sockets_directory_keeps_only_sockets_out_of_the_way() {
    // /run/theseus with a stale socket goes with it; with anything else, it stays.
    let mut r = Rig::separate();
    r.ok(&separate(|a| a.apply = true));
    drop(std::os::unix::net::UnixListener::bind(r.at("/run/theseus/theseus.sock")).unwrap());
    assert!(std::fs::symlink_metadata(r.at("/run/theseus/theseus.sock"))
        .unwrap()
        .file_type()
        .is_socket());
    let log = r.ok(&separate(|a| {
        a.remove = true;
        a.apply = true;
    }));
    assert!(log.contains("removed /run/theseus/theseus.sock"), "{log}");
    assert!(!r.at("/run/theseus").exists());
}
