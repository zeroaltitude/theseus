
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut cfg = Config::default();
    let mut http = false;
    let mut port = 0u16;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--mode" => {
                let m = value(&mut args, "--mode");
                cfg.mode = Mode::parse(&m).unwrap_or_else(|| {
                    eprintln!("theseus-mcp-fake: no mode {m:?}");
                    usage()
                });
            }
            "--http" => http = true,
            "--port" => port = number(&value(&mut args, "--port"), "--port"),
            "--page-size" => {
                cfg.page_size = number(&value(&mut args, "--page-size"), "--page-size")
            }
            "--slow-ms" => cfg.slow_ms = number(&value(&mut args, "--slow-ms"), "--slow-ms"),
            "--versions" => {
                cfg.versions = value(&mut args, "--versions")
                    .split(',')
                    .map(|v| v.trim().to_string())
                    .filter(|v| !v.is_empty())
                    .collect();
            }
            "--json" => cfg.sse = false,
            "--no-server-stream" => cfg.server_stream = false,
            "--poll" => cfg.poll = true,
            "--bearer-file" => {
                let path = value(&mut args, "--bearer-file");
                let key = std::fs::read_to_string(&path).unwrap_or_else(|e| {
                    eprintln!("theseus-mcp-fake: reading {path}: {e}");
                    std::process::exit(1)
                });
                cfg.bearer = Some(key.trim().to_string());
            }
            "--name" => cfg.name = value(&mut args, "--name"),
            "-h" | "--help" => usage(),
            other => {
                eprintln!("theseus-mcp-fake: unknown argument {other:?}");
                usage()
            }
        }
    }
    if http {
        let fake = Fake::new(cfg);
        match fake.serve_http(port).await {
            Ok(addr) => eprintln!("theseus-mcp-fake: listening on {addr}"),
            Err(e) => {
                eprintln!("theseus-mcp-fake: binding 127.0.0.1:{port}: {e}");
                std::process::exit(1)
            }
        }
        std::future::pending::<()>().await;
    } else {
        cfg.exit_on_crash = true;
        Fake::new(cfg)
            .serve_pipes(tokio::io::stdin(), tokio::io::stdout())
            .await;
    }
}
