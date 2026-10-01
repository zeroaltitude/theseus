//! An empty `libm.a` for musl links.
//!
//! libopus_sys 0.3.3 prints `cargo:rustc-link-lib=m` for a static libopus, and decides
//! that with `cfg!(unix)` in its build script, which describes the build host, not the
//! target. musl keeps its math functions in `libc.a` and installs `libm.a` as an empty
//! archive, but Rust's self-contained musl (what rust-lld links against) ships no `libm.a`,
//! so the link fails with `unable to find library -lm`. An empty archive satisfies it.

use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("musl") {
        let out = PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
        fs::write(out.join("libm.a"), b"!<arch>\n").expect("write the empty libm.a");
        println!("cargo:rustc-link-search=native={}", out.display());
    }
}
