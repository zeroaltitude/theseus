//! The owner's own terminal, for a test that answers as the owner
//! (theseus-5rvss): the daemon reads who is behind a CLI answer from `/proc`,
//! and a process with no controlling terminal is an agent's. A child of a
//! pty (`python3`'s `os.forkpty`) has one, as the owner's shell does.

use std::process::{Command, Stdio};

const PTY: &str = "import os, sys\n\
    pid, fd = os.forkpty()\n\
    if pid == 0:\n\
    \x20   os.execv(sys.argv[1], sys.argv[1:])\n\
    out = b''\n\
    while True:\n\
    \x20   try:\n\
    \x20       d = os.read(fd, 4096)\n\
    \x20   except OSError:\n\
    \x20       break\n\
    \x20   if not d:\n\
    \x20       break\n\
    \x20   out += d\n\
    _, st = os.waitpid(pid, 0)\n\
    sys.stdout.buffer.write(out)\n\
    sys.exit(os.waitstatus_to_exitcode(st))\n";

/// `argv` as the child of a pty, with `envs` added and a job's marker
/// removed. Its exit code and its output (the terminal's one stream).
pub fn on_a_terminal(argv: &[String], envs: &[(&str, &str)]) -> (i32, String) {
    let out = Command::new("python3")
        .args(["-I", "-c", PTY])
        .args(argv)
        .envs(envs.iter().copied())
        .env_remove("THESEUS_SESSION")
        .stdin(Stdio::null())
        .output()
        .expect("python3 gives the owner's stand-in a terminal");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}
