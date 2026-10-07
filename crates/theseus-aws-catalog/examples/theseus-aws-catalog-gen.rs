//! Generates the AWS catalog from the AWS CLI's bundled botocore models.
//!
//! ```text
//! cargo run --release -p theseus-aws-catalog --example theseus-aws-catalog-gen -- \
//!     [--models <botocore data dir>] [--out <file>] [--label <snapshot label>]
//! ```
//!
//! With no arguments it reads the models of the `aws` on PATH (or the ones
//! `THESEUS_BOTOCORE_DATA` names), labels them with that CLI's version (the
//! first word of `aws --version`, for example `aws-cli/2.34.15`), and writes
//! the catalog the library embeds, `data/aws-catalog.bin`. The data directory
//! is the CLI's `awscli/botocore/data` (each service's `service-2.json`,
//! `paginators-1.json`, and `endpoint-rule-set-1.json`, and the shared
//! `partitions.json`). A label comes from the CLI only for the CLI's own
//! models: models from anywhere else need `--label`.
//!
//! The report's first line names the directory it read. The catalog records
//! the label alone: its snapshot is what `aws.describe` shows the model, and
//! a path in it would change the catalog from one machine to the next. The
//! weekly updater is "update the CLI, run this, run the tests and the gate"
//! (AWS design §2, principle 8). Among the tests, the output-shape rule
//! (theseus-core's `aws::tests_secret_shapes`, theseus-ye7o) fails an
//! operation whose output holds a credential-shaped member and is not
//! secret-bearing: decide it there (a mint, a stored secret, or an allowed
//! row with its reason) before the update lands.
//!
//! It is an example, not a binary, so that brotli's encoder stays a
//! dev-dependency: the library, and so Theseus, carries only the decoder.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use theseus_aws_catalog::compile::{self, compile_dir, BOTOCORE_DATA};

/// The catalog the library embeds.
const EMBEDDED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data/aws-catalog.bin");

const USAGE: &str = "usage: theseus-aws-catalog-gen [--models <botocore data dir>] \
                     [--out <file>] [--label <snapshot label>]";

/// Brotli at its best quality, with a 1 MiB window (the largest service,
/// EC2, is about 340 KB uncompressed).
fn compress(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut w = brotli::CompressorWriter::new(&mut out, 64 * 1024, 11, 20);
        w.write_all(raw).expect("writing to memory");
    }
    out
}

fn main() -> ExitCode {
    let (mut models, mut out, mut label) = (None, None, None);
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let slot = match flag.as_str() {
            "--models" => &mut models,
            "--out" => &mut out,
            "--label" => &mut label,
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        };
        let Some(value) = args.next() else {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        };
        *slot = Some(value);
    }
    match run(models, out, label) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("theseus-aws-catalog-gen: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(models: Option<String>, out: Option<String>, label: Option<String>) -> Result<(), String> {
    let (dir, from) = match models {
        Some(d) => (PathBuf::from(d), "--models"),
        None => (
            compile::botocore_data_dir().map_err(|e| e.to_string())?,
            if std::env::var_os(BOTOCORE_DATA).is_some_and(|v| !v.is_empty()) {
                BOTOCORE_DATA
            } else {
                "the aws on PATH"
            },
        ),
    };
    println!("models       {} ({from})", dir.display());
    let label = match label {
        Some(l) => l,
        None => cli_label_for(&dir)?,
    };
    println!("snapshot     {label}");
    let (bytes, report) = compile_dir(&dir, &label, &compress).map_err(|e| e.to_string())?;
    let out = PathBuf::from(out.as_deref().unwrap_or(EMBEDDED));
    let unchanged = std::fs::read(&out).is_ok_and(|old| old == bytes);
    let tmp = out.with_extension("tmp");
    std::fs::write(&tmp, &bytes)
        .and_then(|()| std::fs::rename(&tmp, &out))
        .map_err(|e| format!("{}: {e}", out.display()))?;
    println!("services     {}", report.services);
    println!("operations   {}", report.operations);
    println!("shapes       {}", report.shapes);
    println!("raw bytes    {}", report.raw_bytes);
    println!(
        "catalog      {} bytes ({}){}",
        report.compressed_bytes,
        out.display(),
        if unchanged { ", unchanged" } else { "" }
    );
    println!(
        "largest      {} ({} bytes raw)",
        report.largest.0, report.largest.1
    );
    println!(
        "endpoints    {} services differ from the default somewhere",
        report.endpoint_rules
    );
    println!(
        "unresolved   {} (service, region) pairs",
        report.endpoint_failures.len()
    );
    let mut by_error: std::collections::BTreeMap<(&str, &str), usize> =
        std::collections::BTreeMap::new();
    for (svc, _, _, e) in &report.endpoint_failures {
        *by_error.entry((svc.as_str(), e.as_str())).or_default() += 1;
    }
    for ((svc, e), n) in by_error.iter().take(12) {
        println!("  {n:5}  {svc}: {e}");
    }
    Ok(())
}

/// The version of the `aws` on PATH, when `dir` holds its models. Another
/// CLI's models (the old one under `/usr/local/aws-cli`, say) would be
/// labelled with the wrong version.
fn cli_label_for(dir: &Path) -> Result<String, String> {
    let aws =
        compile::aws_on_path().ok_or("no aws on PATH to take the label from: give --label")?;
    let own = compile::cli_data_dir(&aws).map_err(|e| e.to_string())?;
    let real = |p: &Path| std::fs::canonicalize(p).ok();
    if real(&own).is_none() || real(&own) != real(dir) {
        return Err(format!(
            "{} are not the models of the aws on PATH ({}, whose are {}): give --label",
            dir.display(),
            aws.display(),
            own.display()
        ));
    }
    compile::cli_label(&aws).map_err(|e| e.to_string())
}
