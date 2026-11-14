//! Which TLS roots the daemon trusts, pinned (theseus-dn7p; the owner, 2026-11-14 13:55 MST, answered "A": write
//! down what it does today and test it, no change in behaviour).
//!
//! It used to be whatever the union of the workspace's crates asked for. Today, as `Cargo.lock` has it:
//!
//! - **HTTPS calls** (model providers, AWS, Google, MCP servers, the judge, Discord's REST) go through `reqwest`
//!   with `rustls-tls`, which in reqwest 0.12 is **the bundled set (webpki-roots) only**. So does the speech
//!   provider's socket (`tokio-tungstenite` with `rustls-tls-webpki-roots`).
//! - **The Discord gateway** (`twilight-gateway`, through `tokio-websockets` 0.13) trusts **the bundled set and
//!   the machine's roots** (`rustls-native-certs`). The voice gateway's socket (songbird's `tokio-websockets` 0.11)
//!   trusts **the machine's roots only**.
//!
//! A CA the machine trusts therefore works on the Discord gateways and is refused for a model provider. That is
//! the behaviour, written down; widening it is the owner's call. This test fails when a dependency change drops a
//! root source one of these has today, or gives the machine's roots to a connection that has none.
//! The one place the rows are listed is this file; `Cargo.toml`'s reqwest line points here.

use std::collections::BTreeMap;
use std::path::Path;

/// Every package of the lock, by name and version, with the names of the crates it depends on.
fn lock() -> BTreeMap<(String, String), Vec<String>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
    let text = std::fs::read_to_string(&path).expect("Cargo.lock");
    let lock: toml::Table = text.parse().expect("Cargo.lock parses");
    lock["package"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            let get = |k: &str| p[k].as_str().unwrap().to_string();
            let deps = p
                .get("dependencies")
                .and_then(|d| d.as_array())
                .into_iter()
                .flatten()
                // "name" or "name version"
                .filter_map(|d| d.as_str()?.split(' ').next().map(str::to_string))
                .collect();
            ((get("name"), get("version")), deps)
        })
        .collect()
}

/// The dependencies of every version of `name` that starts with `version`.
fn deps_of<'a>(
    lock: &'a BTreeMap<(String, String), Vec<String>>,
    name: &str,
    version: &str,
) -> Vec<&'a Vec<String>> {
    let found: Vec<_> = lock
        .iter()
        .filter(|((n, v), _)| n == name && v.starts_with(version))
        .map(|(_, d)| d)
        .collect();
    assert!(
        !found.is_empty(),
        "{name} {version} is no longer in the lock: update this test's rows"
    );
    found
}

fn has(d: &[String], what: &str) -> bool {
    d.iter().any(|x| x == what)
}

const BUNDLED: &str = "webpki-roots";
const MACHINES: &str = "rustls-native-certs";

/// Each row: the connection, its crate and version, and which root sources it has today.
#[test]
fn each_connection_trusts_the_roots_it_trusts_today() {
    let lock = lock();
    let rows: [(&str, &str, &str, bool, bool); 5] = [
        ("HTTPS calls", "reqwest", "0.12", true, false),
        (
            "the speech provider's socket",
            "tokio-tungstenite",
            "0.26",
            true,
            false,
        ),
        (
            "the Discord gateway",
            "tokio-websockets",
            "0.13",
            true,
            true,
        ),
        (
            "the voice gateway's socket",
            "tokio-websockets",
            "0.11",
            false,
            true,
        ),
        (
            "Discord's REST, through hyper-rustls",
            "hyper-rustls",
            "0.27",
            true,
            false,
        ),
    ];
    for (what, name, version, bundled, machines) in rows {
        for d in deps_of(&lock, name, version) {
            assert_eq!(
                has(d, BUNDLED),
                bundled,
                "{what} ({name} {version}): the bundled set"
            );
            assert_eq!(
                has(d, MACHINES),
                machines,
                "{what} ({name} {version}): the machine's roots"
            );
        }
    }
}

/// No other crate asks for the machine's roots: a new one that does would widen what some connection trusts
/// without anyone choosing it.
#[test]
fn only_the_two_gateways_sockets_ask_for_the_machines_roots() {
    let lock = lock();
    let asking: Vec<&str> = lock
        .iter()
        .filter(|(_, d)| has(d, MACHINES))
        .map(|((n, _), _)| n.as_str())
        .collect();
    assert_eq!(asking, ["tokio-websockets", "tokio-websockets"]);
}
