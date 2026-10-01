//! Generates the AWS catalog from the AWS CLI's bundled botocore models.
//!
//! ```text
//! cargo run --release -p theseus-aws-catalog --example theseus-aws-catalog-gen -- \
//!     <botocore data dir> crates/theseus-aws-catalog/data/aws-catalog.bin <snapshot label>
//! ```
//!
//! The data directory is the CLI's `awscli/botocore/data` (each service's
//! `service-2.json`, `paginators-1.json`, and `endpoint-rule-set-1.json`, and
//! the shared `partitions.json`). The label records the models' source, for
//! example `aws-cli/2.34.15`. The weekly updater is "update the CLI, run
//! this, run the tests and the gate" (AWS design §2, principle 8).
//!
//! It is an example, not a binary, so that brotli's encoder stays a
//! dev-dependency: the library, and so Theseus, carries only the decoder.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use theseus_aws_catalog::compile::compile_dir;

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
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [dir, out, label] = args.as_slice() else {
        eprintln!("usage: theseus-aws-catalog-gen <botocore data dir> <out file> <snapshot label>");
        return ExitCode::from(2);
    };
    let (bytes, report) = match compile_dir(&PathBuf::from(dir), label, &compress) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("theseus-aws-catalog-gen: {e}");
            return ExitCode::FAILURE;
        }
    };
    let out = PathBuf::from(out);
    let tmp = out.with_extension("tmp");
    if let Err(e) = std::fs::write(&tmp, &bytes).and_then(|()| std::fs::rename(&tmp, &out)) {
        eprintln!("theseus-aws-catalog-gen: {}: {e}", out.display());
        return ExitCode::FAILURE;
    }
    println!("snapshot     {label}");
    println!("services     {}", report.services);
    println!("operations   {}", report.operations);
    println!("shapes       {}", report.shapes);
    println!("raw bytes    {}", report.raw_bytes);
    println!(
        "catalog      {} bytes ({})",
        report.compressed_bytes,
        out.display()
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
    ExitCode::SUCCESS
}
