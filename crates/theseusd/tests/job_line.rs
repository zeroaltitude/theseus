//! A shell line runs as it always did, though its text is on no command line
//! (theseus-53ujg.2): the real `theseusd job-wrapper` starts `bash -c <line>`
//! as the daemon does, and each case compares what the job did with what the
//! shell does with the line itself.

use std::path::Path;
use std::time::{Duration, Instant};

use theseus_kernel::job::{self, WrapperArgs};
use theseus_kernel::Spool;

/// What a job did: its exit code and its output.
fn run(spool: &Spool, id: &str, argv: &[&str]) -> (i64, String) {
    let args = WrapperArgs {
        spool_dir: spool.dir().to_path_buf(),
        correlation_id: id.into(),
        deadline_ms: 60_000,
        notify_socket: None,
        argv: argv.iter().map(|a| a.to_string()).collect(),
        cwd: Some(spool.dir().to_path_buf()),
        env: vec![("PATH".into(), std::env::var("PATH").unwrap_or_default())],
        umask: None,
        redact: vec![],
        output_max_bytes: job::DEFAULT_OUTPUT_MAX_BYTES,
        sandbox: None,
        cgroup: None,
    };
    job::spawn_detached(
        Path::new(env!("CARGO_BIN_EXE_theseusd")),
        &[job::WRAPPER_MODE],
        spool,
        &args,
    )
    .unwrap();
    let t0 = Instant::now();
    let c = loop {
        if let Some(c) = spool.read_completion(id).unwrap() {
            break c;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(30),
            "{id}: no completion"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    let code = c.detail.as_ref().and_then(|d| d["exit_code"].as_i64());
    let out = std::fs::read_to_string(spool.result_path(id)).unwrap_or_default();
    (code.unwrap_or(-1), out)
}

#[test]
fn a_line_keeps_its_semantics() {
    let dir = tempfile::tempdir().unwrap();
    let spool = Spool::open(&dir.path().join("spool")).unwrap();
    // (the line, its extra arguments, the exit code and output it has by itself)
    let cases: [(&str, &[&str], i64, &str); 9] = [
        ("exit 3", &[], 3, ""),
        ("set -e; false; echo not-reached", &[], 1, ""),
        ("cat <<EOF\nhello $0\nEOF", &["name"], 0, "hello name\n"),
        (
            "echo one\necho two\n\necho three",
            &[],
            0,
            "one\ntwo\nthree\n",
        ),
        (
            "X=5; echo \"a'$X'b\" 'c$X' \"$((X+1))\"",
            &[],
            0,
            "a'5'b c$X 6\n",
        ),
        ("echo \"$0|$1|$2\"", &["zero", "a", "b"], 0, "zero|a|b\n"),
        ("echo \"$0|$#\"", &[], 0, "bash|0\n"),
        (
            "sleep 0.2 & sleep 0.1 & wait; echo waited",
            &[],
            0,
            "waited\n",
        ),
        // stdin is empty, as before.
        ("cat; echo after-stdin", &[], 0, "after-stdin\n"),
    ];
    for (i, (line, rest, code, out)) in cases.into_iter().enumerate() {
        let mut argv = vec!["bash", "-c", line];
        argv.extend(rest);
        let got = run(&spool, &format!("act_{i}"), &argv);
        assert_eq!(got, (code, out.to_string()), "{line:?}");
        // The shell's own answer to the same line is the table's.
        let direct = std::process::Command::new("bash")
            .args(&argv[1..])
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        assert_eq!(
            (
                i64::from(direct.status.code().unwrap()),
                String::from_utf8_lossy(&direct.stdout).into_owned()
            ),
            (code, out.to_string()),
            "the table is what bash does: {line:?}"
        );
    }
    // The runner's variable is neither set in the line's children nor a
    // variable of the line's shell.
    let (code, out) = run(
        &spool,
        "act_env",
        &[
            "bash",
            "-c",
            "env | grep -c THESEUS_JOB_LINE; echo \"${THESEUS_JOB_LINE-unset}\"; sh -c 'echo \"${THESEUS_JOB_LINE-child-unset}\"'",
        ],
    );
    assert_eq!((code, out.as_str()), (0, "0\nunset\nchild-unset\n"));
    // `sh -c` takes the same road; any other argv is run as it is.
    assert_eq!(
        run(&spool, "act_sh", &["sh", "-c", "echo \"$0 $1\"", "s", "x"]),
        (0, "s x\n".into())
    );
    assert_eq!(
        run(&spool, "act_other", &["echo", "-c", "plain"]),
        (0, "-c plain\n".into())
    );
}
