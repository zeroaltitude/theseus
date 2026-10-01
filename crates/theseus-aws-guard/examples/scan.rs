//! Scan CloudFormation templates against the guardrail list, as `aws.stack.plan` will (the AWS design's
//! §3.4 and §3.6). Each hit prints as the floor's confirm says it, then what the scan could not see.
//! Offline: it reads the files and nothing else.
//!
//! `cargo run -p theseus-aws-guard --example scan -- [--account ID] [--region R] [Name=Value …] T.yaml …`
//!
//! `Name=Value` sets a stack parameter; one left unset takes its default.

use std::collections::BTreeMap;

use theseus_aws_guard::{embedded, parse_template, Context};

fn main() {
    let mut ctx = Context {
        account: "111122223333".into(),
        region: "us-west-2".into(),
    };
    let mut parameters = BTreeMap::new();
    let mut files = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--account" => ctx.account = args.next().expect("--account takes an account id"),
            "--region" => ctx.region = args.next().expect("--region takes a region"),
            _ => match a.split_once('=') {
                Some((name, value)) => {
                    parameters.insert(name.to_string(), value.to_string());
                }
                None => files.push(a),
            },
        }
    }
    let mut failed = false;
    for file in &files {
        let scanned = std::fs::read_to_string(file)
            .map_err(|e| e.to_string())
            .and_then(|text| parse_template(&text).map_err(|e| e.to_string()))
            .and_then(|t| {
                embedded()
                    .scan(&t, &ctx, &parameters)
                    .map(|s| {
                        (
                            s.hits.iter().map(|h| h.confirm()).collect::<Vec<_>>(),
                            s.notes,
                        )
                    })
                    .map_err(|e| e.to_string())
            });
        match scanned {
            Ok((hits, notes)) => {
                println!("{file}: {} guardrail hits", hits.len());
                for h in hits {
                    println!("  {h}");
                }
                for n in notes {
                    println!("  note: {n}");
                }
            }
            Err(e) => {
                eprintln!("{file}: {e}");
                failed = true;
            }
        }
    }
    if failed {
        std::process::exit(1);
    }
}
