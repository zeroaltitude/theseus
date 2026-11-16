//! What the lock files and manifests say, the hosts, and the builder's half.
//! No network and no process: text in, words out.

use std::path::Path;

use super::hosts::{self, Endpoints};
use super::lock::{self, Pkg};
use super::{builder, policy};

const CRATES_IO: &str = "registry+https://github.com/rust-lang/crates.io-index";
const SUM_A: &str = "92ecc6618181def0457392ccd0ee51198e065e016d1d527a7ac1b6dc7c1f09d2";
const SUM_B: &str = "0000000000000000000000000000000000000000000000000000000000000001";

/// A `Cargo.lock` of `(name, version, source, checksum)`.
pub(crate) fn cargo_lock(pkgs: &[(&str, &str, Option<&str>, Option<&str>)]) -> String {
    let mut out = String::from("version = 4\n\n");
    for (n, v, s, c) in pkgs {
        out.push_str(&format!("[[package]]\nname = \"{n}\"\nversion = \"{v}\"\n"));
        if let Some(s) = s {
            out.push_str(&format!("source = \"{s}\"\n"));
        }
        if let Some(c) = c {
            out.push_str(&format!("checksum = \"{c}\"\n"));
        }
        out.push('\n');
    }
    out
}

/// A `package-lock.json` of `(path, version, resolved, integrity)`.
pub(crate) fn npm_lock(pkgs: &[(&str, &str, &str, &str)]) -> String {
    let mut m = serde_json::Map::new();
    m.insert("".into(), serde_json::json!({"name": "cockpit"}));
    for (k, v, r, i) in pkgs {
        let mut e = serde_json::json!({"version": v});
        if !r.is_empty() {
            e["resolved"] = (*r).into();
        }
        if !i.is_empty() {
            e["integrity"] = (*i).into();
        }
        m.insert((*k).into(), e);
    }
    serde_json::json!({"name": "cockpit", "lockfileVersion": 3, "packages": m}).to_string()
}

fn ends() -> Endpoints {
    Endpoints::registries()
}

#[test]
fn only_crates_io_packages_with_a_checksum_are_read_from_a_cargo_lock() {
    let text = cargo_lock(&[
        ("theseus-core", "0.1.0", None, None),
        ("itoa", "1.0.17", Some(CRATES_IO), Some(SUM_A)),
    ]);
    let l = lock::cargo(&text).unwrap();
    assert!(l.problems.is_empty(), "{:?}", l.problems);
    assert_eq!(
        l.pkgs.len(),
        1,
        "a workspace member has no source and is no package"
    );
    assert_eq!(
        (l.pkgs[0].name.as_str(), l.pkgs[0].version.as_str()),
        ("itoa", "1.0.17")
    );
    assert_eq!(l.pkgs[0].from, "crates.io");
    // The sparse spelling of the same registry is the same registry.
    let sparse = cargo_lock(&[(
        "itoa",
        "1.0.17",
        Some("registry+sparse+https://index.crates.io/"),
        Some(SUM_A),
    )]);
    assert!(lock::cargo(&sparse).unwrap().problems.is_empty());
}

#[test]
fn a_cargo_package_from_anywhere_else_or_unchecked_is_a_problem_and_not_a_package() {
    for (name, source, sum, said) in [
        (
            "g",
            "git+https://example.invalid/g?rev=abc#abc",
            None,
            "only crates.io is allowed",
        ),
        (
            "r",
            "registry+https://other.invalid/index",
            Some(SUM_A),
            "only crates.io is allowed",
        ),
        ("c", CRATES_IO, None, "no sha256 checksum"),
        ("c", CRATES_IO, Some("abcd"), "no sha256 checksum"),
        ("../x", CRATES_IO, Some(SUM_A), "not a crate name"),
    ] {
        let l = lock::cargo(&cargo_lock(&[(name, "1.0.0", Some(source), sum)])).unwrap();
        assert!(l.pkgs.is_empty(), "{name}");
        assert_eq!(l.problems.len(), 1, "{name}: {:?}", l.problems);
        assert!(l.problems[0].contains(said), "{:?}", l.problems);
    }
    assert!(lock::cargo("this is [not toml").is_err());
}

#[test]
fn an_npm_lock_is_read_and_only_the_npm_registry_with_an_integrity_is_taken() {
    let ok = format!("{}/left-pad/-/left-pad-1.3.0.tgz", ends().npm);
    let text = npm_lock(&[
        ("node_modules/left-pad", "1.3.0", &ok, "sha512-AAAA"),
        (
            "node_modules/@scope/pkg",
            "2.0.0",
            &format!("{}/@scope/pkg/-/pkg-2.0.0.tgz", ends().npm),
            "sha256-BBBB sha1-CCCC",
        ),
    ]);
    let l = lock::npm(&text, &ends()).unwrap();
    assert!(l.problems.is_empty(), "{:?}", l.problems);
    let names: Vec<_> = l
        .pkgs
        .iter()
        .map(|p| (p.name.as_str(), p.check.as_str()))
        .collect();
    assert_eq!(
        names,
        [("@scope/pkg", "sha256-BBBB"), ("left-pad", "sha512-AAAA")]
    );

    for (resolved, integrity, said) in [
        (
            "https://example.invalid/x/-/x-1.0.0.tgz",
            "sha512-AA",
            "only https://registry.npmjs.org is allowed",
        ),
        (
            "git+ssh://git@example.invalid/x.git",
            "sha512-AA",
            "only https://registry.npmjs.org is allowed",
        ),
        (
            "",
            "sha512-AA",
            "only https://registry.npmjs.org is allowed",
        ),
        (
            "https://registry.npmjs.org/x/-/x-1.0.0.tgz",
            "",
            "no sha512",
        ),
        (
            "https://registry.npmjs.org/x/-/x-1.0.0.tgz",
            "sha1-AAAA",
            "no sha512",
        ),
        // A host that merely begins with the registry's name is not the registry.
        (
            "https://registry.npmjs.org.example.invalid/x.tgz",
            "sha512-AA",
            "only https://registry.npmjs.org is allowed",
        ),
        (
            "https://registry.npmjs.org@example.invalid/x.tgz",
            "sha512-AA",
            "only https://registry.npmjs.org is allowed",
        ),
    ] {
        let l = lock::npm(
            &npm_lock(&[("node_modules/x", "1.0.0", resolved, integrity)]),
            &ends(),
        )
        .unwrap();
        assert!(l.pkgs.is_empty(), "{resolved} {integrity}");
        assert!(
            l.problems.iter().any(|p| p.contains(said)),
            "{resolved}: {:?}",
            l.problems
        );
    }
    // A link to a local path, and a lock file of the old shape.
    let link = r#"{"lockfileVersion":3,"packages":{"":{},"node_modules/x":{"resolved":"../x","link":true}}}"#;
    assert!(lock::npm(link, &ends()).unwrap().problems[0].contains("link to a local path"));
    assert!(lock::npm(r#"{"lockfileVersion":1,"dependencies":{}}"#, &ends()).is_err());
}

fn pkg(name: &str, version: &str, check: &str) -> Pkg {
    Pkg {
        name: name.into(),
        version: version.into(),
        from: "crates.io".into(),
        check: check.into(),
        url: String::new(),
    }
}

#[test]
fn what_a_change_did_to_the_packages_is_one_line_each() {
    let old = [
        pkg("itoa", "1.0.17", SUM_A),
        pkg("tokio", "1.40.0", SUM_A),
        pkg("gone", "0.2.0", SUM_A),
        pkg("same", "1.0.0", SUM_A),
        pkg("twin", "1.0.0", SUM_A),
    ];
    let new = [
        pkg("same", "1.0.0", SUM_A),
        pkg("tokio", "1.41.0", SUM_B),
        pkg("fresh", "2.1.0", SUM_B),
        pkg("itoa", "1.0.17", SUM_B),
        pkg("twin", "1.0.0", SUM_A),
        pkg("twin", "2.0.0", SUM_B),
    ];
    let got = lock::changes("cargo", &old, &new);
    assert_eq!(
        got,
        [
            "cargo: added fresh 2.1.0, from crates.io",
            "cargo: removed gone 0.2.0 (it came from crates.io)",
            "cargo: itoa 1.0.17: same version, but its checksum changed",
            "cargo: tokio 1.40.0 -> 1.41.0, from crates.io",
            "cargo: added twin 2.0.0, from crates.io",
        ]
    );
    // A reorder is no change; neither is nothing.
    let mut shuffled = old.to_vec();
    shuffled.reverse();
    assert!(lock::changes("cargo", &old, &shuffled).is_empty());
    assert!(lock::changes("npm", &[], &[]).is_empty());
}

#[test]
fn a_dependency_from_anywhere_but_the_registries_is_found_in_a_manifest() {
    let toml = |text: &str| policy::manifest(Path::new("crates/a/Cargo.toml"), text);
    assert!(toml("[dependencies]\nserde = \"1\"\nlocal = { path = \"../b\" }\n[dev-dependencies]\nx = { version = \"1\", features = [\"f\"] }\n").is_empty());
    for (text, said) in [
        (
            "[dependencies]\nx = { git = \"https://example.invalid/x\" }\n",
            "names a `git`",
        ),
        (
            "[dependencies]\nx = { version = \"1\", registry = \"other\" }\n",
            "names a `registry`",
        ),
        (
            "[build-dependencies]\nx = { git = \"https://example.invalid/x\", rev = \"abc\" }\n",
            "names a `",
        ),
        (
            "[target.'cfg(unix)'.dependencies]\nx = { git = \"https://example.invalid/x\" }\n",
            "names a `git`",
        ),
        (
            "[workspace.dependencies]\nx = { git = \"https://example.invalid/x\" }\n",
            "names a `git`",
        ),
        (
            "[dependencies]\nx = { path = \"../../../outside\" }\n",
            "leaves the tree",
        ),
        (
            "[dependencies]\nx = { path = \"/etc/x\" }\n",
            "leaves the tree",
        ),
        (
            "[patch.crates-io]\nx = { path = \"../../../../x\" }\n",
            "leaves the tree",
        ),
        (
            "[patch.\"https://example.invalid/index\"]\nx = { version = \"1\" }\n",
            "other than crates.io",
        ),
        (
            "[replace]\n\"x:1.0.0\" = { git = \"https://example.invalid/x\" }\n",
            "[replace]",
        ),
        ("[dependencies\n", "could not be read"),
    ] {
        let got = toml(text);
        assert!(got.iter().any(|g| g.contains(said)), "{text}: {got:?}");
        assert!(
            got.iter().all(|g| g.starts_with("crates/a/Cargo.toml: ")),
            "{got:?}"
        );
    }
    // A crate that happens to be named like a source key is no source.
    assert!(toml("[dependencies]\ngit = \"1\"\nregistry = \"1\"\nrev = \"1\"\n").is_empty());
    // `../b` from `crates/a` stays in the tree; one more `..` from the root does not.
    assert!(policy::manifest(
        Path::new("Cargo.toml"),
        "[dependencies]\nx = { path = \"../b\" }\n"
    )[0]
    .contains("leaves the tree"));
}

#[test]
fn a_cargo_config_that_changes_where_packages_come_from_is_found() {
    let cfg = |text: &str| policy::config(Path::new(".cargo/config.toml"), text);
    assert!(cfg(
        "[target.x86_64-unknown-linux-musl]\nlinker = \"rust-lld\"\n[env]\nCC = \"musl-gcc\"\n"
    )
    .is_empty());
    for table in [
        "source.crates-io]\nreplace-with = \"x\"",
        "registries.x]\nindex = \"https://example.invalid\"",
        "patch.crates-io]\nx = { path = \"x\" }",
        "paths]\n",
    ] {
        let got = cfg(&format!("[{table}\n"));
        assert_eq!(got.len(), 1, "{table}: {got:?}");
        assert!(
            got[0].contains("changes where cargo finds packages"),
            "{got:?}"
        );
    }
    assert!(policy::is_manifest("Cargo.toml") && policy::is_manifest("crates/a/Cargo.toml"));
    assert!(!policy::is_manifest("crates/a/NotCargo.toml"));
    assert!(policy::is_config(".cargo/config.toml") && policy::is_config("crates/a/.cargo/config"));
    assert!(!policy::is_config("docs/cargo/config.toml"));
}

/// **The closed list.** The hosts are exactly these, each with a reason, and the two
/// URLs the pipeline's own fetch asks are on it: a new host is a decision a person
/// makes by editing this test.
#[test]
fn the_registries_are_a_closed_list_and_the_fetch_asks_only_them() {
    let named: Vec<String> = hosts::HOSTS
        .iter()
        .map(|h| format!("{}:{}", h.name, h.port))
        .collect();
    assert_eq!(
        named,
        [
            "index.crates.io:443",
            "static.crates.io:443",
            "registry.npmjs.org:443"
        ]
    );
    assert!(
        hosts::HOSTS.iter().all(|h| h.why.len() > 20),
        "each host says why"
    );
    assert_eq!(hosts::egress(), named);
    let real = Endpoints::registries();
    for url in [&real.crates, &real.npm] {
        let host = url
            .strip_prefix("https://")
            .unwrap()
            .split('/')
            .next()
            .unwrap();
        assert!(
            hosts::HOSTS.iter().any(|h| h.name == host && h.port == 443),
            "{url}"
        );
    }
    // Every entry parses as the proxy reads it, and a bare crates.io is not among them.
    assert_eq!(crate::egress::check(&hosts::egress()).unwrap().len(), 3);
    let allow = crate::egress::check(&hosts::egress()).unwrap();
    for host in [
        "crates.io",
        "github.com",
        "api.github.com",
        "docs.rs",
        "registry.npmjs.org.evil.invalid",
    ] {
        assert!(!allow.iter().any(|a| a.permits(host, 443)), "{host}");
    }
    assert!(allow.iter().any(|a| a.permits("static.crates.io", 443)));
    assert!(!allow.iter().any(|a| a.permits("static.crates.io", 80)));
}

#[test]
fn a_builder_gains_the_registries_and_two_variables_and_nothing_else() {
    let sb = crate::sandbox::Sandbox::new(
        &crate::sandbox::SandboxConfig::default(),
        &[std::path::PathBuf::from("/work")],
        &[std::path::PathBuf::from("/state/store")],
    );
    let mut view = sb.job_view();
    assert!(view.egress.is_empty(), "an ordinary L1 job has no network");
    view.egress = vec!["pypi.org:443".into(), "index.crates.io:443".into()];
    builder::extend(&mut view);
    assert_eq!(
        view.egress,
        [
            "pypi.org:443",
            "index.crates.io:443",
            "static.crates.io:443",
            "registry.npmjs.org:443"
        ],
        "the operator's own entries stay, each registry once"
    );
    builder::extend(&mut view);
    assert_eq!(view.egress.len(), 4);
    let env = builder::env(Path::new("/state/self/builder/t"));
    assert_eq!(
        env,
        [
            (
                "CARGO_HOME".to_string(),
                "/state/self/builder/t/.packages/cargo".to_string()
            ),
            (
                "npm_config_cache".to_string(),
                "/state/self/builder/t/.packages/npm".to_string()
            ),
        ]
    );
    // A job that reached only the registries has reached nothing beyond the list.
    let list = builder::operator_list(&["pypi.org:443".to_string()]);
    assert_eq!(list.len(), 4);
}

#[test]
fn a_builders_cache_is_writable_inside_its_folder_and_made() {
    let sb = crate::sandbox::Sandbox::new(
        &crate::sandbox::SandboxConfig::default(),
        &[std::path::PathBuf::from("/work")],
        &[std::path::PathBuf::from("/state/store")],
    );
    let dir = crate::scratch::tempdir();
    let folder = dir.path().join("self/builder/t");
    let mut view = sb.job_view();
    view.hidden.push(dir.path().to_path_buf());
    builder::apply(&mut view, &folder).unwrap();
    let cache = builder::cache_in(&folder);
    assert!(cache.is_dir(), "made");
    assert_eq!(view.rw, std::slice::from_ref(&cache));
    assert!(
        !view.hidden.contains(&dir.path().to_path_buf()),
        "a mask over a directory that holds the bind is lifted"
    );
    assert_eq!(view.egress, hosts::egress());
    // Inside a folder the view already writes, nothing is bound twice.
    let mut view = sb.job_view();
    view.rw = vec![folder.clone()];
    builder::apply(&mut view, &folder).unwrap();
    assert_eq!(view.rw, [folder]);
}
