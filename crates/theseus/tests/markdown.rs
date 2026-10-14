//! The markdown renderer's goldens and its streaming property (theseus-rn7s).
//! Each input in `golden_md/*.md` is drawn at 80, 100 and 120 columns, plain
//! and as ANSI (escapes shown as `\e`); `THESEUS_GOLDEN=write` rewrites the
//! goldens, for an output change you mean, with the diff read.

use std::path::{Path, PathBuf};
use std::time::Instant;

use theseus_client::render::md::Markdown;
use theseus_client::render::style::{Mode, StyledLine};

const WIDTHS: [usize; 3] = [80, 100, 120];

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden_md")
}

fn inputs() -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = std::fs::read_dir(dir())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
        .map(|e| {
            let name = e.path().file_stem().unwrap().to_string_lossy().to_string();
            (name, std::fs::read_to_string(e.path()).unwrap())
        })
        .collect();
    v.sort();
    v
}

/// The lines as a person reads a capture: ANSI with `\e` for the escape, and
/// `$` at each line's end so a trailing space shows.
fn shown(lines: &[StyledLine], ansi: bool) -> String {
    lines
        .iter()
        .map(|l| {
            let t = if ansi {
                l.ansi().replace('\x1b', "\\e")
            } else {
                l.plain()
            };
            format!("{t}$\n")
        })
        .collect()
}

fn golden_of(text: &str) -> String {
    let mut out = String::new();
    for w in WIDTHS {
        for (mode, label) in [(Mode::Plain, "plain"), (Mode::Ansi, "ansi")] {
            out.push_str(&format!("=== {w} columns, {label} ===\n"));
            out.push_str(&shown(&Markdown::render(text, w, mode), mode == Mode::Ansi));
        }
    }
    out
}

#[test]
fn each_input_draws_as_its_golden() {
    let write = std::env::var("THESEUS_GOLDEN").is_ok_and(|v| v == "write");
    let mut bad = Vec::new();
    for (name, text) in inputs() {
        let path = dir().join(format!("{name}.golden.txt"));
        let got = golden_of(&text);
        if write {
            std::fs::write(&path, &got).unwrap();
        } else if std::fs::read_to_string(&path).ok().as_deref() != Some(got.as_str()) {
            bad.push(name);
        }
    }
    assert!(
        bad.is_empty(),
        "drawn differently from the golden: {bad:?} (THESEUS_GOLDEN=write rewrites)"
    );
}

/// A small generator, seeded: the same cuts every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self, below: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % below as u64) as usize
    }
}

fn streamed(
    text: &str,
    width: usize,
    mode: Mode,
    rng: &mut Rng,
    max_piece: usize,
) -> Vec<StyledLine> {
    let mut md = Markdown::new(width, mode);
    let mut out = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut at = 0;
    while at < chars.len() {
        let n = 1 + rng.next(max_piece);
        let piece: String = chars[at..(at + n).min(chars.len())].iter().collect();
        out.extend(md.push(&piece));
        at += n;
    }
    out.extend(md.finish());
    out
}

#[test]
fn a_reply_cut_at_seeded_random_points_draws_as_the_whole() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for (name, text) in inputs() {
        for w in WIDTHS {
            for mode in [Mode::Plain, Mode::Ansi] {
                let whole = Markdown::render(&text, w, mode);
                // One character at a time, then pieces up to 3, 17 and 200.
                for max in [1, 3, 17, 200] {
                    for _ in 0..4 {
                        let got = streamed(&text, w, mode, &mut rng, max);
                        assert_eq!(got, whole, "{name} at {w}, {mode:?}, pieces up to {max}");
                    }
                }
            }
        }
    }
}

#[test]
fn a_streamed_reply_gives_its_lines_out_before_it_ends() {
    let mut md = Markdown::new(80, Mode::Ansi);
    let head = md.push("# Title\n\nFirst paragraph.\n\nSecond, still arri");
    let plain: Vec<String> = head.iter().map(StyledLine::plain).collect();
    assert_eq!(plain, ["Title", "", "First paragraph."]);
    let rest = md.finish();
    assert_eq!(
        rest.iter().map(StyledLine::plain).collect::<Vec<_>>(),
        ["", "Second, still arri"]
    );
}

#[test]
fn plain_mode_holds_no_escape_and_ansi_holds_some() {
    for (name, text) in inputs() {
        for w in WIDTHS {
            let plain = Markdown::render(&text, w, Mode::Plain);
            assert!(
                plain.iter().all(|l| !l.ansi().contains('\x1b')),
                "{name} at {w}"
            );
        }
    }
    let reply = std::fs::read_to_string(dir().join("reply.md")).unwrap();
    let lines = Markdown::render(&reply, 80, Mode::Ansi);
    assert!(
        lines.iter().any(|l| l.ansi().contains("\x1b[1")),
        "bold or a heading is drawn"
    );
}

#[test]
fn no_line_is_wider_than_the_width() {
    for (name, text) in inputs() {
        for w in WIDTHS {
            for mode in [Mode::Plain, Mode::Ansi] {
                for l in Markdown::render(&text, w, mode) {
                    assert!(
                        l.width() <= w,
                        "{name} at {w}: {:?} is {} wide",
                        l.plain(),
                        l.width()
                    );
                }
            }
        }
    }
}

/// Rendering costs at most 2 ms per 100 lines (theseus-rn7s). This is a debug
/// build of the workspace's own crate, the slow case; the bound below is the
/// median of five runs on the cloud VM times three.
#[test]
fn rendering_costs_little_per_hundred_lines() {
    let mut text = String::new();
    for (name, t) in inputs() {
        if name != "longline" {
            text.push_str(&t);
            text.push('\n');
        }
    }
    let mut times = Vec::new();
    let mut lines = 0;
    for _ in 0..5 {
        let at = Instant::now();
        lines = Markdown::render(&text, 100, Mode::Ansi).len();
        times.push(at.elapsed());
    }
    times.sort();
    let per_100 = times[2].as_secs_f64() * 1000.0 * 100.0 / lines as f64;
    eprintln!(
        "markdown: {lines} lines, median {:?}, {per_100:.3} ms per 100 lines",
        times[2]
    );
    assert!(per_100 < BOUND_MS_PER_100, "{per_100:.3} ms per 100 lines");
}

/// Measured on the cloud VM in a debug build, then times three.
const BOUND_MS_PER_100: f64 = 2.0;
