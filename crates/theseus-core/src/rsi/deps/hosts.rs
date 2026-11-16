//! The registries a self build may reach, as a closed list in code, each with
//! its reason. Nothing is read from a config, a branch or a call: a host is
//! here because a person wrote it here.
//!
//! Two readers, never one for the other:
//!
//! - the **builder's** commands (`builder`): their egress list is these hosts and no
//!   others (the operator's own `[sandbox] egress` is not given to a self session,
//!   `Sandbox::egress_for`), so cargo and npm can download what the builder wants to try;
//! - the **pipeline's** fetch (`fetch`): downloads exactly what a branch's lock
//!   files name, for its gate to read. [`Endpoints::registries`] are the two URLs
//!   it asks, and a test holds both to this list.

/// One registry host a self build may reach.
#[derive(Debug, Clone, Copy)]
pub struct Host {
    pub name: &'static str,
    pub port: u16,
    /// Why this host, and what is asked of it.
    pub why: &'static str,
}

/// Every host a self builder's sandboxed commands may reach, in place of the operator's own
/// `[sandbox] egress`. The registries this repository already uses, and no
/// others: no `crates.io` itself (its pages and API are not needed to fetch),
/// no GitHub (a git dependency is refused, `policy`), no other npm host.
pub const HOSTS: &[Host] = &[
    Host {
        name: "index.crates.io",
        port: 443,
        why: "cargo's sparse index: which versions of a crate exist, and what each needs",
    },
    Host {
        name: "static.crates.io",
        port: 443,
        why: "cargo's downloads: the `.crate` file of each version, which the index points at",
    },
    Host {
        name: "registry.npmjs.org",
        port: 443,
        why: "the npm registry the cockpit's lock file names: package metadata and tarballs",
    },
];

/// [`HOSTS`] as an egress list, each entry `host:port` as the proxy reads it.
pub fn egress() -> Vec<String> {
    HOSTS
        .iter()
        .map(|h| format!("{}:{}", h.name, h.port))
        .collect()
}

/// The `Cargo.lock` sources that are crates.io: the legacy form cargo still
/// writes for it, and the sparse one. A package from any other source is
/// refused ([`super::policy`]).
pub const CARGO_SOURCES: [&str; 2] = [
    "registry+https://github.com/rust-lang/crates.io-index",
    "registry+sparse+https://index.crates.io/",
];

/// Where the pipeline's own fetch asks for packages. Real code has one value,
/// [`Endpoints::registries`]; a test builds one that points at a server it owns.
#[derive(Debug, Clone)]
pub struct Endpoints {
    /// The directory of `.crate` files: a file is `<crates>/<name>/<name>-<version>.crate`.
    pub crates: String,
    /// The npm registry's origin: every `resolved` of the cockpit's lock file
    /// must begin with it and a `/`.
    pub npm: String,
}

impl Endpoints {
    /// The registries themselves.
    pub fn registries() -> Self {
        Self {
            crates: "https://static.crates.io/crates".into(),
            npm: "https://registry.npmjs.org".into(),
        }
    }
}
