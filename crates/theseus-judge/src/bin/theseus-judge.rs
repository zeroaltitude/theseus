//! `theseus-judge`: the judge crate's own command line. Today it has one
//! command, `prove`, which reads task records (JSON lines, shaped as
//! `theseus_judge::prove` documents) and writes the exit report.
//!
//! ```text
//! theseus-judge prove <records.jsonl> [--json <path|->] [--markdown <path|->]
//!                     [--min-tasks N] [--min-labeled N]
//! ```
//!
//! With neither output flag, the Markdown goes to stdout. The exit status is 0
//! whenever a report was made, whatever its verdict says ("insufficient" is an
//! answer); a bad input or flag is 1 with the reason.

use std::path::Path;

use anyhow::{bail, Context, Result};
use theseus_judge::prove::{markdown, parse_records, prove, ProveMinimum};

const USAGE: &str = "usage: theseus-judge prove <records.jsonl> [--json <path|->] [--markdown <path|->] [--min-tasks N] [--min-labeled N]";

fn emit(to: &str, text: &str) -> Result<()> {
    if to == "-" {
        print!("{text}");
        return Ok(());
    }
    std::fs::write(Path::new(to), text).with_context(|| format!("writing {to}"))
}

fn run(args: &[String]) -> Result<()> {
    let [cmd, rest @ ..] = args else {
        bail!("{USAGE}")
    };
    if cmd != "prove" {
        bail!("unknown command {cmd:?}\n{USAGE}");
    }
    let (mut input, mut json, mut md) = (None, None, None);
    let mut min = ProveMinimum::default();
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        let mut value = |flag: &str| {
            it.next()
                .cloned()
                .with_context(|| format!("{flag} needs a value"))
        };
        match a.as_str() {
            "--json" => json = Some(value("--json")?),
            "--markdown" => md = Some(value("--markdown")?),
            "--min-tasks" => {
                min.tasks_per_arm = value("--min-tasks")?
                    .parse()
                    .context("--min-tasks: a count")?
            }
            "--min-labeled" => {
                min.labeled_per_metric = value("--min-labeled")?
                    .parse()
                    .context("--min-labeled: a count")?
            }
            f if f.starts_with("--") => bail!("unknown flag {f}\n{USAGE}"),
            p if input.is_none() => input = Some(p.to_string()),
            _ => bail!("one input file only\n{USAGE}"),
        }
    }
    let input = input.with_context(|| USAGE.to_string())?;
    let text = std::fs::read_to_string(&input).with_context(|| format!("reading {input}"))?;
    let report = prove(&parse_records(&text)?, min);
    if let Some(p) = &json {
        emit(p, &(serde_json::to_string_pretty(&report)? + "\n"))?;
    }
    if md.is_some() || json.is_none() {
        emit(md.as_deref().unwrap_or("-"), &markdown(&report))?;
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = run(&args) {
        eprintln!("theseus-judge: {e:#}");
        std::process::exit(1);
    }
}
