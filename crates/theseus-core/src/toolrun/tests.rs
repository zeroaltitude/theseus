use super::*;

mod failed_detail;

#[test]
fn cap_keeps_head_and_tail_and_forbidden_env_names() {
    let s = "a".repeat(100) + &"b".repeat(100);
    let (c, t) = cap(&s, 100, |_| String::new(), &Cuts::none());
    assert!(t);
    assert!(c.starts_with("aaaa") && c.ends_with("bbbb"), "{c}");
    assert!(c.contains("\n…[100 characters not shown]…\n"), "{c}");
    assert_eq!(
        cap("short", 100, |_| unreachable!(), &Cuts::none()),
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
    let (c, t) = cap(
        &text,
        400,
        |left| {
            seen = left.to_string();
            "a narrower call returns them".into()
        },
        &Cuts::none(),
    );
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
    let (c, _) = cap(&even, 200, |_| String::new(), &Cuts::none());
    assert!(
        c.contains("\nline 0011\n…[80 lines (800 characters) not shown]…\nline 0092\n"),
        "{c}"
    );
    // One line too long to cut on an edge: characters only.
    let (c, _) = cap(&"x".repeat(5000), 400, |_| "R".into(), &Cuts::none());
    assert!(c.contains("\n…[4,600 characters not shown: R]…\n"), "{c}");
}

/// `cap` as it was written before its cut points were apart: the oracle its text is held to.
fn cap_as_it_was(
    text: &str,
    max: usize,
    rest: impl FnOnce(&str) -> String,
    cuts: &Cuts,
) -> (String, bool) {
    let n = text.chars().count();
    if n <= max || max < 64 {
        return (text.to_string(), false);
    }
    let at = |chars: usize| {
        text.char_indices()
            .nth(chars)
            .map_or(text.len(), |(i, _)| i)
    };
    let head = max * 6 / 10;
    let (mut h, mut t) = (at(head), at(n - (max - head)));
    if let Some(i) = text[..h].rfind('\n').filter(|i| i + 1 >= h / 2) {
        h = i + 1;
    }
    let mid_line = !text[..t].ends_with('\n');
    if let Some(i) = text[t..]
        .find('\n')
        .filter(|i| mid_line && *i < (text.len() - t) / 2)
    {
        t += i + 1;
    }
    let (h, t) = cuts.around(text, h, t);
    let whole = [h, t]
        .iter()
        .filter(|at| text[..**at].ends_with('\n'))
        .count();
    let left = &text[h..t];
    let chars = narrative::count(left.chars().count() as u64, "character", "characters");
    let size = match whole {
        2 => format!(
            "{} ({chars})",
            narrative::count(left.lines().count() as u64, "line", "lines")
        ),
        _ => chars,
    };
    let how = match rest(left) {
        r if r.is_empty() => r,
        r => format!(": {r}"),
    };
    let head = text[..h].strip_suffix('\n').unwrap_or(&text[..h]);
    (
        format!("{head}\n…[{size} not shown{how}]…\n{}", &text[t..]),
        true,
    )
}

/// Over random results (characters of one to four bytes, control characters, runs of blanks and line
/// breaks; shorter and longer than the cap) with a known value at or near the head's cut and one holding a
/// line break at or near the tail's: the cap is its cut points, then its assembly, and is today's text; the
/// head it shows is `text[..h]` without one line break it ends with, and the tail `text[t..]`, each as it is,
/// with the seam line between them; neither splits a character or a known value. All values invented.
#[test]
fn cap_is_cut_points_then_assembly() {
    use crate::view::text::kept::sample::{self, Draw, BROKEN, VALUE};
    let mut draw = Draw::new(0xbb67_ae85_84ca_a73b);
    for case in 0..800 {
        let max = [10, 63, 64, 65, 100, 200, 400][draw.below(7)];
        let len = max / 2 + draw.below(2 * max);
        let mut text = draw.text(len);
        let n = text.chars().count();
        let head = max * 6 / 10;
        let mut known = Vec::new();
        if draw.below(4) > 0 {
            let near = (head + draw.below(VALUE.len() + 4)).saturating_sub(VALUE.len());
            text = draw.place(&text, VALUE, near.min(n));
            known.push(VALUE);
        }
        if draw.below(4) > 0 {
            let n = text.chars().count();
            let tail = n.saturating_sub(max - head) + draw.below(8);
            let back = draw.below(BROKEN.len());
            text = draw.place(&text, BROKEN, tail.saturating_sub(back));
            known.push(BROKEN);
        }
        let cuts = sample::cuts(&known);
        let rest = |left: &str| match left.len() % 3 {
            0 => String::new(),
            _ => format!("{} bytes", left.len()),
        };
        let (shown, cut) = cap(&text, max, rest, &cuts);
        assert_eq!(
            (shown.clone(), cut),
            cap_as_it_was(&text, max, rest, &cuts),
            "case {case}: {text:?}"
        );
        let Some((h, t)) = cut_points(&text, max, &cuts) else {
            assert!(!cut && shown == text);
            continue;
        };
        assert!(cut && h <= t && t <= text.len(), "case {case}");
        assert_eq!(capped(&text, h, t, rest), shown);
        let kept = text[..h].strip_suffix('\n').map_or(h, |k| k.len());
        sample::whole(&text, &(0..kept), &known);
        sample::whole(&text, &(t..text.len()), &known);
        assert!(shown.starts_with(&text[..kept]) && shown.ends_with(&text[t..]));
        let seam = &shown[kept..shown.len() - (text.len() - t)];
        assert!(
            seam.starts_with("\n…[") && seam.ends_with("]…\n") && seam.contains(" not shown"),
            "case {case}: {seam:?}"
        );
    }
}
