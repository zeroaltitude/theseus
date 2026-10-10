//! A running job's latest output (theseus-n8gk): the one reader of a job's
//! tail while it runs. `job.read` shows the model what it gives, and the
//! work board's `work.peek` is to show the operator the same, through
//! `crate::toolrun::peek`.
//!
//! Bounded whatever the job printed: the tail is read by seek, at most
//! `TAIL_BYTES`, and the lines are counted only in a file of at most
//! `COUNT_MAX` bytes, streamed through a buffer; a longer one says its size
//! instead.

use std::io::{BufRead, BufReader, Read, Seek};
use std::path::Path;

/// The most of a job's output a peek reads for its last lines.
pub const TAIL_BYTES: u64 = 64 * 1024;
/// The longest output whose lines a peek counts.
pub const COUNT_MAX: u64 = 8 * 1024 * 1024;

/// What a peek saw of a job's output.
#[derive(Debug, Default, PartialEq)]
pub struct Peek {
    /// Its last lines, at most the number asked for, whole lines only.
    pub text: String,
    /// How many lines `text` holds.
    pub shown: u64,
    /// The lines the job has printed, when the output is short enough to
    /// count (`COUNT_MAX`).
    pub lines: Option<u64>,
    /// The output's length, in bytes.
    pub bytes: u64,
    /// The output reached its file's head (`[tools] job_output_max_bytes`'s
    /// first part): what it prints now waits in its wrapper until it ends,
    /// so the newest lines are not in the file yet.
    pub held: bool,
}

/// The last `lines` lines of the job output at `path`, which its wrapper
/// writes while the job runs (`Spool::result_path`), capped at
/// `output_max_bytes`. A file not there yet is an empty peek.
pub fn peek(path: &Path, lines: u64, output_max_bytes: u64) -> Peek {
    std::fs::File::open(path)
        .and_then(|f| peek_from(f, lines, output_max_bytes))
        .unwrap_or_default()
}

/// `peek` over any reader that seeks.
pub(super) fn peek_from<R: Read + Seek>(
    mut r: R,
    lines: u64,
    output_max_bytes: u64,
) -> std::io::Result<Peek> {
    let tail = super::job::read_tail(&mut r, TAIL_BYTES)?;
    let bytes = tail.total;
    // A tail cut from a longer output starts inside a line: that part goes.
    let mut text = tail.text.as_str();
    if tail.unread > 0 {
        text = text.split_once('\n').map_or("", |(_, rest)| rest);
    }
    let all: Vec<&str> = text.lines().collect();
    let keep = all.len().saturating_sub(lines as usize);
    let shown = &all[keep..];
    let counted = match bytes {
        0 => Some(0),
        n if n <= COUNT_MAX => {
            r.rewind()?;
            Some(count_lines(BufReader::with_capacity(64 * 1024, r.take(n)))?)
        }
        _ => None,
    };
    let (head, tail_room) = theseus_kernel::redact::split(output_max_bytes);
    Ok(Peek {
        text: shown.join("\n"),
        shown: shown.len() as u64,
        lines: counted,
        bytes,
        held: tail_room > 0 && bytes >= head,
    })
}

/// Lines as an editor counts them: each newline ends one, and text after
/// the last is one more.
fn count_lines<R: BufRead>(mut r: R) -> std::io::Result<u64> {
    let (mut n, mut last) = (0u64, b'\n');
    loop {
        let buf = r.fill_buf()?;
        if buf.is_empty() {
            break;
        }
        n += buf.iter().filter(|&&b| b == b'\n').count() as u64;
        last = buf[buf.len() - 1];
        let len = buf.len();
        r.consume(len);
    }
    Ok(n + u64::from(last != b'\n'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, SeekFrom};

    /// A reader that counts the bytes read from it.
    struct Counted<R> {
        inner: R,
        read: u64,
    }

    impl<R: Read> Read for Counted<R> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = self.inner.read(buf)?;
            self.read += n as u64;
            Ok(n)
        }
    }

    impl<R: Seek> Seek for Counted<R> {
        fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
            self.inner.seek(pos)
        }
    }

    const CAP: u64 = 64 * 1024 * 1024;

    /// The newest lines, counted, and no more than asked for.
    #[test]
    fn a_peek_shows_the_last_lines_and_counts_them_all() {
        let body: String = (1..=412).map(|i| format!("line {i}\n")).collect();
        let p = peek_from(Cursor::new(body.clone().into_bytes()), 3, CAP).unwrap();
        assert_eq!(p.text, "line 410\nline 411\nline 412");
        assert_eq!(
            (p.shown, p.lines, p.bytes),
            (3, Some(412), body.len() as u64)
        );
        assert!(!p.held);
        // A last line still being written counts, and shows.
        let p = peek_from(Cursor::new(b"a\nb\nhalf".to_vec()), 40, CAP).unwrap();
        assert_eq!(
            (p.text.as_str(), p.shown, p.lines),
            ("a\nb\nhalf", 3, Some(3))
        );
        assert_eq!(
            peek_from(Cursor::new(Vec::new()), 40, CAP).unwrap().lines,
            Some(0)
        );
        assert_eq!(
            peek(Path::new("/no/such/invented.out"), 40, CAP),
            Peek::default()
        );
    }

    /// Bounded whatever the job printed (theseus-n8gk): a 40 MiB output is
    /// read for its tail alone, by seek, and its lines are not counted; one
    /// past its file's head says its newest lines wait in its wrapper.
    #[test]
    fn a_peek_reads_a_bounded_tail_of_a_big_output() {
        let big = 40 * 1024 * 1024;
        let mut body = vec![b'x'; big];
        for i in (0..big).step_by(100) {
            body[i] = b'\n';
        }
        body[big - 6..].copy_from_slice(b"\nfinal");
        let mut r = Counted {
            inner: Cursor::new(body),
            read: 0,
        };
        let p = peek_from(&mut r, 2, CAP).unwrap();
        assert!(r.read <= TAIL_BYTES, "read {} bytes", r.read);
        assert_eq!(p.lines, None, "too long to count");
        assert_eq!(p.bytes, big as u64);
        assert!(p.text.ends_with("\nfinal"), "{:?}", p.text);
        assert_eq!(p.shown, 2);
        let (head, _) = theseus_kernel::redact::split(1024 * 1024);
        let at_head = vec![b'y'; head as usize];
        assert!(
            peek_from(Cursor::new(at_head), 1, 1024 * 1024)
                .unwrap()
                .held
        );
    }
}
