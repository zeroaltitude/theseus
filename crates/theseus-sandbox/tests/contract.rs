//! L1's contract (design §2.2's table of §7's clauses): one case per
//! clause, each probing from inside a real L1 job on this machine, and a few
//! more for the init's own promises.
//!
//! The binary re-execs itself as the job's init (`job-sandbox`) and as the
//! probe inside the job (`probe <name> …`, the binary bound read-only into
//! the view), so it brings its own harness (`harness = false`).
//!
//! With `THESEUS_SANDBOX_TEST_CGROUP=1`, in a cgroup delegated to it (run
//! under `systemd-run --user --scope -p Delegate=yes`), the limits case puts
//! the job in a cgroup of its own and proves `pids.max`; without it, the
//! fork loop stops at `RLIMIT_NPROC`.

mod common;

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::{check, job, run, Case, Output};
use serde_json::{json, Value};
use theseus_sandbox::cgroup::{self, JobCgroup};
use theseus_sandbox::egress::{self, Allow, Connection, Proxy, Resolver};
use theseus_sandbox::Spec;

fn main() {
    common::main(CASES, IGNORED, |args| {
        if args.first().map(String::as_str) == Some("probe") {
            probe::main(&args[1..]);
        }
    });
}

/// Run only when asked: it reaches a public host, which no build session does
/// (`THESEUS_SANDBOX_LIVE_EGRESS=github.com:443` names it).
const IGNORED: &[Case] = &[Case {
    name: "egress_18b_live_a_public_host_through_the_proxy",
    run: egress_live,
}];

const CASES: &[Case] = &[
    Case {
        name: "clause_01_namespaces",
        run: namespaces,
    },
    Case {
        name: "clause_02_no_network_by_default",
        run: no_network,
    },
    Case {
        name: "clause_03_no_metadata_service",
        run: no_metadata_service,
    },
    Case {
        name: "clause_04_no_localhost_services",
        run: no_localhost_services,
    },
    Case {
        name: "clause_05_no_capabilities",
        run: no_capabilities,
    },
    Case {
        name: "clause_06_seccomp_default_profile",
        run: seccomp_profile,
    },
    Case {
        name: "clause_07_devices",
        run: devices,
    },
    Case {
        name: "clause_08_proc_and_sys_masked",
        run: proc_and_sys_masked,
    },
    Case {
        name: "clause_09_limits",
        run: limits,
    },
    Case {
        name: "clause_10_tree_killed_on_cancel",
        run: tree_killed_on_cancel,
    },
    Case {
        name: "clause_11_not_granted_is_denied",
        run: not_granted_is_denied,
    },
    Case {
        name: "clause_12_no_ambient_credentials",
        run: no_ambient_credentials,
    },
    Case {
        name: "exit_status_and_signals",
        run: exit_status_and_signals,
    },
    Case {
        name: "a_job_that_cannot_start_says_why",
        run: cannot_start,
    },
    Case {
        name: "scratch_is_reported_and_discarded",
        run: scratch_reported,
    },
    Case {
        name: "sigterm_is_forwarded_to_the_command",
        run: sigterm_forwarded,
    },
    Case {
        name: "egress_18b_an_allowed_host_is_reached_and_recorded",
        run: egress_allowed,
    },
    Case {
        name: "egress_18b_each_refusal_says_why",
        run: egress_refusals,
    },
    Case {
        name: "egress_18b_without_the_proxy_there_is_no_route",
        run: egress_no_route,
    },
];

const NAMESPACES: [&str; 7] = ["user", "pid", "mnt", "net", "uts", "ipc", "cgroup"];

/// The probe `name` as a job in `ws`: this binary, bound read-only into the
/// view. `{ws}` in an argument is the workspace's path.
fn probe_spec(ws: &Path, name: &str, args: &[&str]) -> Spec {
    let exe = std::env::current_exe().expect("the test binary");
    let mut argv = vec![exe.display().to_string(), "probe".into(), name.into()];
    let ws_s = ws.display().to_string();
    argv.extend(args.iter().map(|a| a.replace("{ws}", &ws_s)));
    let mut spec = job(argv, ws);
    spec.ro_paths = vec![exe];
    spec
}

/// Runs a probe that ends well, in `ws`, and gives its JSON.
fn probe_in(
    ws: &Path,
    name: &str,
    args: &[&str],
    edit: impl FnOnce(&mut Spec),
) -> Result<Value, String> {
    let mut spec = probe_spec(ws, name, args);
    edit(&mut spec);
    let (exit, out) = run(&spec)?;
    check(
        exit.success(),
        format!("the probe {name} ended {exit:?}; its output:\n{out}"),
    )?;
    let last = out.lines().last().unwrap_or("");
    // THESEUS_SANDBOX_SHOW=1 prints what each probe saw, as evidence.
    if std::env::var_os("THESEUS_SANDBOX_SHOW").is_some() {
        println!("probe {name}: {last}");
    }
    serde_json::from_str(last).map_err(|e| format!("the probe {name}'s output ({e}):\n{out}"))
}

fn probe(name: &str, args: &[&str]) -> Result<Value, String> {
    let ws = tempfile::tempdir().map_err(|e| e.to_string())?;
    probe_in(ws.path(), name, args, |_| {})
}

fn is(v: &Value, key: &str, want: &str) -> Result<(), String> {
    check(
        v[key] == want,
        format!("{key}: {} (wanted {want})\nall: {v}", v[key]),
    )
}

// Clause 1: user, pid, mount, uts, ipc, and net namespaces (and cgroup).
fn namespaces() -> Result<(), String> {
    let v = probe("ns", &[])?;
    for n in NAMESPACES {
        let host = fs::metadata(format!("/proc/self/ns/{n}"))
            .map_err(|e| format!("{n}: {e}"))?
            .ino();
        let inside = v["ns"][n].as_u64().unwrap_or(0);
        check(
            inside != 0 && inside != host,
            format!("the {n} namespace: inside {inside}, the host's {host}"),
        )?;
    }
    is(&v, "hostname", theseus_sandbox::HOSTNAME)
}

// Clause 2: no network by default.
fn no_network() -> Result<(), String> {
    let v = probe(
        "connect",
        &[
            "tcp:1.1.1.1:443",
            "tcp:8.8.8.8:53",
            "tcp:[2606:4700::1111]:443",
        ],
    )?;
    for t in [
        "tcp:1.1.1.1:443",
        "tcp:8.8.8.8:53",
        "tcp:[2606:4700::1111]:443",
    ] {
        unreachable(&v, t)?;
    }
    check(
        v["interfaces"] == json!(["lo"]),
        format!("interfaces: {}", v["interfaces"]),
    )
}

// Clause 3: no metadata service.
fn no_metadata_service() -> Result<(), String> {
    let targets = ["tcp:169.254.169.254:80", "tcp:[fd00:ec2::254]:80"];
    let v = probe("connect", &targets)?;
    for t in targets {
        unreachable(&v, t)?;
    }
    Ok(())
}

/// A connect to `target` from inside the job found no route: ENETUNREACH.
/// An IPv6 target on a host whose kernel has no IPv6 (`ipv6.disable=1`)
/// ends sooner, at the socket, with EAFNOSUPPORT: the job then has no route
/// because the host has none either (theseus-celu.2). Only the host's own
/// answer widens it: anything else from the job still fails.
fn unreachable(v: &Value, target: &str) -> Result<(), String> {
    let got = &v["connect"][target];
    let (want, why) = if target.starts_with("tcp:[") {
        match host_ipv6_socket() {
            Err(e) if e.raw_os_error() == Some(libc::EAFNOSUPPORT) => (
                "EAFNOSUPPORT",
                "the host's own IPv6 socket fails with EAFNOSUPPORT: its kernel has no IPv6",
            ),
            _ => ("ENETUNREACH", "the host makes IPv6 sockets"),
        }
    } else {
        ("ENETUNREACH", "IPv4")
    };
    check(
        *got == want,
        format!("{target}: {got} (wanted {want}: {why})"),
    )
}

/// An IPv6 socket made on the host, outside L1.
fn host_ipv6_socket() -> std::io::Result<()> {
    let fd = unsafe { libc::socket(libc::AF_INET6, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0) };
    if fd == -1 {
        return Err(std::io::Error::last_os_error());
    }
    unsafe { libc::close(fd) };
    Ok(())
}

// Clause 4: no localhost services, Theseus's own included.
fn no_localhost_services() -> Result<(), String> {
    use std::os::linux::net::SocketAddrExt;
    use std::os::unix::net::{SocketAddr, UnixListener, UnixStream};
    let tcp = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = tcp.local_addr().map_err(|e| e.to_string())?.port();
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let sock = dir.path().join("theseusd.sock");
    let _unix = UnixListener::bind(&sock).map_err(|e| e.to_string())?;
    let name = format!("theseus-sandbox-test-{}", std::process::id());
    let addr = SocketAddr::from_abstract_name(name.as_bytes()).map_err(|e| e.to_string())?;
    let _abstract = UnixListener::bind_addr(&addr).map_err(|e| e.to_string())?;
    // The control: each is reachable from the host.
    std::net::TcpStream::connect(("127.0.0.1", port)).map_err(|e| format!("host tcp: {e}"))?;
    UnixStream::connect(&sock).map_err(|e| format!("host unix: {e}"))?;
    UnixStream::connect_addr(&addr).map_err(|e| format!("host abstract: {e}"))?;

    let (t, u, a) = (
        format!("tcp:127.0.0.1:{port}"),
        format!("unix:{}", sock.display()),
        format!("abstract:{name}"),
    );
    let v = probe("connect", &[&t, &u, &a])?;
    check(
        v["connect"][&t] == "ECONNREFUSED",
        format!("{t}: {}", v["connect"][&t]),
    )?;
    check(
        v["connect"][&u] == "ENOENT",
        format!("{u}: {}", v["connect"][&u]),
    )?;
    check(
        v["connect"][&a] == "ECONNREFUSED",
        format!("{a}: {}", v["connect"][&a]),
    )?;
    // Theseus's state, and with it its socket, is not in the view.
    is(&v, "theseus_state", "ENOENT")
}

// Clause 5: no capabilities, as the operator's own uid.
fn no_capabilities() -> Result<(), String> {
    let v = probe("status", &[])?;
    for k in ["CapInh", "CapPrm", "CapEff", "CapBnd", "CapAmb"] {
        is(&v, k, "0000000000000000")?;
    }
    is(&v, "NoNewPrivs", "1")?;
    let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
    check(
        v["getuid"] == uid && v["geteuid"] == uid && v["getgid"] == gid,
        format!("ids inside: {v}; the operator's: {uid}, {gid}"),
    )
}

// Clause 6: a default seccomp profile.
fn seccomp_profile() -> Result<(), String> {
    let v = probe("seccomp", &[])?;
    is(&v, "Seccomp", "2")?;
    for k in [
        "unshare",
        "mount",
        "ptrace",
        "bpf",
        "keyctl",
        "io_uring_setup",
        "setns",
        "clone_newuser",
        "clone_newnet",
    ] {
        is(&v, k, "EPERM")?;
    }
    is(&v, "clone3", "ENOSYS")?;
    is(&v, "fork", "ok")?;
    is(&v, "thread", "ok")?;
    // A foreign ABI kills: x32 by the filter, i386 by the filter too.
    is(&v, "x32", "SIGSYS")?;
    is(&v, "i386", "SIGSYS")
}

// Clause 7: no device nodes beyond null, zero, random, and urandom.
fn devices() -> Result<(), String> {
    let v = probe("dev", &[])?;
    let want = json!({
        "null": "char", "zero": "char", "random": "char", "urandom": "char",
        "fd": "symlink", "stdin": "symlink", "stdout": "symlink", "stderr": "symlink",
        "shm": "dir",
    });
    check(v["entries"] == want, format!("/dev: {}", v["entries"]))?;
    is(&v, "urandom", "ok")?;
    is(&v, "zero", "ok")?;
    is(&v, "null", "ok")?;
    is(&v, "create", "EROFS")?;
    check(v["mknod"] != "ok", format!("mknod: {}", v["mknod"]))
}

// Clause 8: /proc and /sys masked.
fn proc_and_sys_masked() -> Result<(), String> {
    let ws = tempfile::tempdir().map_err(|e| e.to_string())?;
    let out = Output::new();
    let mut child = common::spawn(&probe_spec(ws.path(), "proc", &[]), &out)?;
    let sys = child.started().sys;
    let exit = child.wait().map_err(|e| e.to_string())?;
    let text = out.read();
    check(exit.success(), format!("{exit:?}\n{text}"))?;
    let v: Value =
        serde_json::from_str(text.lines().last().unwrap_or("")).map_err(|e| e.to_string())?;
    // A kernel built without /proc/kcore (no CONFIG_PROC_KCORE) has none to
    // mask, inside or out: the job finds none only when the host has none
    // (theseus-celu.2).
    let (kcore, why) = match fs::symlink_metadata("/proc/kcore") {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            (json!("ENOENT"), "the host has no /proc/kcore")
        }
        _ => (json!(0), "masked: the host has one"),
    };
    check(
        v["kcore_bytes"] == kcore,
        format!("/proc/kcore: {} (wanted {kcore}: {why})", v["kcore_bytes"]),
    )?;
    check(
        v["keys_bytes"] == 0,
        format!("/proc/keys: {}", v["keys_bytes"]),
    )?;
    check(
        v["proc_sys_write"] != "ok",
        "a write to /proc/sys was taken",
    )?;
    check(
        v["sysrq_write"] != "ok",
        "a write to /proc/sysrq-trigger was taken",
    )?;
    check(
        v["proc_sys_ro"] == true,
        "/proc/sys is not a read-only mount",
    )?;
    check(
        v["pid1"]
            .as_str()
            .is_some_and(|s| s.contains("job-sandbox")),
        format!("/proc/1 is {}", v["pid1"]),
    )?;
    check(
        v["pids"] == json!([1, v["self"]]),
        format!("/proc lists {}", v["pids"]),
    )?;
    check(
        v["cpuinfo"] == true && v["meminfo"] == true,
        "cpuinfo or meminfo is gone",
    )?;
    if sys {
        check(
            v["sys_net"] == json!(["lo"]),
            format!("/sys/class/net: {}", v["sys_net"]),
        )?;
        for k in ["sys_firmware", "sys_security", "sys_cgroup"] {
            check(v[k] == json!([]), format!("{k}: {}", v[k]))?;
        }
        check(v["sys_write"] != "ok", "a write to /sys was taken")
    } else {
        // The kernel refused a fresh sysfs: the view has none, and says so.
        check(
            v["sys_net"] == "ENOENT",
            format!("no sysfs, yet {}", v["sys_net"]),
        )
    }
}

/// A cgroup of the job's own, when this run's cgroup is delegated to it.
fn test_cgroup(name: &str, pids: u64) -> Result<Option<JobCgroup>, String> {
    if std::env::var_os("THESEUS_SANDBOX_TEST_CGROUP").is_none() {
        return Ok(None);
    }
    let own = cgroup::own().map_err(|e| e.to_string())?;
    let dir = if own.ends_with("daemon") {
        own.parent().map(Path::to_path_buf).unwrap_or(own)
    } else {
        own
    };
    let jobs = cgroup::delegate(&dir).map_err(|e| format!("delegating {}: {e}", dir.display()))?;
    let name = format!("{name}-{}", std::process::id());
    JobCgroup::create(&jobs, &name, Some(256), Some(pids))
        .map(Some)
        .map_err(|e| format!("the job's cgroup: {e}"))
}

// Clause 9: limits on pids and disk, with output caps (memory and cpu are
// the cgroup's, where it is delegated).
fn limits() -> Result<(), String> {
    let ws = tempfile::tempdir().map_err(|e| e.to_string())?;
    // A fork loop stops: at pids.max in a cgroup of its own, else at the
    // job's RLIMIT_NPROC, counted in its own user namespace.
    let cg = test_cgroup("forkloop", 16)?;
    let v = probe_in(ws.path(), "forkloop", &[], |s| match &cg {
        Some(cg) => {
            s.cgroup = Some(cg.path().to_path_buf());
            s.limits.pids = 4096;
        }
        None => s.limits.pids = 16,
    })?;
    // Linux never applies RLIMIT_NPROC to a task whose real user is the
    // initial user namespace's root (copy_process's INIT_USER check), and
    // the job's is the operator's: without a cgroup, a root operator's job
    // has no process limit. The clause does not hold, so this still fails
    // (theseus-celu.2).
    is(&v, "error", "EAGAIN").map_err(|e| {
        if cg.is_none() && unsafe { libc::getuid() } == 0 {
            format!(
                "{e}\nthe operator is root (uid 0), whom Linux exempts from \
                 RLIMIT_NPROC, and the job has no cgroup: it has no process \
                 limit (theseus-celu.2)"
            )
        } else {
            e
        }
    })?;
    let forked = v["forked"].as_u64().unwrap_or(0);
    check(
        (10..16).contains(&forked),
        format!("the loop forked {forked}"),
    )?;
    if let Some(cg) = cg {
        let refused = cg.pids_refused().map_err(|e| e.to_string())?;
        check(refused > 0, "pids.events counts no refusal")?;
        check(
            !cg.populated().unwrap_or(true),
            "the job's cgroup is still populated",
        )?;
        cg.remove()
            .map_err(|e| format!("removing the job's cgroup: {e}"))?;
    }
    // A scratch write past its cap: ENOSPC. And /tmp's.
    let v = probe_in(ws.path(), "fill", &["{ws}/big", "64"], |s| {
        s.limits.scratch_mb = 4
    })?;
    is(&v, "error", "ENOSPC")?;
    let written = v["written"].as_u64().unwrap_or(0);
    check(
        (2 << 20..=4 << 20).contains(&written),
        format!("scratch took {written} bytes"),
    )?;
    let v = probe_in(ws.path(), "fill", &["/tmp/big", "64"], |s| {
        s.limits.tmp_mb = 4
    })?;
    is(&v, "error", "ENOSPC")?;
    // Output past its cap is cut, with a note.
    let out = Output::new();
    let mut spec = probe_spec(ws.path(), "spew", &["3"]);
    spec.limits.output_mb = 1;
    let mut child = common::spawn(&spec, &out)?;
    let exit = child.wait().map_err(|e| e.to_string())?;
    let size = fs::metadata(&out.path).map(|m| m.len()).unwrap_or(0);
    check(
        exit.output_capped && exit.signal == Some(libc::SIGXFSZ),
        format!("output past the cap: {exit:?}"),
    )?;
    check(
        size == 1 << 20,
        format!("the output file holds {size} bytes"),
    )
}

/// The pids in the pid namespace `ns` ("pid:[…]"), from the host.
fn in_namespace(ns: &Path) -> Vec<u32> {
    let mut pids = Vec::new();
    for e in fs::read_dir("/proc").into_iter().flatten().flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        if fs::read_link(format!("/proc/{pid}/ns/pid")).ok().as_deref() == Some(ns) {
            pids.push(pid);
        }
    }
    pids
}

fn wait_until(what: &str, mut ok: impl FnMut() -> bool) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ok() {
        if Instant::now() > deadline {
            return Err(format!("timed out waiting for {what}"));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

// Clause 10: the whole tree killed on timeout or cancel.
fn tree_killed_on_cancel() -> Result<(), String> {
    let ws = tempfile::tempdir().map_err(|e| e.to_string())?;
    let spec = job(
        vec![
            "/bin/sh".into(),
            "-c".into(),
            "setsid sleep 300 & exec sleep 300".into(),
        ],
        ws.path(),
    );
    let out = Output::new();
    let mut child = common::spawn(&spec, &out)?;
    let ns = fs::read_link(format!("/proc/{}/ns/pid", child.id())).map_err(|e| e.to_string())?;
    // The init, the command, and the sleeper in a session of its own.
    wait_until("three processes in the job", || {
        in_namespace(&ns).len() >= 3
    })?;
    // The shell forks the sleeper before the sleeper calls setsid(2), so a
    // read can land between the two: wait for the sessions to differ too. A
    // sleeper that cannot leave the session still fails, at the deadline.
    let mut sessions: Vec<String> = Vec::new();
    wait_until("the sleeper to leave the session", || {
        sessions = in_namespace(&ns)
            .iter()
            .filter_map(|p| fs::read_to_string(format!("/proc/{p}/stat")).ok())
            .filter_map(|s| {
                s.rsplit(')')
                    .next()
                    .map(|r| r.split_whitespace().nth(3).unwrap_or("").to_string())
            })
            .collect();
        sessions
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            >= 2
    })
    .map_err(|e| format!("the sleeper did not leave the session ({e}): {sessions:?}"))?;
    child.kill().map_err(|e| e.to_string())?;
    let exit = child.wait().map_err(|e| e.to_string())?;
    check(exit.init_signal == Some(libc::SIGKILL), format!("{exit:?}"))?;
    let left = in_namespace(&ns);
    check(
        left.is_empty(),
        format!("left in the job's pid namespace: {left:?}"),
    )
}

// Clause 11: anything not granted is denied.
fn not_granted_is_denied() -> Result<(), String> {
    let ws = tempfile::tempdir().map_err(|e| e.to_string())?;
    let v = probe_in(ws.path(), "fs", &["{ws}"], |_| {})?;
    for k in ["write_usr", "write_root", "write_etc", "mkdir_root"] {
        is(&v, k, "EROFS")?;
    }
    for k in ["write_workspace", "write_tmp", "write_home"] {
        is(&v, k, "ok")?;
    }
    for k in [
        "gh_hosts",
        "ssh",
        "aws",
        "op_token",
        "theseus_state",
        "run",
        "var",
    ] {
        is(&v, k, "ENOENT")?;
    }
    // The init's own handles (the hidden scratch among its descriptors) are
    // out of the command's reach: it is in another user namespace.
    for k in ["init_fd", "init_environ", "init_root"] {
        is(&v, k, "EACCES")?;
    }
    check(
        v["env"] == json!(["HOME", "LANG", "PATH", "THESEUS_GRANTED"]),
        format!("the job's environment: {}", v["env"]),
    )?;
    check(
        !ws.path().join("new.txt").exists(),
        "a workspace write reached the host's tree",
    )
}

// Clause 12: no ambient credentials.
fn no_ambient_credentials() -> Result<(), String> {
    let ws = tempfile::tempdir().map_err(|e| e.to_string())?;
    let v = probe_in(ws.path(), "fs", &["{ws}"], |_| {})?;
    check(v["ssh_auth_sock"] == false, "SSH_AUTH_SOCK is set")?;
    let (exit, out) = run(&job(
        vec!["gh".into(), "auth".into(), "status".into()],
        ws.path(),
    ))?;
    check(
        exit.code.is_some_and(|c| c != 0) && out.to_lowercase().contains("not logged in"),
        format!("gh auth status: {exit:?}\n{out}"),
    )
}

fn exit_status_and_signals() -> Result<(), String> {
    let ws = tempfile::tempdir().map_err(|e| e.to_string())?;
    let (exit, _) = run(&probe_spec(ws.path(), "exit", &["7"]))?;
    check(
        exit.code == Some(7) && exit.signal.is_none(),
        format!("exit 7: {exit:?}"),
    )?;
    let (exit, _) = run(&probe_spec(ws.path(), "abort", &[]))?;
    check(
        exit.signal == Some(libc::SIGABRT) && exit.init_signal.is_none(),
        format!("abort: {exit:?}"),
    )?;
    // The command starts as a fresh process: no signal blocked or ignored,
    // whatever its ancestors ignored (a Rust runtime ignores SIGPIPE). grep
    // reads its own status, since a Rust probe would ignore SIGPIPE itself.
    let argv = ["grep", "-E", "^Sig(Blk|Ign)", "/proc/self/status"];
    let (exit, out) = run(&job(argv.map(String::from).to_vec(), ws.path()))?;
    check(
        exit.success()
            && out.contains("SigBlk:\t0000000000000000")
            && out.contains("SigIgn:\t0000000000000000"),
        format!("the command's signal state: {exit:?}\n{out}"),
    )
}

/// No child of this process is left: a failed spawn reaps what it started.
fn no_children() -> Result<(), String> {
    let mut status = 0;
    let r = unsafe { libc::waitpid(-1, &mut status, libc::WNOHANG) };
    check(
        r == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ECHILD),
        format!("a child remains (waitpid gave {r})"),
    )
}

fn cannot_start() -> Result<(), String> {
    let ws = tempfile::tempdir().map_err(|e| e.to_string())?;
    let start = |spec: &Spec| {
        let out = Output::new();
        theseus_sandbox::spawn(spec, &common::init(), out.stdio())
            .err()
            .ok_or_else(|| "it started".to_string())
    };
    let e = start(&job(vec!["no-such-command-at-all".into()], ws.path()))?;
    check(
        e.stage == "finding no-such-command-at-all" && e.error.contains("No such file"),
        format!("a missing command: {e}"),
    )?;
    no_children()?;
    let mut spec = job(vec!["/bin/true".into()], ws.path());
    spec.cwd = "/no/such/dir".into();
    let e = start(&spec)?;
    check(
        e.stage == "entering the working directory, /no/such/dir",
        format!("a missing cwd: {e}"),
    )?;
    no_children()?;
    let mut spec = job(vec!["/bin/true".into()], ws.path());
    spec.workspace.push(PathBuf::from("relative/root"));
    let e = start(&spec)?;
    check(
        e.stage == "checking the job",
        format!("a relative root: {e}"),
    )?;
    let mut spec = job(vec!["/bin/true".into()], ws.path());
    spec.workspace.push(ws.path().join("missing"));
    let e = start(&spec)?;
    check(
        e.stage.starts_with("mounting the workspace") && e.error.contains("No such file"),
        format!("a missing root: {e}"),
    )?;
    no_children()
}

fn scratch_reported() -> Result<(), String> {
    let ws = tempfile::tempdir().map_err(|e| e.to_string())?;
    fs::write(ws.path().join("old.txt"), "keep").map_err(|e| e.to_string())?;
    let (exit, out) = run(&probe_spec(ws.path(), "scratch", &["{ws}"]))?;
    check(exit.success(), format!("{exit:?}\n{out}"))?;
    let s = exit.scratch.ok_or("no scratch summary")?;
    check(
        s.files == 3 && s.bytes == 1110 && s.removed == 1,
        format!("scratch: {s:?}"),
    )?;
    let shown = |p: &str| ws.path().join(p).display().to_string();
    for p in ["a.txt", "sub/b.txt", "c.bin"] {
        check(
            s.paths.contains(&shown(p)),
            format!("{p} is not listed: {:?}", s.paths),
        )?;
    }
    check(
        s.summary().starts_with("wrote 3 files, 2 KB, to scratch"),
        s.summary(),
    )?;
    // Discarded: the host's tree is as it was.
    check(
        fs::read_to_string(ws.path().join("old.txt"))
            .ok()
            .as_deref()
            == Some("keep"),
        "old.txt changed on the host",
    )?;
    check(!ws.path().join("a.txt").exists(), "a.txt reached the host")
}

fn sigterm_forwarded() -> Result<(), String> {
    let ws = tempfile::tempdir().map_err(|e| e.to_string())?;
    let spec = job(
        vec![
            "/bin/sh".into(),
            "-c".into(),
            "trap 'exit 3' TERM; while :; do sleep 0.05; done".into(),
        ],
        ws.path(),
    );
    let out = Output::new();
    let mut child = common::spawn(&spec, &out)?;
    let ns = fs::read_link(format!("/proc/{}/ns/pid", child.id())).map_err(|e| e.to_string())?;
    // The loop runs (so the trap is set) once a sleep is in the job.
    wait_until("the shell's loop", || in_namespace(&ns).len() >= 3)?;
    child.terminate().map_err(|e| e.to_string())?;
    let exit = child.wait().map_err(|e| e.to_string())?;
    check(exit.code == Some(3), format!("after SIGTERM: {exit:?}"))
}

/// A server on the host's 127.0.0.1 that echoes what it reads: the stand-in
/// for an allowed host (DD5's tests reach theirs the same way).
fn echo_server() -> Result<std::net::SocketAddr, String> {
    let l = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let addr = l.local_addr().map_err(|e| e.to_string())?;
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            std::thread::spawn(move || {
                if let Ok(mut w) = s.try_clone() {
                    let _ = std::io::copy(&mut &s, &mut w);
                }
            });
        }
    });
    Ok(addr)
}

/// The probe `egress` with `targets`, in a job with egress: the init opens
/// the listener inside the job's namespace and hands it out, and this
/// process serves it, as the wrapper will. (How it ended, its JSON, and
/// the proxy's records.)
fn egress_job(
    targets: &[&str],
    allow: &[String],
    resolver: Resolver,
) -> Result<(Value, Vec<Connection>), String> {
    let ws = tempfile::tempdir().map_err(|e| e.to_string())?;
    let mut spec = probe_spec(ws.path(), "egress", targets);
    spec.egress_port = Some(egress::PORT);
    spec.env.extend(egress::proxy_env(egress::PORT));
    let allow = allow
        .iter()
        .map(|a| a.parse::<Allow>())
        .collect::<Result<Vec<_>, _>>()?;
    let out = Output::new();
    let mut child = common::spawn(&spec, &out)?;
    let listener = child
        .take_egress_listener()
        .ok_or("the init handed out no listener")?;
    let proxy = Proxy::new(listener, allow, resolver)
        .start()
        .map_err(|e| e.to_string())?;
    let exit = child.wait().map_err(|e| e.to_string())?;
    let log = proxy.stop(Duration::from_secs(2));
    let text = out.read();
    check(
        exit.success(),
        format!("the egress probe: {exit:?}\n{text}"),
    )?;
    let v = serde_json::from_str(text.lines().last().unwrap_or(""))
        .map_err(|e| format!("{e}: {text}"))?;
    if std::env::var_os("THESEUS_SANDBOX_SHOW").is_some() {
        println!("probe egress: {v}\nproxy log: {log:?}");
    }
    Ok((v, log))
}

fn lo() -> std::net::IpAddr {
    std::net::IpAddr::from([127, 0, 0, 1])
}

/// `.test` names answered with 127.0.0.1, taken as public (a test's
/// server), as DD5's `Dns.hosts` and `Dns.public` are in its tests.
fn test_resolver(names: &[&str]) -> Resolver {
    let mut r = Resolver::default();
    for n in names {
        r.hosts.insert(n.to_string(), vec![lo()]);
    }
    r.public.push(lo());
    r
}

// 18b: the listener handoff, CONNECT, the allowlist, and the record.
fn egress_allowed() -> Result<(), String> {
    let target = echo_server()?;
    let t = format!("echo.test:{}", target.port());
    let via = format!("via={t}");
    let (v, log) = egress_job(
        &[&via],
        std::slice::from_ref(&t),
        test_resolver(&["echo.test"]),
    )?;
    check(
        v[&via]["status"] == 200 && v[&via]["echo"] == "ping\n",
        format!("{via}: {}", v[&via]),
    )?;
    let [c] = log.as_slice() else {
        return Err(format!("the proxy recorded {log:?}"));
    };
    check(
        c.host == "echo.test"
            && c.port == target.port()
            && c.addr == Some(target)
            && (c.up, c.down) == (5, 5)
            && c.refused.is_none()
            && c.ms < 5_000,
        format!("the record: {c:?}"),
    )
}

// 18b: off the list, a name that resolves to localhost or to the metadata
// service, the metadata address itself, and a method that is not CONNECT.
fn egress_refusals() -> Result<(), String> {
    let port = echo_server()?.port();
    // Nothing is taken as public here, so localhost is refused by the
    // system's own answer, whichever loopback address it gives.
    let mut resolver = Resolver::default();
    resolver.hosts.insert(
        "rebind.test".into(),
        vec![std::net::IpAddr::from([127, 0, 0, 2])],
    );
    resolver.hosts.insert(
        "metadata.test".into(),
        vec![std::net::IpAddr::from([169, 254, 169, 254])],
    );
    let allow = [
        format!("echo.test:{port}"),
        format!("rebind.test:{port}"),
        "metadata.test:80".into(),
        "169.254.169.254:80".into(),
        format!("localhost:{port}"),
    ];
    let cases: [(String, u16, String); 6] = [
        (
            format!("via=other.test:{port}"),
            403,
            format!("other.test:{port} is not on this job's egress list"),
        ),
        (
            format!("via=rebind.test:{port}"),
            403,
            "rebind.test resolves to 127.0.0.2, a loopback address".into(),
        ),
        (
            "via=metadata.test:80".into(),
            403,
            "metadata.test resolves to 169.254.169.254, a link-local address".into(),
        ),
        (
            "via=169.254.169.254:80".into(),
            403,
            "169.254.169.254 is a link-local address".into(),
        ),
        (
            format!("via=localhost:{port}"),
            403,
            "a loopback address".into(),
        ),
        (
            "get".into(),
            405,
            "only CONNECT is served: plain http:// forwarding is not offered".into(),
        ),
    ];
    let targets: Vec<&str> = cases.iter().map(|(t, _, _)| t.as_str()).collect();
    let (v, log) = egress_job(&targets, &allow, resolver)?;
    for (t, status, why) in &cases {
        let body = v[t]["body"].as_str().unwrap_or("");
        check(
            v[t]["status"] == *status && body.contains(why.as_str()),
            format!("{t}: {} (wanted {status}, {why:?})", v[t]),
        )?;
    }
    check(
        log.len() == cases.len() && log.iter().all(|c| c.refused.is_some() && c.addr.is_none()),
        format!("the proxy's records: {log:?}"),
    )
}

// 18b: a program that ignores the proxy variables has no route, egress or
// not: the host's loopback is not the job's, and nothing leads out.
fn egress_no_route() -> Result<(), String> {
    let target = echo_server()?;
    let direct = format!("direct={target}");
    let (v, log) = egress_job(
        &[&direct, "direct=1.1.1.1:443", "direct=169.254.169.254:80"],
        &[format!("echo.test:{}", target.port())],
        test_resolver(&["echo.test"]),
    )?;
    is(&v, &direct, "ECONNREFUSED")?;
    is(&v, "direct=1.1.1.1:443", "ENETUNREACH")?;
    is(&v, "direct=169.254.169.254:80", "ENETUNREACH")?;
    check(log.is_empty(), format!("the proxy saw {log:?}"))
}

// 18b's live check, run only when asked: a public host on the list, through
// the proxy, by the system's resolver.
fn egress_live() -> Result<(), String> {
    let host = std::env::var("THESEUS_SANDBOX_LIVE_EGRESS")
        .map_err(|_| "set THESEUS_SANDBOX_LIVE_EGRESS=host:port")?;
    let open = format!("open={host}");
    let (v, log) = egress_job(&[&open], std::slice::from_ref(&host), Resolver::default())?;
    println!("live egress to {host}: {}\nrecord: {log:?}", v[&open]);
    check(v[&open]["status"] == 200, format!("{open}: {}", v[&open]))
}

/// The probes, run inside L1. Each prints one JSON line.
mod probe {
    use std::fs;
    use std::io::{self, Read, Write};
    use std::os::unix::fs::MetadataExt;
    use std::path::Path;
    use std::time::Duration;

    use serde_json::{json, Map, Value};

    pub fn main(args: &[String]) -> ! {
        let name = args.first().map(String::as_str).unwrap_or("");
        let rest: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
        let v = match name {
            "ns" => ns(),
            "connect" => connect(&rest),
            "status" => status(),
            "seccomp" => seccomp(),
            "dev" => dev(),
            "proc" => proc_sys(),
            "forkloop" => forkloop(),
            "fill" => fill(rest[0], rest[1].parse().unwrap_or(1)),
            "spew" => spew(rest[0].parse().unwrap_or(1)),
            "fs" => fs_probe(rest[0]),
            "scratch" => scratch(rest[0]),
            "egress" => egress(&rest),
            "exit" => std::process::exit(rest[0].parse().unwrap_or(1)),
            "abort" => std::process::abort(),
            _ => json!({"error": format!("no probe {name}")}),
        };
        println!("{v}");
        std::process::exit(0)
    }

    fn errno() -> i32 {
        io::Error::last_os_error().raw_os_error().unwrap_or(0)
    }

    fn name(e: i32) -> String {
        let n = match e {
            libc::EPERM => "EPERM",
            libc::ENOENT => "ENOENT",
            libc::EACCES => "EACCES",
            libc::EROFS => "EROFS",
            libc::ENETUNREACH => "ENETUNREACH",
            libc::EHOSTUNREACH => "EHOSTUNREACH",
            libc::ECONNREFUSED => "ECONNREFUSED",
            libc::ENOSPC => "ENOSPC",
            libc::EAGAIN => "EAGAIN",
            libc::ENOSYS => "ENOSYS",
            libc::EINVAL => "EINVAL",
            libc::EFBIG => "EFBIG",
            libc::ETIMEDOUT => "ETIMEDOUT",
            libc::EEXIST => "EEXIST",
            libc::EAFNOSUPPORT => "EAFNOSUPPORT",
            _ => return format!("errno {e}"),
        };
        n.into()
    }

    fn outcome<T>(r: io::Result<T>) -> Value {
        match r {
            Ok(_) => json!("ok"),
            Err(e) => json!(e.raw_os_error().map(name).unwrap_or_else(|| e.to_string())),
        }
    }

    /// The result of a raw call: "ok", or its errno's name.
    fn raw(r: i64) -> Value {
        if r == -1 {
            json!(name(errno()))
        } else {
            json!("ok")
        }
    }

    fn ns() -> Value {
        let mut m = Map::new();
        for n in super::NAMESPACES {
            let ino = fs::metadata(format!("/proc/self/ns/{n}"))
                .map(|m| m.ino())
                .unwrap_or(0);
            m.insert(n.into(), json!(ino));
        }
        let mut uts: libc::utsname = unsafe { std::mem::zeroed() };
        unsafe { libc::uname(&mut uts) };
        let host = unsafe { std::ffi::CStr::from_ptr(uts.nodename.as_ptr()) };
        json!({"ns": m, "hostname": host.to_string_lossy()})
    }

    fn interfaces() -> Vec<String> {
        fs::read_to_string("/proc/net/dev")
            .unwrap_or_default()
            .lines()
            .skip(2)
            .filter_map(|l| l.split(':').next().map(|n| n.trim().to_string()))
            .collect()
    }

    fn connect(targets: &[&str]) -> Value {
        use std::os::linux::net::SocketAddrExt;
        use std::os::unix::net::{SocketAddr, UnixStream};
        let mut m = Map::new();
        for t in targets {
            let r: io::Result<()> = if let Some(a) = t.strip_prefix("tcp:") {
                match a.parse() {
                    Ok(addr) => std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(3))
                        .map(drop),
                    Err(e) => Err(io::Error::other(e)),
                }
            } else if let Some(p) = t.strip_prefix("unix:") {
                UnixStream::connect(p).map(drop)
            } else if let Some(n) = t.strip_prefix("abstract:") {
                SocketAddr::from_abstract_name(n.as_bytes())
                    .and_then(|a| UnixStream::connect_addr(&a).map(drop))
            } else {
                Err(io::Error::other("an unknown target"))
            };
            m.insert(t.to_string(), outcome(r));
        }
        let home = std::env::var("HOME").unwrap_or_default();
        json!({
            "connect": m,
            "interfaces": interfaces(),
            "theseus_state": outcome(fs::metadata(Path::new(&home).join(".theseus"))),
        })
    }

    fn status() -> Value {
        let mut m = Map::new();
        for l in fs::read_to_string("/proc/self/status")
            .unwrap_or_default()
            .lines()
        {
            if let Some((k, v)) = l.split_once(':') {
                let wanted = [
                    "NoNewPrivs",
                    "Seccomp",
                    "Uid",
                    "Gid",
                    "Groups",
                    "SigBlk",
                    "SigIgn",
                ];
                if k.starts_with("Cap") || wanted.contains(&k) {
                    m.insert(k.into(), json!(v.trim()));
                }
            }
        }
        unsafe {
            m.insert("getuid".into(), json!(libc::getuid()));
            m.insert("geteuid".into(), json!(libc::geteuid()));
            m.insert("getgid".into(), json!(libc::getgid()));
        }
        Value::Object(m)
    }

    /// Runs `f` in a forked child: "ok" if it exits 0, else the signal
    /// that killed it.
    fn in_child(f: impl FnOnce()) -> Value {
        match unsafe { libc::fork() } {
            -1 => json!(name(errno())),
            0 => {
                f();
                unsafe { libc::_exit(0) }
            }
            pid => {
                let mut st = 0;
                unsafe { libc::waitpid(pid, &mut st, 0) };
                if libc::WIFSIGNALED(st) {
                    let sig = libc::WTERMSIG(st);
                    json!(match sig {
                        libc::SIGSYS => "SIGSYS".to_string(),
                        libc::SIGSEGV => "SIGSEGV".to_string(),
                        s => format!("signal {s}"),
                    })
                } else if libc::WEXITSTATUS(st) == 0 {
                    json!("ok")
                } else {
                    json!(format!("exit {}", libc::WEXITSTATUS(st)))
                }
            }
        }
    }

    /// A clone whose child, should the call succeed, exits at once.
    fn clone_with(flags: libc::c_int) -> Value {
        let r = unsafe {
            libc::syscall(
                libc::SYS_clone,
                (flags | libc::SIGCHLD) as libc::c_ulong,
                0,
                0,
                0,
                0,
            )
        };
        if r == 0 {
            unsafe { libc::_exit(0) }
        }
        if r > 0 {
            unsafe { libc::waitpid(r as i32, std::ptr::null_mut(), 0) };
        }
        raw(r)
    }

    fn seccomp() -> Value {
        let mut m = Map::new();
        let mut put = |k: &str, v: Value| {
            m.insert(k.into(), v);
        };
        unsafe {
            put("unshare", raw(libc::unshare(libc::CLONE_NEWUSER) as i64));
            put(
                "mount",
                raw(libc::mount(
                    c"none".as_ptr(),
                    c"/tmp".as_ptr(),
                    c"tmpfs".as_ptr(),
                    0,
                    std::ptr::null(),
                ) as i64),
            );
            put("ptrace", raw(libc::ptrace(libc::PTRACE_TRACEME, 0, 0, 0)));
            put("bpf", raw(libc::syscall(libc::SYS_bpf, 0, 0, 0)));
            // KEYCTL_GET_KEYRING_ID of the session keyring.
            put("keyctl", raw(libc::syscall(libc::SYS_keyctl, 0, -3, 0)));
            let mut params = [0u8; 120];
            let r = libc::syscall(libc::SYS_io_uring_setup, 1, params.as_mut_ptr());
            if r >= 0 {
                libc::close(r as i32);
            }
            put("io_uring_setup", raw(r));
            let net = libc::open(
                c"/proc/self/ns/net".as_ptr(),
                libc::O_RDONLY | libc::O_CLOEXEC,
            );
            put("setns", raw(libc::setns(net, libc::CLONE_NEWNET) as i64));
            // clone3 with only an exit signal: a plain fork, were it allowed.
            let mut args = [0u64; 11];
            args[4] = libc::SIGCHLD as u64;
            let r = libc::syscall(libc::SYS_clone3, args.as_mut_ptr(), 88usize);
            if r == 0 {
                libc::_exit(0);
            }
            if r > 0 {
                libc::waitpid(r as i32, std::ptr::null_mut(), 0);
            }
            put("clone3", raw(r));
        }
        put("clone_newuser", clone_with(libc::CLONE_NEWUSER));
        put("clone_newnet", clone_with(libc::CLONE_NEWNET));
        put("fork", in_child(|| {}));
        put(
            "thread",
            outcome(
                std::thread::spawn(|| 1)
                    .join()
                    .map_err(|_| io::Error::other("panic")),
            ),
        );
        #[cfg(target_arch = "x86_64")]
        {
            put(
                "x32",
                in_child(|| unsafe {
                    libc::syscall(0x4000_0000 | libc::SYS_getpid);
                }),
            );
            put(
                "i386",
                in_child(|| unsafe {
                    // getpid in the i386 ABI, from 64-bit code.
                    std::arch::asm!(
                        "int 0x80",
                        inlateout("rax") 20u64 => _,
                        out("r8") _, out("r9") _, out("r10") _, out("r11") _,
                        options(nostack)
                    );
                }),
            );
        }
        let status = status();
        m.insert("Seccomp".into(), status["Seccomp"].clone());
        Value::Object(m)
    }

    fn dev() -> Value {
        let mut entries = Map::new();
        for e in fs::read_dir("/dev").into_iter().flatten().flatten() {
            // Through the mount: a directory entry's own type is that of the
            // file the device is bound over.
            let kind = match fs::symlink_metadata(e.path()).map(|m| m.file_type()) {
                Ok(t) if t.is_symlink() => "symlink",
                Ok(t) if t.is_dir() => "dir",
                Ok(t) => {
                    use std::os::unix::fs::FileTypeExt;
                    if t.is_char_device() {
                        "char"
                    } else if t.is_block_device() {
                        "block"
                    } else {
                        "file"
                    }
                }
                Err(_) => "?",
            };
            entries.insert(e.file_name().to_string_lossy().into(), json!(kind));
        }
        let read16 = |p: &str| -> io::Result<Vec<u8>> {
            let mut b = vec![0u8; 16];
            fs::File::open(p)?.read_exact(&mut b)?;
            Ok(b)
        };
        let zero = read16("/dev/zero").and_then(|b| {
            if b.iter().all(|&x| x == 0) {
                Ok(())
            } else {
                Err(io::Error::other("not zero"))
            }
        });
        let mknod = unsafe {
            libc::mknod(
                c"/dev/sda".as_ptr(),
                libc::S_IFBLK | 0o600,
                libc::makedev(8, 0),
            )
        };
        json!({
            "entries": entries,
            "urandom": outcome(read16("/dev/urandom")),
            "zero": outcome(zero),
            "null": outcome(fs::OpenOptions::new().write(true).open("/dev/null").and_then(|mut f| f.write_all(b"x"))),
            "create": outcome(fs::File::create("/dev/new")),
            "mknod": raw(mknod as i64),
        })
    }

    fn list(p: &str) -> Value {
        match fs::read_dir(p) {
            Ok(rd) => {
                let mut v: Vec<String> = rd
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().into())
                    .collect();
                v.sort();
                json!(v)
            }
            Err(e) => outcome::<()>(Err(e)),
        }
    }

    fn proc_sys() -> Value {
        let bytes = |p: &str| -> Value {
            let mut b = Vec::new();
            match fs::File::open(p).and_then(|mut f| f.read_to_end(&mut b)) {
                Ok(n) => json!(n),
                Err(e) => outcome::<()>(Err(e)),
            }
        };
        let write = |p: &str| {
            outcome(
                fs::OpenOptions::new()
                    .write(true)
                    .open(p)
                    .and_then(|mut f| f.write_all(b"1")),
            )
        };
        let mountinfo = fs::read_to_string("/proc/self/mountinfo").unwrap_or_default();
        let proc_sys_ro = mountinfo.lines().any(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            f.get(4) == Some(&"/proc/sys")
                && f.get(5).is_some_and(|o| o.split(',').any(|x| x == "ro"))
        });
        let pid1 = fs::read("/proc/1/cmdline")
            .map(|b| String::from_utf8_lossy(&b).replace('\0', " "))
            .unwrap_or_default();
        let mut pids: Vec<u32> = fs::read_dir("/proc")
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| e.file_name().to_str().and_then(|s| s.parse().ok()))
            .collect();
        pids.sort();
        let nonempty = |p: &str| {
            fs::read_to_string(p)
                .map(|s| !s.is_empty())
                .unwrap_or(false)
        };
        json!({
            "kcore_bytes": bytes("/proc/kcore"),
            "keys_bytes": bytes("/proc/keys"),
            "proc_sys_write": write("/proc/sys/kernel/hostname"),
            "sysrq_write": write("/proc/sysrq-trigger"),
            "proc_sys_ro": proc_sys_ro,
            "pid1": pid1,
            "pids": pids,
            "self": std::process::id(),
            "cpuinfo": nonempty("/proc/cpuinfo"),
            "meminfo": nonempty("/proc/meminfo"),
            "sys_net": list("/sys/class/net"),
            "sys_firmware": list("/sys/firmware"),
            "sys_security": list("/sys/kernel/security"),
            "sys_cgroup": list("/sys/fs/cgroup"),
            "sys_write": write("/sys/kernel/uevent_helper"),
        })
    }

    fn forkloop() -> Value {
        let mut forked = 0u64;
        let error = loop {
            match unsafe { libc::fork() } {
                -1 => break Some(name(errno())),
                0 => unsafe {
                    libc::pause();
                    libc::_exit(0)
                },
                _ => {
                    forked += 1;
                    if forked >= 4096 {
                        break None;
                    }
                }
            }
        };
        json!({"forked": forked, "error": error})
    }

    fn fill(path: &str, max_mb: u64) -> Value {
        let mut written = 0u64;
        let buf = vec![0xabu8; 1 << 16];
        let error = match fs::File::create(path) {
            Err(e) => Some(e),
            Ok(mut f) => loop {
                if written >= max_mb << 20 {
                    break None;
                }
                match f.write(&buf) {
                    Ok(n) => written += n as u64,
                    Err(e) => break Some(e),
                }
            },
        };
        json!({"written": written, "error": error.map(|e| outcome::<()>(Err(e)))})
    }

    fn spew(mb: u64) -> Value {
        let buf = vec![b'x'; 1 << 16];
        let mut out = io::stdout().lock();
        for _ in 0..(mb << 4) {
            if out.write_all(&buf).is_err() {
                break;
            }
        }
        json!({"spewed": mb})
    }

    fn fs_probe(ws: &str) -> Value {
        let home = std::env::var("HOME").unwrap_or_default();
        let h = |p: &str| Path::new(&home).join(p);
        let mut env: Vec<String> = std::env::vars_os()
            .map(|(k, _)| k.to_string_lossy().into())
            .collect();
        env.sort();
        json!({
            "write_usr": outcome(fs::File::create("/usr/theseus-probe")),
            "write_root": outcome(fs::File::create("/theseus-probe")),
            "write_etc": outcome(fs::File::create("/etc/theseus-probe")),
            "mkdir_root": outcome(fs::create_dir("/theseus-dir")),
            "write_workspace": outcome(fs::write(Path::new(ws).join("new.txt"), "x")),
            "write_tmp": outcome(fs::write("/tmp/x", "x")),
            "write_home": outcome(fs::write(h("x"), "x")),
            "gh_hosts": outcome(fs::metadata(h(".config/gh/hosts.yml"))),
            "ssh": outcome(fs::metadata(h(".ssh"))),
            "aws": outcome(fs::metadata(h(".aws"))),
            "op_token": outcome(fs::metadata(h(".openclaw-1password-service-token"))),
            "theseus_state": outcome(fs::metadata(h(".theseus"))),
            "run": outcome(fs::metadata("/run")),
            "var": outcome(fs::metadata("/var")),
            // Its fd directory lists (the init is the operator's uid too), but
            // no link in it can be followed or opened.
            "init_fd": outcome(fs::read_link("/proc/1/fd/4").and_then(|_| fs::File::open("/proc/1/fd/4"))),
            "init_environ": outcome(fs::read("/proc/1/environ")),
            "init_root": outcome(fs::read_dir("/proc/1/root")),
            "env": env,
            "ssh_auth_sock": std::env::var_os("SSH_AUTH_SOCK").is_some(),
        })
    }

    /// 18b, from inside the job: `via=host:port` tunnels through the proxy
    /// that `HTTPS_PROXY` names and sends a line to be echoed; `open=` only
    /// asks for the tunnel; `get` sends a plain GET to the proxy; `direct=`
    /// connects without it.
    fn egress(targets: &[&str]) -> Value {
        let proxy = std::env::var("HTTPS_PROXY").unwrap_or_default();
        let proxy = proxy.trim_start_matches("http://").trim_end_matches('/');
        let mut m = Map::new();
        for t in targets {
            let v = if let Some(to) = t.strip_prefix("via=") {
                through(
                    proxy,
                    &format!("CONNECT {to} HTTP/1.1\r\nHost: {to}\r\n\r\n"),
                    true,
                )
            } else if let Some(to) = t.strip_prefix("open=") {
                through(
                    proxy,
                    &format!("CONNECT {to} HTTP/1.1\r\nHost: {to}\r\n\r\n"),
                    false,
                )
            } else if *t == "get" {
                let get = "GET http://example.test/ HTTP/1.1\r\nHost: example.test\r\n\r\n";
                through(proxy, get, false)
            } else if let Some(to) = t.strip_prefix("direct=") {
                match to.parse() {
                    Ok(a) => outcome(std::net::TcpStream::connect_timeout(
                        &a,
                        Duration::from_secs(3),
                    )),
                    Err(e) => json!(format!("{e}")),
                }
            } else {
                json!("an unknown target")
            };
            m.insert(t.to_string(), v);
        }
        Value::Object(m)
    }

    fn through(proxy: &str, request: &str, echo: bool) -> Value {
        let mut c = match std::net::TcpStream::connect(proxy) {
            Ok(c) => c,
            Err(e) => return json!({"error": format!("the proxy at {proxy:?}: {e}")}),
        };
        let _ = c.set_read_timeout(Some(Duration::from_secs(5)));
        if let Err(e) = c.write_all(request.as_bytes()) {
            return json!({"error": e.to_string()});
        }
        let mut buf = Vec::new();
        let mut chunk = [0u8; 512];
        let end = loop {
            if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break i + 4;
            }
            match c.read(&mut chunk) {
                Ok(0) | Err(_) => return json!({"error": "no response head"}),
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
            }
        };
        let head = String::from_utf8_lossy(&buf[..end]).into_owned();
        let mut rest = buf[end..].to_vec();
        let status: u16 = head
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        if status == 200 && echo {
            if c.write_all(b"ping\n").is_err() {
                return json!({"status": status, "error": "the tunnel would not take a write"});
            }
            while rest.len() < 5 {
                match c.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => rest.extend_from_slice(&chunk[..n]),
                }
            }
            json!({"status": status, "echo": String::from_utf8_lossy(&rest)})
        } else if status == 200 {
            json!({"status": status})
        } else {
            let _ = c.read_to_end(&mut rest);
            json!({"status": status, "body": String::from_utf8_lossy(&rest).trim_end()})
        }
    }

    fn scratch(ws: &str) -> Value {
        let ws = Path::new(ws);
        let r = (|| -> io::Result<()> {
            fs::write(ws.join("a.txt"), [b'a'; 10])?;
            fs::create_dir(ws.join("sub"))?;
            fs::write(ws.join("sub/b.txt"), [b'b'; 100])?;
            fs::write(ws.join("c.bin"), [b'c'; 1000])?;
            fs::remove_file(ws.join("old.txt"))
        })();
        json!({"wrote": outcome(r)})
    }
}
