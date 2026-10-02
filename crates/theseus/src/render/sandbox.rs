//! L1's lines (M4 17b): health's `sandbox:` line. Apart from `render.rs`,
//! whose length the shape budget caps (`scripts/long-files.txt`).

use theseus_protocol::sandbox::SandboxHealth;

/// `sandbox: L1 works (start 3.1 ms; sysfs, lo up) · default l0 · jobs: 12
/// at L0, 3 in L1 · an L1 job gets 512 processes, 1024 MB of scratch, files
/// up to 64 MB · cgroup none: …`.
pub fn sandbox_line(s: &SandboxHealth) -> String {
    let probe = match &s.probe {
        None => "not probed yet".to_string(),
        Some(p) if p.ok => {
            let mut found = Vec::new();
            if p.sys == Some(false) {
                found.push("no sysfs: the kernel refused it");
            }
            if p.lo == Some(false) {
                found.push("lo down");
            }
            let start = p
                .start_ms
                .map_or_else(String::new, |m| format!("start {m:.1} ms"));
            let all: Vec<String> = std::iter::once(start)
                .filter(|s| !s.is_empty())
                .chain(found.iter().map(|s| s.to_string()))
                .collect();
            format!("L1 works ({})", all.join("; "))
        }
        Some(p) => format!(
            "L1 is NOT available here: {} (an L1 call fails, and never runs at L0)",
            p.why.as_deref().unwrap_or("no reason given")
        ),
    };
    let mut parts = vec![probe, format!("default {}", s.default)];
    if !s.l1_argv.is_empty() {
        parts.push(format!("always L1: {}", s.l1_argv.join("; ")));
    }
    parts.push(format!("jobs: {} at L0, {} in L1", s.jobs_l0, s.jobs_l1));
    let memory = match s.cgroup.as_deref() {
        Some(c) if c.starts_with("delegated") => format!("{} MB of memory, ", s.memory_mb),
        _ => String::new(),
    };
    parts.push(format!(
        "an L1 job gets {memory}{} processes, {} MB of scratch, files up to {} MB",
        s.pids, s.scratch_mb, s.output_mb
    ));
    if let Some(c) = &s.cgroup {
        parts.push(format!("cgroup {c}"));
    }
    if let Some(p) = s.probe.as_ref().filter(|p| !p.skipped.is_empty()) {
        parts.push(format!("ro_paths missing: {}", p.skipped.join(", ")));
    }
    format!("sandbox: {}", parts.join(" · "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::sandbox::SandboxProbe;

    #[test]
    fn the_sandbox_line_says_whether_l1_works_and_what_a_job_gets() {
        let mut s = SandboxHealth {
            default: "l0".into(),
            memory_mb: 2048,
            pids: 512,
            scratch_mb: 1024,
            output_mb: 64,
            jobs_l0: 12,
            jobs_l1: 3,
            ..Default::default()
        };
        assert!(sandbox_line(&s).starts_with("sandbox: not probed yet · default l0"));
        s.probe = Some(SandboxProbe {
            ok: true,
            start_ms: Some(3.08),
            sys: Some(true),
            lo: Some(true),
            ..Default::default()
        });
        s.cgroup = Some("none: the daemon runs in x.scope".into());
        let line = sandbox_line(&s);
        assert!(
            line.starts_with("sandbox: L1 works (start 3.1 ms) · "),
            "{line}"
        );
        assert!(line.contains("jobs: 12 at L0, 3 in L1"), "{line}");
        assert!(line.contains("an L1 job gets 512 processes"), "{line}");
        s.cgroup = Some("delegated: /sys/fs/cgroup/u.service".into());
        assert!(sandbox_line(&s).contains("gets 2048 MB of memory, 512 processes"));
        s.probe = Some(SandboxProbe {
            ok: false,
            why: Some("cloning the init into its namespaces: EPERM".into()),
            ..Default::default()
        });
        let line = sandbox_line(&s);
        assert!(line.contains("L1 is NOT available here: cloning"), "{line}");
        assert!(line.contains("never runs at L0"), "{line}");
    }
}
