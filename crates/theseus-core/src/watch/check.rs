//! Running one check (theseus-f2r2b): a file read, a fetch, a command in L1.
//! None of them is on a turn's thread: the file and the command run on the
//! blocking pool, the fetch on the runtime, each bounded (30 s, 64 KiB of
//! output). A watch's value and rows are what the check read: the record is
//! raw, and the courier that posts a message substitutes the vault's values.

use std::io::Read;
use std::sync::Arc;

use theseus_kernel::probe_run::ProbeRun;

use super::cond::{self, Condition, Observed};
use super::spec::Check;
use super::step::Entry;
use crate::google::Google;
use crate::web::Web;

/// The longest a check runs.
pub const CHECK_SECS: u64 = 30;
/// The most output of a check that is read.
pub const OUTPUT_BYTES: usize = 64 * 1024;

/// How a command is run: L1 in the daemon, a stand-in in a test.
pub type RunHook = Arc<dyn Fn(&[String]) -> ProbeRun + Send + Sync>;

/// What a check needs from the daemon.
#[derive(Clone)]
pub struct Tools {
    pub web: Arc<Web>,
    pub run: RunHook,
    /// The account a mail watch reads; none without `[google]`.
    pub google: Option<Arc<Google>>,
}

/// One look at `e`'s watch, or why there was none. A mail look's
/// `google.called` rows are left in `rows` for the caller to record.
pub async fn observe(
    tools: &Tools,
    e: &Entry,
    rows: &mut Vec<serde_json::Value>,
) -> Result<Observed, String> {
    let spec = &e.spec;
    match &spec.check {
        Check::File { path } => {
            let path = path.clone();
            blocking(move || file(&path)).await
        }
        Check::Fetch { url, json } => {
            let (status, body) = tools.web.get_text(url, OUTPUT_BYTES, CHECK_SECS).await?;
            if !(200..300).contains(&status) {
                return Err(format!("{url} answered HTTP {status}"));
            }
            value_of(&body, json.as_deref(), None)
        }
        // Never called for a reply (it is not due); a look would see nothing.
        Check::Reply { .. } => Ok(Observed::default()),
        Check::Mail { q } => {
            let g = tools
                .google
                .as_deref()
                .ok_or("mail watches need [google] with gmail")?;
            super::mail::look(g, q, e.cursor.as_deref(), rows).await
        }
        Check::Run { argv, json } => {
            let (argv, run) = (argv.clone(), tools.run.clone());
            let ran = blocking(move || Ok(run(&argv))).await?;
            of_run(&ran, &spec.cond, json.as_deref())
        }
    }
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| format!("the check did not finish: {e}"))?
}

/// A file: missing is a value (`exists` is false), not a failure.
fn file(path: &str) -> Result<Observed, String> {
    let f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Observed::default()),
        Err(e) => return Err(format!("{path}: {e}")),
    };
    let mut buf = Vec::new();
    f.take(OUTPUT_BYTES as u64)
        .read_to_end(&mut buf)
        .map_err(|e| format!("{path}: {e}"))?;
    let text = String::from_utf8_lossy(&buf);
    Ok(Observed {
        value: text.trim().to_string(),
        code: None,
        exists: true,
        mail: None,
    })
}

/// A command's output, read as a value; its exit is the answer when the
/// condition is about it, and a failure otherwise.
pub fn of_run(ran: &ProbeRun, cond: &Condition, json: Option<&str>) -> Result<Observed, String> {
    if let Some(why) = &ran.failed {
        return Err(why.clone());
    }
    let code = ran.code;
    if code != Some(0) && !cond.reads_exit() {
        let said = [&ran.stderr, &ran.stdout]
            .into_iter()
            .find_map(|s| s.lines().map(str::trim).find(|l| !l.is_empty()))
            .unwrap_or("");
        return Err(match code {
            Some(c) if said.is_empty() => format!("exited {c}"),
            Some(c) => format!("{said} (exit {c})"),
            None => "it was ended by a signal".into(),
        });
    }
    let value = match json {
        Some(path) if code == Some(0) => cond::json_value(&ran.stdout, path)?,
        _ => ran.stdout.trim().to_string(),
    };
    Ok(Observed {
        value,
        code,
        exists: code == Some(0),
        mail: None,
    })
}

/// Text read as a value, the JSON path applied.
fn value_of(text: &str, json: Option<&str>, code: Option<i32>) -> Result<Observed, String> {
    let value = match json {
        Some(path) => cond::json_value(text, path)?,
        None => text.trim().to_string(),
    };
    Ok(Observed {
        value,
        code,
        exists: true,
        mail: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ran(code: i32, out: &str, err: &str) -> ProbeRun {
        ProbeRun {
            code: Some(code),
            stdout: out.into(),
            stderr: err.into(),
            failed: None,
        }
    }

    #[test]
    fn a_command_is_a_value_unless_it_failed() {
        let eq = cond::parse("equals MERGED").unwrap();
        let o = of_run(&ran(0, "MERGED\n", ""), &eq, None).unwrap();
        assert_eq!(o.value, "MERGED");
        let e = of_run(&ran(1, "", "gh: not logged in\nrun gh auth\n"), &eq, None).unwrap_err();
        assert_eq!(e, "gh: not logged in (exit 1)");
        assert_eq!(of_run(&ran(3, "", ""), &eq, None).unwrap_err(), "exited 3");
        let exit0 = cond::parse("exit 0").unwrap();
        let o = of_run(&ran(1, "", "no"), &exit0, None).unwrap();
        assert_eq!(o.code, Some(1), "with `exit`, the status is the answer");
        assert!(!exit0.met(&o, None));
        let json = of_run(&ran(0, r#"{"state":"OPEN"}"#, ""), &eq, Some(".state")).unwrap();
        assert_eq!(json.value, "OPEN");
        let did_not_start = ProbeRun {
            code: None,
            stdout: String::new(),
            stderr: String::new(),
            failed: Some("the job could not start in L1".into()),
        };
        assert!(of_run(&did_not_start, &exit0, None)
            .unwrap_err()
            .contains("L1"));
    }
}
