//! A job whose command never started (theseus-f7tz): its wrapper's spawn
//! failed, and its completion's `detail.spawn_error` says why. The result
//! said nothing of it before, and read "(no output)" with no exit code: in a
//! container that refused the spawn's system call, the model saw an empty
//! answer to every `proc.run`, and gave up on the shell.

use serde_json::Value;

/// The result's line for a job whose command could not start, with what to
/// try where the reason says one: `None` for a job that started. `term`:
/// whether the `term.*` tools, which start a program another way, are here.
pub(super) fn line(detail: &Value, term: bool) -> Option<String> {
    let why = detail.get("spawn_error").and_then(Value::as_str)?;
    let errno = why
        .rsplit_once("(os error ")
        .and_then(|(_, n)| n.trim_end_matches(')').parse::<i32>().ok());
    let hint = match errno {
        Some(libc::ENOENT) => {
            " The program was not found on its PATH (or at the path given), or its working \
             directory does not exist."
                .to_string()
        }
        Some(libc::EACCES) => {
            " The program is not executable here, or its working directory cannot be entered."
                .to_string()
        }
        Some(libc::ENOSYS | libc::EPERM) => format!(
            " This host refuses a system call the harness starts commands with (a container's \
             seccomp profile, or an older kernel), so no command will start this way here{}.",
            match term {
                true => "; the term.* tools start a program on a terminal another way",
                false => "",
            }
        ),
        _ => String::new(),
    };
    Some(format!(
        "[the command could not start: {why}. It did not run, and printed nothing.{hint}]\n"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Each reason is said, with what to try where it names one; a job that
    /// started has no line.
    #[test]
    fn a_spawn_failure_says_why_and_what_to_try() {
        let enosys = json!({"spawn_error": "Function not implemented (os error 38)"});
        let l = line(&enosys, true).unwrap();
        assert!(
            l.starts_with("[the command could not start: Function not implemented (os error 38)."),
            "{l}"
        );
        assert!(l.contains("seccomp") && l.contains("term.*"), "{l}");
        assert!(!line(&enosys, false).unwrap().contains("term.*"));
        let enoent = json!({"spawn_error": "No such file or directory (os error 2)"});
        assert!(line(&enoent, true).unwrap().contains("not found"));
        let other = json!({"spawn_error": "an argument holds a NUL byte"});
        assert_eq!(
            line(&other, true).unwrap(),
            "[the command could not start: an argument holds a NUL byte. It did not run, and \
             printed nothing.]\n"
        );
        assert_eq!(line(&json!({"exit_code": 0}), true), None);
        assert_eq!(line(&Value::Null, true), None);
    }
}
