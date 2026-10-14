//! A job's usual time (theseus-hkx30): per program key, the last
//! `KEEP` durations of jobs that ran on this machine, and their median as
//! `Eta::usual_ms`. The durations are the ACTION records' own
//! (`settled_at_ms - dispatched_at_ms`), so nothing is stored for it: the
//! board folds each job as its action settles, and a start warms it from the
//! newest settled jobs after serving (`Push::warm_eta`).

use std::collections::{HashMap, VecDeque};

use theseus_protocol::work::Eta;

/// Durations kept per program key.
pub(super) const KEEP: usize = 16;

/// A key's runs needed before an estimate is offered: one run is no usual.
const MIN_RUNS: usize = 2;

/// The program a job is: `argv[0]`'s base name and its first word that is
/// not a flag (`cargo test`, `npm run`, `ls`). A path in the second place
/// (`./run.sh`) is its own word: only words that look like a subcommand
/// (lower-case letters, digits, `-`, `_`) name one, so `cat /etc/hosts` and
/// `cat /etc/passwd` are one program, `cat`.
pub(super) fn key_of(argv: &[String]) -> Option<String> {
    let program = argv.first()?;
    let base = std::path::Path::new(program)
        .file_name()
        .map_or_else(|| program.clone(), |f| f.to_string_lossy().into_owned());
    let sub = argv
        .iter()
        .skip(1)
        .find(|a| !a.starts_with('-'))
        .filter(|a| {
            !a.is_empty()
                && a.len() <= 24
                && a.starts_with(|c: char| c.is_ascii_lowercase())
                && a.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        });
    Some(match sub {
        Some(s) => format!("{base} {s}"),
        None => base,
    })
}

#[derive(Default)]
pub(super) struct EtaBook {
    runs: HashMap<String, VecDeque<u64>>,
}

impl EtaBook {
    /// One finished job's duration under its program's key.
    pub(super) fn note(&mut self, key: &str, ms: u64) {
        let v = self.runs.entry(key.to_string()).or_default();
        if v.len() == KEEP {
            v.pop_front();
        }
        v.push_back(ms);
    }

    /// The median of the kept runs, once there are enough of them.
    pub(super) fn usual(&self, key: &str) -> Option<Eta> {
        let v = self.runs.get(key).filter(|v| v.len() >= MIN_RUNS)?;
        let mut sorted: Vec<u64> = v.iter().copied().collect();
        sorted.sort_unstable();
        let n = sorted.len();
        let usual_ms = if n % 2 == 1 {
            sorted[n / 2]
        } else {
            (sorted[n / 2 - 1] + sorted[n / 2]) / 2
        };
        Some(Eta {
            usual_ms,
            runs: n as u32,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    /// The key is the program and its subcommand, never an argument.
    #[test]
    fn a_key_is_the_program_and_its_first_word() {
        assert_eq!(
            key_of(&argv(&["/usr/bin/cargo", "test", "-p", "core"])).as_deref(),
            Some("cargo test")
        );
        assert_eq!(
            key_of(&argv(&["cargo", "--locked", "build"])).as_deref(),
            Some("cargo build")
        );
        assert_eq!(
            key_of(&argv(&["cat", "/etc/hosts"])).as_deref(),
            Some("cat")
        );
        assert_eq!(key_of(&argv(&["./run.sh"])).as_deref(), Some("run.sh"));
        assert_eq!(key_of(&[]), None);
    }

    /// One run is no estimate; the median of the last sixteen is.
    #[test]
    fn the_estimate_is_the_median_of_the_last_sixteen() {
        let mut b = EtaBook::default();
        b.note("cargo test", 1_000);
        assert!(b.usual("cargo test").is_none(), "one run says nothing");
        b.note("cargo test", 3_000);
        assert_eq!(b.usual("cargo test").unwrap().usual_ms, 2_000);
        for _ in 0..20 {
            b.note("cargo test", 10_000);
        }
        let e = b.usual("cargo test").unwrap();
        assert_eq!(
            (e.usual_ms, e.runs),
            (10_000, KEEP as u32),
            "old runs aged out"
        );
    }
}
