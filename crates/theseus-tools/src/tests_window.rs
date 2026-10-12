//! `fs.read` of a window of a file over the cap (theseus-ywdd).

use std::io::Write as _;

use serde_json::json;

use crate::{fs::Read, Tool, ToolCtx};

#[test]
fn a_long_line_is_not_kept_past_what_its_cut_needs() {
    let mut r = std::io::Cursor::new(vec![b'x'; 5 * MIB]);
    let (mut line, mut scanned) = (Vec::new(), 0);
    let l = super::fs_window::next_line(&mut r, &mut line, true, &mut scanned, u64::MAX).unwrap();
    assert!(
        l.cut && l.n == 5 * MIB && line.len() < 10_000,
        "{}",
        line.len()
    );
}

const MIB: usize = 1024 * 1024;

fn numbered(d: &tempfile::TempDir, name: &str, bytes: usize) -> std::path::PathBuf {
    let p = d.path().join(name);
    let mut w = std::io::BufWriter::new(std::fs::File::create(&p).unwrap());
    let (mut n, mut size) = (1usize, 0usize);
    while size < bytes {
        let l = format!("row {n}\n");
        w.write_all(l.as_bytes()).unwrap();
        size += l.len();
        n += 1;
    }
    p
}

fn read(d: &tempfile::TempDir, input: serde_json::Value) -> Result<crate::ToolOutput, String> {
    Read.run(&input, &ToolCtx::for_tests(d.path()))
        .map_err(|e| e.message)
}

#[test]
fn a_window_of_a_big_file_is_its_lines_and_only_the_scan_is_read() {
    let d = tempfile::tempdir().unwrap();
    numbered(&d, "big.csv", 20 * MIB);
    let o = read(&d, json!({"path": "big.csv", "limit": 6})).unwrap();
    assert!(o.text.contains("     1\trow 1\n"), "{}", o.text);
    assert!(o.text.contains("     6\trow 6\n"));
    assert!(!o.text.contains("     7\t"));
    assert!(o.text.contains("lines 1-6 of a 2"), "{}", o.text);
    assert!(o.text.contains("-byte file"));
    assert!(o.meta["scanned"].as_u64().unwrap() < 4096, "{}", o.meta);
    let o = read(&d, json!({"path": "big.csv", "offset": 1000, "limit": 3})).unwrap();
    assert!(o
        .text
        .starts_with("  1000\trow 1000\n  1001\trow 1001\n  1002\trow 1002\n"));
    assert!(!o.text.contains("  1003\t"));
    assert!(o.meta["scanned"].as_u64().unwrap() < 64 * 1024);
}

#[test]
fn one_huge_line_is_cut_and_its_bytes_are_not_kept() {
    let d = tempfile::tempdir().unwrap();
    let mut w = std::fs::File::create(d.path().join("one")).unwrap();
    for _ in 0..20 {
        w.write_all(&vec![b'x'; MIB]).unwrap();
    }
    w.write_all(b"\nnext\n").unwrap();
    let o = read(&d, json!({"path": "one", "limit": 2})).unwrap();
    assert!(o.text.len() < 10_000, "{} bytes", o.text.len());
    assert!(o.text.contains("[line truncated]"));
    assert!(o.text.contains("     2\tnext"), "{}", o.text);
    // The window's byte cap holds across many lines too.
    numbered(&d, "many", 17 * MIB);
    let o = read(&d, json!({"path": "many", "limit": 1000000})).unwrap();
    assert!(o.text.len() <= 256 * 1024 + 200, "{}", o.text.len());
    assert!(o.text.contains("pass offset="));
}

#[test]
fn an_offset_past_the_scan_bound_says_so_and_points_at_grep() {
    let d = tempfile::tempdir().unwrap();
    let p = numbered(&d, "big", 17 * MIB);
    let r = super::fs_window::read_bounded(&p, 900_000, 3, 256 * 1024, 4 * MIB as u64)
        .unwrap()
        .unwrap();
    assert!(r.text.contains("not scanned that far"), "{}", r.text);
    assert!(r.text.contains("fs_grep"));
    let scanned = r.meta["scanned"].as_u64().unwrap();
    assert!(
        (4 * MIB as u64..5 * MIB as u64).contains(&scanned),
        "{scanned}"
    );
}

#[test]
fn a_big_file_with_neither_offset_nor_limit_is_refused_with_the_way_to_read_it() {
    let d = tempfile::tempdir().unwrap();
    numbered(&d, "big", 17 * MIB);
    let e = read(&d, json!({"path": "big"})).unwrap_err();
    assert!(
        e.contains("is not read whole") && e.contains("give offset and limit"),
        "{e}"
    );
}

#[test]
fn a_big_image_is_refused_as_before_even_with_a_limit() {
    let d = tempfile::tempdir().unwrap();
    // A GIF header with no zero byte in it, so only the image sniff can refuse it.
    let mut v = b"GIF89a\x01\x01\x01\x01".to_vec();
    v.resize(17 * MIB, 0x41);
    std::fs::write(d.path().join("i.gif"), v).unwrap();
    let e = read(&d, json!({"path": "i.gif", "limit": 5})).unwrap_err();
    assert!(e.contains("is not read whole"), "{e}");
}

#[test]
fn a_big_pdf_keeps_its_own_cap_and_a_big_binary_is_not_windowed() {
    let d = tempfile::tempdir().unwrap();
    let mut v = b"%PDF-1.4\n".to_vec();
    v.resize(33 * MIB, b' ');
    std::fs::write(d.path().join("p.pdf"), v).unwrap();
    let e = read(&d, json!({"path": "p.pdf", "limit": 5})).unwrap_err();
    assert!(e.contains("is not read whole"), "{e}");
    std::fs::write(d.path().join("b.bin"), vec![0u8; 17 * MIB]).unwrap();
    let e = read(&d, json!({"path": "b.bin", "limit": 5})).unwrap_err();
    assert!(e.contains("is not read whole"), "{e}");
}

// The review's tests (theseus-ywdd): the bounds the first tests left open.

#[test]
fn one_line_with_no_newline_costs_the_scan_bound_not_the_file() {
    let d = tempfile::tempdir().unwrap();
    let mut w = std::fs::File::create(d.path().join("flat")).unwrap();
    for _ in 0..20 {
        w.write_all(&vec![b'x'; MIB]).unwrap();
    }
    drop(w);
    let p = d.path().join("flat");
    let r = super::fs_window::read_bounded(&p, 1, 6, 256 * 1024, 4 * MIB as u64)
        .unwrap()
        .unwrap();
    let scanned = r.meta["scanned"].as_u64().unwrap();
    assert!(scanned < 5 * MIB as u64, "{scanned}");
    assert!(r.text.contains("[line truncated]"), "{}", r.text.len());
    assert!(r.text.contains("scan stopped"), "{}", r.text);
    assert!(r.text.len() < 10_000);
}

#[test]
fn a_window_that_runs_into_the_scan_bound_says_where_it_stopped() {
    let d = tempfile::tempdir().unwrap();
    let p = numbered(&d, "big", 17 * MIB);
    // A bound whose rows stay under fs_read's own cap (theseus-v73m).
    let r = super::fs_window::read_bounded(&p, 1, 1_000_000, MIB, 32 * 1024)
        .unwrap()
        .unwrap();
    let scanned = r.meta["scanned"].as_u64().unwrap();
    // The bound holds at a line's end, not the reader's buffer's edge.
    assert!((32 * 1024..32 * 1024 + 16).contains(&scanned), "{scanned}");
    assert!(
        r.text.contains("scan stopped"),
        "{}",
        &r.text[r.text.len() - 200..]
    );
    assert!(!r.text.contains("pass offset="));
}

#[test]
fn a_window_ending_exactly_at_the_end_of_the_file_does_not_say_to_read_on() {
    let d = tempfile::tempdir().unwrap();
    let p = numbered(&d, "big", 17 * MIB);
    let total = std::fs::read(&p)
        .unwrap()
        .iter()
        .filter(|b| **b == b'\n')
        .count();
    let o = read(&d, json!({"path": "big", "offset": total - 2, "limit": 3})).unwrap();
    assert!(
        o.text.contains(&format!("lines {}-{total} of", total - 2)),
        "{}",
        o.text
    );
    assert!(!o.text.contains("pass offset="), "{}", o.text);
    let o = read(&d, json!({"path": "big", "offset": total - 2, "limit": 2})).unwrap();
    assert!(
        o.text.contains(&format!("pass offset={total} to read on")),
        "{}",
        o.text
    );
    let o = read(&d, json!({"path": "big", "offset": total + 5, "limit": 3})).unwrap();
    assert!(
        o.text.contains("is past the end") && o.text.contains("and is a "),
        "{}",
        o.text
    );
}

#[test]
fn a_window_with_nothing_to_show_reads_on_from_where_it_started() {
    let d = tempfile::tempdir().unwrap();
    numbered(&d, "big", 17 * MIB);
    let o = read(&d, json!({"path": "big", "offset": 500, "limit": 0})).unwrap();
    assert!(o.text.contains("pass offset=500 to read on"), "{}", o.text);
}

#[test]
fn text_that_begins_pk_is_text() {
    let d = tempfile::tempdir().unwrap();
    let mut v = b"PKG_NAME=widget\n".to_vec();
    v.resize(17 * MIB, b'a');
    std::fs::write(d.path().join("log"), v).unwrap();
    let o = read(&d, json!({"path": "log", "limit": 1})).unwrap();
    assert!(o.text.contains("PKG_NAME=widget"), "{}", &o.text[..100]);
}

#[test]
fn a_symlink_to_a_big_file_is_read_by_window_and_a_fifo_is_refused() {
    let d = tempfile::tempdir().unwrap();
    numbered(&d, "big", 17 * MIB);
    std::os::unix::fs::symlink(d.path().join("big"), d.path().join("link")).unwrap();
    let o = read(&d, json!({"path": "link", "limit": 2})).unwrap();
    assert!(o.text.contains("     2\trow 2"), "{}", o.text);
    let fifo = d.path().join("fifo");
    let c = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    std::os::unix::fs::symlink(&fifo, d.path().join("flink")).unwrap();
    for name in ["fifo", "flink"] {
        let e = read(&d, json!({"path": name, "limit": 2})).unwrap_err();
        assert!(e.contains("FIFO"), "{e}");
    }
    let e = read(&d, json!({"path": "/dev/zero", "limit": 2})).unwrap_err();
    assert!(e.contains("character device"), "{e}");
}

#[test]
fn a_refused_window_does_not_ask_for_the_offset_and_limit_it_was_given() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("b.bin"), vec![0u8; 17 * MIB]).unwrap();
    let e = read(&d, json!({"path": "b.bin", "limit": 5})).unwrap_err();
    assert!(
        e.contains("not plain text") && !e.contains("give offset"),
        "{e}"
    );
    let e = read(&d, json!({"path": "b.bin"})).unwrap_err();
    assert!(e.contains("give offset and limit"), "{e}");
}

#[test]
fn a_big_document_that_is_all_text_is_refused_by_its_kind() {
    let d = tempfile::tempdir().unwrap();
    let mut rtf = b"{\\rtf1\\ansi hello}\n".to_vec();
    rtf.resize(17 * MIB, b'a');
    std::fs::write(d.path().join("d.rtf"), rtf).unwrap();
    let mut nb = b"{\"cells\": []}\n".to_vec();
    nb.resize(17 * MIB, b'a');
    std::fs::write(d.path().join("n.ipynb"), nb).unwrap();
    for name in ["d.rtf", "n.ipynb"] {
        let e = read(&d, json!({"path": name, "limit": 5})).unwrap_err();
        assert!(e.contains("not plain text"), "{name}: {e}");
    }
}
