//! `theseus-root-install <commit>`: the whole program. Everything it does is in `lib.rs`; this file only reads its one
//! argument, throws away its environment, and hands over the real paths, which are compiled in. It has no way to be
//! given others (`Config::real` is the only configuration it names, and a test holds this file to that).

use std::ffi::OsString;

use theseus_root_install::{run, Config};

fn main() {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    // No environment is read: it is cleared, and the one variable set is a fixed `PATH`.
    for (k, _) in std::env::vars_os() {
        std::env::remove_var(k);
    }
    std::env::set_var("PATH", "/usr/sbin:/usr/bin:/sbin:/bin");
    let code = run(&Config::real(), &args, &mut |line| {
        theseus_root_install::log_line(line);
    });
    std::process::exit(code);
}
