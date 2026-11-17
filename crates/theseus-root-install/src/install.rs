//! The install itself: copy into a directory made beside the target, check the copy, and rename it into place.

use std::fs::File;
use std::io::Read;
use std::os::fd::{AsRawFd, OwnedFd, RawFd};

use crate::stage::{
    copy_program, ctx, is_dir, is_file, open_stage, open_trusted, require_programs,
};
use crate::{refuse, stamp, sys, Config, Outcome, Refusal, MAX_BUILDS, PROGRAMS};

/// Install `commit`: one directory, `<lib dir>/<first eight digits>`, or a refusal that changed nothing.
pub(crate) fn install(cfg: &Config, commit: &str) -> Result<Outcome, Refusal> {
    if cfg.real && sys::euid() != 0 {
        return refuse("it must run as root, through its sudoers rule");
    }
    sys::set_umask(0o022);
    let (stage, stage_uid) = open_stage(cfg)?;
    require_programs(stage.as_raw_fd(), "the staging directory")?;
    let lib = open_trusted(cfg, &cfg.lib_dir)?;
    let c8 = &commit[..8];
    let target = cfg.lib_dir.join(c8);
    let exists = ctx(
        sys::lstat_at(lib.as_raw_fd(), c8),
        "looking for the build's directory",
    )?
    .is_some();
    if !exists {
        let builds = ctx(sys::list(lib.as_raw_fd()), "listing the program directory")?
            .iter()
            .filter(|n| {
                n.len() == 8
                    && n.bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
            .count();
        if builds >= MAX_BUILDS {
            return refuse(format!(
                "{} already holds {builds} builds (at most {MAX_BUILDS}): the administrator removes old ones",
                cfg.lib_dir.display()
            ));
        }
    }
    let temp = format!(".install-{c8}-{}", std::process::id());
    ctx(
        sys::mkdirat(lib.as_raw_fd(), &temp, 0o700),
        "making the temporary directory",
    )?;
    let outcome = publish(cfg, commit, (&stage, stage_uid), &lib, &temp, &target);
    // Whatever happened, the temporary directory is this run's own, and is gone (a rename took it, or it is removed).
    remove_temp(lib.as_raw_fd(), &temp);
    outcome
}

fn publish(
    cfg: &Config,
    commit: &str,
    (stage, stage_uid): (&OwnedFd, u32),
    lib: &OwnedFd,
    temp: &str,
    target: &std::path::Path,
) -> Result<Outcome, Refusal> {
    let c8 = &commit[..8];
    let tmp = ctx(
        sys::openat(
            lib.as_raw_fd(),
            temp,
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
            0,
        ),
        "opening the temporary directory",
    )?;
    for name in PROGRAMS {
        copy_program(stage.as_raw_fd(), stage_uid, name, tmp.as_raw_fd(), cfg)?;
    }
    // The stamp is read from the copy: the owner can no longer change it.
    let bytes = read_all(&tmp, "theseusd")?;
    if let Err(why) = stamp::names(&bytes, commit) {
        return refuse(format!("the theseusd it was given is not {commit}: {why}"));
    }
    if cfg.real {
        ctx(
            sys::fchown(tmp.as_raw_fd(), 0, 0),
            "giving the directory to root",
        )?;
    }
    ctx(
        sys::fchmod(tmp.as_raw_fd(), 0o755),
        "setting the directory's mode",
    )?;
    ctx(sys::fsync(tmp.as_raw_fd()), "syncing the directory")?;
    match sys::rename_noreplace(lib.as_raw_fd(), temp, c8) {
        Ok(()) => {
            ctx(sys::fsync(lib.as_raw_fd()), "syncing the program directory")?;
            Ok(Outcome::Installed(target.to_path_buf()))
        }
        Err(e)
            if e.raw_os_error() == Some(libc::EEXIST)
                || e.raw_os_error() == Some(libc::ENOTEMPTY) =>
        {
            same_as_copy(cfg, lib.as_raw_fd(), c8, tmp.as_raw_fd())?;
            Ok(Outcome::Already(target.to_path_buf()))
        }
        Err(e) => refuse(format!("renaming it into place: {e}")),
    }
}

fn read_all(dir: &OwnedFd, name: &str) -> Result<Vec<u8>, Refusal> {
    let fd = ctx(
        sys::openat(dir.as_raw_fd(), name, libc::O_RDONLY | libc::O_NOFOLLOW, 0),
        name,
    )?;
    let mut bytes = Vec::new();
    ctx(sys::file(fd).read_to_end(&mut bytes), name)?;
    Ok(bytes)
}

/// The directory already there holds exactly what was copied: a directory, root's, 0755, the four programs and
/// nothing else, each a regular file with the same bytes. Anything else is a refusal: it is never overwritten.
fn same_as_copy(cfg: &Config, lib: RawFd, c8: &str, copy: RawFd) -> Result<(), Refusal> {
    let differs = |why: String| -> Result<(), Refusal> {
        refuse(format!(
            "{c8} already exists and differs ({why}); it is never overwritten"
        ))
    };
    let there = ctx(
        sys::openat(
            lib,
            c8,
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
            0,
        ),
        "opening the existing directory",
    )?;
    let st = ctx(
        sys::fstat(there.as_raw_fd()),
        "reading the existing directory",
    )?;
    if !is_dir(&st) || st.st_uid != cfg.trusted_uid || st.st_mode & 0o7777 != 0o755 {
        return differs("not a root-owned 0755 directory".into());
    }
    if let Err(Refusal(why)) = require_programs(there.as_raw_fd(), "the existing directory") {
        return differs(why);
    }
    for name in PROGRAMS {
        let Some(st) = ctx(sys::lstat_at(there.as_raw_fd(), name), name)? else {
            return differs(format!("{name} is missing"));
        };
        if !is_file(&st) || st.st_uid != cfg.trusted_uid || st.st_mode & 0o7777 != 0o755 {
            return differs(format!("{name} is not a root-owned 0755 regular file"));
        }
        let a = sys::openat(
            there.as_raw_fd(),
            name,
            libc::O_RDONLY | libc::O_NOFOLLOW,
            0,
        );
        let b = sys::openat(copy, name, libc::O_RDONLY | libc::O_NOFOLLOW, 0);
        let (a, b) = (ctx(a, name)?, ctx(b, name)?);
        if !same_bytes(sys::file(a), sys::file(b))? {
            return differs(format!("{name} has other content"));
        }
    }
    Ok(())
}

fn same_bytes(mut a: File, mut b: File) -> Result<bool, Refusal> {
    let (mut x, mut y) = (vec![0u8; 1 << 16], vec![0u8; 1 << 16]);
    loop {
        let n = ctx(a.read(&mut x), "comparing")?;
        let mut got = 0;
        while got < n {
            let m = ctx(b.read(&mut y[got..n]), "comparing")?;
            if m == 0 {
                return Ok(false);
            }
            got += m;
        }
        if x[..n] != y[..n] {
            return Ok(false);
        }
        if n == 0 {
            // `a` is at its end; `b` must be too.
            return Ok(ctx(b.read(&mut y[..1]), "comparing")? == 0);
        }
    }
}

/// Remove this run's temporary directory, if it is still there (a rename took it away on success).
fn remove_temp(lib: RawFd, temp: &str) {
    let Ok(Some(_)) = sys::lstat_at(lib, temp) else {
        return;
    };
    if let Ok(fd) = sys::openat(
        lib,
        temp,
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
        0,
    ) {
        for name in sys::list(fd.as_raw_fd()).unwrap_or_default() {
            let _ = sys::unlinkat(fd.as_raw_fd(), &name, false);
        }
    }
    let _ = sys::unlinkat(lib, temp, true);
}
