//! The size probe's base (AWS design §5, P2): what Theseus already links for
//! HTTP (tokio, reqwest on rustls, serde_json), without the AWS client. Its
//! release size, subtracted from `size-probe-client`'s, is what the client
//! adds to the binary.

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let url = std::env::args().nth(1).unwrap_or_default();
    let http = reqwest::Client::builder().build().expect("a client");
    if let Ok(r) = http.get(&url).send().await {
        let v: serde_json::Value = r.json().await.unwrap_or_default();
        println!("{v}");
    }
}
