//! `fs.edit` and `fs.patch`'s line endings: CRLF files edited by a quote with
//! bare newlines, and a replacement written in the region's own endings.

/// `s` with every line ending a CRLF, whatever it was.
pub(crate) fn to_crlf(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\n', "\r\n")
}

/// Byte ranges of `before` that equal `quote` when each `\r\n` in `before`
/// (and in `quote`) is read as `\n`: a quote over a mixed file, where one
/// region spans both endings. Counted as `str::matches` counts (not
/// overlapping, left to right), so uniqueness and `replace_all` hold.
pub(crate) fn lf_regions(before: &str, quote: &str) -> Vec<(usize, usize)> {
    let quote = quote.replace("\r\n", "\n");
    let mut norm = String::with_capacity(before.len());
    // `at[i]` is the byte of `before` that byte `i` of `norm` came from.
    let mut at = Vec::with_capacity(before.len() + 1);
    let b = before.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let crlf = b[i] == b'\r' && b.get(i + 1) == Some(&b'\n');
        let len = if crlf {
            2
        } else {
            before[i..].chars().next().map_or(1, char::len_utf8)
        };
        let piece = if crlf { "\n" } else { &before[i..i + len] };
        at.extend(std::iter::repeat_n(i, piece.len()));
        norm.push_str(piece);
        i += len;
    }
    at.push(before.len());
    match quote.is_empty() {
        true => Vec::new(),
        false => norm
            .match_indices(quote.as_str())
            .map(|(s, m)| (at[s], at[s + m.len()]))
            .collect(),
    }
}

/// `before` with each of `regions` replaced by `new` (its newlines read as
/// `\n`). A replacement's newlines take the region's own line endings in
/// order; a newline beyond them takes the region's last ending, and a region
/// with no newline gives `\r\n` (the file has CRLF). Every byte outside a
/// region is copied as it was.
pub(crate) fn splice_lf_regions(before: &str, regions: &[(usize, usize)], new: &str) -> String {
    let new = new.replace("\r\n", "\n");
    let mut out = String::with_capacity(before.len());
    let mut last = 0;
    for &(s, e) in regions {
        out.push_str(&before[last..s]);
        let ends: Vec<&str> = before[s..e]
            .split_inclusive('\n')
            .filter(|l| l.ends_with('\n'))
            .map(|l| if l.ends_with("\r\n") { "\r\n" } else { "\n" })
            .collect();
        let mut k = 0;
        for (j, line) in new.split('\n').enumerate() {
            if j > 0 {
                out.push_str(ends.get(k).or(ends.last()).copied().unwrap_or("\r\n"));
                k += 1;
            }
            out.push_str(line);
        }
        last = e;
    }
    out.push_str(&before[last..]);
    out
}

/// Whether every line ending in `s` is a CRLF, and there is at least one.
pub(crate) fn is_all_crlf(s: &str) -> bool {
    s.contains("\r\n") && !s.replace("\r\n", "").contains('\n')
}
