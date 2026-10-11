//! `fs.read` of a window of a file over the size cap (theseus-ywdd): lines
//! streamed from the start, never the whole file. The scan to the offset is
//! bounded, and so are the window's bytes and each line's.

use std::io::{BufRead, BufReader};
use std::path::Path;

use serde_json::json;

use crate::fs::{open_regular, MAX_FILE_BYTES, MAX_LINE_CHARS};
use crate::{ToolFailure, ToolOutput};

/// The most a window's scan reads to reach its offset. A line is found only by
/// reading every byte before it, so an offset costs its bytes in disk reads:
/// 256 MiB is about a second from a warm cache and a few from a cold disk,
/// 16 times the whole-read cap, and past it `fs_grep` finds the part faster.
pub(crate) const MAX_SCAN_BYTES: u64 = 256 * 1024 * 1024;

/// The head read to sniff a file's kind before it is streamed.
const HEAD_BYTES: usize = 8192;

/// Why a big file is not read: the old refusal stands. `asked`: the call gave
/// offset or limit already, so the file is not text a window can be read from.
pub(crate) fn refusal(path: &Path, size: u64, asked: bool) -> String {
    if asked {
        return format!(
            "{} is {size} bytes; it is not read whole (over {MAX_FILE_BYTES} bytes) and it is not \
             plain text, so no window of it is read (an image, an archive, a document, or a \
             binary): use fs_grep, or proc_run for a tool that reads it",
            path.display()
        );
    }
    format!(
        "{} is {size} bytes; it is not read whole (over {MAX_FILE_BYTES} bytes): give offset and \
         limit to read a window of it, or use fs_grep",
        path.display()
    )
}

/// Lines `offset..` (1-based), at most `limit` and `max_bytes` of them, of a
/// text file over the cap. `Ok(None)` when the file is not plain text (an
/// image, an archive or document, a binary): the caller refuses it as before.
pub(crate) fn read(
    path: &Path,
    offset: usize,
    limit: usize,
    max_bytes: usize,
) -> Result<Option<ToolOutput>, ToolFailure> {
    read_bounded(path, offset, limit, max_bytes, MAX_SCAN_BYTES)
}

/// `read` with the scan bound given, so a test need not write 256 MiB.
pub(crate) fn read_bounded(
    path: &Path,
    offset: usize,
    limit: usize,
    max_bytes: usize,
    max_scan: u64,
) -> Result<Option<ToolOutput>, ToolFailure> {
    let (f, meta) =
        open_regular(path, true).map_err(|e| ToolFailure::new(e.say(path, "fs_read")))?;
    let size = meta.len();
    let io = |e: std::io::Error| ToolFailure::new(format!("cannot read {}: {e}", path.display()));
    let mut r = BufReader::with_capacity(64 * 1024, f);
    {
        let head = r.fill_buf().map_err(io)?;
        let head = &head[..head.len().min(HEAD_BYTES)];
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
        let kind = theseus_files::kind::sniff(head, name.as_deref().unwrap_or(""), &[]);
        if head.contains(&0) || crate::image::sniff(head).is_some() || kind.has_text() {
            return Ok(None);
        }
    }
    let mut scanned = 0u64;
    let mut line_no = 0usize;
    let mut out = String::new();
    let mut room = crate::fs_read_cap::Room::new(max_bytes);
    let mut last = offset - 1;
    let mut stopped = None;
    let mut line = Vec::new();
    loop {
        let want = line_no + 1 >= offset;
        let n = next_line(&mut r, &mut line, want, &mut scanned, max_scan).map_err(io)?;
        if n.n == 0 {
            if !n.ended {
                stopped = Some("scan");
            }
            break;
        }
        if !want {
            if !n.ended {
                stopped = Some("scan");
                break;
            }
            line_no += 1;
            continue;
        }
        line_no += 1;
        if line_no - offset >= limit {
            line_no -= 1;
            stopped = Some("limit");
            break;
        }
        let text = String::from_utf8_lossy(&line);
        let text = text.trim_end_matches(['\n', '\r']);
        let l = if n.cut || text.chars().count() > MAX_LINE_CHARS {
            format!(
                "{}… [line truncated]",
                text.chars().take(MAX_LINE_CHARS).collect::<String>()
            )
        } else {
            text.to_string()
        };
        let row = format!("{line_no:>6}\t{l}\n");
        if !room.takes(&row) {
            line_no -= 1;
            stopped = Some("bytes");
            break;
        }
        out.push_str(&row);
        last = line_no;
        if !n.ended {
            stopped = Some("scan");
            break;
        }
    }
    let big = format_size(size);
    let text = if line_no < offset && stopped.is_none() {
        format!("(offset {offset} is past the end: the file has {line_no} lines, and is a {big} file)\n")
    } else if stopped == Some("scan") && last < offset {
        format!(
            "(offset {offset} is more than {max_scan} bytes into a {big} file: not scanned \
             that far; use fs_grep to find the part you need)\n"
        )
    } else {
        let more = match stopped {
            Some("scan") => format!(
                "; the scan stopped at {max_scan} bytes into the file, so it is not read past \
                 line {last}: use fs_grep to find the part you need"
            ),
            Some(_) => format!("; pass offset={} to read on", last + 1),
            None => String::new(),
        };
        format!("{out}[partial read: lines {offset}-{last} of a {big} file{more}]\n")
    };
    Ok(Some(ToolOutput {
        text,
        meta: json!({"path": path, "bytes": size, "from": offset, "to": last,
                     "partial": true, "scanned": scanned}),
    }))
}

/// What one `next_line` read.
pub(crate) struct Line {
    /// Bytes consumed, the newline included; 0 at the end, or at the bound
    /// (`ended` false).
    pub(crate) n: usize,
    /// The line was longer than the buffer keeps, or the scan bound ended it.
    pub(crate) cut: bool,
    /// A newline or the end of the file ended it: false when the scan bound did.
    pub(crate) ended: bool,
}

/// The next line into `line`, keeping at most the bytes `MAX_LINE_CHARS`
/// characters can take; the rest of a long line is consumed, not kept. A line
/// that is not wanted is consumed without keeping anything. The scan bound
/// ends a line too, wanted or not, so one line with no newline in a huge file
/// costs the bound, not the file.
pub(crate) fn next_line<R: BufRead>(
    r: &mut R,
    line: &mut Vec<u8>,
    keep: bool,
    scanned: &mut u64,
    max_scan: u64,
) -> std::io::Result<Line> {
    line.clear();
    // At the bound already: nothing more is read (theseus-v73m: a line that
    // ended within a buffer once read on past the bound, up to the buffer's
    // edge).
    if *scanned >= max_scan {
        return Ok(Line {
            n: 0,
            cut: true,
            ended: false,
        });
    }
    let room = MAX_LINE_CHARS * 4 + 2;
    let (mut n, mut cut) = (0usize, false);
    loop {
        let buf = r.fill_buf()?;
        if buf.is_empty() {
            return Ok(Line {
                n,
                cut,
                ended: true,
            });
        }
        let (take, done) = match buf.iter().position(|b| *b == b'\n') {
            Some(i) => (i + 1, true),
            None => (buf.len(), false),
        };
        if keep {
            let space = room.saturating_sub(line.len());
            if take > space {
                cut = true;
            }
            line.extend_from_slice(&buf[..take.min(space)]);
        }
        r.consume(take);
        n += take;
        *scanned += take as u64;
        if done || *scanned >= max_scan {
            return Ok(Line {
                n,
                cut: cut || !done,
                ended: done,
            });
        }
    }
}

fn format_size(n: u64) -> String {
    let s = n.to_string();
    let mut o = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            o.push(',');
        }
        o.push(c);
    }
    format!("{o}-byte")
}
