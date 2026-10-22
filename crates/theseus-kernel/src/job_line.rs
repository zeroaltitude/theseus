//! A shell line off every command line Theseus owns (theseus-53ujg.2).
//!
//! A job `bash -c '<line>'` shows its line to `ps` and `pkill -f` on its shell's
//! command line, so `pkill -f server; server &` ended the job itself. At exec
//! the wrapper runs such an argv as a fixed runner instead, with the line in an
//! environment variable that the runner unsets before it evals it: neither a
//! command line nor a child's environment holds the text.

/// The variable that carries the line from the wrapper to its shell.
const LINE_VAR: &str = "THESEUS_JOB_LINE";

/// The runner: the variable is unset inside the eval's own text, then the line runs.
const RUNNER: &str = "eval \"unset THESEUS_JOB_LINE\n$THESEUS_JOB_LINE\"";

/// `argv` as it is exec'd, and the variable it needs. `[bash|sh, -c, line,
/// args…]` becomes `[bash|sh, -c, runner, args…]` with the line in the
/// variable; any other argv is returned as it is.
pub(crate) fn runner(argv: &[String]) -> (Vec<String>, Option<(&'static str, String)>) {
    let shell = argv.first().map(|s| s.rsplit('/').next().unwrap_or(s));
    match (shell, argv.get(1).map(String::as_str), argv.get(2)) {
        (Some("bash" | "sh"), Some("-c"), Some(line)) => {
            let mut out = argv.to_vec();
            out[2] = RUNNER.to_string();
            (out, Some((LINE_VAR, line.clone())))
        }
        _ => (argv.to_vec(), None),
    }
}
