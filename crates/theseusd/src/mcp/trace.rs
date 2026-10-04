//! Which job an MCP client runs in (step 41b, M7 §2.5): a session it opens,
//! or sends a turn to, takes that job's session's hold of external text
//! (theseus-b5cl's rule, as the CLI's `opened_from` gives it).
//!
//! The client's end of the connection is found in `/proc/net/tcp` (its
//! socket's inode, `peer::client_inode`), then among the daemon's own
//! descendants (a job's process is one: the daemon is a child subreaper, so
//! even a job's orphans stay under it), by its file descriptors. The job's
//! session is the `THESEUS_SESSION` of that process, or of its nearest
//! ancestor under the daemon that has one, so a client that strips its own
//! environment still names its job's. A light guard, as the CLI's is: a job
//! could hand its socket to a process outside, or strip every ancestor's
//! view by double-forking to the daemon. Only the daemon's descendants are
//! read, so the cost is the jobs', never the machine's.

use std::collections::HashMap;
use std::net::SocketAddr;

use theseus_protocol::JOB_SESSION_ENV;

/// The session of the job whose process holds the client's end of the
/// connection from `client` to `server`, if a descendant of this process
/// holds it and it or an ancestor names one.
pub fn job_session(server: SocketAddr, client: SocketAddr) -> Option<String> {
    let inode = theseus_core::peer::client_inode(server, client)?;
    let root = std::process::id();
    let parents = parents();
    let holder = under(root, &parents)
        .into_iter()
        .find(|pid| holds(*pid, inode))?;
    let mut pid = holder;
    loop {
        if let Some(s) = named(pid) {
            return Some(s);
        }
        match parents.get(&pid) {
            Some(&p) if p != root && p > 1 => pid = p,
            _ => return None,
        }
    }
}

/// Every process's parent, from `/proc/<pid>/stat`.
fn parents() -> HashMap<u32, u32> {
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return HashMap::new();
    };
    dir.filter_map(Result::ok)
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter_map(|pid| {
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
            // `pid (comm) state ppid …`; the name may hold spaces and parens.
            let after = &stat[stat.rfind(')')? + 1..];
            let ppid = after.split_whitespace().nth(1)?.parse().ok()?;
            Some((pid, ppid))
        })
        .collect()
}

/// The descendants of `root`, nearest first.
fn under(root: u32, parents: &HashMap<u32, u32>) -> Vec<u32> {
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for (&pid, &ppid) in parents {
        children.entry(ppid).or_default().push(pid);
    }
    let mut out = Vec::new();
    let mut next = vec![root];
    while let Some(p) = next.pop() {
        for &c in children.get(&p).map(Vec::as_slice).unwrap_or_default() {
            out.push(c);
            next.push(c);
        }
    }
    out
}

/// Whether `pid` holds the socket `inode` open.
fn holds(pid: u32, inode: u64) -> bool {
    let want = format!("socket:[{inode}]");
    let Ok(fds) = std::fs::read_dir(format!("/proc/{pid}/fd")) else {
        return false;
    };
    fds.filter_map(Result::ok)
        .any(|fd| std::fs::read_link(fd.path()).is_ok_and(|l| l.as_os_str() == want.as_str()))
}

/// The session `pid`'s environment names, if any.
fn named(pid: u32) -> Option<String> {
    let env = std::fs::read(format!("/proc/{pid}/environ")).ok()?;
    let key = format!("{JOB_SESSION_ENV}=");
    env.split(|b| *b == 0)
        .find_map(|kv| kv.strip_prefix(key.as_bytes()))
        .map(|v| String::from_utf8_lossy(v).into_owned())
        .filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::CommandExt;

    use super::*;

    /// The session `job_session` finds for the next connection, which
    /// `child` makes, in a process group of its own, killed after.
    async fn traced(
        listener: &tokio::net::TcpListener,
        mut child: std::process::Child,
    ) -> Option<String> {
        let (io, client) =
            tokio::time::timeout(std::time::Duration::from_secs(10), listener.accept())
                .await
                .unwrap()
                .unwrap();
        let server = io.local_addr().unwrap();
        let got = tokio::task::spawn_blocking(move || job_session(server, client))
            .await
            .unwrap();
        // The whole group: its shells and its `sleep`.
        let _ = std::process::Command::new("kill")
            .args(["-9", "--", &format!("-{}", child.id())])
            .status();
        let _ = child.wait();
        got
    }

    /// A child of this process that connects, with `THESEUS_SESSION` in its
    /// environment, is found by its socket and names its session; a child
    /// whose own environment is stripped names its parent's; a connection
    /// from this process itself names none.
    #[tokio::test]
    async fn a_jobs_process_is_found_by_its_socket_and_names_its_session() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let script = format!("exec 3<>/dev/tcp/127.0.0.1/{}; sleep 30", addr.port());
        let mut job = std::process::Command::new("bash");
        job.process_group(0)
            .args(["-c", &script])
            .env(JOB_SESSION_ENV, "ses_lantern_job");
        let child = job.spawn().expect("bash runs");
        assert_eq!(
            traced(&listener, child).await.as_deref(),
            Some("ses_lantern_job")
        );
        // Stripped: `env -i` runs the client with no environment, under a
        // shell that keeps it.
        let mut stripped = std::process::Command::new("bash");
        stripped
            .process_group(0)
            .args(["-c", &format!("env -i /bin/bash -c '{script}'; true")])
            .env(JOB_SESSION_ENV, "ses_lantern_parent");
        let child = stripped.spawn().expect("bash runs");
        assert_eq!(
            traced(&listener, child).await.as_deref(),
            Some("ses_lantern_parent")
        );
        let own = tokio::net::TcpStream::connect(addr).await.unwrap();
        let (io, client) = listener.accept().await.unwrap();
        let server = io.local_addr().unwrap();
        assert_eq!(own.local_addr().unwrap(), client);
        let got = tokio::task::spawn_blocking(move || job_session(server, client))
            .await
            .unwrap();
        assert_eq!(got, None);
    }
}
