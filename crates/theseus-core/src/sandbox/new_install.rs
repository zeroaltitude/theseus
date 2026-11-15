//! The sandbox default a new install's generated configuration carries
//! (theseus-fdxy). The design promises newcomers L1, the sandbox, by default,
//! and L1 cannot run everywhere (a root daemon, a container), where a call
//! that asks for it fails and never falls back to L0. So the configuration
//! `theseusd example-config --for-this-host` writes for a new install says
//! `default = "l1"` only where L1's own self-test just ran a job here, and
//! keeps `default = "l0"`, with a comment that says why, where it did not.
//! The built-in default stays `l0` and the plain template is unchanged, so an
//! upgrade changes nothing for an install that already has a configuration.

use theseus_protocol::sandbox::SandboxLaunch;

use super::{launch_of, Sandbox};

/// `template` with its `[sandbox] default` set for a host where L1's
/// self-test came out as `l1`, and the one line that says which way and why.
pub fn template_for(template: &str, l1: &SandboxLaunch) -> (String, String) {
    let said = if l1.ok {
        "L1, the sandbox, runs on this host, so the new configuration's default is l1: every job \
         runs in the sandbox."
            .to_string()
    } else {
        format!(
            "L1, the sandbox, cannot run on this host ({}), so the new configuration's default \
             stays l0: jobs run without it, and a call that asks for it fails.",
            l1.why.as_deref().unwrap_or("no reason given")
        )
    };
    let mut out = String::with_capacity(template.len() + said.len() + 4);
    let (mut in_sandbox, mut done) = (false, false);
    for line in template.split_inclusive('\n') {
        if line.starts_with('[') {
            in_sandbox = line.trim_end() == "[sandbox]";
        }
        if in_sandbox && !done && line.starts_with("default = \"l0\"") {
            done = true;
            if l1.ok {
                out.push_str(&line.replacen("\"l0\"", "\"l1\"", 1));
            } else {
                out.push_str(&format!("# {said}\n{line}"));
            }
            continue;
        }
        out.push_str(line);
    }
    (out, said)
}

/// The bundled template for this host: L1's self-test runs here and now,
/// over the view a job of the example configuration gets. A host whose own
/// refusal is known (a root daemon) is not asked to start a job.
pub fn this_host() -> (String, String) {
    let launch = match theseus_sandbox::refused_here() {
        Some(why) => SandboxLaunch {
            why: Some(why.to_string()),
            ..SandboxLaunch::default()
        },
        None => {
            let sandbox: Sandbox = crate::toolrun::sandbox_for(&crate::Config::example());
            let at_ms = theseus_protocol::now_unix_ms();
            let detail = theseus_kernel::job::self_test(&sandbox.job_view());
            launch_of(&detail, at_ms).unwrap_or_default()
        }
    };
    template_for(crate::Config::EXAMPLE_TOML, &launch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;

    fn launch(ok: bool, why: Option<&str>) -> SandboxLaunch {
        SandboxLaunch {
            ok,
            why: why.map(str::to_string),
            ..SandboxLaunch::default()
        }
    }

    /// The lines that differ, as (old, new) pairs.
    fn changed<'a>(a: &'a str, b: &'a str) -> Vec<(&'a str, &'a str)> {
        let a: Vec<&str> = a.lines().collect();
        let b: Vec<&str> = b.lines().collect();
        assert_eq!(a.len(), b.len(), "line counts differ");
        a.into_iter().zip(b).filter(|(x, y)| x != y).collect()
    }

    #[test]
    fn a_host_where_l1_runs_gets_l1_as_its_default_and_nothing_else_moves() {
        let (text, said) = template_for(Config::EXAMPLE_TOML, &launch(true, None));
        let diff = changed(Config::EXAMPLE_TOML, &text);
        assert_eq!(diff.len(), 1, "{diff:?}");
        assert!(diff[0].0.starts_with("default = \"l0\""), "{diff:?}");
        assert!(diff[0].1.starts_with("default = \"l1\""), "{diff:?}");
        let cfg = Config::parse(&text).expect("the new configuration loads").0;
        assert_eq!(cfg.sandbox.default.as_str(), "l1");
        assert!(said.contains("default is l1"), "{said}");
    }

    #[test]
    fn a_host_where_l1_cannot_run_keeps_l0_and_says_why_beside_it() {
        let why = "the daemon runs as root";
        let (text, said) = template_for(Config::EXAMPLE_TOML, &launch(false, Some(why)));
        // One comment line is added; no value moves.
        let old: Vec<&str> = Config::EXAMPLE_TOML.lines().collect();
        let new: Vec<&str> = text.lines().collect();
        assert_eq!(new.len(), old.len() + 1);
        let at = new
            .iter()
            .position(|l| l.starts_with("default = \"l0\""))
            .unwrap();
        assert!(
            new[at - 1].starts_with("# L1, the sandbox, cannot run"),
            "{}",
            new[at - 1]
        );
        assert!(new[at - 1].contains(why), "{}", new[at - 1]);
        assert!(said.contains(why) && said.contains("stays l0"), "{said}");
        let cfg = Config::parse(&text).expect("the new configuration loads").0;
        assert_eq!(cfg.sandbox.default.as_str(), "l0");
    }

    #[test]
    fn only_the_sandbox_tables_default_is_set() {
        // Another table's `default = "l0"` line (a template that has two) is left alone.
        let t = "[other]\ndefault = \"l0\"\n[sandbox]\ndefault = \"l0\"  # note\n";
        let (text, _) = template_for(t, &launch(true, None));
        assert_eq!(
            text,
            "[other]\ndefault = \"l0\"\n[sandbox]\ndefault = \"l1\"  # note\n"
        );
    }

    #[test]
    fn the_built_in_default_is_still_l0() {
        // An upgrade changes nothing: the plain template and the built-in say l0.
        assert_eq!(Config::example().sandbox.default.as_str(), "l0");
        assert!(Config::EXAMPLE_TOML.contains("[sandbox]\ndefault = \"l0\""));
    }
}
