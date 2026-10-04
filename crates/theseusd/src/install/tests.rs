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
use super::layout::{real, system_unit, unit_arg, unit_env, user_unit};
use super::*;

const OPERATOR: &str = "ada";
const SOURCE: &str = "op://Example/theseus-config/notesPlain";
const STAMP: &str = "20261001T120000Z";
/// The `--user` rig's token file, and its unit.
const TOKEN: &str = "/home/ada/.config/theseus/op-token";
const UNIT: &str = "/home/ada/.config/systemd/user/theseusd.service";
/// What a test's token file holds: no output may carry it.
const SENTINEL: &[u8] = b"SENTINEL-not-a-token-DO-NOT-PRINT\n";

/// A temp dir: `root/` is the machine, `build/theseusd` the binary,
/// `old/` an old daemon's state dir, and `ada/` the operator's files.
struct Rig {
    tmp: tempfile::TempDir,
    host: Fake,
    env: Env,
    g: Globals,
    /// What the last run printed, kept when it failed too.
    printed: String,
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
            printed: String::new(),
        }
    }

    /// `sudo theseusd install --separate …`, by ada.
    fn separate() -> Self {
        let r = Self::new(
            |t| Env {
                root: t.join("root"),
                exe: t.join("build/theseusd"),
                argv: vec!["install".into(), "--separate".into()],
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

    /// `theseusd install --user`, by ada, from her shell, with a token file
    /// of hers that is right (mode 0600; a stand-in text, never a token).
    fn user() -> Self {
        let mut r = Self::new(
            |t| Env {
                root: t.join("root"),
                exe: "/opt/theseus/bin/theseusd".into(),
                argv: vec!["install".into(), "--user".into()],
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
        );
        r.token(TOKEN, 0o600);
        r
    }

    /// A token file at `p`, ada's, with this mode; its text is a sentinel the
    /// plan must never print.
    fn token(&mut self, p: &str, mode: u32) {
        let at = self.at(p);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(&at, SENTINEL).unwrap();
        std::fs::set_permissions(&at, std::fs::Permissions::from_mode(mode)).unwrap();
        self.host.chown(&at, 1000, 1000).unwrap();
    }

    /// The exit status and the output, the temp dir shown as `$TMP`.
    fn run(&mut self, a: &InstallArgs) -> Result<(i32, String)> {
        let mut out = vec![];
        let code = run_with(a, &self.g, &self.env, &mut self.host, &mut out);
        self.printed = self.clean(&String::from_utf8_lossy(&out));
        Ok((code?, self.printed.clone()))
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
                // A token file with no read bit is still part of the tree.
                let bytes = std::fs::read(e.path()).unwrap_or_else(|_| b"<unreadable>".to_vec());
                all.push((rel, mode, Some(bytes)));
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
    let before = tree(&r.env.root);
    let out = r.ok(&args(|a| a.user = true));
    same("user.plan", &out, include_str!("golden/user.plan"));
    assert_eq!(tree(&r.env.root), before, "a plan wrote");
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
    // A stop signals the daemon alone, so its jobs run on; no cgroup is
    // delegated, and there is no stop hook (theseus-gyin).
    assert!(
        text.contains("\nKillSignal=SIGINT\n") && text.contains("\nKillMode=process\n"),
        "{text}"
    );
    assert!(
        !text.contains("Delegate=") && !text.contains("ExecStopPost="),
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
    assert!(out.contains("content: line 10 is `ExecStart=/opt/theseus/bin/theseusd --config op://Example/theseus-config/notesPlain"), "{out}");
    let log = r.ok(&args(|a| {
        a.user = true;
        a.remove = true;
        a.apply = true;
    }));
    assert!(log.contains(&format!("removed {unit}")), "{log}");
    assert!(!r.at(unit).exists());
}

/// `--unit` gives a second daemon, a scratch one beside the operator's, a unit of its own: its file,
/// its name in the plan's command and notes and in the unit, and the operator's unit untouched by its
/// apply and its remove. The operator's own unit reads as it always has (the golden plan).
#[test]
fn a_second_daemons_unit_has_its_own_name_and_leaves_the_first_alone() {
    let mut r = Rig::user();
    r.ok(&args(|a| {
        a.user = true;
        a.apply = true;
    }));
    let first = std::fs::read(r.at(UNIT)).unwrap();
    r.g.state_dir = Some("/tmp/scratch/state".into());
    r.g.socket = Some("/tmp/scratch/sock".into());
    let scratch = "/home/ada/.config/systemd/user/theseus-scratch.service";
    let named = |a: &mut InstallArgs| {
        a.user = true;
        a.unit = Some("theseus-scratch".into());
    };
    let plan = r.ok(&args(named));
    assert!(
        plan.starts_with("theseusd install --user --unit theseus-scratch: the plan."),
        "{plan}"
    );
    for want in [
        "systemctl --user enable --now theseus-scratch.service",
        "journalctl --user -u theseus-scratch: its log",
    ] {
        assert!(plan.contains(want), "{want:?} missing from:\n{plan}");
    }
    let log = r.ok(&args(|a| {
        named(a);
        a.apply = true;
    }));
    assert!(log.contains(&format!("wrote {scratch}")), "{log}");
    let text = std::fs::read_to_string(r.at(scratch)).unwrap();
    for want in [
        "\nDescription=Theseus daemon (theseus-scratch)\n",
        "(`systemctl --user edit theseus-scratch`)",
        " --state-dir /tmp/scratch/state --socket /tmp/scratch/sock ",
    ] {
        assert!(text.contains(want), "{want:?} missing from:\n{text}");
    }
    // A name with `.service` after it is the same unit.
    assert_eq!(
        r.ok(&args(|a| {
            a.user = true;
            a.unit = Some("theseus-scratch.service".into());
            a.check = true;
        })),
        "theseusd install --user --unit theseus-scratch: the machine matches the layout.\n"
    );
    // Its remove takes its own unit, and the operator's stays as it was.
    let log = r.ok(&args(|a| {
        named(a);
        a.remove = true;
        a.apply = true;
    }));
    assert!(log.contains(&format!("removed {scratch}")), "{log}");
    assert!(!r.at(scratch).exists());
    assert_eq!(std::fs::read(r.at(UNIT)).unwrap(), first);
    // A name systemd would not take is refused, and nothing is read or written.
    for bad in ["", ".service", "../x", "a b", "x/y"] {
        let e = r.err(&args(|a| {
            a.user = true;
            a.unit = Some(bad.into());
        }));
        assert!(e.contains("a unit's name is letters"), "{bad:?}: {e}");
    }
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
            "then re-run: /opt/theseus/bin/theseusd install --user --op-token-file <file>. A unit \
             never holds the token itself."
        ),
        "{out}"
    );
    assert!(
        !out.contains("token  "),
        "no token file, no token item:\n{out}"
    );
}

/// `--apply` with no token file named refuses before it writes anything, unless
/// `--token-from-drop-in` says a drop-in supplies the token; a plan only notes it
/// (theseus-4xyj).
#[test]
fn a_user_apply_without_a_token_file_refuses_unless_a_drop_in_supplies_it() {
    let mut r = Rig::user();
    r.g.op_token_file = None;
    let before = tree(&r.env.root);
    let e = r.err(&args(|a| {
        a.user = true;
        a.apply = true;
    }));
    assert!(e.contains("nothing was changed"), "{e}");
    assert!(e.contains("--op-token-file"), "{e}");
    assert!(e.contains("--token-from-drop-in"), "{e}");
    assert_eq!(tree(&r.env.root), before, "a refused --apply wrote");
    assert!(!r.at(UNIT).exists());
    // A plan is not refused: it says the same as a note.
    let plan = r.ok(&args(|a| a.user = true));
    assert!(plan.contains("the unit names no token file"), "{plan}");
    // A check is not either.
    r.run(&args(|a| {
        a.user = true;
        a.check = true;
    }))
    .unwrap();
    // The flag lets the apply through, and the unit names no token file.
    let log = r.ok(&args(|a| {
        a.user = true;
        a.apply = true;
        a.token_from_drop_in = true;
    }));
    assert!(log.contains(&format!("wrote {UNIT}")), "{log}");
    let text = std::fs::read_to_string(r.at(UNIT)).unwrap();
    assert!(!text.contains("--op-token-file"), "{text}");
    // --remove needs no token.
    r.g.op_token_file = None;
    r.ok(&args(|a| {
        a.user = true;
        a.remove = true;
        a.apply = true;
    }));
    assert!(!r.at(UNIT).exists());
}

/// Both daemon units restart after 1 s and stop a crash loop at 10 starts in 300 s, so the
/// crash files it keeps are not buried by an endless restart (theseus-0v8s).
#[test]
fn the_daemon_units_bound_a_crash_loop() {
    let exec = vec!["/opt/theseus/bin/theseusd".to_string()];
    let user = user_unit(USER_UNIT, &exec, &[]).unwrap();
    let system = system_unit(&exec).unwrap();
    for (name, unit) in [("user", &user), ("system", &system)] {
        let (unit_sect, service) = unit.split_once("[Service]").unwrap();
        assert!(
            unit_sect.contains("StartLimitIntervalSec=300\n"),
            "{name}:\n{unit}"
        );
        assert!(
            unit_sect.contains("StartLimitBurst=10\n"),
            "{name}:\n{unit}"
        );
        assert!(
            service.contains("Restart=on-failure\nRestartSec=1\n"),
            "{name}:\n{unit}"
        );
    }
}

/// The hint is the command as it was typed, with the flag added after it
/// (theseus-w1nf): a flag the operator gave is not lost on the re-run, and a
/// word with a space in it is quoted so the line pastes back whole.
#[test]
fn the_hint_repeats_the_command_as_it_was_typed() {
    let mut r = Rig::user();
    r.g.op_token_file = None;
    r.env.argv = [
        "--state-dir",
        "/home/ada/my state",
        "install",
        "--user",
        "--apply",
    ]
    .map(String::from)
    .into();
    let out = r.ok(&args(|a| {
        a.user = true;
        // A plan, whatever argv says: --apply with no token file refuses (theseus-4xyj).
        a.apply = false;
    }));
    assert!(
        out.contains(
            "then re-run: /opt/theseus/bin/theseusd --state-dir '/home/ada/my state' install \
             --user --apply --op-token-file <file>."
        ),
        "{out}"
    );
    assert_eq!(shell_word("a'b"), r"'a'\''b'");
    assert_eq!(shell_word(""), "''");
    assert_eq!(shell_word("/p/a-b_c.d:e@f"), "/p/a-b_c.d:e@f");
}

/// A faulty token file, as a plan says it: (the setup, what the refusal says).
type Fault<'a> = (&'a str, fn(&mut Rig), &'a str);

/// What `--user`'s plan says of each way a token file can be wrong, and that
/// it never says what the file holds (theseus-w1nf). The plan only prints, so
/// the files stay as they are, and a file that no one may read (mode 0200) is
/// still judged: the plan never opens it.
#[test]
fn the_user_plan_names_each_fault_in_the_token_file_and_how_to_fix_it() {
    let faults: [Fault; 11] = [
        (
            "missing",
            |r| std::fs::remove_file(r.at(TOKEN)).unwrap(),
            "refused: it does not exist. Fix: make it with mode 0600, holding the 1Password \
             service-account token (docs/user-service.md shows how)",
        ),
        (
            "a directory",
            |r| {
                std::fs::remove_file(r.at(TOKEN)).unwrap();
                std::fs::create_dir(r.at(TOKEN)).unwrap();
            },
            "refused: it is not a regular file (a symlink, a directory, or a device). Fix: name \
             the file itself",
        ),
        (
            "a symlink to a good file",
            |r| {
                let real = r.at("/home/ada/.config/theseus/real-token");
                std::fs::rename(r.at(TOKEN), &real).unwrap();
                std::os::unix::fs::symlink(&real, r.at(TOKEN)).unwrap();
            },
            "refused: it is not a regular file (a symlink, a directory, or a device). Fix: name \
             the file itself",
        ),
        (
            "mode 0644",
            |r| std::fs::set_permissions(r.at(TOKEN), std::fs::Permissions::from_mode(0o644)).unwrap(),
            "refused: mode 0644 is looser than 0600. Fix: chmod 600 /home/ada/.config/theseus/op-token",
        ),
        (
            "mode 0640",
            |r| std::fs::set_permissions(r.at(TOKEN), std::fs::Permissions::from_mode(0o640)).unwrap(),
            "refused: mode 0640 is looser than 0600. Fix: chmod 600 /home/ada/.config/theseus/op-token",
        ),
        (
            "mode 0700, an executable token",
            |r| std::fs::set_permissions(r.at(TOKEN), std::fs::Permissions::from_mode(0o700)).unwrap(),
            "refused: mode 0700 is looser than 0600. Fix: chmod 600 /home/ada/.config/theseus/op-token",
        ),
        (
            "mode 0200, which its owner cannot read",
            |r| std::fs::set_permissions(r.at(TOKEN), std::fs::Permissions::from_mode(0o200)).unwrap(),
            "refused: mode 0200 does not let you read it. Fix: chmod 600 \
             /home/ada/.config/theseus/op-token",
        ),
        (
            "another user's",
            |r| r.host.chown(&r.at(TOKEN), 0, 0).unwrap(),
            "refused: it is owned by root (uid 0), not by you (ada). Fix: sudo chown ada \
             /home/ada/.config/theseus/op-token",
        ),
        (
            "empty",
            |r| std::fs::write(r.at(TOKEN), b"").unwrap(),
            "refused: it is empty. Fix: put the 1Password service-account token in it",
        ),
        (
            "an owner's uid nobody has",
            |r| r.host.chown(&r.at(TOKEN), 4242, 4242).unwrap(),
            "refused: it is owned by uid 4242, not by you (ada). Fix: sudo chown ada \
             /home/ada/.config/theseus/op-token",
        ),
        (
            "three faults at once",
            |r| {
                std::fs::set_permissions(r.at(TOKEN), std::fs::Permissions::from_mode(0o666)).unwrap();
                std::fs::write(r.at(TOKEN), b"").unwrap();
                r.host.chown(&r.at(TOKEN), 0, 0).unwrap();
            },
            "refused: it is owned by root (uid 0), not by you (ada); mode 0666 is looser than \
             0600; it is empty. Fix: sudo chown ada /home/ada/.config/theseus/op-token; chmod 600 \
             /home/ada/.config/theseus/op-token; put the 1Password service-account token in it",
        ),
    ];
    for (what, setup, says) in faults {
        let mut r = Rig::user();
        setup(&mut r);
        let before = tree(&r.env.root);
        let (code, out) = r.run(&args(|a| a.user = true)).unwrap();
        assert_eq!(code, 1, "{what}: a refusal is exit 1:\n{out}");
        assert!(out.contains(says), "{what}: want `{says}` in:\n{out}");
        assert!(
            out.contains("  REFUSE  token  /home/ada/.config/theseus/op-token (yours alone"),
            "{what}:\n{out}"
        );
        assert!(
            out.contains("1 refused: --apply changes nothing until each is resolved."),
            "{what}:\n{out}"
        );
        assert!(
            !out.contains("SENTINEL"),
            "{what}: the plan printed the token"
        );
        assert_eq!(tree(&r.env.root), before, "{what}: a plan wrote");
    }
}

/// The right token files: the operator's, regular, not empty, mode 0600 or
/// stricter. A plan shows `ok`, and the exit is 0.
#[test]
fn the_user_plan_accepts_a_token_file_that_is_0600_or_stricter() {
    for mode in [0o600, 0o400] {
        let mut r = Rig::user();
        std::fs::set_permissions(r.at(TOKEN), std::fs::Permissions::from_mode(mode)).unwrap();
        let (code, out) = r.run(&args(|a| a.user = true)).unwrap();
        assert_eq!(code, 0, "mode {mode:04o}:\n{out}");
        assert!(
            out.contains("  ok      token  /home/ada/.config/theseus/op-token (yours alone"),
            "mode {mode:04o}:\n{out}"
        );
        assert!(!out.contains("refused"), "mode {mode:04o}:\n{out}");
        assert!(
            !out.contains("SENTINEL"),
            "mode {mode:04o}: the plan printed the token"
        );
    }
}

/// `--apply` writes nothing while the token file is wrong, says which, and
/// does its work once the file is right (theseus-w1nf).
#[test]
fn a_user_apply_refuses_until_the_token_file_is_right() {
    let mut r = Rig::user();
    std::fs::set_permissions(r.at(TOKEN), std::fs::Permissions::from_mode(0o644)).unwrap();
    let before = tree(&r.env.root);
    let e = r.err(&args(|a| {
        a.user = true;
        a.apply = true;
    }));
    assert!(e.contains("nothing was changed: 1 refused (above)"), "{e}");
    assert!(
        r.printed
            .contains("mode 0644 is looser than 0600. Fix: chmod 600"),
        "{}",
        r.printed
    );
    assert!(!r.printed.contains("SENTINEL"), "{}", r.printed);
    assert_eq!(tree(&r.env.root), before, "a refused --apply wrote");
    assert!(!r.at(UNIT).exists());

    // Mended: the same command now does it.
    std::fs::set_permissions(r.at(TOKEN), std::fs::Permissions::from_mode(0o600)).unwrap();
    let log = r.ok(&args(|a| {
        a.user = true;
        a.apply = true;
    }));
    assert!(log.contains(&format!("wrote {UNIT}")), "{log}");
    assert!(r.at(UNIT).exists());
    // The unit names the token file; the token is not in it.
    let text = std::fs::read_to_string(r.at(UNIT)).unwrap();
    assert!(
        text.contains(" --op-token-file /home/ada/.config/theseus/op-token"),
        "{text}"
    );
    assert!(!text.contains("SENTINEL"), "{text}");
}

/// After an install, `--check` names a token file that has gone wrong since,
/// and `--remove` does not care about it (theseus-w1nf).
#[test]
fn a_user_check_finds_a_token_file_gone_wrong_and_remove_ignores_it() {
    let mut r = Rig::user();
    r.ok(&args(|a| {
        a.user = true;
        a.apply = true;
    }));
    std::fs::set_permissions(r.at(TOKEN), std::fs::Permissions::from_mode(0o666)).unwrap();
    let (code, out) = r
        .run(&args(|a| {
            a.user = true;
            a.check = true;
        }))
        .unwrap();
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("1 difference(s) from the layout"), "{out}");
    assert!(out.contains("mode 0666 is looser than 0600"), "{out}");
    let log = r.ok(&args(|a| {
        a.user = true;
        a.remove = true;
        a.apply = true;
    }));
    assert!(log.contains(&format!("removed {UNIT}")), "{log}");
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
