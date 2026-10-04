//! The servers the probe found working, and how each wants to be started:
//! its command, its languages, and its quirks. L2's `[lsp]` config can name
//! one of these, or give a command of its own.
//!
//! The quirks, as the probe measured them:
//! - **ty** and **TypeScript 7** (`tsc --lsp --stdio`) only pull diagnostics.
//! - **TypeScript 7** answers `shutdown` and ignores `exit`: the stop's kill
//!   ends it, after the grace.
//! - **typescript-language-server** waits forever on a project with no
//!   `node_modules/typescript` unless `initializationOptions.tsserver.path`
//!   names a `tsserver.js`.
//! - **rust-analyzer** reports readiness with `experimental/serverStatus`,
//!   and needs rustup's `cargo` first on `PATH` (given an old system cargo it
//!   sits idle and never loads the workspace): the caller's environment.

use std::path::PathBuf;

use serde_json::{json, Value};

/// One server's start: its command and the options the client needs.
#[derive(Debug, Clone, PartialEq)]
pub struct Preset {
    pub name: &'static str,
    pub argv: Vec<String>,
    /// The language ids it serves.
    pub languages: &'static [&'static str],
    pub initialization_options: Value,
    pub settings: Value,
    pub expects_server_status: bool,
}

impl Preset {
    /// The client options for this server on `root`.
    pub fn options(&self, root: impl Into<PathBuf>) -> crate::Options {
        let mut o = crate::Options::new(root);
        o.initialization_options = self.initialization_options.clone();
        o.settings = self.settings.clone();
        o.expects_server_status = self.expects_server_status;
        o
    }
}

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| (*s).to_string()).collect()
}

const PYTHON: &[&str] = &["python"];
const TS_JS: &[&str] = &[
    "typescript",
    "typescriptreact",
    "javascript",
    "javascriptreact",
];

pub fn ty() -> Preset {
    Preset {
        name: "ty",
        argv: argv(&["ty", "server"]),
        languages: PYTHON,
        initialization_options: Value::Null,
        settings: Value::Null,
        expects_server_status: false,
    }
}

pub fn pyright() -> Preset {
    Preset {
        name: "pyright",
        argv: argv(&["pyright-langserver", "--stdio"]),
        languages: PYTHON,
        initialization_options: Value::Null,
        settings: json!({ "python": { "analysis": { "diagnosticMode": "openFilesOnly" } } }),
        expects_server_status: false,
    }
}

pub fn basedpyright() -> Preset {
    Preset {
        name: "basedpyright",
        argv: argv(&["basedpyright-langserver", "--stdio"]),
        settings: json!({ "basedpyright": { "analysis": { "diagnosticMode": "openFilesOnly" } } }),
        ..pyright()
    }
}

/// TypeScript 7's native server.
pub fn tsgo() -> Preset {
    Preset {
        name: "tsgo",
        argv: argv(&["tsc", "--lsp", "--stdio"]),
        languages: TS_JS,
        initialization_options: Value::Null,
        settings: Value::Null,
        expects_server_status: false,
    }
}

/// typescript-language-server over TypeScript 5; `tsserver` is the path of a
/// `tsserver.js` (`node_modules/typescript/lib/tsserver.js`).
pub fn typescript_language_server(tsserver: &str) -> Preset {
    Preset {
        name: "typescript-language-server",
        argv: argv(&["typescript-language-server", "--stdio"]),
        languages: TS_JS,
        initialization_options: json!({ "tsserver": { "path": tsserver } }),
        settings: Value::Null,
        expects_server_status: false,
    }
}

pub fn rust_analyzer() -> Preset {
    Preset {
        name: "rust-analyzer",
        argv: argv(&["rust-analyzer"]),
        languages: &["rust"],
        initialization_options: Value::Null,
        settings: Value::Null,
        expects_server_status: true,
    }
}
