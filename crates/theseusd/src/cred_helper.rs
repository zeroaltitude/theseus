//! `theseus-cred` (M4 18d): the credential helper inside an L1 job. It is
//! this binary, bound read-only at `/run/theseus/bin/theseus-cred`, and
//! `argv[0]` picks its role before anything else runs.
//!
//! `theseus-cred get <secret>` asks the job's socket for a secret and prints
//! its value and a newline, for a script's `$(…)`: `GH_TOKEN="$(theseus-cred
//! get github_token)" gh api user`. A refusal (an unknown name, a decline, a
//! lapse) prints why on stderr and exits 1; a usage error exits 2. It waits
//! as long as the answer takes: an approval waits on the operator, within
//! the job's own deadline, which ends the job.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;

use theseus_core::secrets::Secret;
use theseus_protocol::cred::{CredAnswer, CredAsk, CredKind};

/// The helper's name, as `argv[0]` gives it.
pub const NAME: &str = theseus_core::cred::HELPER_NAME;

/// Whether this process is the helper: `argv[0]`'s file name.
pub fn is_helper() -> bool {
    std::env::args_os()
        .next()
        .and_then(|a| {
            Path::new(&a)
                .file_name()
                .map(|f| f == std::ffi::OsStr::new(NAME))
        })
        .unwrap_or(false)
}

/// The helper's whole run: its exit code.
pub fn main() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let name = match args.as_slice() {
        [verb, name] if verb == "get" && !name.is_empty() => name.clone(),
        _ => {
            eprintln!("usage: {NAME} get <secret>");
            return 2;
        }
    };
    let socket = Path::new(theseus_core::cred::SOCKET_DIR).join(theseus_core::cred::SOCKET);
    match ask(&socket, &name) {
        Ok(value) => {
            let mut out = std::io::stdout().lock();
            let wrote = out
                .write_all(value.expose().as_bytes())
                .and_then(|()| out.write_all(b"\n"))
                .and_then(|()| out.flush());
            match wrote {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("{NAME}: could not print the value: {e}");
                    1
                }
            }
        }
        Err(why) => {
            eprintln!("{NAME}: {name}: {why}");
            1
        }
    }
}

/// One request on the job's socket: the value, or why not. Each copy of it
/// here is wiped when it goes (`Secret`).
fn ask(socket: &Path, name: &str) -> Result<Secret, String> {
    let s = UnixStream::connect(socket).map_err(|e| {
        format!(
            "no credential socket at {} ({e}): only a job in L1, the sandbox, can ask",
            socket.display()
        )
    })?;
    let mut req = serde_json::to_vec(&CredAsk {
        kind: CredKind::Secret,
        name: name.to_string(),
    })
    .map_err(|e| e.to_string())?;
    req.push(b'\n');
    (&s).write_all(&req)
        .map_err(|e| format!("the request could not be sent: {e}"))?;
    let mut line = String::new();
    BufReader::new(&s)
        .read_line(&mut line)
        .map_err(|e| format!("no answer came: {e}"))?;
    let line = Secret::new(line);
    let answer: CredAnswer = serde_json::from_str(line.expose().trim())
        .map_err(|_| "the daemon's answer could not be read (it may have stopped)".to_string())?;
    match answer {
        CredAnswer { value: Some(v), .. } => Ok(Secret::new(v)),
        CredAnswer { error, .. } => Err(error.unwrap_or_else(|| "refused".into())),
    }
}
