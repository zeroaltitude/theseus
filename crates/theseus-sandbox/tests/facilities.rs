//! What [`theseus_sandbox::Facilities`] grants, one grant at a time, and what
//! it still refuses (theseus-1eemp).
//!
//! The 2026-11-11 08:43 self build was the first ever to reach the suite, and
//! 123 of 9,819 tests failed with no facility to fail on: 58 wanted a pty, 49
//! wanted the calls that build a sandbox, one wanted `/dev/full`. The owner's
//! call that day was to grant them rather than to skip the tests, so each
//! grant is a rung here, measured on its own:
//!
//! - nothing granted is the floor the contract tests describe: no `/dev/ptmx`,
//!   no `/dev/full`, and `clone(CLONE_NEW*)` refused;
//! - `pty` alone opens a terminal, and changes nothing else;
//! - `devices` alone binds what [`theseus_sandbox::MORE_DEVICES`] names;
//! - `nesting` alone lets a job make a sandbox of its own.
//!
//! And the one that is not about a count: **a granted view still cannot lift
//! a `hidden` cover.** `a_granted_view_cannot_lift_a_hidden_cover` plants a
//! secret under a bound root, covers it, and tries the three ways out from
//! inside the view — `umount2` where it stands, `umount2` in a nested user
//! and mount namespace, and a bind of the cover's parent — asserting the
//! bytes stay unreadable each time. That test is the argument for the whole
//! grant; if it ever goes red the grant must come back off.

mod common;

use std::io;
use std::path::Path;

use serde_json::{json, Value};
use theseus_sandbox::{Facilities, MORE_DEVICES};

use common::{check, job, run, Case};

fn main() {
    common::main(CASES, &[], |args| {
        if args.first().map(String::as_str) == Some("probe") {
            probe::main(&args[1..]);
        }
    });
}

const CASES: &[Case] = &[
    Case {
        name: "nothing_granted_has_no_pty_no_extra_devices_and_no_nesting",
        run: floor,
    },
    Case {
        name: "a_pty_grant_opens_a_terminal",
        run: pty_grant,
    },
    Case {
        name: "a_devices_grant_binds_every_name_it_promises",
        run: devices_grant,
    },
    Case {
        name: "a_nesting_grant_lets_a_job_make_a_sandbox_of_its_own",
        run: nesting_grant,
    },
    Case {
        name: "a_visible_proc_grant_takes_the_masks_off_and_nothing_else_does",
        run: visible_proc_grant,
    },
    Case {
        name: "a_granted_view_cannot_lift_a_hidden_cover",
        run: cover_holds,
    },
];

/// The probe `name` as a job in `ws`, with `f` granted: this binary, bound
/// read-only into the view, as `contract`'s probes run.
fn probe_in(ws: &Path, f: Facilities, name: &str, args: &[&str]) -> Result<Value, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut argv = vec![exe.display().to_string(), "probe".into(), name.into()];
    let ws_s = ws.display().to_string();
    argv.extend(args.iter().map(|a| a.replace("{ws}", &ws_s)));
    let mut spec = job(argv, ws);
    spec.ro_paths = vec![exe];
    spec.facilities = f;
    let (exit, out) = run(&spec)?;
    check(
        exit.success(),
        format!("the probe {name} ended {exit:?}; its output:\n{out}"),
    )?;
    let last = out.lines().last().unwrap_or("");
    // THESEUS_SANDBOX_SHOW=1 prints what each rung saw, as `contract`'s
    // probes do: the ladder's evidence is this line, grant by grant.
    if std::env::var_os("THESEUS_SANDBOX_SHOW").is_some() {
        println!("granted {:?}: {last}", f.named());
    }
    serde_json::from_str(last).map_err(|e| format!("the probe {name}'s output ({e}):\n{out}"))
}

fn probe(f: Facilities, name: &str, args: &[&str]) -> Result<Value, String> {
    let ws = tempfile::tempdir().map_err(|e| e.to_string())?;
    probe_in(ws.path(), f, name, args)
}

/// Nothing granted: the three facilities are each absent, and this is the
/// state every one of the 123 failures was measured in.
fn floor() -> Result<(), String> {
    let v = probe(Facilities::NONE, "facilities", &[])?;
    check(
        v["openpty"] != "ok",
        format!("a job with no pty grant opened a terminal: {v}"),
    )?;
    check(
        v["ptmx"] == false,
        format!("/dev/ptmx without the grant: {v}"),
    )?;
    for name in MORE_DEVICES {
        check(
            v["dev"][name] == false,
            format!("/dev/{name} without the devices grant: {v}"),
        )?;
    }
    // The four every job gets are there whatever is granted.
    for name in theseus_sandbox::DEVICES {
        check(
            v["dev"][name] == true,
            format!("/dev/{name} is a job's whatever is granted: {v}"),
        )?;
    }
    check(
        v["newuser"] != "ok" && v["newns"] != "ok",
        format!("a job with no nesting grant made a namespace: {v}"),
    )
}

/// `pty` alone: a terminal opens, and the other two grants stay off.
fn pty_grant() -> Result<(), String> {
    let f = Facilities {
        pty: true,
        ..Facilities::NONE
    };
    let v = probe(f, "facilities", &[])?;
    check(v["ptmx"] == true, format!("no /dev/ptmx: {v}"))?;
    check(v["openpty"] == "ok", format!("openpty: {v}"))?;
    // The instance is the job's own, so its first terminal is /dev/pts/0.
    check(
        v["pts_name"] == "/dev/pts/0",
        format!("the terminal is not from this job's own devpts: {v}"),
    )?;
    check(
        v["dev"]["full"] == false && v["newuser"] != "ok",
        format!("the pty grant turned on another: {v}"),
    )
}

/// `devices` alone: every name `MORE_DEVICES` promises is bound, and the
/// names are not a list nobody checks.
fn devices_grant() -> Result<(), String> {
    let f = Facilities {
        devices: true,
        ..Facilities::NONE
    };
    let v = probe(f, "facilities", &[])?;
    for name in MORE_DEVICES {
        // A node the host itself lacks is skipped by design, so the
        // assertion is: bound if the host has it.
        if Path::new("/dev").join(name).exists() {
            check(
                v["dev"][name] == true,
                format!("/dev/{name} with the devices grant: {v}"),
            )?;
        }
    }
    // `/dev/full` is the one the suite actually turns on: a write to it must
    // answer ENOSPC rather than NotFound.
    check(
        v["full_write"] == "ENOSPC",
        format!("a write to /dev/full: {v}"),
    )?;
    check(
        v["ptmx"] == false && v["newuser"] != "ok",
        format!("the devices grant turned on another: {v}"),
    )
}

/// `nesting` alone: the calls a sandbox is built from work, and the ones the
/// init never makes are still refused.
fn nesting_grant() -> Result<(), String> {
    let f = Facilities {
        nesting: true,
        ..Facilities::NONE
    };
    let v = probe(f, "facilities", &[])?;
    check(
        v["newuser"] == "ok",
        format!("a nesting grant must allow a new user namespace: {v}"),
    )?;
    check(
        v["newns"] == "ok",
        format!("a nesting grant must allow a new mount namespace: {v}"),
    )?;
    check(
        v["nested_tmpfs"] == "ok",
        format!("a mount inside the nested namespace: {v}"),
    )?;
    // Sized to what the init calls: `setns` is not one of them.
    check(
        v["setns"] == "EPERM",
        format!("setns stays denied in a nesting grant: {v}"),
    )?;
    check(
        v["ptmx"] == false && v["dev"]["full"] == false,
        format!("the nesting grant turned on another: {v}"),
    )
}

/// `visible_proc` alone: Docker's masks come off `/proc` and `/sys`, and no
/// other grant touches them.
///
/// **A correction this test earned, and the reason it is not named after the
/// hypothesis it was written for.** I expected `mount_too_revealing` to be
/// what stops a nested sandbox under a masked `/proc` — the kernel refuses a
/// fresh procfs in a less-privileged mount namespace unless that namespace
/// already holds a fully visible one, and our masks are locked children.
/// Measured, that is **not** what happens: with `nesting` alone and the masks
/// still on, a child in a new user, mount and pid namespace mounts a procfs
/// successfully (`nested_proc: "ok"` in this probe's output).
///
/// **That measurement stands, but it is not the gate, and on 2026-11-13 the
/// gate itself was measured.** `nested_proc` mounts a bare procfs at `/tmp`,
/// where nothing is covered; a real init mounts at `/proc`, where the
/// inherited locked covers are. At this very rung on 6.18 the two answers
/// disagree — `nested_proc: "ok"` while a real nested L1 job is refused with
/// `mounting /proc: Operation not permitted` — so this probe is **not** a
/// valid proxy for what `visible_proc` buys. It is kernel-dependent besides,
/// which was not recorded here: under `nesting` alone it is `ok` on 6.18 and
/// `EPERM` on `7.0.0-1014-aws`. The cause of that refusal **is** the masks
/// after all — measured on both kernels with the two rungs differing in one
/// bool — and `theseus_sandbox::Facilities::visible_proc`'s own doc carries
/// it. This case still claims only what it can see: which masks each grant
/// moves.
fn visible_proc_grant() -> Result<(), String> {
    let v = probe(
        Facilities {
            visible_proc: true,
            ..Facilities::NONE
        },
        "facilities",
        &[],
    )?;
    check(
        v["proc_masks"] == 0,
        format!("visible_proc must leave no mask mount under /proc: {v}"),
    )?;
    // And it is narrowed to /proc: `/sys`'s masks stay on. The one ladder
    // run that took them off as well ended on the same 119 of 123 -- one
    // unreproducible run inside the 2026-11-11 08:43 self build, on a rig
    // that no longer exists; `Facilities::visible_proc`'s own doc holds its
    // terms. That is EVIDENCE that nothing moved and NOT
    // proof that `/sys`'s masks cost no test: row B5 measures the ladder,
    // and until it has, the narrowing stands on the rule that a grant
    // reaches no further than its subject needs -- not on that count.
    check(
        v["sys_masks"].as_u64().unwrap_or(0) >= 3,
        format!("visible_proc must leave /sys's masks ON: {v}"),
    )?;
    check(
        v["ptmx"] == false && v["dev"]["full"] == false && v["newuser"] != "ok",
        format!("the visible_proc grant turned on another: {v}"),
    )?;
    // And every other rung keeps them on, so the assertion discriminates.
    for f in [
        Facilities::NONE,
        Facilities {
            pty: true,
            ..Facilities::NONE
        },
        Facilities {
            nesting: true,
            ..Facilities::NONE
        },
    ] {
        let v = probe(f, "facilities", &[])?;
        check(
            v["proc_masks"].as_u64().unwrap_or(0) >= 10,
            format!("{:?} must keep Docker's /proc masks on: {v}", f.named()),
        )?;
    }
    Ok(())
}

/// **The grant's own safety argument, planted rather than asserted.** A
/// secret under a bound workspace root, covered by `hidden`, with every
/// facility granted: the three routes out all fail and the bytes stay
/// unreadable.
fn cover_holds() -> Result<(), String> {
    let ws = tempfile::tempdir().map_err(|e| e.to_string())?;
    let sealed = ws.path().join("sealed");
    std::fs::create_dir_all(&sealed).map_err(|e| e.to_string())?;
    std::fs::write(sealed.join("answers"), "SEALED-SECRET").map_err(|e| e.to_string())?;
    std::fs::write(ws.path().join("control"), "control").map_err(|e| e.to_string())?;
    std::fs::create_dir_all(ws.path().join("mnt")).map_err(|e| e.to_string())?;

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut argv = vec![exe.display().to_string(), "probe".into(), "unmask".into()];
    argv.push(sealed.display().to_string());
    argv.push(ws.path().join("mnt").display().to_string());
    let mut spec = job(argv, ws.path());
    spec.ro_paths = vec![exe];
    spec.hidden = vec![sealed];
    // The MOST permissive view this build can make: the cover must hold here
    // or nowhere.
    spec.facilities = Facilities {
        pty: true,
        devices: true,
        nesting: true,
        visible_proc: true,
    };
    let (exit, out) = run(&spec)?;
    check(
        exit.success(),
        format!("the unmask probe ended {exit:?}; its output:\n{out}"),
    )?;
    let v: Value = serde_json::from_str(out.lines().last().unwrap_or(""))
        .map_err(|e| format!("the unmask probe's output ({e}):\n{out}"))?;

    // The control file beside the sealed set proves the root really is bound:
    // a view that bound nothing would "hide" the secret by accident.
    check(
        v["control"] == "control",
        format!("the workspace root is not bound, so this test proves nothing: {v}"),
    )?;
    for step in [
        "read",
        "umount_here",
        "umount_nested",
        "bind_parent",
        "read_after",
        "read_via_bind",
    ] {
        check(
            v[step] != "SEALED-SECRET",
            format!("the cover was lifted at {step}: {v}"),
        )?;
    }
    check(
        v["umount_here"] == "EPERM",
        format!("umount2 where the job stands must be EPERM: {v}"),
    )?;
    // EINVAL is `do_umount`'s answer for MNT_LOCKED, which is the kernel
    // rule the whole grant leans on.
    check(
        v["umount_nested"] == "EINVAL",
        format!("umount2 of an inherited cover must be EINVAL (MNT_LOCKED): {v}"),
    )?;
    check(
        v["bind_parent"] == "EINVAL",
        format!("a non-recursive bind of a directory with locked children must be EINVAL: {v}"),
    )
}

/// The probe, inside the view.
mod probe {
    use super::{io, json, Path, Value};

    pub fn main(args: &[String]) -> ! {
        let out = match args.first().map(String::as_str) {
            Some("facilities") => facilities(),
            Some("unmask") => unmask(&args[1], &args[2]),
            other => json!({"error": format!("no probe {other:?}")}),
        };
        println!("{out}");
        std::process::exit(0)
    }

    /// `Ok`/`errno name` for a raw call's return.
    fn outcome(rc: libc::c_int) -> String {
        if rc != -1 {
            return "ok".into();
        }
        let e = io::Error::last_os_error();
        match e.raw_os_error() {
            Some(libc::EPERM) => "EPERM".into(),
            Some(libc::EINVAL) => "EINVAL".into(),
            Some(libc::ENOENT) => "ENOENT".into(),
            Some(libc::ENOSPC) => "ENOSPC".into(),
            Some(libc::EACCES) => "EACCES".into(),
            _ => e.to_string(),
        }
    }

    fn facilities() -> Value {
        let mut dev = serde_json::Map::new();
        for name in theseus_sandbox::DEVICES
            .iter()
            .chain(theseus_sandbox::MORE_DEVICES.iter())
        {
            dev.insert(
                (*name).to_string(),
                Value::Bool(Path::new("/dev").join(name).exists()),
            );
        }
        let (mut m, mut s) = (-1, -1);
        let rc = unsafe {
            libc::openpty(
                &mut m,
                &mut s,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        let pts_name = if rc == 0 {
            let p = unsafe { libc::ptsname(m) };
            if p.is_null() {
                String::new()
            } else {
                unsafe { std::ffi::CStr::from_ptr(p) }
                    .to_string_lossy()
                    .into_owned()
            }
        } else {
            String::new()
        };
        if rc == 0 {
            unsafe {
                libc::close(m);
                libc::close(s)
            };
        }
        // `/dev/full` answers ENOSPC on a write, which is the whole reason
        // one test wants it.
        let full_write = {
            let fd = unsafe { libc::open(c"/dev/full".as_ptr(), libc::O_WRONLY) };
            if fd < 0 {
                outcome(fd)
            } else {
                let r = unsafe { libc::write(fd, b"x".as_ptr().cast(), 1) };
                unsafe { libc::close(fd) };
                outcome(if r < 0 { -1 } else { 0 })
            }
        };
        // Done in a child: a successful unshare would change this process.
        let newuser = in_child(|| unsafe { libc::unshare(libc::CLONE_NEWUSER) });
        let newns = in_child(|| unsafe { libc::unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWNS) });
        let nested_tmpfs = in_child(|| unsafe {
            if libc::unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWNS) == -1 {
                return -1;
            }
            libc::mount(
                c"tmpfs".as_ptr(),
                c"/tmp".as_ptr(),
                c"tmpfs".as_ptr(),
                0,
                std::ptr::null(),
            )
        });
        // A procfs mounted inside a NEW user, mount and pid namespace: the
        // shape the real init uses, and the only way to measure
        // `mount_too_revealing` rather than the capability check in front of
        // it.
        let nested_proc = nested_proc();
        // The masks, by their effect: `/proc/kcore` covered by a bind of
        // /dev/null reads as an empty regular file of size 0, and
        // `/sys/fs/cgroup` covered by an empty read-only tmpfs has no
        // entries. Unmasked, kcore is huge and cgroup is populated.
        // The masks, counted directly rather than guessed at from an effect:
        // each is a mount, so each is a line of `mountinfo` whose mount point
        // is under `/proc/` or `/sys/`. (Reading them by effect does not work
        // — `/sys/fs/cgroup` is empty on a fresh sysfs whether it is masked
        // or not, because no cgroup2 is ever mounted in a view.)
        let under = |prefix: &str| {
            std::fs::read_to_string("/proc/self/mountinfo")
                .map(|t| {
                    t.lines()
                        .filter_map(|l| l.split(' ').nth(4))
                        .filter(|p| p.starts_with(prefix))
                        .count()
                })
                .unwrap_or(0)
        };
        let (proc_masks, sys_masks) = (under("/proc/"), under("/sys/"));
        let setns = in_child(|| unsafe {
            let fd = libc::open(c"/proc/self/ns/uts".as_ptr(), libc::O_RDONLY);
            if fd < 0 {
                return -1;
            }
            libc::setns(fd, libc::CLONE_NEWUTS)
        });
        json!({
            "dev": dev,
            "ptmx": Path::new("/dev/ptmx").exists(),
            "openpty": if rc == 0 { "ok".to_string() } else { outcome(rc) },
            "pts_name": pts_name,
            "full_write": full_write,
            "newuser": newuser,
            "newns": newns,
            "nested_tmpfs": nested_tmpfs,
            "nested_proc": nested_proc,
            "proc_masks": proc_masks,
            "sys_masks": sys_masks,
            "setns": setns,
        })
    }

    /// `mount -t proc` as the real init does it: a new user, mount and pid
    /// namespace, then a fork, so the trying process is pid 1 of a pid
    /// namespace its own user namespace owns. `proc_init_fs_context`'s
    /// capability check then passes and the only gate left is
    /// `mount_too_revealing`, which is what this measures.
    fn nested_proc() -> String {
        in_child(|| unsafe {
            if libc::unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWNS | libc::CLONE_NEWPID) == -1 {
                return -1;
            }
            match libc::fork() {
                -1 => -1,
                0 => {
                    let rc = libc::mount(
                        c"proc".as_ptr(),
                        c"/tmp".as_ptr(),
                        c"proc".as_ptr(),
                        0,
                        std::ptr::null(),
                    );
                    let code = if rc == -1 {
                        io::Error::last_os_error().raw_os_error().unwrap_or(1)
                    } else {
                        0
                    };
                    libc::_exit(code)
                }
                pid => {
                    let mut status = 0;
                    libc::waitpid(pid, &mut status, 0);
                    let code = (status >> 8) & 0xff;
                    if code == 0 {
                        0
                    } else {
                        *libc::__errno_location() = code;
                        -1
                    }
                }
            }
        })
    }

    /// Runs `f` in a forked child, so a namespace it makes dies with it, and
    /// gives the child's reading of `outcome`. The child writes the errno
    /// through its exit status: 0 is ok, else the errno itself.
    fn in_child(f: impl FnOnce() -> libc::c_int) -> String {
        match unsafe { libc::fork() } {
            -1 => "fork failed".into(),
            0 => {
                let rc = f();
                let code = if rc == -1 {
                    io::Error::last_os_error().raw_os_error().unwrap_or(1)
                } else {
                    0
                };
                std::process::exit(code)
            }
            pid => {
                let mut status = 0;
                unsafe { libc::waitpid(pid, &mut status, 0) };
                let code = (status >> 8) & 0xff;
                match code {
                    0 => "ok".into(),
                    libc::EPERM => "EPERM".into(),
                    libc::EINVAL => "EINVAL".into(),
                    libc::ENOENT => "ENOENT".into(),
                    e => format!("errno {e}"),
                }
            }
        }
    }

    /// The three routes at a `hidden` cover, from inside a fully granted
    /// view. `sealed` is the covered directory's path; `mnt` a writable
    /// directory in the view to bind into.
    fn unmask(sealed: &str, mnt: &str) -> Value {
        let answers = Path::new(sealed).join("answers");
        let read = |p: &Path| std::fs::read_to_string(p).unwrap_or_default();
        let control = read(&Path::new(sealed).parent().unwrap().join("control"));
        let before = read(&answers);

        // 1. umount2 where the job stands. The job's mount namespace is owned
        //    by user namespace 1 and the command runs in 2, so `may_mount`
        //    refuses whatever seccomp allows.
        let umount_here = {
            let c = std::ffi::CString::new(sealed).unwrap();
            super::probe::outcome(unsafe { libc::umount2(c.as_ptr(), 0) })
        };

        // 2 and 3. A new user and mount namespace, where the job does hold
        //    CAP_SYS_ADMIN — and where every inherited mount is MNT_LOCKED.
        //    Both tries and both reads happen in the child, since the
        //    namespace is the child's.
        let nested = nested_tries(sealed, mnt, &answers);

        json!({
            "control": control,
            "read": before,
            "umount_here": umount_here,
            "umount_nested": nested["umount"],
            "bind_parent": nested["bind"],
            "read_after": nested["read_after"],
            "read_via_bind": nested["read_via_bind"],
        })
    }

    /// The nested half of `unmask`, run in a child so its namespaces go with
    /// it. The child reports as one JSON line on a pipe.
    fn nested_tries(sealed: &str, mnt: &str, answers: &Path) -> Value {
        let mut fds = [0; 2];
        if unsafe { libc::pipe(fds.as_mut_ptr()) } == -1 {
            return json!({"umount": "pipe failed"});
        }
        let (r, w) = (fds[0], fds[1]);
        match unsafe { libc::fork() } {
            -1 => json!({"umount": "fork failed"}),
            0 => {
                unsafe { libc::close(r) };
                let out = nested_child(sealed, mnt, answers).to_string();
                let bytes = out.as_bytes();
                unsafe { libc::write(w, bytes.as_ptr().cast(), bytes.len()) };
                unsafe { libc::close(w) };
                std::process::exit(0)
            }
            pid => {
                unsafe { libc::close(w) };
                let mut buf = Vec::new();
                let mut chunk = [0u8; 1024];
                loop {
                    let n = unsafe { libc::read(r, chunk.as_mut_ptr().cast(), chunk.len()) };
                    if n <= 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n as usize]);
                }
                unsafe { libc::close(r) };
                let mut status = 0;
                unsafe { libc::waitpid(pid, &mut status, 0) };
                serde_json::from_slice(&buf)
                    .unwrap_or_else(|e| json!({"umount": format!("the child said nothing ({e})")}))
            }
        }
    }

    fn nested_child(sealed: &str, mnt: &str, answers: &Path) -> Value {
        let cs = |s: &str| std::ffi::CString::new(s).unwrap();
        if unsafe { libc::unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWNS) } == -1 {
            return json!({"umount": format!("unshare: {}", super::probe::outcome(-1))});
        }
        let sealed_c = cs(sealed);
        let umount = super::probe::outcome(unsafe { libc::umount2(sealed_c.as_ptr(), 0) });
        let read_after = std::fs::read_to_string(answers).unwrap_or_default();
        // The other way round a cover: bind its parent somewhere else,
        // non-recursively, so the cover would not come along.
        let parent = Path::new(sealed).parent().unwrap().display().to_string();
        let bind = super::probe::outcome(unsafe {
            libc::mount(
                cs(&parent).as_ptr(),
                cs(mnt).as_ptr(),
                std::ptr::null(),
                libc::MS_BIND,
                std::ptr::null(),
            )
        });
        let read_via_bind =
            std::fs::read_to_string(Path::new(mnt).join("sealed/answers")).unwrap_or_default();
        json!({
            "umount": umount,
            "bind": bind,
            "read_after": read_after,
            "read_via_bind": read_via_bind,
        })
    }
}
