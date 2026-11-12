//! `fs.rs`'s own tests: the file tools (`read`, `write`, `edit`, `patch`, `glob`, `grep`,
//! `list`) on real trees, a FIFO, a socket or a device refused, a read cut at its cap, and
//! `grep` on borrowed cores.
//!
//! They were an inline `mod tests` at the foot of `fs.rs`, 28 % of that file. They read
//! `fs`'s crate-visible surface only (`INLINE_FILES` is `pub(crate)` for them), so they sit
//! beside it, as the crate's other `tests_*` modules do, and the shape budget's 2,500 lines
//! are `fs.rs`'s own.

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::SystemTime;

use serde_json::{json, Value};

use crate::fs::*;
use crate::{Cores, Media, Tool, ToolCtx};

fn ctx(d: &tempfile::TempDir) -> ToolCtx {
    ToolCtx::for_tests(d.path())
}

/// A PNG header for the given size, then `pad` zero bytes.
fn png(width: u32, height: u32, pad: usize) -> Vec<u8> {
    let mut v = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
    v.extend_from_slice(&width.to_be_bytes());
    v.extend_from_slice(&height.to_be_bytes());
    v.extend_from_slice(&[8, 2, 0, 0, 0, 0, 0, 0, 0]);
    v.resize(v.len() + pad, 0);
    v
}

/// A named pipe at `path`.
fn mkfifo(path: &Path) {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0, "mkfifo");
}

/// `call` on another thread, given five seconds. A call that waits on a
/// FIFO is let go, by opening the FIFO's other end, before the test fails,
/// so a revert of the fix fails here rather than hanging the suite.
fn within_5s<T: Send + 'static>(fifo: &Path, call: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(call());
    });
    match rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(r) => r,
        Err(_) => {
            drop(fs::OpenOptions::new().write(true).open(fifo));
            panic!("the call waited on {} for 5 s", fifo.display());
        }
    }
}

/// Review 2's R9: a FIFO in the roots is refused at once, by name, by
/// every tool that reads a file, never read (a read waits for a writer,
/// holding a core and a thread). So are a socket and a device.
#[test]
fn a_fifo_a_socket_or_a_device_is_refused_never_read() {
    let d = tempfile::tempdir().unwrap();
    let fifo = d.path().join("pipe");
    mkfifo(&fifo);
    let base = d.path().to_path_buf();
    let read = within_5s(&fifo, move || {
        Read.run(&json!({"path": "pipe"}), &ToolCtx::for_tests(&base))
    });
    let e = read.unwrap_err().message;
    assert!(
        e.ends_with("pipe is a FIFO, not a regular file; fs_read reads only regular files (a read of a FIFO, a socket, or a device can wait forever for a writer)"),
        "{e}"
    );
    let base = d.path().to_path_buf();
    let edit = within_5s(&fifo, move || {
        Edit.run(
            &json!({"path": "pipe", "old_string": "a", "new_string": "b"}),
            &ToolCtx::for_tests(&base),
        )
    });
    assert!(edit
        .unwrap_err()
        .message
        .contains("pipe is a FIFO, not a regular file; fs_edit"));
    let base = d.path().to_path_buf();
    let diff = within_5s(&fifo, move || {
        crate::text::Diff.run(
            &json!({"a_path": "pipe", "b": "x\n"}),
            &ToolCtx::for_tests(&base),
        )
    });
    assert!(diff
        .unwrap_err()
        .message
        .contains("is a FIFO, not a regular file; text_diff"));
    // A search passes it by, named or walked past.
    for path in ["pipe", "."] {
        let base = d.path().to_path_buf();
        let grep = within_5s(&fifo, move || {
            Grep.run(
                &json!({"pattern": "a", "path": path}),
                &ToolCtx::for_tests(&base),
            )
        });
        assert!(grep.is_ok(), "{path}");
    }
    let _sock = std::os::unix::net::UnixListener::bind(d.path().join("sock")).unwrap();
    let e = Read
        .run(&json!({"path": "sock"}), &ctx(&d))
        .unwrap_err()
        .message;
    assert!(e.contains("sock is a socket, not a regular file"), "{e}");
    let e = Read
        .run(&json!({"path": "/dev/null"}), &ctx(&d))
        .unwrap_err()
        .message;
    assert!(
        e.contains("/dev/null is a character device, not a regular file"),
        "{e}"
    );
}

/// The read goes through the cap, so a file that grew after its size was
/// checked is cut there, never read whole (review 2's R9).
#[test]
fn a_read_stops_at_its_cap() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("grows.txt");
    fs::write(&p, "0123456789").unwrap();
    assert_eq!(read_regular(&p, 10, true).unwrap(), b"0123456789");
    match read_regular(&p, 4, true) {
        Err(Unread::Over(n)) => assert_eq!(n, 5, "it read one byte past the cap, no more"),
        other => panic!("{other:?}"),
    }
    let link = d.path().join("link.txt");
    std::os::unix::fs::symlink(&p, &link).unwrap();
    assert!(read_regular(&link, 10, true).is_ok());
    assert!(
        matches!(
            read_regular(&link, 10, false),
            Err(Unread::Kind(_) | Unread::Io(_))
        ),
        "without follow, a link is not read"
    );
}

/// `fs.read` of an image returns it for the model (theseus-9g2); one
/// over the 5 MiB limit is described and refused with the reason.
#[test]
fn reading_an_image_returns_it_and_a_huge_one_is_refused() {
    let d = tempfile::tempdir().unwrap();
    let c = ctx(&d);
    let bytes = png(640, 480, 500);
    std::fs::write(d.path().join("shot.png"), &bytes).unwrap();
    let (out, img) = Read
        .run_with_media(&json!({"path": "shot.png"}), &c)
        .unwrap();
    assert!(
        out.text
            .ends_with("shot.png is a PNG image, 640×480, 533 bytes."),
        "{}",
        out.text
    );
    assert_eq!(out.meta["image"], "image/png");
    let Some(Media::Image(img)) = img else {
        panic!("an image")
    };
    assert_eq!((img.name.as_str(), img.info.width), ("shot.png", 640));
    assert_eq!(img.bytes, bytes);
    // `run` (what a caller without images gets) says the same, without the bytes.
    let plain = Read.run(&json!({"path": "shot.png"}), &c).unwrap();
    assert_eq!(plain.text, out.text);

    std::fs::write(d.path().join("huge.png"), png(4000, 3000, 6 * 1024 * 1024)).unwrap();
    let (out, img) = Read
        .run_with_media(&json!({"path": "huge.png"}), &c)
        .unwrap();
    assert!(img.is_none());
    assert!(
        out.text
            .ends_with(": not shown, an image over the 5 MiB limit."),
        "{}",
        out.text
    );
    assert_eq!(out.meta["not_shown"], "an image over the 5 MiB limit");
}

/// What a cut left out of a read is rows, by number (theseus-46v): the
/// call `rest` names returns exactly them, a row cut part-way included.
#[test]
fn a_reads_rest_names_the_rows_left_out_and_the_call_returns_them() {
    let d = tempfile::tempdir().unwrap();
    let c = ctx(&d);
    let body: String = (1..=40).map(|i| format!("ledger row {i}\n")).collect();
    std::fs::write(d.path().join("rows.txt"), body).unwrap();
    let out = Read.run(&json!({"path": "rows.txt"}), &c).unwrap().text;
    let rows: Vec<&str> = out.lines().collect();

    // Whole rows 12 to 30.
    let left = rows[11..30].join("\n") + "\n";
    let rest = Read.rest(&left);
    assert_eq!(
        rest,
        "lines 12-30; fs_read with offset=12 and limit=19 returns them"
    );
    let again = Read
        .run(&json!({"path": "rows.txt", "offset": 12, "limit": 19}), &c)
        .unwrap()
        .text;
    // Those rows, then the read's own line on where the file goes on.
    assert!(again.starts_with(&left), "the call returns them: {again}");
    assert!(again[left.len()..].starts_with("[showing lines 12-30 of 40;"));

    // Cut part-way: the end of row 11 and the start of row 31.
    let left = format!(" row 11\n{}\n    31\tledg", rows[11..30].join("\n"));
    assert_eq!(
        Read.rest(&left),
        "lines 11-31; fs_read with offset=11 and limit=21 returns them"
    );
    // Nothing that reads as a row: a smaller read.
    assert_eq!(
        Read.rest("no rows here"),
        "fs_read with a smaller limit returns them"
    );
}

#[test]
fn read_write_edit_roundtrip_with_errors_the_model_can_act_on() {
    let d = tempfile::tempdir().unwrap();
    let c = ctx(&d);
    let out = WriteFile
        .run(
            &json!({"path": "src/a.rs", "content": "fn a() {}\nfn b() {}\n"}),
            &c,
        )
        .unwrap();
    assert!(out.text.starts_with("Created"), "{}", out.text);
    let r = Read.run(&json!({"path": "src/a.rs"}), &c).unwrap();
    assert_eq!(r.text, "     1\tfn a() {}\n     2\tfn b() {}\n");
    let e = Edit
        .run(
            &json!({"path": "src/a.rs", "old_string": "fn b() {}", "new_string": "fn b() { a() }"}),
            &c,
        )
        .unwrap();
    assert!(
        e.text.contains("+fn b() { a() }") && e.text.contains("-fn b() {}"),
        "{}",
        e.text
    );
    let miss = Edit
        .run(
            &json!({"path": "src/a.rs", "old_string": "nope", "new_string": "x"}),
            &c,
        )
        .unwrap_err();
    assert!(miss.message.contains("not found"));
    WriteFile
        .run(&json!({"path": "dup.txt", "content": "x x x"}), &c)
        .unwrap();
    let dup = Edit
        .run(
            &json!({"path": "dup.txt", "old_string": "x", "new_string": "y"}),
            &c,
        )
        .unwrap_err();
    assert!(dup.message.contains("occurs 3 times"));
    Edit.run(
        &json!({"path": "dup.txt", "old_string": "x", "new_string": "y", "replace_all": true}),
        &c,
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(d.path().join("dup.txt")).unwrap(),
        "y y y"
    );
    // An identical edit (usually a retry of one that landed) is not invalid
    // input: it is a no-op that says so and leaves the file alone. Text that
    // is absent is still the ordinary not-found error.
    let same = json!({"path": "dup.txt", "old_string": "y y", "new_string": "y y"});
    assert!(
        Edit.plan(&same, &c).is_ok(),
        "an identical edit is not invalid input"
    );
    let noop = Edit.run(&same, &c).unwrap();
    assert!(noop.text.starts_with("No change made"), "{}", noop.text);
    assert_eq!(noop.meta["replacements"], 0);
    assert_eq!(
        fs::read_to_string(d.path().join("dup.txt")).unwrap(),
        "y y y"
    );
    let absent = Edit
        .run(
            &json!({"path": "dup.txt", "old_string": "zzz", "new_string": "zzz"}),
            &c,
        )
        .unwrap_err();
    assert!(absent.message.contains("not found"));
    assert!(Edit
        .plan(
            &json!({"path": "a", "old_string": "", "new_string": "x"}),
            &c
        )
        .is_err());
    assert!(
        Read.plan(&json!({"path": "a", "bogus": 1}), &c).is_err(),
        "unknown fields are rejected"
    );
    // Paging.
    WriteFile.run(&json!({"path": "long.txt", "content": (1..=10).map(|i| format!("l{i}\n")).collect::<String>()}), &c).unwrap();
    let p = Read
        .run(&json!({"path": "long.txt", "offset": 4, "limit": 2}), &c)
        .unwrap();
    assert!(
        p.text.starts_with("     4\tl4\n     5\tl5\n") && p.text.contains("offset=6"),
        "{}",
        p.text
    );
}

/// The daemon runs under umask 077 (theseus-wz2), and what a tool makes in
/// the workspace gets the operator's mode instead: a new file, and each
/// directory made for it. A file that exists keeps its own mode.
#[test]
fn new_files_and_directories_get_the_operators_mode() {
    use std::os::unix::fs::PermissionsExt;
    let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    let d = tempfile::tempdir().unwrap();
    let c = ToolCtx {
        umask: Some(0o027),
        ..ctx(&d)
    };
    WriteFile
        .run(&json!({"path": "deep/er/new.txt", "content": "x"}), &c)
        .unwrap();
    assert_eq!(mode(&d.path().join("deep")), 0o750);
    assert_eq!(mode(&d.path().join("deep/er")), 0o750);
    assert_eq!(mode(&d.path().join("deep/er/new.txt")), 0o640);
    let kept = d.path().join("kept.txt");
    fs::write(&kept, "a").unwrap();
    fs::set_permissions(&kept, fs::Permissions::from_mode(0o604)).unwrap();
    WriteFile
        .run(&json!({"path": "kept.txt", "content": "b"}), &c)
        .unwrap();
    assert_eq!(mode(&kept), 0o604);
    let private = ToolCtx {
        umask: Some(0o077),
        ..ctx(&d)
    };
    let patch = "--- /dev/null\n+++ b/made/by-patch.txt\n@@ -0,0 +1 @@\n+hello\n";
    Patch.run(&json!({"patch": patch}), &private).unwrap();
    assert_eq!(mode(&d.path().join("made")), 0o700);
    assert_eq!(mode(&d.path().join("made/by-patch.txt")), 0o600);
}

#[test]
fn patch_applies_atomically_across_files_or_not_at_all() {
    let d = tempfile::tempdir().unwrap();
    let c = ctx(&d);
    fs::write(d.path().join("x.txt"), "one\ntwo\nthree\n").unwrap();
    let patch = "--- a/x.txt\n+++ b/x.txt\n@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\n--- /dev/null\n+++ b/new.txt\n@@ -0,0 +1 @@\n+hello\n";
    let plan = Patch.plan(&json!({"patch": patch}), &c).unwrap();
    assert_eq!(plan.resources.len(), 2);
    let out = Patch.run(&json!({"patch": patch}), &c).unwrap();
    assert!(out.text.contains("+1 -1"), "{}", out.text);
    assert_eq!(
        fs::read_to_string(d.path().join("x.txt")).unwrap(),
        "one\nTWO\nthree\n"
    );
    assert_eq!(
        fs::read_to_string(d.path().join("new.txt")).unwrap(),
        "hello\n"
    );
    // A second section that does not apply leaves the first untouched.
    let bad = "--- a/x.txt\n+++ b/x.txt\n@@ -1,3 +1,3 @@\n one\n-TWO\n+2\n three\n--- a/new.txt\n+++ b/new.txt\n@@ -1 +1 @@\n-nothere\n+x\n";
    assert!(Patch.run(&json!({"patch": bad}), &c).is_err());
    assert_eq!(
        fs::read_to_string(d.path().join("x.txt")).unwrap(),
        "one\nTWO\nthree\n"
    );
    // A deletion must show every line it removes; a partial one deletes nothing.
    let partial = "--- a/x.txt\n+++ /dev/null\n@@ -1,1 +0,0 @@\n-one\n";
    let e = Patch.run(&json!({"patch": partial}), &c).unwrap_err();
    assert!(
        e.message.contains("must remove every line"),
        "{}",
        e.message
    );
    assert!(d.path().join("x.txt").exists());
    let whole = "--- a/x.txt\n+++ /dev/null\n@@ -1,3 +0,0 @@\n-one\n-TWO\n-three\n";
    Patch.run(&json!({"patch": whole}), &c).unwrap();
    assert!(!d.path().join("x.txt").exists());
}

/// Cores for the tests: a thread per job, at most `n` at once.
#[derive(Debug)]
struct Threads {
    free: Arc<AtomicUsize>,
    lent: AtomicUsize,
}

impl Cores for Threads {
    fn try_spawn(&self, job: Box<dyn FnOnce() + Send>) -> bool {
        let took = self
            .free
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1));
        if took.is_err() {
            return false;
        }
        self.lent.fetch_add(1, Ordering::SeqCst);
        let free = self.free.clone();
        std::thread::spawn(move || {
            job();
            free.fetch_add(1, Ordering::SeqCst);
        });
        true
    }
}

/// `fs.grep` searches a big tree's files ahead on borrowed cores
/// (theseus-a60), and says exactly what it says on its own thread: every
/// mode, with context, a glob, no match, and a cap that falls inside a
/// file searched ahead.
#[test]
fn grep_says_the_same_with_borrowed_cores() {
    let d = tempfile::tempdir().unwrap();
    for i in 0..1000 {
        let dir = d.path().join(format!("d{}", i % 10));
        fs::create_dir_all(&dir).unwrap();
        let text: String = (0..20)
            .map(|l| match i % 7 == 0 && l % (1 + i % 5) == 0 {
                true => format!("needle {i} {l}\n"),
                false => format!("hay {i} {l}\n"),
            })
            .collect();
        fs::write(dir.join(format!("f{i}.txt")), text).unwrap();
    }
    let threads = Arc::new(Threads {
        free: Arc::new(AtomicUsize::new(3)),
        lent: AtomicUsize::new(0),
    });
    let lent = ToolCtx {
        cores: Some(threads.clone()),
        ..ctx(&d)
    };
    let mut crossed = false;
    for input in [
        json!({"pattern": "needle"}),
        json!({"pattern": "needle", "max_results": 1000}),
        json!({"pattern": "needle", "context": 2, "max_results": 777}),
        json!({"pattern": "needle", "max_results": 100000}),
        json!({"pattern": "needle", "output_mode": "files", "max_results": 100}),
        json!({"pattern": "needle", "output_mode": "count", "max_results": 1000}),
        json!({"pattern": "needle 99[0-9]", "glob": "f99*.txt"}),
        json!({"pattern": "nowhere"}),
    ] {
        let alone = Grep.run(&input, &ctx(&d)).unwrap();
        let helped = Grep.run(&input, &lent).unwrap();
        assert_eq!(alone.text, helped.text, "{input}");
        assert_eq!(alone.meta, helped.meta, "{input}");
        let scanned = alone.meta["files_scanned"].as_u64().unwrap() as usize;
        crossed |= alone.meta["capped"] == true
            && input.get("output_mode").is_none()
            && scanned > INLINE_FILES;
    }
    assert!(crossed, "a cap fell among the files searched ahead");
    assert!(threads.lent.load(Ordering::SeqCst) > 0, "it borrowed cores");
    // A helper that found no chunk left may still be on its way out.
    let t0 = std::time::Instant::now();
    while threads.free.load(Ordering::SeqCst) < 3 {
        assert!(t0.elapsed().as_secs() < 5, "every core came back");
        std::thread::yield_now();
    }
}

#[test]
fn glob_grep_list_respect_gitignore_and_report_counts() {
    let d = tempfile::tempdir().unwrap();
    let c = ctx(&d);
    fs::create_dir_all(d.path().join("src/sub")).unwrap();
    fs::create_dir_all(d.path().join("target")).unwrap();
    fs::write(d.path().join(".gitignore"), "target/\n").unwrap();
    fs::write(
        d.path().join("src/lib.rs"),
        "pub fn alpha() {}\n// TODO: beta\n",
    )
    .unwrap();
    fs::write(
        d.path().join("src/sub/m.rs"),
        "fn gamma() {}\nfn alpha_two() {}\n",
    )
    .unwrap();
    fs::write(d.path().join("target/junk.rs"), "fn alpha() {}\n").unwrap();
    let g = Glob.run(&json!({"pattern": "**/*.rs"}), &c).unwrap();
    assert_eq!(g.meta["matches"], 2, "{}", g.text);
    assert!(!g.text.contains("target"));
    let hits = Grep.run(&json!({"pattern": "fn alpha"}), &c).unwrap();
    assert_eq!(hits.meta["matching_lines"], 2, "{}", hits.text);
    assert!(hits.text.contains("lib.rs:1:pub fn alpha() {}"));
    let files = Grep
        .run(&json!({"pattern": "TODO", "output_mode": "files"}), &c)
        .unwrap();
    assert!(
        files.text.lines().next().unwrap().ends_with("lib.rs"),
        "{}",
        files.text
    );
    let ci = Grep
        .run(
            &json!({"pattern": "todo", "case_insensitive": true, "output_mode": "count"}),
            &c,
        )
        .unwrap();
    assert!(
        ci.text.lines().next().unwrap().ends_with("lib.rs:1"),
        "{}",
        ci.text
    );
    let ctx_hit = Grep
        .run(&json!({"pattern": "gamma", "context": 1}), &c)
        .unwrap();
    assert!(
        ctx_hit.text.contains("m.rs-2-fn alpha_two() {}"),
        "{}",
        ctx_hit.text
    );
    assert!(Grep.plan(&json!({"pattern": "("}), &c).is_err());
    let l = List.run(&json!({}), &c).unwrap();
    assert!(
        l.text.contains("src/") && l.text.contains("lib.rs (") && !l.text.contains("target/"),
        "{}",
        l.text
    );
}

/// Five lantern logs, written a minute apart, oldest first, and a hidden
/// one: the tree the listing tests read.
fn lantern_logs(d: &tempfile::TempDir) {
    let t0 = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_790_000_000);
    for i in 1..=5 {
        let p = d.path().join(format!("lantern-{i}.txt"));
        fs::write(&p, "wick\nwick\nwick\n").unwrap();
        fs::File::options()
            .write(true)
            .open(&p)
            .unwrap()
            .set_modified(t0 + std::time::Duration::from_secs(60 * i))
            .unwrap();
    }
    fs::write(d.path().join(".lantern-key.txt"), "wick\n").unwrap();
}

/// Appendix F's rule (theseus-8ye): a glob's result states its scope (the
/// pattern, where, newest first, what the walk never sees) and exactly
/// what it left out (the oldest, by count, and the call that returns
/// them); an empty result says what was looked at and what was not.
#[test]
fn a_globs_result_says_its_scope_and_the_oldest_it_left_out() {
    let d = tempfile::tempdir().unwrap();
    lantern_logs(&d);
    let base = d.path().display().to_string();
    let narrow = ToolCtx {
        max_entries: 3,
        ..ctx(&d)
    };
    let g = Glob.run(&json!({"pattern": "*.txt"}), &narrow).unwrap();
    let lines: Vec<&str> = g.text.lines().collect();
    assert_eq!(lines.len(), 4, "{}", g.text);
    assert!(
        lines[0].ends_with("lantern-5.txt"),
        "newest first: {}",
        g.text
    );
    assert_eq!(
        lines[3],
        format!(
            "[3 of 5 files matching *.txt under {base}, newest first; the 2 oldest not shown: a \
             narrower pattern or path returns them; hidden files and .gitignore'd paths are not \
             searched]"
        )
    );
    assert_eq!(
        (g.meta["matches"].clone(), g.meta["shown"].clone()),
        (json!(5), json!(3))
    );
    let all = Glob.run(&json!({"pattern": "*.txt"}), &ctx(&d)).unwrap();
    assert!(
        all.text.ends_with(&format!(
            "\n[5 files matching *.txt under {base}, newest first; hidden files and \
             .gitignore'd paths are not searched]"
        )),
        "{}",
        all.text
    );
    let none = Glob
        .run(&json!({"pattern": ".lantern*"}), &ctx(&d))
        .unwrap();
    assert_eq!(
        none.text,
        format!(
            "No files match .lantern* under {base} (5 files looked at); hidden files and \
             .gitignore'd paths are not searched."
        )
    );
}

/// A grep's result states how many lines or files it found, in how many
/// of how many files searched, where; capped, it says it stopped, where,
/// how many files it never opened, that more may match, and the call
/// that returns them (theseus-8ye).
#[test]
fn a_greps_result_says_its_scope_and_where_the_cap_stopped_it() {
    let d = tempfile::tempdir().unwrap();
    lantern_logs(&d);
    let base = d.path().display().to_string();
    let c = ctx(&d);
    let walked = "hidden files and .gitignore'd paths are not searched";
    let grep = |input: Value| Grep.run(&input, &c).unwrap();
    let last = |t: &str| t.lines().last().unwrap().to_string();
    let full = grep(json!({"pattern": "wick"}));
    assert_eq!(full.text.lines().count(), 16, "{}", full.text);
    assert_eq!(
        last(&full.text),
        format!("[15 matching lines in 5 files; 5 files under {base} searched; {walked}]")
    );
    let capped = grep(json!({"pattern": "wick", "max_results": 4}));
    assert_eq!(capped.text.lines().count(), 5, "{}", capped.text);
    let stopped_in = capped
        .text
        .lines()
        .nth(3)
        .unwrap()
        .split(':')
        .next()
        .unwrap();
    assert_eq!(
        last(&capped.text),
        format!(
            "[stopped at max_results=4: the first 4 matching lines, from 2 files, the last in \
             {stopped_in}; 3 more of 5 files under {base} not searched, so more may match: a \
             narrower search, or a larger max_results, returns them; {walked}]"
        )
    );
    assert_eq!(
        (
            capped.meta["capped"].clone(),
            capped.meta["files_searched"].clone(),
            capped.meta["files_total"].clone()
        ),
        (json!(true), json!(2), json!(5))
    );
    let files = grep(json!({"pattern": "wick", "output_mode": "files", "max_results": 2}));
    assert_eq!(
        last(&files.text),
        format!(
            "[stopped at max_results=2: the first 2 files with matches; 3 more of 5 files under \
             {base} not searched, so more may match: a narrower search, or a larger \
             max_results, returns them; {walked}]"
        )
    );
    let counted = grep(json!({"pattern": "wick", "output_mode": "count"}));
    assert_eq!(
        last(&counted.text),
        format!(
            "[5 files with matches (15 matching lines); 5 files under {base} searched; {walked}]"
        )
    );
    let only = grep(json!({"pattern": "wick", "glob": "lantern-1*"}));
    assert_eq!(
        last(&only.text),
        format!(
            "[3 matching lines in 1 file; 1 file matching lantern-1* under {base} searched; \
             {walked}]"
        )
    );
    let none = grep(json!({"pattern": "tallow"}));
    assert_eq!(
        none.text,
        format!("No matches for /tallow/; 5 files under {base} searched; {walked}.")
    );
    let one = d.path().join("lantern-2.txt").display().to_string();
    let in_one = grep(json!({"pattern": "wick", "path": one}));
    assert_eq!(last(&in_one.text), format!("[3 matching lines in {one}]"));
    let cut_one = grep(json!({"pattern": "wick", "path": one, "max_results": 2}));
    assert_eq!(
        last(&cut_one.text),
        format!(
            "[stopped at max_results=2: the first 2 matching lines in {one}; its later lines \
             were not searched, so more may match: a larger max_results returns them]"
        )
    );
}

/// A listing states its scope (where, to what depth) and what it left
/// out: the entries past its length, from where in path order; the
/// directories whose contents lie below its depth; the hidden and ignored
/// entries unless `all` shows them (theseus-8ye).
#[test]
fn a_listings_result_says_its_scope_and_what_it_left_out() {
    let d = tempfile::tempdir().unwrap();
    lantern_logs(&d);
    fs::create_dir_all(d.path().join("harbor/dock/crates")).unwrap();
    fs::write(d.path().join("harbor/dock/crates/rope.txt"), "coil\n").unwrap();
    let base = d.path().display().to_string();
    let l = List.run(&json!({}), &ctx(&d)).unwrap();
    assert_eq!(
        l.text.lines().last().unwrap(),
        format!(
            "[7 entries under {base} to depth 2; the contents of 1 directory at depth 2 not \
             listed: fs_list of one, or a greater depth, lists them; hidden and .gitignore'd \
             entries not shown: all=true shows them]"
        )
    );
    let narrow = ToolCtx {
        max_entries: 3,
        ..ctx(&d)
    };
    let cut = List.run(&json!({"depth": 3}), &narrow).unwrap();
    assert_eq!(
        cut.text.lines().last().unwrap(),
        format!(
            "[3 of 8 entries under {base} to depth 3, in path order; the rest, from \
             lantern-1.txt on, not shown: fs_list of a subdirectory, or with a lower depth, \
             returns them; the contents of 1 directory at depth 3 not listed: fs_list of one, \
             or a greater depth, lists them; hidden and .gitignore'd entries not shown: \
             all=true shows them]"
        )
    );
    let every = List
        .run(&json!({"depth": 6, "all": true}), &ctx(&d))
        .unwrap();
    assert_eq!(
        every.text.lines().last().unwrap(),
        format!("[10 entries under {base} to depth 6]")
    );
}
