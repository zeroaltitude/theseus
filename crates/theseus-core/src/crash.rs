//! The crash file (Review 2's consideration 1, theseus-xonq). The release
//! build aborts on a panic (`panic = "abort"`), which is kept: a restart takes
//! about 20 ms, and nothing half-done survives a panic to do harm. Before the
//! abort, a panic hook writes what panicked (the thread, the location, and
//! the message) to `crash.json` beside the store, so the next start can say
//! what ended the last one, and health can show it.
//!
//! The next start takes the file after serving (`take`): it moves it into
//! `crashes/`, where every crash is kept, says so in the log and in a
//! `server.crashed` row, and health reports it (`last`). The message stays in
//! the file, as it is in the log: a panic's message can quote any text the
//! daemon held, so neither the row nor health carries it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Where each crash file goes once a start has found it.
pub const DIR: &str = "crashes";

/// The file the panic hook writes in the state dir, one for each daemon a
/// state dir may have (`socket`, `stdio`), so neither takes the other's.
pub fn file(state_dir: &Path, mode: &str) -> PathBuf {
    state_dir.join(format!("crash-{mode}.json"))
}
/// A message longer than this is cut in the file.
const MESSAGE_MAX: usize = 4096;

/// What panicked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Crash {
    pub at_unix_ms: u64,
    pub pid: u32,
    /// The build that panicked.
    pub version: String,
    /// `socket` or `stdio`: which daemon of the state dir it was.
    pub mode: String,
    /// The thread's name (`main`, a runtime worker, `store-verify`), or its id.
    pub thread: String,
    /// `file:line:column`.
    pub location: String,
    pub message: String,
}

/// Install the panic hook for a daemon serving from `state_dir`. It writes
/// the crash file, then runs the hook it replaced (which prints the panic to
/// stderr, the log), and the abort follows. A crash file it cannot write is
/// said on stderr; the panic goes on as it would have.
///
/// Two threads that panic at once both run the hook (theseus-g7pw). The
/// first to take `WRITTEN` writes its crash; the other waits for that write
/// to end, then writes nothing, so its abort cannot cut the first's write
/// short, and the file holds the panic that began the end. The later panic
/// is still printed by the hook it replaced, in the log.
pub fn install(state_dir: &Path, mode: &'static str) {
    static WRITTEN: std::sync::Mutex<bool> = std::sync::Mutex::new(false);
    let path = file(state_dir, mode);
    let before = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let crash = Crash::of(info, mode);
        {
            let mut written = WRITTEN.lock().unwrap_or_else(|e| e.into_inner());
            if !*written {
                *written = true;
                if let Err(e) = write(&path, &crash) {
                    eprintln!(
                        "theseusd: the crash file {} was not written: {e}",
                        path.display()
                    );
                }
            }
        }
        before(info);
    }));
}

impl Crash {
    fn of(info: &std::panic::PanicHookInfo<'_>, mode: &str) -> Self {
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(a panic whose payload is not text)".into());
        let current = std::thread::current();
        Crash {
            at_unix_ms: theseus_protocol::now_unix_ms(),
            pid: std::process::id(),
            version: crate::VERSION.into(),
            mode: mode.into(),
            thread: current
                .name()
                .map_or_else(|| format!("{:?}", current.id()), str::to_string),
            location: info.location().map_or_else(
                || "unknown".into(),
                |l| format!("{}:{}:{}", l.file(), l.line(), l.column()),
            ),
            message: crate::toolrun::cap(&message, MESSAGE_MAX, |_| String::new()).0,
        }
    }
}

/// Write `crash` to `path` whole, or not at all: a temporary file, synced,
/// then renamed over it. The temporary file is this writer's own, named by
/// its pid and thread: a shared name let a second writer's create truncate
/// the first's file before its rename, and the crash file was left empty
/// (theseus-g7pw).
fn write(path: &Path, crash: &Crash) -> std::io::Result<()> {
    use std::io::Write as _;
    let tmp = path.with_extension(format!(
        "json.{}.{}.tmp",
        std::process::id(),
        // SAFETY: gettid has no arguments.
        unsafe { libc::syscall(libc::SYS_gettid) }
    ));
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(&serde_json::to_vec_pretty(crash).map_err(std::io::Error::other)?)?;
    f.sync_all()?;
    std::fs::rename(&tmp, path)
}

/// The crash file the last run of this mode left, taken by this start: moved
/// into `crashes/` as `crash-<unix ms>-<pid>.json`, which is returned with
/// it. `None` when the last run left none.
pub fn take(state_dir: &Path, mode: &str) -> anyhow::Result<Option<(Crash, PathBuf)>> {
    let path = file(state_dir, mode);
    let text = match std::fs::read(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let crash: Crash = serde_json::from_slice(&text)?;
    let dir = state_dir.join(DIR);
    std::fs::create_dir_all(&dir)?;
    let kept = dir.join(format!("crash-{}-{}.json", crash.at_unix_ms, crash.pid));
    std::fs::rename(&path, &kept)?;
    Ok(Some((crash, kept)))
}

/// The newest crash of this mode a start has found, with its file: what
/// health reports.
pub fn last(state_dir: &Path, mode: &str) -> Option<(Crash, PathBuf)> {
    let mut newest: Option<(Crash, PathBuf)> = None;
    for e in std::fs::read_dir(state_dir.join(DIR)).ok()?.flatten() {
        let p = e.path();
        if p.extension().is_none_or(|x| x != "json") {
            continue;
        }
        let Some(c) = std::fs::read(&p)
            .ok()
            .and_then(|t| serde_json::from_slice::<Crash>(&t).ok())
            .filter(|c| c.mode == mode)
        else {
            continue;
        };
        if newest
            .as_ref()
            .is_none_or(|(n, _)| c.at_unix_ms > n.at_unix_ms)
        {
            newest = Some((c, p));
        }
    }
    newest
}

/// A planted panic for tests of the crash file: a debug build whose
/// `THESEUS_TEST_PANIC` names `at` panics there, then aborts, as the release
/// build's `panic = "abort"` does after the hook (a debug build unwinds, and
/// a runtime would catch a task's panic). The process is made non-dumpable
/// first: on WSL a core goes to the Windows drive, about 40 MB a test run.
/// A release build has no such plant.
pub fn planted(at: &str) {
    #[cfg(debug_assertions)]
    if std::env::var("THESEUS_TEST_PANIC").is_ok_and(|v| v == at) {
        let _ = std::panic::catch_unwind(|| panic!("a planted panic at {at} (THESEUS_TEST_PANIC)"));
        // SAFETY: prctl with PR_SET_DUMPABLE takes plain integers and
        // touches nothing of ours.
        unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) };
        std::process::abort();
    }
    #[cfg(not(debug_assertions))]
    let _ = at;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crash(at: u64, location: &str) -> Crash {
        Crash {
            at_unix_ms: at,
            pid: 42,
            version: "0.0.0".into(),
            mode: "socket".into(),
            thread: "tokio-runtime-worker".into(),
            location: location.into(),
            message: "byte index 5 is not a char boundary".into(),
        }
    }

    /// The child of `two_threads_that_panic_at_once_leave_one_whole_crash_file`:
    /// with `THESEUS_TEST_CRASH_CHILD` naming a state dir, it installs the
    /// hook over one that aborts, as the release build's `panic = "abort"`
    /// does after it, and eight threads panic at once. Run alone, it does
    /// nothing.
    #[test]
    fn panicking_child() {
        let Some(state) = std::env::var_os("THESEUS_TEST_CRASH_CHILD") else {
            return;
        };
        std::panic::set_hook(Box::new(|_| {
            // SAFETY: prctl with PR_SET_DUMPABLE takes plain integers and
            // touches nothing of ours: no core for a planted abort.
            unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) };
            std::process::abort();
        }));
        install(Path::new(&state), "socket");
        let at_once = std::sync::Arc::new(std::sync::Barrier::new(8));
        let threads: Vec<_> = (0..8)
            .map(|i| {
                let at_once = at_once.clone();
                std::thread::Builder::new()
                    .name(format!("panicker-{i}"))
                    .spawn(move || {
                        at_once.wait();
                        panic!("a planted panic on thread {i}");
                    })
                    .unwrap()
            })
            .collect();
        for t in threads {
            let _ = t.join();
        }
        unreachable!("the first panic aborts the process");
    }

    /// Two threads that panic at once (theseus-g7pw): each writer had the
    /// same temporary file, so the second's create truncated the first's
    /// before its rename, or the second's abort cut the first's write short,
    /// and the crash file was left empty or absent. Now the first panic's
    /// record is whole, every time, and no temporary file is left.
    #[test]
    fn two_threads_that_panic_at_once_leave_one_whole_crash_file() {
        let exe = std::env::current_exe().unwrap();
        for run in 0..40 {
            let dir = tempfile::tempdir().unwrap();
            let out = std::process::Command::new(&exe)
                .args(["--exact", "crash::tests::panicking_child", "--nocapture"])
                .env("THESEUS_TEST_CRASH_CHILD", dir.path())
                .stdin(std::process::Stdio::null())
                .output()
                .unwrap();
            assert!(
                !out.status.success(),
                "run {run}: the child aborts: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            let text = std::fs::read(file(dir.path(), "socket")).unwrap_or_else(|e| {
                panic!(
                    "run {run}: no crash file ({e}); the dir holds {:?}",
                    std::fs::read_dir(dir.path())
                        .unwrap()
                        .flatten()
                        .map(|e| e.file_name())
                        .collect::<Vec<_>>()
                )
            });
            let crash: Crash = serde_json::from_slice(&text).unwrap_or_else(|e| {
                panic!(
                    "run {run}: the crash file is not whole ({e}): {:?}",
                    String::from_utf8_lossy(&text)
                )
            });
            assert!(
                crash.thread.starts_with("panicker-")
                    && crash.message == format!("a planted panic on thread {}", &crash.thread[9..]),
                "run {run}: {crash:?}"
            );
            let left: Vec<_> = std::fs::read_dir(dir.path())
                .unwrap()
                .flatten()
                .map(|e| e.file_name())
                .collect();
            assert_eq!(left.len(), 1, "run {run}: only the crash file: {left:?}");
        }
    }

    /// A start takes the file the last run left, keeps it under `crashes/`,
    /// and health's `last` is the newest kept; a start that finds none takes
    /// nothing.
    #[test]
    fn a_start_takes_the_crash_file_and_health_reads_the_newest() {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path();
        assert!(take(state, "socket").unwrap().is_none());
        assert!(last(state, "socket").is_none());
        write(&file(state, "socket"), &crash(1_000, "src/html.rs:120:9")).unwrap();
        // The stdio daemon of the same state dir takes only its own.
        assert!(take(state, "stdio").unwrap().is_none());
        let (c, kept) = take(state, "socket").unwrap().unwrap();
        assert_eq!(c.location, "src/html.rs:120:9");
        assert_eq!(kept, state.join(DIR).join("crash-1000-42.json"));
        assert!(
            !file(state, "socket").exists(),
            "taken, so the next start finds none"
        );
        assert!(take(state, "socket").unwrap().is_none());
        write(&file(state, "socket"), &crash(2_000, "src/wake.rs:88:5")).unwrap();
        take(state, "socket").unwrap().unwrap();
        let (newest, path) = last(state, "socket").unwrap();
        assert_eq!(
            (newest.at_unix_ms, newest.location.as_str()),
            (2_000, "src/wake.rs:88:5")
        );
        assert!(path.ends_with("crash-2000-42.json"));
        assert!(last(state, "stdio").is_none());
    }
}
