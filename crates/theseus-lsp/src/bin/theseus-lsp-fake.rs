//! The fake language server as a process, on its stdin and stdout, for the
//! tests that need a real process (the stop's kill, a crash). A crash exits
//! with status 3.
//!
//! ```text
//! theseus-lsp-fake [--push | --pull | --pull-registered | --pull-and-check]
//!                  [--no-versions] [--slow-ms N] [--push-delay-ms N]
//!                  [--check-end-first] [--load-ms N] [--crash-after N]
//!                  [--ignore-exit] [--ask]
//! ```

use theseus_lsp::fake::{parse_args, Fake};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut cfg = parse_args(std::env::args().skip(1)).unwrap_or_else(|e| {
        eprintln!("theseus-lsp-fake: {e}");
        eprintln!(
            "usage: theseus-lsp-fake [--push | --pull | --pull-registered | --pull-and-check] \
             [--no-versions] [--slow-ms N] [--push-delay-ms N] [--check-end-first] [--load-ms N] \
             [--crash-after N] [--ignore-exit] [--ask]"
        );
        std::process::exit(2)
    });
    cfg.exit_on_crash = true;
    Fake::serve(cfg, tokio::io::stdin(), tokio::io::stdout()).await;
}
