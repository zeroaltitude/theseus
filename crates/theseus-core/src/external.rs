//! The `external` marker (DD5): a tool result whose text came from outside Theseus (`http.fetch`, `web.search`,
//! an MCP server marked external, a job that connected beyond `[sandbox] egress`) carries `external` on its node.
//! The marker is all there is: it holds nothing and waits for no one. What reads it is the content pack (the
//! source of a label), Jev's security state (the first outside text a session read), the index (recall keeps
//! outside text out unless asked) and egress's "outside text". The `injection` label is the one place a read of
//! outside text can stop a later call, and it does that at the act bar, by the owner's design (R6, theseus-gyjyo.8).
//!
//! One program's output is marked too, with no config and no hold: `gh` prints a stranger's issue and
//! review text (`Listed`), so a consumer that reads only the marker (consolidation, which never
//! synthesizes a cluster with an external source, the index's `external` flag that recall filters on, and
//! the gate's `read_any`) keeps treating it as outside text. `[policy] external_programs` is retired; the
//! list is this file's `PROGRAMS`.

use crate::node::{Body, Node};

/// The programs whose output is a stranger's text: `gh issue view` prints what an outside person wrote.
pub const PROGRAMS: &[&str] = &["gh"];

/// A job or a terminal that runs one of [`PROGRAMS`]: what its result is marked with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    /// The program, as [`PROGRAMS`] names it.
    pub program: &'static str,
    /// The command as the marker names it: the program and its subcommand (`gh issue`), never the rest of
    /// its argv; or a launcher and what it names (`sh, whose command names gh`).
    pub command: String,
}

impl Listed {
    /// The listed program a `proc.run` of `input` runs: its `argv[0]`, by file name; or, when that is a
    /// shell or a launcher (`sh -c`, `env`, `xargs`, an interpreter, a runner: `broker::launcher`), any
    /// word of its command that names one. It reads words, not what runs (`sh -c 'echo gh'` counts), which
    /// is the cheap side to err on. None when it names none, or `input` has no argv.
    pub fn of(input: &serde_json::Value) -> Option<Listed> {
        // A batch's steps, each read as its own call's (theseus-7gir.3).
        if let Some(steps) = input.get("steps").and_then(|s| s.as_array()) {
            return steps.iter().find_map(Self::of);
        }
        let argv: Vec<String> = input
            .get("argv")?
            .as_array()?
            .iter()
            .filter_map(|a| a.as_str().map(str::to_string))
            .collect();
        let first = argv.first()?;
        if let Some(p) = named(first) {
            let shown = crate::narrative::subject("", Some(&argv), None, std::path::Path::new("/"));
            return Some(Listed {
                program: p,
                command: shown.trim_start().to_string(),
            });
        }
        crate::broker::launcher(file_name(first))?;
        let p = argv[1..].iter().flat_map(|a| words(a)).find_map(named)?;
        Some(Listed {
            program: p,
            command: format!("{}, whose command names {p}", file_name(first)),
        })
    }

    /// The listed program text typed into a terminal names: any of its words, as a launcher's command is
    /// read. A terminal sent `gh issue view 1` is outside text from then on.
    pub fn in_text(text: &str) -> Option<Listed> {
        let p = words(text).find_map(named)?;
        Some(Listed {
            program: p,
            command: format!("a terminal sent keys that name {p}"),
        })
    }

    /// The marker on its result: its `url` the command.
    pub fn marker(&self) -> theseus_tools::External {
        theseus_tools::External {
            url: self.command.clone(),
        }
    }
}

/// The listed program `word` names, by file name: `gh` of `/usr/bin/gh`.
fn named(word: &str) -> Option<&'static str> {
    PROGRAMS.iter().copied().find(|p| *p == file_name(word))
}

/// A path's file name: `gh` of `/usr/bin/gh`.
fn file_name(p: &str) -> &str {
    p.rsplit('/').next().unwrap_or(p)
}

/// The words of a launcher's argument, split at spaces and the shell's punctuation:
/// `cd x && gh issue view 1 | head` has `gh` among them.
fn words(arg: &str) -> impl Iterator<Item = &str> {
    arg.split(|c: char| c.is_whitespace() || "|&;()<>`'\"$={}[],!*?\\".contains(c))
        .filter(|w| !w.is_empty())
}

/// Whether `node` is a result marked as outside text.
pub fn is_outside(node: &Node) -> bool {
    matches!(
        &node.body,
        Body::ToolResult {
            external: Some(_),
            ..
        }
    )
}

/// The first outside text a session read: the tool whose result it was, what the result is marked with (the
/// page's final URL, the search's request, the hosts a job reached), and when the node was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outside {
    pub tool: String,
    pub url: String,
    pub at_ms: u64,
}

/// The first of `nodes` that is outside text, which is the one Jev's state names.
pub fn first(nodes: &[Node]) -> Option<Outside> {
    nodes.iter().find_map(|n| match &n.body {
        Body::ToolResult {
            external: Some(e),
            tool,
            ..
        } => Some(Outside {
            tool: tool.clone(),
            url: e.url.clone(),
            at_ms: n.created_at_ms,
        }),
        _ => None,
    })
}

/// Whether `session_id` has read outside text: any result node of it is marked. A session whose nodes cannot be
/// read counts as having, so a fetch or a search in it is judged (the gate's `judged`).
pub fn read_any(store: &crate::store::Store, session_id: &str) -> bool {
    match store.session_nodes(session_id) {
        Ok(nodes) => nodes.iter().any(|(_, n)| is_outside(n)),
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::ResultStatus;

    fn result(id: &str, tool: &str, at_ms: u64, url: Option<&str>) -> Node {
        let mut n = Node::harness_body(
            "nod",
            "ses_a",
            "test",
            Body::ToolResult {
                tool_use_id: format!("tu_{id}"),
                tool: tool.into(),
                status: ResultStatus::Ok,
                is_error: false,
                content: "text".into(),
                correlation_id: None,
                bytes_total: 4,
                truncated: false,
                full_ref: None,
                duration_ms: None,
                late: false,
                meta: serde_json::Value::Null,
                image: None,
                external: url.map(|u| theseus_tools::External { url: u.into() }),
            },
        );
        n.id = id.into();
        n.created_at_ms = at_ms;
        n
    }

    /// The marker is what makes a result outside text, and the first one is what Jev's state names.
    #[test]
    fn the_marker_is_read_and_the_first_marked_result_is_named() {
        let local = result("n1", "fs.read", 10, None);
        let page = result("n2", "http.fetch", 20, Some("https://site.test/a"));
        let search = result("n3", "web.search", 30, Some("tides"));
        assert!(!is_outside(&local) && is_outside(&page) && is_outside(&search));
        assert_eq!(first(std::slice::from_ref(&local)), None);
        assert_eq!(
            first(&[local, page, search]),
            Some(Outside {
                tool: "http.fetch".into(),
                url: "https://site.test/a".into(),
                at_ms: 20
            })
        );
    }

    fn run_of(argv: &[&str]) -> serde_json::Value {
        serde_json::json!({ "argv": argv })
    }

    /// `gh` is named by its `argv[0]`'s file name, or, in a shell or a launcher, by any word of its
    /// command; another program, or one that only mentions `gh` in its arguments, is not.
    #[test]
    fn gh_is_named_by_its_file_name_or_by_a_launchers_words() {
        let direct = Listed::of(&run_of(&["gh", "issue", "view", "12"])).expect("gh is listed");
        assert_eq!(
            (direct.program, direct.command.as_str()),
            ("gh", "gh issue")
        );
        assert_eq!(direct.marker().url, "gh issue");
        assert_eq!(
            Listed::of(&run_of(&["/usr/local/bin/gh", "pr", "list"])).map(|l| l.command),
            Some("gh pr".into())
        );
        for launched in [
            &["sh", "-c", "cd work && gh issue view 1 | head -5"][..],
            &["env", "GH_REPO=invented/x", "gh", "api", "user"],
            &["xargs", "-I{}", "gh", "issue", "view", "{}"],
            &["bash", "-c", "$(command -v gh) version"],
            &["python3", "-c", "import os; os.system('gh issue view 1')"],
            &["timeout", "30", "/usr/bin/gh", "api", "user"],
        ] {
            let l =
                Listed::of(&run_of(launched)).unwrap_or_else(|| panic!("{launched:?} names gh"));
            assert_eq!(l.program, "gh");
            let shell = launched[0].rsplit('/').next().unwrap();
            assert_eq!(l.command, format!("{shell}, whose command names gh"));
        }
        for unlisted in [
            &["git", "log"][..],
            &["grep", "gh", "notes.txt"],
            &["sh", "-c", "echo hi"],
            &["sh", "-c", "ghost --version"],
        ] {
            assert_eq!(Listed::of(&run_of(unlisted)), None, "{unlisted:?}");
        }
        assert_eq!(
            Listed::of(&serde_json::json!({"cmd": "gh"})),
            None,
            "no argv"
        );
        assert_eq!(
            Listed::in_text("gh issue view 1 || true\n").map(|l| l.program),
            Some("gh")
        );
        assert_eq!(Listed::in_text("echo plain\n"), None);
    }
}
