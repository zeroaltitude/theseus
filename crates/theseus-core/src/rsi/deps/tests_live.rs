//! Against the real registries, once, on a fleet box: the proof the unit tests
//! cannot give, that what the pipeline fetches is what cargo and npm will build
//! and install from, with no network.
//!
//! `cargo test -p theseus-core -- --ignored rsi::deps::tests_live` (or the same
//! as a nextest filter with `--run-ignored only`). Never run by the gate.

use std::process::Command;

use super::fetch::Fetcher;
use super::hosts::Endpoints;
use super::lock;

const CRATES_IO: &str = "registry+https://github.com/rust-lang/crates.io-index";
/// `itoa 1.0.17`'s checksum, as crates.io's index records it.
const ITOA: &str = "92ecc6618181def0457392ccd0ee51198e065e016d1d527a7ac1b6dc7c1f09d2";

/// A crate that is in no cache on the box is fetched from crates.io, checked, laid out as
/// a directory source, and a project that depends on it builds with `--offline --locked`
/// and cargo's home set to the cache's: the way the gate builds.
#[tokio::test]
#[ignore = "reaches the real crates.io: run once, on a fleet box"]
async fn a_crate_from_crates_io_builds_offline_from_the_cache_the_pipeline_made() {
    let dir = crate::scratch::tempdir();
    let cache = dir.path().join("cache");
    let text = format!(
        "version = 4\n\n[[package]]\nname = \"exp\"\nversion = \"0.1.0\"\ndependencies = [\"itoa\"]\n\n\
         [[package]]\nname = \"itoa\"\nversion = \"1.0.17\"\nsource = \"{CRATES_IO}\"\nchecksum = \"{ITOA}\"\n"
    );
    let pkgs = lock::cargo(&text).unwrap().pkgs;
    let f = Fetcher::new(Endpoints::registries()).unwrap();
    let stats = f
        .cargo(&cache.join("cargo-home/vendor"), &pkgs)
        .await
        .unwrap();
    assert_eq!(stats.fetched, 1);
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    super::write_cargo_home(&cache, &cache, &home).unwrap();

    let project = dir.path().join("project");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(
        project.join("Cargo.toml"),
        "[package]\nname = \"exp\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[dependencies]\nitoa = \"=1.0.17\"\n",
    )
    .unwrap();
    std::fs::write(project.join("Cargo.lock"), &text).unwrap();
    std::fs::write(
        project.join("src/main.rs"),
        "fn main() { println!(\"{}\", itoa::Buffer::new().format(7u32)); }\n",
    )
    .unwrap();
    let out = Command::new("cargo")
        .args(["run", "--offline", "--locked", "--quiet"])
        .current_dir(&project)
        .env("CARGO_HOME", cache.join("cargo-home"))
        .env("CARGO_TARGET_DIR", dir.path().join("target"))
        .env("CARGO_NET_OFFLINE", "true")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{err}");
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "7");
}

/// The cockpit's whole lock file is fetched from the npm registry, each tarball checked
/// against its integrity, and `npm ci --offline --ignore-scripts` installs from that
/// cache alone.
#[tokio::test]
#[ignore = "reaches the real npm registry: run once, on a fleet box with npm"]
async fn the_cockpits_lock_file_installs_offline_from_the_cache_the_pipeline_made() {
    let cockpit = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../cockpit");
    let text = std::fs::read_to_string(cockpit.join("package-lock.json")).unwrap();
    let l = lock::npm(&text, &Endpoints::registries()).unwrap();
    assert!(l.problems.is_empty(), "{:?}", l.problems);
    assert!(l.pkgs.len() > 100, "{} packages", l.pkgs.len());
    let dir = crate::scratch::tempdir();
    let cache = dir.path().join("npm");
    let f = Fetcher::new(Endpoints::registries()).unwrap();
    let stats = f.npm(&cache, &l.pkgs).await.unwrap();
    assert!(stats.fetched > 100, "{stats:?}");
    eprintln!("fetched {} tarballs", stats.fetched);
    let project = dir.path().join("cockpit");
    std::fs::create_dir_all(&project).unwrap();
    for f in ["package.json", "package-lock.json"] {
        std::fs::copy(cockpit.join(f), project.join(f)).unwrap();
    }
    let out = Command::new("npm")
        .args([
            "ci",
            "--offline",
            "--ignore-scripts",
            "--no-audit",
            "--no-fund",
        ])
        .current_dir(&project)
        .env("npm_config_cache", &cache)
        .env("npm_config_userconfig", "/dev/null")
        .output();
    let Ok(out) = out else {
        panic!("npm is not on this box's PATH");
    };
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{err}");
    assert!(project.join("node_modules").is_dir());
}
