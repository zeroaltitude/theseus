
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut key_file = None;
    let mut port = 0u16;
    let mut turn_ms = 200;
    let mut rpm = 60;
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        match flag.as_str() {
            "--key-file" => key_file = it.next(),
            "--port" => {
                port = u16::try_from(number(&mut it, "--port"))
                    .unwrap_or_else(|_| fail("--port is too large"))
            }
            "--turn-ms" => turn_ms = number(&mut it, "--turn-ms"),
            "--rpm" => rpm = u32::try_from(number(&mut it, "--rpm")).unwrap_or(u32::MAX),
            other => fail(&format!("unknown argument {other:?}")),
        }
    }
    let key_file = key_file.unwrap_or_else(|| fail("--key-file is required"));
    let key = std::fs::read_to_string(&key_file)
        .unwrap_or_else(|e| fail(&format!("reading {key_file}: {e}")));
    let mut cfg = Config::new(([127, 0, 0, 1], port).into(), key.trim());
    cfg.requests_per_minute = rpm;
    let core = Arc::new(FakeCore::new(Duration::from_millis(turn_ms)));
    let server = Server::bind(cfg, core)
        .await
        .unwrap_or_else(|e| fail(&format!("binding: {e}")));
    eprintln!(
        "theseus mcp server (fake core): listening on {}",
        server.addr()
    );
    // Its counters, each time they change, for a live check to read.
    let mut last = String::new();
    loop {
        tokio::time::sleep(Duration::from_millis(250)).await;
        let s = server.stats();
        let mut refused: Vec<String> = s
            .refused
            .iter()
            .map(|(w, n)| format!("{w:?} {n}"))
            .collect();
        refused.sort();
        let now = format!(
            "stats: sessions {} (opened {}, last by {}), clients {:?}, calls {}, errors {}, refused [{}]",
            s.sessions,
            s.opened,
            s.last_client.as_deref().unwrap_or("none"),
            s.clients,
            s.calls,
            s.errors,
            refused.join(", ")
        );
        if now != last {
            eprintln!("{now}");
            last = now;
        }
    }
}
