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

/// Drives `argv` at a pty as a person does: each step waits for its prompt
/// text to appear in what the program printed, then types its reply and a
/// newline. `{code:N}` in a reply is the authenticator code of the step `N`
/// steps from now, from the secret that an `enroll` run printed (kept in the
/// file `PTY_SECRET_FILE` names, so a later run can use it); `<EOF>` types the
/// terminal's end-of-input. `PTY_PREFILL` is typed before the program has
/// started, as a newline left over from the command's own line is. When the
/// steps are spent the program is left to end; one that is quiet for `idle`
/// seconds is killed (exit status 124), so a program stuck at a prompt cannot
/// hold the test. The program's one stream, as it printed, echo and all.
const TYPED: &str =
    "import os, sys, json, select, time, signal, re, hmac, hashlib, base64, struct\n\
    steps = json.loads(os.environ.pop('PTY_STEPS'))\n\
    idle = float(os.environ.pop('PTY_IDLE'))\n\
    prefill = os.environ.pop('PTY_PREFILL', '')\n\
    sfile = os.environ.get('PTY_SECRET_FILE')\n\
    def totp(off):\n\
    \x20   sec = open(sfile).read().strip()\n\
    \x20   key = base64.b32decode(sec + '=' * (-len(sec) % 8))\n\
    \x20   c = int(time.time()) // 30 + off\n\
    \x20   h = hmac.new(key, struct.pack('>Q', c), hashlib.sha1).digest()\n\
    \x20   o = h[-1] & 15\n\
    \x20   return '%06d' % ((struct.unpack('>I', h[o:o+4])[0] & 0x7fffffff) % 10**6)\n\
    pid, fd = os.forkpty()\n\
    if pid == 0:\n\
    \x20   os.execv(sys.argv[1], sys.argv[1:])\n\
    if prefill:\n\
    \x20   os.write(fd, prefill.encode())\n\
    out = b''\n\
    seen = 0\n\
    last = time.time()\n\
    timed_out = False\n\
    while True:\n\
    \x20   r, _, _ = select.select([fd], [], [], 0.05)\n\
    \x20   if r:\n\
    \x20       try:\n\
    \x20           d = os.read(fd, 4096)\n\
    \x20       except OSError:\n\
    \x20           break\n\
    \x20       if not d:\n\
    \x20           break\n\
    \x20       out += d\n\
    \x20       last = time.time()\n\
    \x20       m = re.search(rb'secret:\\s+([A-Z2-7]+)', out)\n\
    \x20       if m and sfile:\n\
    \x20           open(sfile, 'w').write(m.group(1).decode())\n\
    \x20   if steps and steps[0][0].encode() in out[seen:]:\n\
    \x20       want, reply = steps.pop(0)\n\
    \x20       seen = len(out)\n\
    \x20       time.sleep(0.05)\n\
    \x20       reply = re.sub(r'\\{code:(-?\\d+)\\}', lambda m: totp(int(m.group(1))), reply)\n\
    \x20       if reply == '<EOF>':\n\
    \x20           os.write(fd, b'\\x04')\n\
    \x20       else:\n\
    \x20           os.write(fd, reply.encode() + b'\\n')\n\
    \x20       last = time.time()\n\
    \x20   elif time.time() - last > idle:\n\
    \x20       timed_out = True\n\
    \x20       os.kill(pid, signal.SIGKILL)\n\
    \x20       break\n\
    _, st = os.waitpid(pid, 0)\n\
    sys.stdout.buffer.write(out)\n\
    sys.exit(124 if timed_out else os.waitstatus_to_exitcode(st))\n";

/// `argv` at a pty with `steps` typed as its prompts appear (see `TYPED`),
/// and `prefill` already waiting on the terminal when it starts. Its exit code
/// and its output.
pub fn typed_at_a_terminal(
    argv: &[String],
    envs: &[(&str, &str)],
    steps: &[(&str, &str)],
    prefill: &str,
) -> (i32, String) {
    let steps: Vec<[&str; 2]> = steps.iter().map(|(p, r)| [*p, *r]).collect();
    let out = Command::new("python3")
        .args(["-I", "-c", TYPED])
        .args(argv)
        .envs(envs.iter().copied())
        .env("PTY_STEPS", serde_json::to_string(&steps).unwrap())
        .env("PTY_IDLE", "8")
        .env("PTY_PREFILL", prefill)
        .env_remove("THESEUS_SESSION")
        .stdin(Stdio::null())
        .output()
        .expect("python3 gives the owner's stand-in a terminal");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}
