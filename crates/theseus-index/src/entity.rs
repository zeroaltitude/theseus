//! Entities: exact-match terms found by rules (M6 §2.2, the "exact indexes"
//! of §4.3, as a projection: a mention needs no WAL record). Each is stored
//! as `type:value`, and a query's text goes through the same rules, so a
//! query matches exactly what a node named.
//!
//! | Type | Rule | Example |
//! |---|---|---|
//! | `path` | a token with a slash that starts at a root (`/`, `~/`, `./`, `../`), or has two slashes, or ends in a file name with an extension; a line suffix (`:12`) dropped | `path:crates/theseus-store/src/wal.rs` |
//! | `file` | the file name of such a path, or a bare file name with a known extension | `file:wal.rs` |
//! | `beads` | `<prefix>-<id>` whose id has a digit or a `.N` child suffix, or whose prefix is a known Beads prefix; a child also names its root | `beads:theseus-zaz.12`, `beads:theseus-zaz` |
//! | `commit` | 7 to 40 hex digits with at least one digit and one letter; the 7-digit prefix too, so a short hash matches a long one | `commit:d069c4c` |
//! | `host` | the host of a URL | `host:github.com` |
//! | `crate` | `theseus-<name>`, a Rust path's first segment (`tantivy::Index`), `-p`/`--package`, `cargo add`/`install`, a `crates/<name>/` directory, and a `name = "1.2"` dependency line; `_` read as `-` | `crate:theseus-store` |
//! | `mention` | Discord's `<@id>`, and `@name` that is not an e-mail address or a package scope | `mention:271828182845904523`, `mention:eddie` |
//!
//! The rules overlap on purpose (`theseus-sim` is a crate name and has the
//! shape of a Beads id): a term is exact either way, so an extra type costs
//! one term and never a wrong match.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use regex::Regex;

/// Beads prefixes whose three-letter ids carry no digit (`theseus-hee`).
const BEADS_PREFIXES: &[&str] = &["theseus", "openclaw"];

/// Rust path segments that are not crates.
const NOT_CRATES: &[&str] = &["self", "super", "crate", "Self"];

/// TOML keys of a manifest that look like a dependency line.
const NOT_DEPENDENCIES: &[&str] = &["version", "edition", "rust-version", "resolver", "name"];

const FILE_EXTENSIONS: &[&str] = &[
    "rs", "toml", "md", "json", "yaml", "yml", "ts", "tsx", "js", "jsx", "py", "sh", "txt", "lock",
    "log", "seg", "redb", "html", "css", "go", "c", "h", "cpp", "hpp", "java", "kt", "swift",
    "sql", "proto", "pdf", "png", "jpg", "csv", "xml", "ini", "cfg", "conf", "service", "nix",
];

static URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:https?|wss?|ftp)://(?:[^\s/@]+@)?([a-z0-9][a-z0-9.-]*[a-z0-9])").unwrap()
});
static DISCORD_MENTION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<@!?(\d+)>").unwrap());
static AT_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|[^\w.@/])@([A-Za-z][A-Za-z0-9_.-]{0,31})").unwrap());
/// A lowercase first segment: `Body::UserMessage` names a type, not a crate.
static RUST_PATH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b([a-z_][a-z0-9_]*)::[A-Za-z_{*]").unwrap());
static PACKAGE_FLAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:\s|^)(?:-p|--package)[\s=]+([A-Za-z0-9][A-Za-z0-9_-]*)").unwrap()
});
/// The names after `cargo add` on the same line.
static CARGO_ADD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bcargo[ \t]+(?:add|install)((?:[ \t]+[a-z0-9][a-z0-9_-]*)+)").unwrap()
});
static DEPENDENCY_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?m)^\s*([a-z][a-z0-9_-]*)\s*=\s*(?:"[\^~=<>]?\d[0-9.]*"|\{[^}\n]*\bversion\s*=)"#,
    )
    .unwrap()
});
static THESEUS_CRATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\btheseus[-_]([a-z]{3,})\b").unwrap());
static BEADS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([a-z][a-z0-9]*(?:-[a-z][a-z0-9]*)*)-([a-z0-9]{2,5})((?:\.\d+)*)$").unwrap()
});
static HEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9a-f]{7,40}$").unwrap());
static PATHLIKE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?:~|\.{1,2})?/?[A-Za-z0-9_.@+-]+(?:/[A-Za-z0-9_.@+-]+)+/?$|^/[A-Za-z0-9_.@+-]+/?$",
    )
    .unwrap()
});
static LINE_SUFFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?::\d+){1,2}$").unwrap());

/// Every entity in `text`, as `type:value` terms, sorted and deduplicated.
pub fn entities(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for c in URL.captures_iter(text) {
        out.insert(format!("host:{}", c[1].to_ascii_lowercase()));
    }
    for c in DISCORD_MENTION.captures_iter(text) {
        out.insert(format!("mention:{}", &c[1]));
    }
    for m in AT_NAME.captures_iter(text) {
        let whole = m.get(0).unwrap();
        let name = m[1].trim_end_matches(['.', '-']);
        // A package scope (`@types/node`), not a person.
        if text[whole.end()..].starts_with('/') || name.is_empty() {
            continue;
        }
        out.insert(format!("mention:{}", name.to_ascii_lowercase()));
    }
    for c in RUST_PATH.captures_iter(text) {
        if !NOT_CRATES.contains(&&c[1]) && c[1] != *"_" {
            out.insert(crate_term(&c[1]));
        }
    }
    for c in PACKAGE_FLAG.captures_iter(text) {
        out.insert(crate_term(&c[1]));
    }
    for c in CARGO_ADD.captures_iter(text) {
        for name in c[1].split_whitespace() {
            out.insert(crate_term(name));
        }
    }
    for c in DEPENDENCY_LINE.captures_iter(text) {
        if !NOT_DEPENDENCIES.contains(&&c[1]) {
            out.insert(crate_term(&c[1]));
        }
    }
    for c in THESEUS_CRATE.captures_iter(text) {
        out.insert(crate_term(&format!("theseus-{}", &c[1])));
    }
    for raw in text.split(|c: char| c.is_whitespace() || "\"'`()[]{}<>,;|=".contains(c)) {
        token(raw, &mut out);
    }
    out
}

fn crate_term(name: &str) -> String {
    format!("crate:{}", name.to_ascii_lowercase().replace('_', "-"))
}

/// One whitespace-delimited token: a path, a file, a Beads id, or a hash.
fn token(raw: &str, out: &mut BTreeSet<String>) {
    let t = raw.trim_end_matches(['.', ',', ':', ';', '!', '?', ')', ']']);
    if t.is_empty() || t.contains("://") {
        return;
    }
    let t = LINE_SUFFIX.replace(t, "");
    let t = t.as_ref();
    if t.contains('/') {
        path(t, out);
        return;
    }
    if let Some(name) = file_name(t) {
        out.insert(format!("file:{name}"));
    }
    if let Some(c) = BEADS.captures(t) {
        let (prefix, id, child) = (&c[1], &c[2], &c[3]);
        let known = BEADS_PREFIXES
            .iter()
            .any(|p| prefix == *p || prefix.starts_with(&format!("{p}-")));
        let digit = id.chars().any(|ch| ch.is_ascii_digit());
        if digit || !child.is_empty() || (known && id.len() == 3) {
            out.insert(format!("beads:{t}"));
            if !child.is_empty() {
                out.insert(format!("beads:{prefix}-{id}"));
            }
        }
    }
    let lower = t.to_ascii_lowercase();
    if HEX.is_match(&lower)
        && lower.bytes().any(|b| b.is_ascii_digit())
        && lower.bytes().any(|b| b.is_ascii_alphabetic())
    {
        out.insert(format!("commit:{}", &lower[..7]));
        if lower.len() > 7 {
            out.insert(format!("commit:{lower}"));
        }
    }
}

fn path(t: &str, out: &mut BTreeSet<String>) {
    if !PATHLIKE.is_match(t) || !t.bytes().any(|b| b.is_ascii_alphabetic()) {
        return;
    }
    let trimmed = t.trim_end_matches('/');
    let rooted = ["/", "~/", "./", "../"].iter().any(|r| t.starts_with(r));
    let slashes = trimmed.matches('/').count();
    let last = trimmed.rsplit('/').next().unwrap_or_default();
    let file = file_name(last);
    if !(rooted || slashes >= 2 || file.is_some()) {
        return;
    }
    out.insert(format!("path:{trimmed}"));
    if let Some(name) = file {
        out.insert(format!("file:{name}"));
    }
    let parts: Vec<&str> = trimmed.split('/').collect();
    for w in parts.windows(2) {
        if w[0] == "crates" && !w[1].is_empty() {
            out.insert(crate_term(w[1]));
        }
    }
}

/// `name.ext` with a known extension.
fn file_name(t: &str) -> Option<&str> {
    let (stem, ext) = t.rsplit_once('.')?;
    let ok = !stem.is_empty()
        && stem
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
        && FILE_EXTENSIONS.contains(&ext);
    ok.then_some(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has(text: &str, want: &[&str]) {
        let got = entities(text);
        for w in want {
            assert!(got.contains(*w), "{text:?}: missing {w}, got {got:?}");
        }
    }

    fn lacks(text: &str, unwanted: &[&str]) {
        let got = entities(text);
        for u in unwanted {
            assert!(!got.contains(*u), "{text:?}: has {u}, got {got:?}");
        }
    }

    #[test]
    fn file_paths_and_their_names() {
        has(
            "see crates/theseus-store/src/wal.rs:120, and ~/.theseus/store/",
            &[
                "path:crates/theseus-store/src/wal.rs",
                "file:wal.rs",
                "crate:theseus-store",
                "path:~/.theseus/store",
            ],
        );
        has(
            "(./scripts/gate.sh)",
            &["path:./scripts/gate.sh", "file:gate.sh"],
        );
        has("edit Cargo.toml now", &["file:Cargo.toml"]);
        has("/etc/hosts", &["path:/etc/hosts"]);
        // Prose with a slash is not a path; nor is a date or a fraction.
        lacks(
            "and/or read/write 2026/09/30 1/2",
            &["path:and/or", "path:read/write", "path:2026/09/30"],
        );
    }

    #[test]
    fn beads_ids_and_their_roots() {
        has(
            "lane theseus-zaz.12 and openclaw-1lw7 (harbor-flr2), theseus-hee, openclaw-vestige-ive",
            &[
                "beads:theseus-zaz.12",
                "beads:theseus-zaz",
                "beads:openclaw-1lw7",
                "beads:harbor-flr2",
                "beads:theseus-hee",
                "beads:openclaw-vestige-ive",
            ],
        );
        lacks(
            "serde-json theseus-core well-known one-off",
            &[
                "beads:serde-json",
                "beads:theseus-core",
                "beads:well-known",
                "beads:one-off",
            ],
        );
    }

    #[test]
    fn commit_hashes_short_and_long() {
        has(
            "at d069c4c and 5b13509f2c3a4e7d8a9b0c1d2e3f4a5b6c7d8e9f",
            &[
                "commit:d069c4c",
                "commit:5b13509",
                "commit:5b13509f2c3a4e7d8a9b0c1d2e3f4a5b6c7d8e9f",
            ],
        );
        // A port, a date, and a word are not hashes.
        lacks(
            "port 7433 on 20260930, a facade",
            &["commit:7433", "commit:2026093", "commit:facade"],
        );
    }

    #[test]
    fn url_hosts() {
        has(
            "read https://GitHub.com/zeroaltitude/theseus and http://user@127.0.0.1:7433/x",
            &["host:github.com", "host:127.0.0.1"],
        );
        lacks(
            "https://github.com/zeroaltitude/theseus",
            &["path:github.com/zeroaltitude/theseus"],
        );
    }

    #[test]
    fn crate_names() {
        has(
            "use tantivy::Index; theseus_store::follow, cargo test -p theseus-index, \
             cargo add regex serde_json",
            &[
                "crate:tantivy",
                "crate:theseus-store",
                "crate:theseus-index",
                "crate:regex",
                "crate:serde-json",
            ],
        );
        has(
            "[dependencies]\ntantivy = \"0.26\"\nredb = { version = \"4\" }\n",
            &["crate:tantivy", "crate:redb"],
        );
        lacks(
            "version = \"0.0.1\"\nself::x super::y",
            &["crate:version", "crate:self", "crate:super"],
        );
    }

    #[test]
    fn mentions() {
        has(
            "hey <@271828182845904523> and @Eddie.",
            &["mention:271828182845904523", "mention:eddie"],
        );
        lacks(
            "mail eddie@example.com, npm i @types/node",
            &["mention:example.com", "mention:types"],
        );
    }

    #[test]
    fn a_query_finds_the_same_terms_as_the_text() {
        let text = "fixed in theseus-zaz.12 at d069c4c: crates/theseus-follow/src/lib.rs";
        let query = "what changed in crates/theseus-follow/src/lib.rs for theseus-zaz.12?";
        let both: Vec<_> = entities(text)
            .intersection(&entities(query))
            .cloned()
            .collect();
        assert!(
            both.contains(&"path:crates/theseus-follow/src/lib.rs".to_string()),
            "{both:?}"
        );
        assert!(
            both.contains(&"beads:theseus-zaz.12".to_string()),
            "{both:?}"
        );
    }
}
