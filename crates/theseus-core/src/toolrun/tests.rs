use super::*;

#[test]
fn cap_keeps_head_and_tail_and_forbidden_env_names() {
    let s = "a".repeat(100) + &"b".repeat(100);
    let (c, t) = cap(&s, 100, |_| String::new());
    assert!(t);
    assert!(c.starts_with("aaaa") && c.ends_with("bbbb"), "{c}");
    assert!(c.contains("\n…[100 characters not shown]…\n"), "{c}");
    assert_eq!(
        cap("short", 100, |_| unreachable!()),
        ("short".to_string(), false)
    );
    assert!(forbidden_env("OP_SERVICE_ACCOUNT_TOKEN"));
    assert!(forbidden_env("GITHUB_TOKEN"));
    assert!(forbidden_env("aws_secret_access_key"));
    assert!(!forbidden_env("RUST_LOG"));
}

fn fake_env(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
    move |k| {
        pairs
            .iter()
            .find(|(n, _)| *n == k)
            .map(|(_, v)| v.to_string())
    }
}

fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// The positive: the owner's toolchain names, in the daemon's environment,
/// reach `proc_env` with their values (theseus-v4cj).
#[test]
fn proc_env_carries_the_toolchain_with_its_values() {
    let env = fake_env(&[
        ("VIRTUAL_ENV", "/opt/example/venv"),
        ("NVM_DIR", "/opt/example/nvm"),
        ("JAVA_HOME", "/opt/example/jdk"),
    ]);
    let (kept, refused) = resolve_proc_env(&crate::config::ToolsConfig::default().proc_env, env);
    assert!(refused.is_empty(), "{refused:?}");
    for (k, v) in [
        ("VIRTUAL_ENV", "/opt/example/venv"),
        ("NVM_DIR", "/opt/example/nvm"),
        ("JAVA_HOME", "/opt/example/jdk"),
    ] {
        assert!(
            kept.contains(&(k.to_string(), v.to_string())),
            "{k}: {kept:?}"
        );
    }
    assert_eq!(kept.len(), 3, "{kept:?}");
}

/// The negative, with the positive count beside it: secret-shaped names in
/// the daemon's environment, even when the owner lists them, never pass.
#[test]
fn proc_env_drops_every_secret_shaped_name_and_keeps_the_toolchain() {
    let env = fake_env(&[
        ("VIRTUAL_ENV", "/opt/example/venv"),
        ("NVM_DIR", "/opt/example/nvm"),
        ("JAVA_HOME", "/opt/example/jdk"),
        ("AWS_SECRET_ACCESS_KEY", "example-a"),
        ("OP_SERVICE_ACCOUNT_TOKEN", "example-b"),
        ("GITHUB_TOKEN", "example-c"),
        ("ANTHROPIC_API_KEY", "example-d"),
        ("THESEUS_CONFIG", "example-e"),
    ]);
    let mut configured = crate::config::ToolsConfig::default().proc_env;
    let secrets = [
        "AWS_SECRET_ACCESS_KEY",
        "OP_SERVICE_ACCOUNT_TOKEN",
        "GITHUB_TOKEN",
        "ANTHROPIC_API_KEY",
        "THESEUS_CONFIG",
    ];
    configured.extend(secrets.iter().map(|s| s.to_string()));
    let (kept, refused) = resolve_proc_env(&configured, env);
    let kept_names: Vec<&str> = kept.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(kept.len(), 3, "{kept_names:?}");
    for k in ["VIRTUAL_ENV", "NVM_DIR", "JAVA_HOME"] {
        assert!(kept_names.contains(&k), "{k}");
    }
    for k in secrets {
        assert!(!kept_names.contains(&k), "{k} reached a job");
    }
    // And each is reported, not silently dropped.
    assert_eq!(refused, names(&secrets));
}

/// No default name is swallowed by the secret rule, or the row silently
/// does not work.
#[test]
fn no_default_proc_env_name_is_forbidden() {
    let d = crate::config::ToolsConfig::default().proc_env;
    assert!(d.len() >= 9, "{d:?}");
    for k in &d {
        assert!(!forbidden_env(k), "{k} is a default and forbidden");
    }
}

/// A cut lands on lines' edges, says how many lines and characters are
/// not shown, hands `rest` exactly those, and says what `rest` answers,
/// never that anything is stored (theseus-46v).
#[test]
fn cap_cuts_on_line_edges_and_says_what_it_left_out_and_how_to_get_it() {
    let text: String = (1..=60)
        .map(|i| format!("entry {i:02} of the roster\n"))
        .collect();
    let mut seen = String::new();
    let (c, t) = cap(&text, 400, |left| {
        seen = left.to_string();
        "a narrower call returns them".into()
    });
    assert!(t);
    let (head, rest) = c.split_once("\n…[").unwrap();
    let (marker, tail) = rest.split_once("]…\n").unwrap();
    assert_eq!(
        format!("{head}\n{seen}{tail}"),
        text,
        "nothing else is lost"
    );
    assert!(
        seen.starts_with("entry ") && seen.ends_with('\n'),
        "{seen:?}"
    );
    assert_eq!(
        marker,
        format!(
            "{} lines ({} characters) not shown: a narrower call returns them",
            seen.lines().count(),
            crate::narrative::thousands(seen.chars().count() as u64)
        )
    );
    assert!(!c.contains("stored"), "{c}");
    // Cuts that fall on edges already keep every line they can.
    let even: String = (0..100).map(|i| format!("line {i:04}\n")).collect();
    let (c, _) = cap(&even, 200, |_| String::new());
    assert!(
        c.contains("\nline 0011\n…[80 lines (800 characters) not shown]…\nline 0092\n"),
        "{c}"
    );
    // One line too long to cut on an edge: characters only.
    let (c, _) = cap(&"x".repeat(5000), 400, |_| "R".into());
    assert!(c.contains("\n…[4,600 characters not shown: R]…\n"), "{c}");
}
