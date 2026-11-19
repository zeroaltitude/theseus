//! A debug build's plant, for a test that holds a writer in its write as a stalled disk holds it (never in a release
//! build). The variable `var` names a path; while a FIFO is there, the caller says it has arrived by making
//! `<path>.in`, and waits until a test opens the FIFO for writing. A test that is done keeps its end open, so a
//! later write passes at once.

/// Hold the calling thread at the FIFO named by `var`, if there is one.
pub fn hold_by_fifo(var: &str) {
    use std::os::unix::fs::FileTypeExt;
    let Some(path) = std::env::var_os(var) else {
        return;
    };
    if !std::fs::metadata(&path).is_ok_and(|m| m.file_type().is_fifo()) {
        return;
    }
    let mut marker = path.clone();
    marker.push(".in");
    let _ = std::fs::write(marker, b"");
    let _ = std::fs::File::open(&path);
}
