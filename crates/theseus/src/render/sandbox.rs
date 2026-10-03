//! L1's lines (M4 17b): health's `sandbox:` line. Apart from `render.rs`,
//! whose length the shape budget caps (`scripts/long-files.txt`).

use theseus_protocol::sandbox::{launch_words, SandboxHealth};

/// `sandbox: the last L1 launch worked (start 3.1 ms) · default l0 · jobs:
/// 12 at L0, 3 in L1 · an L1 job gets 512 processes, 1024 MB of scratch,
/// files up to 64 MB · no egress listed`. Before the first L1 job, `no L1
/// job yet since start`; for a root daemon, why L1 is unavailable
/// (theseus-gyin, theseus-pv6i).
pub fn sandbox_line(s: &SandboxHealth) -> String {
    let l1 = match (&s.refuses, &s.last_launch) {
        (Some(why), _) => format!("L1 is unavailable: {why}"),
        (None, None) => "no L1 job yet since start".to_string(),
        (None, Some(l)) => format!("the last L1 launch {}", launch_words(l)),
    };
    let mut parts = vec![l1, format!("default {}", s.default)];
    if !s.l1_argv.is_empty() {
        parts.push(format!("always L1: {}", s.l1_argv.join("; ")));
    }
    parts.push(format!("jobs: {} at L0, {} in L1", s.jobs_l0, s.jobs_l1));
    parts.push(format!(
        "an L1 job gets {} processes, {} MB of scratch, files up to {} MB",
        s.pids, s.scratch_mb, s.output_mb
    ));
    parts.push(egress(s));
    if let Some(l) = s.last_launch.as_ref().filter(|l| !l.skipped.is_empty()) {
        parts.push(format!("ro_paths missing: {}", l.skipped.join(", ")));
    }
    format!("sandbox: {}", parts.join(" · "))
}

/// Its egress (M4 18c): `egress: 2 hosts listed (github.com:443,
/// *.crates.io:443); 5 connections, 1.2 KB up, 340.1 KB down; 1 refused
/// (latest: pypi.org:443 is not on this job's egress list)`, or `no
/// egress listed` while `[sandbox] egress` is empty.
fn egress(s: &SandboxHealth) -> String {
    let listed = match s.egress.len() {
        0 => "no egress listed".to_string(),
        n => format!(
            "egress: {n} host{} listed ({})",
            if n == 1 { "" } else { "s" },
            s.egress.join(", ")
        ),
    };
    let mut out = listed;
    if s.egress_connections > 0 {
        out.push_str(&format!(
            "; {} connection{}, {} up, {} down",
            s.egress_connections,
            if s.egress_connections == 1 { "" } else { "s" },
            bytes(s.egress_up),
            bytes(s.egress_down)
        ));
    }
    if s.egress_refused > 0 {
        out.push_str(&format!("; {} refused", s.egress_refused));
        if let Some(why) = &s.egress_last_refused {
            out.push_str(&format!(" (latest: {why})"));
        }
    }
    out
}

fn bytes(n: u64) -> String {
    match n {
        0..1_000 => format!("{n} B"),
        1_000..1_000_000 => format!("{:.1} KB", n as f64 / 1e3),
        _ => format!("{:.1} MB", n as f64 / 1e6),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::sandbox::SandboxLaunch;

    /// The line's head is the last real L1 launch since the start, or that
    /// there has been none, or why L1 is unavailable (theseus-gyin).
    #[test]
    fn the_sandbox_line_says_how_the_last_l1_launch_went_and_what_a_job_gets() {
        let mut s = SandboxHealth {
            default: "l0".into(),
            pids: 512,
            scratch_mb: 1024,
            output_mb: 64,
            jobs_l0: 12,
            jobs_l1: 3,
            ..Default::default()
        };
        assert!(
            sandbox_line(&s).starts_with("sandbox: no L1 job yet since start · default l0"),
            "{}",
            sandbox_line(&s)
        );
        s.last_launch = Some(SandboxLaunch {
            ok: true,
            start_ms: Some(3.08),
            sys: Some(true),
            lo: Some(false),
            skipped: vec!["/opt/gone".into()],
            ..Default::default()
        });
        let line = sandbox_line(&s);
        assert!(
            line.starts_with("sandbox: the last L1 launch worked (start 3.1 ms; lo down) · "),
            "{line}"
        );
        assert!(line.contains("jobs: 12 at L0, 3 in L1"), "{line}");
        assert!(
            line.contains("an L1 job gets 512 processes, 1024 MB of scratch"),
            "{line}"
        );
        assert!(line.ends_with(" · ro_paths missing: /opt/gone"), "{line}");
        s.last_launch = Some(SandboxLaunch {
            ok: false,
            why: Some("cloning the init into its namespaces: EPERM".into()),
            ..Default::default()
        });
        let line = sandbox_line(&s);
        assert!(
            line.contains("the last L1 launch failed: cloning the init"),
            "{line}"
        );
        assert!(line.contains("never runs at L0"), "{line}");
        assert!(line.contains(" · no egress listed"), "{line}");
        s.refuses = Some("the daemon runs as root".into());
        let line = sandbox_line(&s);
        assert!(
            line.starts_with("sandbox: L1 is unavailable: the daemon runs as root · "),
            "{line}"
        );
    }

    /// Health's egress (M4 18c): the list's size and hosts, and since the
    /// start the connections, bytes, and refusals, with the latest reason.
    #[test]
    fn the_sandbox_line_says_the_egress_list_and_its_refusals() {
        let s = SandboxHealth {
            egress: vec!["github.com:443".into(), "*.crates.io:443".into()],
            egress_connections: 5,
            egress_up: 1_200,
            egress_down: 340_100,
            egress_refused: 1,
            egress_last_refused: Some("pypi.org:443 is not on this job's egress list".into()),
            ..Default::default()
        };
        let line = sandbox_line(&s);
        assert!(
            line.contains(
                " · egress: 2 hosts listed (github.com:443, *.crates.io:443); 5 connections, \
                 1.2 KB up, 340.1 KB down; 1 refused (latest: pypi.org:443 is not on this \
                 job's egress list)"
            ),
            "{line}"
        );
    }
}
