//! A program on a pty (theseus-n88g.4): opened with libc's `posix_openpt`,
//! its child started through `children::spawn` as every child the daemon
//! starts is, in a session and process group of its own (`setsid`), with
//! the pty as its controlling terminal, so a close reaches the program and
//! everything it started.
//!
//! A thread per terminal reads the pty's master and feeds the screen. It
//! blocks in `read` (never on a runtime worker) and ends when the pty's
//! last slave closes, which is when the program and every child that held
//! the terminal have gone.

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use theseus_kernel::children;
use theseus_kernel::tree;

use super::vt::Screen;

/// What the reader thread and the tools share.
pub struct Shared {
    pub screen: Mutex<Screen>,
    /// Bumped at every chunk of output: a read that waits watches it.
    pub version: tokio::sync::watch::Sender<u64>,
    /// The pty's last slave has closed: nothing more will be written.
    pub hung_up: AtomicBool,
    /// When output last came, in ms since the epoch.
    pub last_output_ms: AtomicU64,
    /// Bytes the program wrote, and bytes sent to it.
    pub bytes_out: AtomicU64,
    pub bytes_in: AtomicU64,
}

/// A running program and its pty's master.
pub struct Pty {
    pub shared: Arc<Shared>,
    master: Mutex<File>,
    child: Mutex<Child>,
    pid: u32,
    /// The pty's slave (`/dev/pts/N`): a process that still holds it open
    /// at a close is the terminal's, whatever tree it left.
    slave_path: std::path::PathBuf,
}

/// What a pty's program is started with.
pub struct Spawn<'a> {
    pub argv: &'a [String],
    pub cwd: &'a Path,
    pub env: &'a [(String, String)],
    pub rows: u16,
    pub cols: u16,
    pub umask: Option<u32>,
}

fn last_error(what: &str) -> std::io::Error {
    let e = std::io::Error::last_os_error();
    std::io::Error::new(e.kind(), format!("{what}: {e}"))
}

/// A new pty pair: the master, and the slave's open file.
fn open_pair(rows: u16, cols: u16) -> std::io::Result<(OwnedFd, File, std::path::PathBuf)> {
    // SAFETY: plain libc calls on a descriptor this function owns; each
    // result is checked before it is used.
    unsafe {
        let m = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC);
        if m < 0 {
            return Err(last_error("posix_openpt"));
        }
        let master = OwnedFd::from_raw_fd(m);
        if libc::grantpt(m) != 0 {
            return Err(last_error("grantpt"));
        }
        if libc::unlockpt(m) != 0 {
            return Err(last_error("unlockpt"));
        }
        let mut name = [0 as libc::c_char; 128];
        if libc::ptsname_r(m, name.as_mut_ptr(), name.len()) != 0 {
            return Err(last_error("ptsname_r"));
        }
        let s = libc::open(
            name.as_ptr(),
            libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC,
        );
        if s < 0 {
            return Err(last_error("opening the pty's slave"));
        }
        let slave = File::from_raw_fd(s);
        let path = std::ffi::CStr::from_ptr(name.as_ptr())
            .to_string_lossy()
            .into_owned();
        let size = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        if libc::ioctl(m, libc::TIOCSWINSZ, &size) != 0 {
            return Err(last_error("setting the pty's size"));
        }
        Ok((master, slave, path.into()))
    }
}

impl Pty {
    /// Start `argv` on a new pty, and its reader thread.
    pub fn spawn(s: &Spawn<'_>) -> std::io::Result<Pty> {
        let program = s
            .argv
            .first()
            .ok_or_else(|| std::io::Error::other("no program to run"))?;
        let (master, slave, slave_path) = open_pair(s.rows, s.cols)?;
        let mut cmd = Command::new(program);
        cmd.args(&s.argv[1..])
            .current_dir(s.cwd)
            .env_clear()
            .envs(s.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .stdin(Stdio::from(slave.try_clone()?))
            .stdout(Stdio::from(slave.try_clone()?))
            .stderr(Stdio::from(slave));
        let umask = s.umask;
        // SAFETY: setsid, ioctl, and umask are async-signal-safe, and touch
        // only the child.
        unsafe {
            cmd.pre_exec(move || {
                // Its own session and process group, so a close reaches the
                // whole group, and the pty is its controlling terminal, so
                // Ctrl-C and a hang-up reach the program in front.
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::ioctl(0, libc::TIOCSCTTY, 0) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                if let Some(u) = umask {
                    libc::umask(u);
                }
                Ok(())
            });
        }
        // Registered as an owned child as it is spawned: its spawner (the
        // close) waits for it, and the daemon's sweep leaves it alone.
        let child = children::spawn(children::Kind::Owned, || cmd.spawn(), |c| Some(c.id()))?;
        drop(cmd);
        let pid = child.id();
        let reader = File::from(master.try_clone()?);
        let writer = File::from(master);
        let shared = Arc::new(Shared {
            screen: Mutex::new(Screen::new(s.rows, s.cols)),
            version: tokio::sync::watch::Sender::new(0),
            hung_up: AtomicBool::new(false),
            last_output_ms: AtomicU64::new(theseus_protocol::now_unix_ms()),
            bytes_out: AtomicU64::new(0),
            bytes_in: AtomicU64::new(0),
        });
        let replies = writer.try_clone()?;
        let feed = shared.clone();
        std::thread::Builder::new()
            .name(format!("term-{pid}"))
            .spawn(move || read_loop(reader, replies, &feed))?;
        Ok(Pty {
            shared,
            master: Mutex::new(writer),
            child: Mutex::new(child),
            pid,
            slave_path,
        })
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Send bytes to the program, as typed keys.
    pub fn send(&self, bytes: &[u8]) -> std::io::Result<()> {
        let mut m = self.master.lock().unwrap_or_else(|p| p.into_inner());
        m.write_all(bytes)?;
        self.shared
            .bytes_in
            .fetch_add(bytes.len() as u64, Ordering::Relaxed);
        Ok(())
    }

    /// How the program ended, once it has; it is reaped then.
    pub fn exited(&self) -> Option<std::process::ExitStatus> {
        let mut c = self.child.lock().unwrap_or_else(|p| p.into_inner());
        c.try_wait().ok().flatten()
    }

    /// Stop the program and everything it started: a hang-up and SIGTERM to
    /// its process group and to each of its descendants, up to `grace` for
    /// them to go, then SIGKILL to what is left, and the program reaped. It
    /// blocks for up to the grace and the kill's wait, so it runs off the
    /// runtime's workers. Returns how the program ended, and how many
    /// processes it had to kill.
    pub fn close(&self, grace: Duration) -> (Option<std::process::ExitStatus>, usize) {
        let mut c = self.child.lock().unwrap_or_else(|p| p.into_inner());
        if let Ok(Some(status)) = c.try_wait() {
            // The program is gone; anything it left in its group still goes.
            signal_group(self.pid, libc::SIGKILL);
            return (Some(status), 0);
        }
        // Its descendants as they are now: one that left the process group
        // (a `setsid` of its own) is still signalled by its pid.
        let mut procs = tree::descendants(self.pid);
        procs.extend(holders(&self.slave_path));
        if let Some(st) = tree::stat(self.pid) {
            procs.push(tree::Proc {
                pid: self.pid,
                start: st.start,
            });
        }
        for sig in [libc::SIGHUP, libc::SIGTERM] {
            signal_group(self.pid, sig);
            for p in &procs {
                tree::signal(*p, sig);
            }
        }
        let t0 = Instant::now();
        while t0.elapsed() < grace {
            if matches!(c.try_wait(), Ok(Some(_))) && procs.iter().all(|p| !tree::alive(*p)) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut killed = 0;
        let late: Vec<tree::Proc> = procs
            .iter()
            .copied()
            .chain(tree::descendants(self.pid))
            .chain(holders(&self.slave_path))
            .filter(|p| tree::alive(*p))
            .collect();
        signal_group(self.pid, libc::SIGKILL);
        for p in late {
            if tree::signal(p, libc::SIGKILL) {
                killed += 1;
            }
        }
        (c.wait().ok(), killed)
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        // A terminal dropped without its close (a test's panic): nothing it
        // ran outlives it.
        if self.exited().is_none() {
            self.close(Duration::ZERO);
        }
    }
}

/// Every process that holds the pty's slave open: what the terminal ran,
/// including a process that left its tree (`setsid` and a double fork, or a
/// parent that died). Read from `/proc/*/fd`, only at a close.
fn holders(slave: &Path) -> Vec<tree::Proc> {
    let me = std::process::id();
    let Ok(procs) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    procs
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter(|&pid| pid != me)
        .filter(|&pid| {
            std::fs::read_dir(format!("/proc/{pid}/fd")).is_ok_and(|fds| {
                fds.flatten()
                    .any(|fd| std::fs::read_link(fd.path()).is_ok_and(|l| l == slave))
            })
        })
        .filter_map(|pid| {
            Some(tree::Proc {
                pid,
                start: tree::stat(pid)?.start,
            })
        })
        .collect()
}

fn signal_group(pgid: u32, sig: libc::c_int) {
    // SAFETY: a signal to a process group this terminal's program leads.
    unsafe {
        libc::kill(-(pgid as libc::pid_t), sig);
    }
}

/// The reader thread: the pty's output into the screen, the screen's
/// answers back to the program, until the last slave closes (EIO).
fn read_loop(mut master: File, mut replies: File, shared: &Shared) {
    let mut buf = [0u8; 8192];
    loop {
        match master.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let answer = {
                    let mut s = shared.screen.lock().unwrap_or_else(|p| p.into_inner());
                    s.feed(&buf[..n]);
                    s.take_replies()
                };
                if !answer.is_empty() {
                    let _ = replies.write_all(&answer);
                }
                shared.bytes_out.fetch_add(n as u64, Ordering::Relaxed);
                shared
                    .last_output_ms
                    .store(theseus_protocol::now_unix_ms(), Ordering::Relaxed);
                shared.version.send_modify(|v| *v += 1);
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    shared.hung_up.store(true, Ordering::Relaxed);
    shared.version.send_modify(|v| *v += 1);
}
