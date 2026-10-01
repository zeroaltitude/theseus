//! The daemon's files are its operator's alone (theseus-wz2). `private`,
//! called before `theseusd` creates anything, sets the process umask to 077,
//! so every file, directory, and socket the daemon makes has no group or other
//! bits: the store, the spool and raw job output, the config copy, the
//! protocol socket. The umask it replaced is the operator's, and it stays the
//! operator's for what the daemon makes on their behalf in the workspace: a
//! job's command runs under it (`WrapperArgs::umask`), and a file tool gives a
//! new file or directory its mode (`ToolCtx::umask`).

use std::sync::atomic::{AtomicU32, Ordering};

/// Carries the operator's umask across a restart in place: an exec keeps the
/// process's umask, which is 077 by then.
pub const ENV: &str = "THESEUS_OPERATOR_UMASK";

const UNSET: u32 = u32::MAX;
static OPERATOR: AtomicU32 = AtomicU32::new(UNSET);

/// Set the process umask to 077, and keep the operator's: the one `ENV`
/// carried from before an exec, else the one this replaced. Returns it.
pub fn private() -> u32 {
    // SAFETY: umask only swaps the process's file mode creation mask.
    let replaced: libc::mode_t = unsafe { libc::umask(0o077) };
    let operator = std::env::var(ENV)
        .ok()
        .and_then(|v| parse(&v))
        .unwrap_or(replaced & 0o777);
    OPERATOR.store(operator, Ordering::Relaxed);
    operator
}

/// The operator's umask, once `private` has run in this process.
pub fn operator() -> Option<u32> {
    let u = OPERATOR.load(Ordering::Relaxed);
    (u != UNSET).then_some(u)
}

/// A umask as octal text (`0002`, `022`), as `ENV` and the wrapper's
/// `--umask` carry it.
pub fn parse(v: &str) -> Option<u32> {
    u32::from_str_radix(v.trim(), 8)
        .ok()
        .filter(|u| *u <= 0o777)
}

/// A umask as `ENV` and `--umask` carry it.
pub fn format(u: u32) -> String {
    format!("{:04o}", u & 0o777)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_umask_round_trips_as_octal_text() {
        for u in [0, 0o002, 0o022, 0o027, 0o077, 0o777] {
            assert_eq!(parse(&format(u)), Some(u));
        }
        assert_eq!(parse("0002"), Some(2));
        assert_eq!(parse(" 022\n"), Some(0o22));
        for bad in ["", "8", "0o22", "1000", "-1", "x"] {
            assert_eq!(parse(bad), None, "{bad:?}");
        }
    }
}
