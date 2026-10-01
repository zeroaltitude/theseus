//! The size probe with the AWS client (see `size-probe-base`): the same
//! program, plus one call through `theseus-aws` whose service and operation
//! come from the command line, so every protocol, the signer, and the
//! embedded catalog are linked.

use theseus_aws::{Attribution, Call, Client, ClientConfig, Credentials};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let url = args.first().cloned().unwrap_or_default();
    let http = reqwest::Client::builder().build().expect("a client");
    if let Ok(r) = http.get(&url).send().await {
        let v: serde_json::Value = r.json().await.unwrap_or_default();
        println!("{v}");
    }
    if let [_, service, operation, input] = args.as_slice() {
        let input: serde_json::Value = serde_json::from_str(input).unwrap_or_default();
        let client = Client::new(ClientConfig::new("us-west-2"));
        let creds = Credentials::new(
            std::env::var("AWS_ACCESS_KEY_ID").unwrap_or_default(),
            std::env::var("AWS_SECRET_ACCESS_KEY").unwrap_or_default(),
            None,
            None,
        );
        let attribution = Attribution::default();
        let call = Call {
            service,
            operation,
            input: &input,
            region: None,
            pages: 1,
            attribution: &attribution,
        };
        println!("{:?}", client.call(&call, &creds).await.map(|o| o.body));
        println!(
            "{:?}",
            theseus_aws::catalog::describe_services(client.catalog().expect("the catalog"))
        );
    }
}
