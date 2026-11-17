//! The directories the helper trusts, and the staging directory's files.

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, OwnedFd, RawFd};
use std::path::{Component, Path};

use crate::{refuse, sys, Config, Refusal, MAX_PROGRAM, PROGRAMS};

/// An I/O error, said with what was being done.
pub(crate) fn ctx<T>(r: std::io::Result<T>, what: &str) -> Result<T, Refusal> {
    r.map_err(|e| Refusal(format!("{what}: {e}")))
}

pub(crate) fn is_dir(st: &libc::stat) -> bool {
    st.st_mode & libc::S_IFMT == libc::S_IFDIR
}

pub(crate) fn is_file(st: &libc::stat) -> bool {
    st.st_mode & libc::S_IFMT == libc::S_IFREG
}

/// Whether anyone but the owner may write it.
fn others_write(st: &libc::stat) -> bool {
    st.st_mode & 0o022 != 0
}

/// `path`, opened one component at a time below the anchor, each with `O_NOFOLLOW`. The anchor and every directory
/// below it must be owned by the trusted uid and writable by no one else, so no component can be replaced by someone
/// who is not trusted, and a link in it is refused.
pub(crate) fn open_trusted(cfg: &Config, path: &Path) -> Result<OwnedFd, Refusal> {
    let Ok(below) = path.strip_prefix(&cfg.anchor) else {
        return refuse(format!(
            "{} is not under {}",
            path.display(),
            cfg.anchor.display()
        ));
    };
    let anchor = cfg.anchor.to_string_lossy();
    let mut fd = ctx(
        sys::open_path(
            &anchor,
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
        ),
        &format!("opening {anchor}"),
    )?;
    let mut here = cfg.anchor.clone();
    trusted(&fd, &here, cfg)?;
    for part in below.components() {
        let Component::Normal(name) = part else {
            return refuse(format!(
                "{} has a component that is not a plain name",
                path.display()
            ));
        };
        let name = name.to_string_lossy();
        here.push(&*name);
        fd = ctx(
            sys::openat(
                fd.as_raw_fd(),
                &name,
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
                0,
            ),
            &format!(
                "opening {} (a link, or not a directory, is refused)",
                here.display()
            ),
        )?;
        trusted(&fd, &here, cfg)?;
    }
    Ok(fd)
}

fn trusted(fd: &OwnedFd, at: &Path, cfg: &Config) -> Result<(), Refusal> {
    let st = ctx(
        sys::fstat(fd.as_raw_fd()),
        &format!("reading {}", at.display()),
    )?;
    if st.st_uid != cfg.trusted_uid || others_write(&st) {
        return refuse(format!(
            "{} must be owned by uid {} and writable by no one else",
            at.display(),
            cfg.trusted_uid
        ));
    }
    Ok(())
}

/// The staging directory, opened, and the uid that owns it. Its parents are trusted; it is a directory (not a link),
/// writable by no one but its owner, and for the real program owned by someone other than root.
pub(crate) fn open_stage(cfg: &Config) -> Result<(OwnedFd, u32), Refusal> {
    let (Some(parent), Some(leaf)) = (cfg.stage.parent(), cfg.stage.file_name()) else {
        return refuse("the staging directory has no parent");
    };
    let parent_fd = open_trusted(cfg, parent)?;
    let fd = ctx(
        sys::openat(
            parent_fd.as_raw_fd(),
            &leaf.to_string_lossy(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
            0,
        ),
        &format!(
            "opening {} (a link, or not a directory, is refused)",
            cfg.stage.display()
        ),
    )?;
    let st = ctx(sys::fstat(fd.as_raw_fd()), "reading the staging directory")?;
    if others_write(&st) {
        return refuse(format!(
            "{} is writable by its group or others",
            cfg.stage.display()
        ));
    }
    if cfg.real && st.st_uid == 0 {
        return refuse(format!(
            "{} is owned by root: it must be the service's user's",
            cfg.stage.display()
        ));
    }
    Ok((fd, st.st_uid))
}

/// The names in `fd` must be exactly the four programs.
pub(crate) fn require_programs(fd: RawFd, what: &str) -> Result<(), Refusal> {
    let have = ctx(sys::list(fd), &format!("listing {what}"))?;
    let mut want: Vec<String> = PROGRAMS.iter().map(|p| (*p).to_string()).collect();
    want.sort();
    if have == want {
        return Ok(());
    }
    let extra: Vec<&String> = have.iter().filter(|n| !want.contains(n)).collect();
    let missing: Vec<&String> = want.iter().filter(|n| !have.contains(n)).collect();
    refuse(format!(
        "{what} must hold exactly {}: extra {extra:?}, missing {missing:?}",
        PROGRAMS.join(", ")
    ))
}

/// One program, copied from the staging directory into `dst` (a directory descriptor): a regular file owned by the
/// staging directory's owner with one link, opened without following links and checked on the open descriptor, then
/// copied into a file made for the purpose, 0755, and for the real program root's.
pub(crate) fn copy_program(
    src: RawFd,
    stage_uid: u32,
    name: &str,
    dst: RawFd,
    cfg: &Config,
) -> Result<(), Refusal> {
    let what = format!("{name} in the staging directory");
    // Looked at before it is opened, so a device or a pipe is never opened.
    let Some(before) = ctx(sys::lstat_at(src, name), &what)? else {
        return refuse(format!("{what} is missing"));
    };
    if !is_file(&before) {
        return refuse(format!(
            "{what} is not a regular file (a link or another kind is refused)"
        ));
    }
    let from = ctx(
        sys::openat(
            src,
            name,
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK,
            0,
        ),
        &what,
    )?;
    let st = ctx(sys::fstat(from.as_raw_fd()), &what)?;
    if !is_file(&st) || st.st_dev != before.st_dev || st.st_ino != before.st_ino {
        return refuse(format!("{what} changed while it was opened"));
    }
    if st.st_uid != stage_uid {
        return refuse(format!(
            "{what} is not owned by the staging directory's owner"
        ));
    }
    if st.st_nlink != 1 {
        return refuse(format!("{what} has more than one link"));
    }
    if st.st_size <= 0 || st.st_size as u64 > MAX_PROGRAM {
        return refuse(format!(
            "{what} is empty or larger than {MAX_PROGRAM} bytes"
        ));
    }
    let out = ctx(
        sys::openat(
            dst,
            name,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW,
            0o600,
        ),
        &format!("creating {name}"),
    )?;
    let (mut input, mut output) = (sys::file(from), sys::file(out));
    copy_capped(&mut input, &mut output, &what)?;
    if cfg.real {
        ctx(
            sys::fchown(output.as_raw_fd(), 0, 0),
            &format!("giving {name} to root"),
        )?;
    }
    ctx(
        sys::fchmod(output.as_raw_fd(), 0o755),
        &format!("setting {name}'s mode"),
    )?;
    ctx(output.sync_all(), &format!("syncing {name}"))
}

/// Everything of `input`, up to the cap (a file that grows as it is read is refused), requiring an ELF header.
fn copy_capped(input: &mut File, output: &mut File, what: &str) -> Result<(), Refusal> {
    let mut buf = vec![0u8; 1 << 16];
    let mut total: u64 = 0;
    loop {
        let n = ctx(input.read(&mut buf), what)?;
        if n == 0 {
            break;
        }
        if total == 0 && !buf[..n].starts_with(b"\x7fELF") {
            return refuse(format!("{what} is not a program (no ELF header)"));
        }
        total += n as u64;
        if total > MAX_PROGRAM {
            return refuse(format!("{what} is larger than {MAX_PROGRAM} bytes"));
        }
        ctx(output.write_all(&buf[..n]), "writing the copy")?;
    }
    if total == 0 {
        return refuse(format!("{what} is empty"));
    }
    Ok(())
}
