//! The name miner (`onto.v1`, theseus-xzxqv): Jev cannot write a name, so a
//! category it calls `new` takes one that code found in what the message and
//! its session already say. Pure and cheap, no model: it lists at most
//! [`MAX_NAMES`], deduplicated without regard to case, in the order the
//! sources are tried:
//!
//! 1. the thread's and the project's titles;
//! 2. repository and directory names (a repository's URL gives its last
//!    part, a path its nearest directory that names something);
//! 3. hosts and services (the host of a URL, a bare domain);
//! 4. capitalised terms (a run of capitalised words, or one in mid-sentence);
//! 5. file paths.
//!
//! Which sources apply to a kind is its row's `mined_from`; the builder asks
//! for the sources it has been given, and every name still passes the
//! scrubber before a state holds it.

/// The most names a state offers.
pub const MAX_NAMES: usize = 10;

/// The longest name, in characters.
const NAME_CHARS: usize = 60;

/// A source of names, as a kind's `mined_from` picks them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Titles,
    Paths,
    Hosts,
    Capitalised,
}

/// The text the names are mined from.
#[derive(Debug, Clone, Copy, Default)]
pub struct Text<'a> {
    pub message: &'a str,
    pub previous: Option<&'a str>,
    pub thread_title: Option<&'a str>,
    pub project_title: Option<&'a str>,
}

/// Path components that name no project: layout, not content.
const GENERIC: &[&str] = &[
    "src",
    "crates",
    "lib",
    "libs",
    "bin",
    "tests",
    "test",
    "docs",
    "doc",
    "home",
    "usr",
    "var",
    "etc",
    "opt",
    "tmp",
    "root",
    "target",
    "build",
    "dist",
    "node_modules",
    "projects",
    "work",
    "workspace",
    "repos",
    "code",
    "dev",
    "users",
    "mnt",
    "app",
    "apps",
    "packages",
    "pkg",
];

/// Extensions that make a dotted word a file, not a host.
const FILE_EXTENSIONS: &[&str] = &[
    "rs", "py", "md", "txt", "json", "toml", "yaml", "yml", "ts", "tsx", "js", "jsx", "css",
    "html", "c", "h", "cpp", "go", "java", "sh", "sql", "lock", "csv", "log", "pdf", "png", "jpg",
    "xml", "ini", "cfg", "rb", "php", "kt", "swift",
];

/// Top-level domains a bare dotted word must end in to be called a host.
const TLDS: &[&str] = &[
    "com", "org", "net", "io", "dev", "ai", "app", "cloud", "bio", "co", "gov", "edu", "uk", "de",
    "fr", "ca", "xyz", "sh", "tech",
];

/// Words a capitalised run does not start or end with.
const STOP: &[&str] = &[
    "the",
    "this",
    "that",
    "these",
    "those",
    "and",
    "but",
    "for",
    "with",
    "can",
    "could",
    "would",
    "should",
    "please",
    "thanks",
    "thank",
    "yes",
    "not",
    "now",
    "then",
    "what",
    "when",
    "where",
    "which",
    "who",
    "why",
    "how",
    "are",
    "was",
    "were",
    "has",
    "have",
    "had",
    "did",
    "does",
    "let",
    "also",
    "just",
    "there",
    "here",
    "you",
    "your",
    "its",
    "our",
    "his",
    "her",
    "they",
    "them",
    "hey",
    "hello",
    "ok",
    "okay",
    "sure",
    "any",
    "all",
    "one",
    "some",
    "see",
    "use",
    "get",
    "make",
    "run",
    "add",
    "fix",
    "try",
    "check",
    "look",
    "show",
    "tell",
    "give",
    "take",
    "send",
    "write",
    "read",
    "set",
    "put",
    "need",
    "want",
    "like",
    "good",
    "great",
    "next",
    "first",
    "last",
    "new",
    "old",
    "from",
    "into",
    "about",
    "after",
    "before",
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
    "today",
    "tomorrow",
    "yesterday",
    "i",
    "we",
    "it",
    "if",
    "so",
    "or",
    "as",
    "at",
    "by",
    "in",
    "is",
    "of",
    "on",
    "to",
    "up",
    "no",
];

/// A name as the state shows it: one line, cut at the limit.
fn tidy(s: &str) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    one.chars().take(NAME_CHARS).collect()
}

/// A word with the punctuation round it taken off.
fn bare(w: &str) -> &str {
    w.trim_matches(|c: char| {
        !c.is_alphanumeric() && !matches!(c, '/' | '~' | '.' | '_' | '-' | ':' | '@')
    })
    .trim_end_matches(['.', ':', ',', ';'])
}

/// The host of a URL token, and its path.
fn url_parts(tok: &str) -> Option<(&str, &str)> {
    let rest = tok.split_once("://")?.1;
    let rest = rest.split_once('@').map_or(rest, |(_, r)| r);
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = host.split(':').next().unwrap_or(host);
    (host.contains('.') || host == "localhost").then_some((host, path))
}

/// The repository a URL path names: `org/repo(.git)` gives `repo`.
fn repo_of(path: &str) -> Option<&str> {
    let mut parts = path.split(['/', '?', '#']).filter(|p| !p.is_empty());
    let _org = parts.next()?;
    let repo = parts.next()?.trim_end_matches(".git");
    (!repo.is_empty()).then_some(repo)
}

fn is_path(tok: &str) -> bool {
    !tok.contains("://")
        && tok.contains('/')
        && (tok.starts_with(['/', '~', '.']) || tok.rsplit('/').next().is_some_and(has_extension))
}

fn has_extension(name: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(stem, ext)| !stem.is_empty() && FILE_EXTENSIONS.contains(&ext))
}

/// A path's nearest directory that names something.
fn directory_of(path: &str) -> Option<&str> {
    let parts: Vec<&str> = path
        .split('/')
        .filter(|p| !p.is_empty() && *p != "." && *p != ".." && *p != "~")
        .collect();
    let dirs = &parts[..parts.len().saturating_sub(1)];
    dirs.iter()
        .find(|p| !GENERIC.contains(&p.to_lowercase().as_str()) && !p.contains('.'))
        .copied()
}

fn is_host(tok: &str) -> bool {
    if tok.contains('/') || tok.contains("://") || has_extension(tok) {
        return false;
    }
    let labels: Vec<&str> = tok.split('.').collect();
    labels.len() >= 2
        && labels
            .iter()
            .all(|l| !l.is_empty() && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        && labels
            .last()
            .is_some_and(|t| TLDS.contains(&t.to_ascii_lowercase().as_str()))
}

fn capitalised(w: &str) -> bool {
    let mut cs = w.chars();
    cs.next().is_some_and(char::is_uppercase)
        && w.chars().filter(|c| c.is_alphabetic()).count() >= 3
        && w.chars().all(|c| c.is_alphanumeric() || c == '-')
        && !STOP.contains(&w.to_lowercase().as_str())
}

/// The capitalised terms of `text`: a run of two or more capitalised words
/// anywhere, or one capitalised word that does not begin a sentence.
fn terms(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut run: Vec<&str> = Vec::new();
    let mut sentence_start = true;
    let flush = |run: &mut Vec<&str>, start: bool, out: &mut Vec<String>| {
        match run.len() {
            0 => {}
            1 if start => {}
            _ => out.push(run.join(" ")),
        }
        run.clear();
    };
    let mut run_started_sentence = true;
    for raw in text.split_whitespace() {
        let w = bare(raw);
        let ends = raw.ends_with(['.', '!', '?', ':']);
        if capitalised(w) && !w.contains("://") {
            if run.is_empty() {
                run_started_sentence = sentence_start;
            }
            run.push(w);
        } else {
            flush(&mut run, run_started_sentence, &mut out);
        }
        if ends {
            flush(&mut run, run_started_sentence, &mut out);
        }
        sentence_start = ends;
    }
    flush(&mut run, run_started_sentence, &mut out);
    out
}

/// At most [`MAX_NAMES`] names from `sources`, deduplicated.
pub fn mine(text: &Text<'_>, sources: &[Source]) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let add = |name: &str, found: &mut Vec<String>| {
        let name = tidy(name);
        if name.is_empty() || found.len() >= MAX_NAMES {
            return;
        }
        if !found.iter().any(|f| f.eq_ignore_ascii_case(&name)) {
            found.push(name);
        }
    };
    let has = |s: Source| sources.contains(&s);
    if has(Source::Titles) {
        for t in [text.thread_title, text.project_title]
            .into_iter()
            .flatten()
        {
            add(t, &mut found);
        }
    }
    let both = [Some(text.message), text.previous];
    let toks: Vec<&str> = both
        .iter()
        .flatten()
        .flat_map(|t| t.split_whitespace())
        .map(bare)
        .filter(|t| !t.is_empty())
        .collect();
    if has(Source::Paths) {
        for t in &toks {
            if let Some((_, path)) = url_parts(t) {
                if let Some(repo) = repo_of(path) {
                    add(repo, &mut found);
                }
            } else if is_path(t) {
                if let Some(d) = directory_of(t) {
                    add(d, &mut found);
                }
            }
        }
    }
    if has(Source::Hosts) {
        for t in &toks {
            if let Some((host, _)) = url_parts(t) {
                add(host, &mut found);
            } else if is_host(t) {
                add(t, &mut found);
            }
        }
    }
    if has(Source::Capitalised) {
        for t in terms(text.message) {
            add(&t, &mut found);
        }
    }
    if has(Source::Paths) {
        for t in &toks {
            if is_path(t) {
                add(t, &mut found);
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Source; 4] = [
        Source::Titles,
        Source::Paths,
        Source::Hosts,
        Source::Capitalised,
    ];

    fn mined(message: &str) -> Vec<String> {
        mine(
            &Text {
                message,
                ..Text::default()
            },
            &ALL,
        )
    }

    #[test]
    fn a_path_gives_its_directory_and_itself() {
        assert_eq!(
            mined("can you read ~/projects/harbor-bench/crates/harbor-core/src/main.rs again"),
            vec![
                "harbor-bench",
                "~/projects/harbor-bench/crates/harbor-core/src/main.rs"
            ]
        );
    }

    #[test]
    fn a_host_is_named_and_a_url_gives_its_host() {
        let got =
            mined("the staging box is db1.acme-labs.io and https://status.acme-labs.io/x is up");
        assert_eq!(got, vec!["db1.acme-labs.io", "status.acme-labs.io"]);
    }

    #[test]
    fn a_repository_url_gives_the_repository_then_its_host() {
        let got = mined("clone https://github.com/finchworks/lantern-core.git first");
        assert_eq!(got, vec!["lantern-core", "github.com"]);
    }

    #[test]
    fn capitalised_terms_are_runs_or_mid_sentence_words() {
        let got =
            mined("Please ask about the Lantern Ledger migration. Thanks! We use Postgres too.");
        assert_eq!(got, vec!["Lantern Ledger", "Postgres"]);
    }

    #[test]
    fn plain_chat_mines_nothing_and_a_title_comes_first() {
        assert!(mined("ok sounds good, thanks. can you do that tomorrow?").is_empty());
        let got = mine(
            &Text {
                message: "what about the Ledger",
                thread_title: Some("Lantern roll-out"),
                project_title: Some("lantern roll-out"),
                ..Text::default()
            },
            &ALL,
        );
        assert_eq!(
            got,
            vec!["Lantern roll-out", "Ledger"],
            "deduplicated, titles first"
        );
    }

    #[test]
    fn a_kinds_sources_limit_what_is_mined_and_ten_is_the_most() {
        let text = (0..30)
            .map(|i| format!("host{i}.example.com"))
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(
            mine(
                &Text {
                    message: &text,
                    ..Text::default()
                },
                &ALL
            )
            .len(),
            MAX_NAMES
        );
        assert!(mine(
            &Text {
                message: &text,
                ..Text::default()
            },
            &[Source::Paths]
        )
        .is_empty());
    }
}
