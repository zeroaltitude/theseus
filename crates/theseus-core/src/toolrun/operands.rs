//! The operands of a program's argv, for the grouping rule (theseus-0hufe):
//! what a call names that another call of the same response may depend on.
//!
//! An **operand** is a non-flag word that names a thing: a service, unit,
//! container, host, port, URL, branch, database or directory. Left out are
//! the program itself, flags (`--user`), bare numbers (`sleep 2`), a few
//! words that value-flags take in every call (`json`), and, per pair of
//! calls, the words that are only the shape of the invocation: the leading
//! subcommand words both calls begin with (`rollout status`, `compose up`).
//! A flag's value counts (`-C ../wt-a`, `--name=db`), but takes no part in the
//! positions the shape is read from. A script (`bash -c "a && b"`) is each of
//! its simple commands, the command's own name left out.
//!
//! The rule errs toward waiting: two calls that share an operand stay in
//! order, and a false match costs only the time one waits for the other.

use std::collections::BTreeSet;

/// How many leading positional words two calls may share as the shape of
/// their invocation, before a shared word is a thing both name.
const SHAPE_WORDS: usize = 2;

/// Words a value-flag takes in many calls, and so name nothing.
const COMMON: &[&str] = &[
    "json", "yaml", "yml", "text", "true", "false", "all", "help", "version", "-", ".", "..",
];

/// One command's operands.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Invocation {
    /// The words that stood alone, in order: the subcommands and the things.
    positional: Vec<String>,
    /// A flag's values, and the ports a word holds (`port:8080`).
    values: BTreeSet<String>,
}

impl Invocation {
    fn names(&self, word: &str) -> bool {
        self.values.contains(word) || self.positional.iter().any(|w| w == word)
    }
}

/// The commands of `argvs` (a call's one argv, or a batch's steps), each as
/// its operands.
pub(super) fn of_argvs<'a>(argvs: impl IntoIterator<Item = &'a Vec<String>>) -> Vec<Invocation> {
    let mut out = Vec::new();
    for argv in argvs {
        let mut own = Invocation::default();
        let mut short = false;
        for arg in argv.iter().skip(1) {
            if arg.contains(|c: char| c.is_whitespace() || ";|&".contains(c)) {
                out.extend(script(arg));
            } else {
                feed(&mut own, arg, &mut short);
            }
        }
        out.push(own);
    }
    out.retain(|i| !i.positional.is_empty() || !i.values.is_empty());
    out
}

/// A script's simple commands, each as an invocation without its own name.
fn script(text: &str) -> Vec<Invocation> {
    text.split(|c| ";|&\n(){}".contains(c))
        .map(|command| {
            let mut own = Invocation::default();
            let mut short = false;
            let words = command.split_whitespace().skip_while(|w| is_assignment(w));
            for w in words.skip(1) {
                feed(&mut own, w, &mut short);
            }
            own
        })
        .collect()
}

/// `NAME=value`, set in front of a command.
fn is_assignment(w: &str) -> bool {
    w.split_once('=').is_some_and(|(k, _)| {
        !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// One word of a command line into `inv`; `short` is whether the word before
/// was a short flag, whose next word is its value.
fn feed(inv: &mut Invocation, word: &str, short: &mut bool) {
    let after_short = std::mem::take(short);
    if let Some(flag) = word.strip_prefix('-').filter(|f| !f.is_empty()) {
        match flag.split_once('=') {
            Some((_, v)) => add(inv, v, true),
            None => *short = !flag.starts_with('-'),
        }
        return;
    }
    add(inv, word, after_short);
}

/// `word`, cleaned, as a value of a flag or a positional operand.
fn add(inv: &mut Invocation, word: &str, value: bool) {
    let w = word.trim_matches(|c: char| "'\"`,".contains(c));
    let is_number = !w.is_empty() && w.chars().all(|c| c.is_ascii_digit() || ".,".contains(c));
    let redirect = w.starts_with(['>', '<', '$'])
        || w.trim_start_matches(|c: char| c.is_ascii_digit())
            .starts_with(['>', '<']);
    if w.is_empty() || is_number || redirect || w.starts_with("/dev/") || COMMON.contains(&w) {
        return;
    }
    for port in ports_of(w) {
        inv.values.insert(format!("port:{port}"));
    }
    if value {
        inv.values.insert(w.to_string());
    } else {
        inv.positional.push(w.to_string());
    }
}

/// The ports a `host:port` or `port:port` word names (`localhost:8080`,
/// `8080:80` is both); a URL's are the host's business.
fn ports_of(w: &str) -> Vec<&str> {
    let digits = |s: &str| !s.is_empty() && s.len() <= 5 && s.chars().all(|c| c.is_ascii_digit());
    match w.rsplit_once(':') {
        Some((host, port)) if !w.contains('/') && digits(port) && !host.is_empty() => {
            let mut ports = vec![port];
            ports.extend(Some(host).filter(|h| digits(h)));
            ports
        }
        _ => Vec::new(),
    }
}

/// An operand two sets of commands share, if any: the first found, in the
/// order of `a`'s words. Not the words each pair begins with alike (their
/// shape), so `kubectl rollout status` twice shares nothing by itself.
pub(super) fn shared(a: &[Invocation], b: &[Invocation]) -> Option<String> {
    for x in a {
        for y in b {
            let alike = x
                .positional
                .iter()
                .zip(&y.positional)
                .take(SHAPE_WORDS)
                .take_while(|(p, q)| p == q)
                .count();
            let words = x.positional.iter().skip(alike).chain(&x.values);
            let found = words.filter(|w| y.names(w)).find(|w| {
                // A word in the shape of both is no thing they share.
                !x.positional[..alike].contains(w)
            });
            if let Some(w) = found {
                return Some(w.clone());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inv(line: &str) -> Vec<Invocation> {
        let argv: Vec<String> = line.split(' ').map(String::from).collect();
        of_argvs([&argv])
    }

    /// The operand the two command lines share, if any.
    fn between(a: &str, b: &str) -> Option<String> {
        shared(&inv(a), &inv(b))
    }

    #[test]
    fn what_depends_on_an_earlier_call_shares_an_operand() {
        let web = Some("web".to_string());
        assert_eq!(
            between("systemctl --user start web", "systemctl --user status web"),
            web
        );
        assert_eq!(
            between("docker run --name db img", "docker exec db psql -c select"),
            Some("db".to_string())
        );
        assert_eq!(
            between("git worktree add ../wt-a", "make -C ../wt-a"),
            Some("../wt-a".to_string())
        );
        assert_eq!(
            between("docker build -t app .", "docker run app"),
            Some("app".to_string())
        );
        // Not reached, and said so (the module doc): `api.yaml` and
        // `deploy/api` share no whole word.
        assert_eq!(
            between(
                "kubectl apply -f api.yaml",
                "kubectl rollout status deploy/api"
            ),
            None
        );
        assert_eq!(
            between("python3 -m http.server 8080", "curl localhost:8080"),
            None,
            "a bare number is no operand"
        );
        assert_eq!(
            between("docker run -p 8080:80 img", "curl localhost:8080"),
            Some("port:8080".to_string())
        );
    }

    #[test]
    fn independent_families_share_nothing() {
        let none = |a: &str, b: &str| assert_eq!(between(a, b), None, "{a} / {b}");
        none("sleep 2", "sleep 2");
        none("pytest tests/a.py", "pytest tests/b.py");
        none(
            "curl -sS https://a.example/x",
            "curl -sS https://b.example/y",
        );
        none("make -C ../a", "make -C ../b");
        none("git -C ../a status", "git -C ../b status");
        none("cargo build -p core", "cargo build -p cli");
        none("kubectl get pods -o json", "kubectl get nodes -o json");
        none("npm test", "npm test");
        none("systemctl restart web", "systemctl restart db");
    }

    #[test]
    fn a_script_is_its_commands_without_their_names() {
        let a = of_argvs([&vec![
            "bash".into(),
            "-c".into(),
            "cd web && make; echo done".into(),
        ]]);
        assert!(a.iter().any(|i| i.positional == ["web"]), "{a:?}");
        let b = of_argvs([&vec![
            "bash".into(),
            "-c".into(),
            "FOO=1 systemctl start web".into(),
        ]]);
        assert_eq!(shared(&a, &b), Some("web".to_string()));
        let c = of_argvs([&vec![
            "bash".into(),
            "-c".into(),
            "echo done; echo hi".into(),
        ]]);
        assert_eq!(
            shared(&a, &c),
            None,
            "`echo done` twice is one shape, not a thing both name"
        );
    }
}
