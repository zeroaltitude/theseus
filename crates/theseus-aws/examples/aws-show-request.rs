//! Prints the request the client would send for a call, unsigned parts only
//! (a debugging aid; no network).
//!
//! ```text
//! cargo run -p theseus-aws --example aws-show-request -- ec2 DescribeInstances '{"InstanceIds": ["i-1"]}'
//! ```

use std::time::SystemTime;

use theseus_aws::{Attribution, Call, Client, ClientConfig, Credentials};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [service, operation, input] = args.as_slice() else {
        eprintln!("usage: aws-show-request <service> <operation> <input JSON>");
        std::process::exit(2);
    };
    let input: serde_json::Value = serde_json::from_str(input).expect("JSON input");
    let client = Client::new(ClientConfig::new("us-west-2"));
    let creds = Credentials::new("AKIDEXAMPLE", "example", None, None);
    let attribution = Attribution::default();
    let call = Call {
        service,
        operation,
        input: &input,
        region: None,
        pages: 1,
        attribution: &attribution,
    };
    match client.prepare(&call, &creds, SystemTime::now()) {
        Ok(p) => {
            let r = &p.request;
            println!("{} {}", r.method, r.url());
            for (k, v) in &r.headers {
                if !k.eq_ignore_ascii_case("authorization") {
                    println!("{k}: {v}");
                }
            }
            println!();
            println!("{}", String::from_utf8_lossy(&r.body));
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
