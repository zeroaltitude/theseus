//! The build stamp: the commit a `theseusd` names in its own bytes.
//!
//! `theseusd` carries `theseus-build-stamp:<commit>:` as a constant (`crates/theseusd/src/main.rs`), and the helper
//! reads it from the bytes of its **copy** of the program, because running a program the owner supplied as root would
//! hand the owner root. The stamp catches a mistake (a stale build, the wrong directory); a program written to carry
//! the right stamp carries it, so it is no defence against the owner's account, and the design note says so.

/// What every stamp starts with.
pub const PREFIX: &[u8] = b"theseus-build-stamp:";

/// Every commit a program's bytes name, one per stamp, in the order found; a stamp whose commit is not forty lowercase
/// hex digits followed by `:` is reported as `Err` with what was there (cut short).
pub fn commits_in(bytes: &[u8]) -> Vec<Result<String, String>> {
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(i) = find(&bytes[at..], PREFIX) {
        let start = at + i + PREFIX.len();
        let tail = &bytes[start..bytes.len().min(start + 41)];
        let hex = tail.len() == 41
            && tail[40] == b':'
            && tail[..40]
                .iter()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b));
        found.push(if hex {
            Ok(String::from_utf8_lossy(&tail[..40]).into_owned())
        } else {
            Err(String::from_utf8_lossy(&tail[..tail.len().min(12)]).into_owned())
        });
        at = start;
    }
    found
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Whether `bytes` carry at least one stamp and every stamp names `commit`. The error says what was found.
pub fn names(bytes: &[u8], commit: &str) -> Result<(), String> {
    let found = commits_in(bytes);
    if found.is_empty() {
        return Err("it carries no build stamp".into());
    }
    for f in &found {
        match f {
            Ok(c) if c == commit => {}
            Ok(c) => return Err(format!("it was built from {c}, not {commit}")),
            Err(odd) => return Err(format!("a build stamp in it is malformed ({odd:?})")),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const C: &str = "0123456789abcdef0123456789abcdef01234567";

    /// `theseusd` slices the commit out of its stamp by this prefix's length (`STAMP_PREFIX_LEN`), as a number, so that
    /// its program carries the prefix text once.
    #[test]
    fn the_prefix_is_twenty_bytes() {
        assert_eq!(PREFIX.len(), 20);
    }

    #[test]
    fn every_stamp_in_the_bytes_is_found_and_read() {
        let mut b = b"junk".to_vec();
        b.extend_from_slice(format!("theseus-build-stamp:{C}:more").as_bytes());
        assert_eq!(commits_in(&b), vec![Ok(C.to_string())]);
        assert!(names(&b, C).is_ok());
        assert!(names(&b, "f".repeat(40).as_str()).is_err());
        assert!(names(b"nothing here", C).is_err());
    }

    #[test]
    fn a_stamp_that_is_cut_short_or_not_hex_is_malformed() {
        for tail in [
            &b"abc"[..],
            b"0123456789ABCDEF0123456789abcdef01234567:",
            b"0123456789abcdef0123456789abcdef01234567",
        ] {
            let mut b = PREFIX.to_vec();
            b.extend_from_slice(tail);
            assert!(matches!(commits_in(&b)[..], [Err(_)]), "{tail:?}");
        }
    }
}
