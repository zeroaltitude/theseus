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
//!   With a large workspace loaded it takes seconds to exit, so its preset
//!   gives it a longer grace before the kill. Its pull answers with its own
//!   analysis alone, which by default misses rustc's errors (E0277, E0425):
//!   those it pushes from `cargo check`, which it runs after a save under
//!   the progress token `rust-analyzer/flycheck/<n>`, so its preset names
//!   that token (`check_token`, theseus-c6hv).

use std::path::PathBuf;
use std::time::Duration;

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
    /// The progress token prefix of its after-save check
    /// (`Options::check_token`).
    pub check_token: Option<&'static str>,
    /// How long it has to exit after `exit` before the kill.
    pub exit_grace: Duration,
}

impl Preset {
    /// The client options for this server on `root`.
    pub fn options(&self, root: impl Into<PathBuf>) -> crate::Options {
        let mut o = crate::Options::new(root);
        o.initialization_options = self.initialization_options.clone();
        o.settings = self.settings.clone();
        o.expects_server_status = self.expects_server_status;
        o.check_token = self.check_token.map(String::from);
        o.exit_grace = self.exit_grace;
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
        check_token: None,
        exit_grace: Duration::from_secs(1),
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
        check_token: None,
        exit_grace: Duration::from_secs(1),
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
        check_token: None,
        exit_grace: Duration::from_secs(1),
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
        check_token: None,
        exit_grace: Duration::from_secs(1),
    }
}

pub fn rust_analyzer() -> Preset {
    Preset {
        name: "rust-analyzer",
        argv: argv(&["rust-analyzer"]),
        languages: &["rust"],
        // Its checks build in `target/rust-analyzer`, never taking cargo's
        // lock on the agent's own build directory (theseus-ext.12). Its
        // `workspace/configuration` answer says the same, since that answer
        // replaces what `initialize` gave: settings of the operator's own
        // replace these, so they keep `cargo.targetDir` too.
        initialization_options: json!({ "cargo": { "targetDir": true } }),
        settings: json!({ "rust-analyzer": { "cargo": { "targetDir": true } } }),
        expects_server_status: true,
        // The probe's token was `rust-analyzer/flycheck/0`: one per workspace.
        check_token: Some("rust-analyzer/flycheck/"),
        // With a workspace loaded it takes seconds to exit: 2.6 s on this
        // repository's (4.2 GB), so the default 1 s grace killed it.
        exit_grace: Duration::from_secs(5),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only rust-analyzer checks after a save: its options name the check's
    /// token, and no other preset's do (theseus-c6hv).
    #[test]
    fn only_rust_analyzer_names_a_check_token() {
        let root = std::path::Path::new("/w");
        assert_eq!(
            rust_analyzer().options(root).check_token.as_deref(),
            Some("rust-analyzer/flycheck/")
        );
        let others = [
            ty(),
            pyright(),
            basedpyright(),
            tsgo(),
            typescript_language_server(""),
        ];
        for p in others {
            assert_eq!(p.options(root).check_token, None, "{}", p.name);
        }
    }
}
