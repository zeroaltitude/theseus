//! `@` files (theseus-vjzt, phase 1): typing `@` in the input line opens a
//! picker over the files under the conversation's directory, filtered as the
//! word grows; Tab or Enter writes the chosen path into the line.
//!
//! **What reaches the model is the path, never the bytes.** A message that
//! names `@src/lib.rs` goes as typed, with one line added at its foot that
//! gives the file's absolute path and says to read it with `fs.read`. The
//! file is then read the ordinary way: through the roots, the gate, the
//! data-gate labels and the read's own caps and cut notes. This client reads
//! no file's content and so has no second, uncapped way for bytes to enter a
//! request (the other choice, a file's text in the message, was rejected for
//! that reason, and because a client's read would skip the gate's labels).
//!
//! Like the session picker (`picker.rs`) the list is read once and a keystroke
//! filters it in memory: no request, no stat. The read is two steps and both
//! are the loop's: `tool.list` says the daemon's workspace roots (once, at the
//! first `@`), then [`walk`] lists the directory, and only if the directory is
//! inside a root, off the loop's thread (`Effect::ListFiles`). Nothing is
//! offered from outside the roots, a symlink is never followed, and a file
//! that is not in the list is refused at Enter, saying so (`@@` sends a
//! literal `@`).

use std::path::Path;

use serde_json::Value;
use theseus_protocol::method;

use super::dock::{menu_window, MENU_ROWS};
use super::state::{call, Chat, Effect, Purpose};
use super::style::{one, Line, Sty};

/// The most files one listing keeps; a longer one says it was cut.
pub const MAX_FILES: usize = 20_000;
/// The deepest directory the listing enters.
const MAX_DEPTH: usize = 12;
/// Directories no one means to name a file in.
const SKIP_DIRS: &[&str] = &[".git", "node_modules", "target"];

/// What a listing found.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Walked {
    /// Paths under the directory, `/`-separated, shallow ones first.
    pub files: Vec<String>,
    /// More files existed than [`MAX_FILES`].
    pub truncated: bool,
}

/// The files under `dir`, as long as `dir` is inside one of `roots`. Blocking:
/// the loop runs it off its thread. A symlink is not followed (neither a
/// linked file nor a linked directory is listed), a path with whitespace or
/// that is not UTF-8 cannot be typed as one word and is left out.
pub fn walk(dir: &Path, roots: &[String]) -> Result<Walked, String> {
    let dir = std::fs::canonicalize(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let roots: Vec<_> = roots
        .iter()
        .filter_map(|r| std::fs::canonicalize(r).ok())
        .collect();
    if roots.is_empty() {
        return Err("the daemon reports no workspace root, so no file is offered".into());
    }
    if !roots.iter().any(|r| dir.starts_with(r)) {
        return Err(format!(
            "{} is outside the workspace roots, so no file is offered",
            dir.display()
        ));
    }
    let mut out = Walked::default();
    let mut level = vec![(dir, String::new(), 0usize)];
    while !level.is_empty() {
        let mut next = Vec::new();
        for (path, prefix, depth) in level {
            let Ok(read) = std::fs::read_dir(&path) else {
                continue;
            };
            let mut entries: Vec<_> = read.filter_map(Result::ok).collect();
            entries.sort_by_key(std::fs::DirEntry::file_name);
            for e in entries {
                let Some(name) = e.file_name().to_str().map(str::to_string) else {
                    continue;
                };
                let Ok(kind) = e.file_type() else { continue };
                let rel = format!("{prefix}{name}");
                if kind.is_dir() {
                    if depth < MAX_DEPTH && !SKIP_DIRS.contains(&name.as_str()) {
                        next.push((e.path(), format!("{rel}/"), depth + 1));
                    }
                } else if kind.is_file() && !rel.contains(char::is_whitespace) {
                    if out.files.len() >= MAX_FILES {
                        out.truncated = true;
                        return Ok(out);
                    }
                    out.files.push(rel);
                }
            }
        }
        level = next;
    }
    Ok(out)
}

/// Where the list stands.
#[derive(Debug, Default)]
enum Load {
    /// No `@` typed yet: nothing asked.
    #[default]
    Unasked,
    /// `tool.list` asked, for the roots.
    Roots,
    /// The listing runs.
    Walking,
    Ready(Index),
    Failed(String),
}

#[derive(Debug)]
struct Index {
    /// Each path, and its lower-case once.
    files: Vec<(String, String)>,
    truncated: bool,
}

impl Index {
    fn new(w: Walked) -> Self {
        Self {
            files: w
                .files
                .into_iter()
                .map(|f| {
                    let lower = f.to_lowercase();
                    (f, lower)
                })
                .collect(),
            truncated: w.truncated,
        }
    }

    fn has(&self, path: &str) -> bool {
        self.files.iter().any(|(f, _)| f == path)
    }

    /// The paths that contain `query`, case blind, those whose file name
    /// begins with it first, each group in listing order.
    fn matching(&self, query: &str) -> Vec<&str> {
        let q = query.to_lowercase();
        let (mut named, mut rest) = (Vec::new(), Vec::new());
        for (f, lower) in &self.files {
            if !lower.contains(&q) {
                continue;
            }
            let base = lower.rsplit('/').next().unwrap_or(lower);
            if base.starts_with(&q) {
                named.push(f.as_str());
            } else {
                rest.push(f.as_str());
            }
        }
        named.extend(rest);
        named
    }
}

/// The picker's state in the chat.
#[derive(Debug, Default)]
pub struct AtFiles {
    load: Load,
    sel: usize,
    /// Esc closed the list; the next change of the word opens it again.
    closed: bool,
    /// The word the selection was made for.
    word: String,
}

/// The `@word` that ends at the cursor, without its `@`; None when the word
/// before the cursor is not one that starts with a single `@`.
fn query_of(word: &str) -> Option<&str> {
    let q = word.strip_prefix('@')?;
    (!q.starts_with('@')).then_some(q)
}

/// What goes with a message that names files: the footer of paths, and the
/// images as attachments.
#[derive(Default)]
pub(super) struct Resolved {
    pub footer: String,
    pub attachments: Vec<theseus_protocol::Attachment>,
    /// What the transcript says went by reference, so the person sees it.
    pub note: String,
}

/// The dim line under the echo: what went by reference and what went as an
/// image; empty when nothing did.
fn attached_note(by_path: &[&str], images: &[&str]) -> String {
    let mut parts = Vec::new();
    if !by_path.is_empty() {
        parts.push(format!("paths sent: {}", by_path.join(", ")));
    }
    if !images.is_empty() {
        parts.push(format!("images attached: {}", images.join(", ")));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("  ↪ {}", parts.join(" · "))
    }
}

/// Is a listed path an image the daemon renders for a vision model?
fn is_image(name: &str) -> bool {
    let ext = Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp")
}

/// Every `@path` the text names, as typed: words that begin with one `@`.
/// A trailing `,;:!?)` is the sentence's, not the path's. `@@x` and a lone
/// `@` name nothing.
pub fn mentions(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for w in text.split_whitespace() {
        let Some(q) = query_of(w) else { continue };
        let q = q.trim_end_matches([',', ';', ':', '!', '?', ')', ']', '"', '\'']);
        if !q.is_empty() && !found.iter().any(|f| f == q) {
            found.push(q.to_string());
        }
    }
    found
}

impl Chat {
    fn at_word(&self) -> String {
        self.editor.word_before_cursor()
    }

    /// The query, while the cursor is in an `@word` and the list is not closed.
    fn at_query(&self) -> Option<String> {
        if self.at.closed {
            return None;
        }
        query_of(&self.at_word()).map(str::to_string)
    }

    /// The matches the picker shows, and None when the list is not ready.
    fn at_matches(&self) -> Option<Vec<&str>> {
        let q = self.at_query()?;
        match &self.at.load {
            Load::Ready(ix) => Some(ix.matching(&q)),
            _ => None,
        }
    }

    /// Does the picker hold the keys (a ready list with a match to choose)?
    pub(super) fn at_open(&self) -> bool {
        self.at_matches().is_some_and(|m| !m.is_empty())
    }

    /// Is the word at the cursor already a whole listed path (then Enter sends)?
    pub(super) fn at_whole(&self) -> bool {
        match (&self.at.load, self.at_query()) {
            (Load::Ready(ix), Some(q)) => ix.has(&q),
            _ => false,
        }
    }

    /// After any key: a changed word starts its selection over and opens the
    /// list; the first `@` asks the daemon for the roots.
    pub(super) fn at_sync(&mut self) -> Vec<Effect> {
        let word = self.at_word();
        if word != self.at.word {
            self.at.word = word;
            self.at.sel = 0;
            self.at.closed = false;
        }
        // The list is also wanted when an `@path` sits elsewhere in the line (a
        // recalled message): Enter would refuse it for good otherwise.
        let wanted = self.at_query().is_some() || !mentions(&self.editor.typed_text()).is_empty();
        if wanted && matches!(self.at.load, Load::Unasked) {
            self.at.load = Load::Roots;
            return vec![call(method::TOOL_LIST, Value::Null, Purpose::AtRoots)];
        }
        Vec::new()
    }

    /// The connection was lost with `tool.list` unanswered: ask again at the
    /// next `@`.
    pub(super) fn at_lost(&mut self) {
        if matches!(self.at.load, Load::Roots) {
            self.at.load = Load::Unasked;
        }
    }

    /// The keys the open picker takes: Up and Down choose, Tab picks, Enter
    /// picks a half-typed word (a whole listed path sends the message). True
    /// when the key was the picker's.
    pub(super) fn at_key(&mut self, action: &super::keys::Action) -> bool {
        use super::keys::Action;
        if !self.at_open() {
            return false;
        }
        match action {
            Action::Up => self.at_move(false),
            Action::Down => self.at_move(true),
            Action::Tab => self.at_accept(),
            Action::Submit if !self.at_whole() => self.at_accept(),
            _ => return false,
        }
        true
    }

    /// Up and Down.
    pub(super) fn at_move(&mut self, down: bool) {
        let n = self.at_matches().map_or(0, |m| m.len());
        if down {
            self.at.sel = (self.at.sel + 1).min(n.saturating_sub(1));
        } else {
            self.at.sel = self.at.sel.saturating_sub(1);
        }
    }

    /// Esc: close the list, keeping the word. True when it was open.
    pub(super) fn at_escape(&mut self) -> bool {
        if self.at_query().is_some() && !matches!(self.at.load, Load::Unasked) {
            self.at.closed = true;
            return true;
        }
        false
    }

    /// Tab or Enter: the chosen path replaces the word.
    pub(super) fn at_accept(&mut self) {
        let Some(path) = self.at_matches().and_then(|m| {
            let i = self.at.sel.min(m.len().saturating_sub(1));
            m.get(i).map(|p| (*p).to_string())
        }) else {
            return;
        };
        self.editor.replace_word_before_cursor(&format!("@{path} "));
    }

    /// `tool.list` answered: the roots; the listing is next, off the loop.
    pub(super) fn at_roots(
        &mut self,
        result: Result<Value, theseus_protocol::RpcError>,
    ) -> Vec<Effect> {
        let roots = result.map_err(|e| e.message).and_then(|v| {
            serde_json::from_value::<theseus_protocol::ToolListResult>(v)
                .map(|r| r.roots)
                .map_err(|e| e.to_string())
        });
        match roots {
            Ok(roots) => {
                self.at.load = Load::Walking;
                vec![Effect::ListFiles {
                    dir: self.cfg.dir.clone(),
                    roots,
                }]
            }
            Err(why) => {
                self.at.load = Load::Failed(format!("could not read the workspace roots: {why}"));
                Vec::new()
            }
        }
    }

    /// The listing finished.
    pub fn at_walked(&mut self, result: Result<Walked, String>) {
        self.at.load = match result {
            Ok(w) => Load::Ready(Index::new(w)),
            Err(why) => Load::Failed(why),
        };
    }

    /// Before a message goes: every `@path` it names must be a listed file.
    /// Ok carries what goes with the message (a footer of paths for the files,
    /// the images as attachments); Err is what to tell the person, and nothing
    /// is sent. An image (png, jpg, jpeg, gif, webp) is read here, in `submit`,
    /// by the CLI's `attachment_for`, and goes as an `Attachment` instead of
    /// a path.
    pub(super) fn at_resolve(&self) -> Result<Resolved, String> {
        let names = mentions(&self.editor.typed_text());
        if names.is_empty() {
            return Ok(Resolved::default());
        }
        let ix = match &self.at.load {
            Load::Ready(ix) => ix,
            Load::Failed(why) => {
                return Err(format!("@{}: {why} · @@ sends a literal @", names[0]))
            }
            _ => {
                return Err(format!(
                    "@{}: the file list is still being read · enter again, or @@ sends a literal @",
                    names[0]
                ))
            }
        };
        let mut out = Resolved::default();
        let mut lines = Vec::new();
        let (mut by_path, mut images) = (Vec::new(), Vec::new());
        for n in &names {
            if !ix.has(n) {
                return Err(format!(
                    "@{n}: no such file under {} · @@ sends a literal @",
                    self.cfg.shown_dir
                ));
            }
            let abs = format!("{}/{n}", self.cfg.dir.trim_end_matches('/'));
            if is_image(n) {
                out.attachments.push(Self::image_for(n, &abs)?);
                images.push(n.as_str());
            } else {
                lines.push(abs);
                by_path.push(n.as_str());
            }
        }
        if !lines.is_empty() {
            out.footer = format!(
                "\n\nFiles named with @ (paths, not contents; read them with fs.read):\n{}",
                lines.join("\n")
            );
        }
        out.note = attached_note(&by_path, &images);
        Ok(out)
    }

    /// One image as an attachment, or the refusal's words (an unreadable
    /// file, or one over the CLI's cap, which `attachment_for` reports as
    /// `not_read`).
    fn image_for(name: &str, abs: &str) -> Result<theseus_protocol::Attachment, String> {
        let a = theseus_client::attach::attachment_labeled(Path::new(abs), "@image")
            .map_err(|e| format!("@{name}: {e:#}"))?;
        match &a.not_read {
            Some(why) => Err(format!("@{name}: {why}")),
            None => Ok(a),
        }
    }

    /// The rows under the input while the word at the cursor is an `@word`.
    pub(super) fn at_rows(&self) -> Vec<Line> {
        let Some(q) = self.at_query() else {
            return Vec::new();
        };
        match &self.at.load {
            Load::Unasked | Load::Roots | Load::Walking => {
                vec![one(Sty::Dim, "  reading the files…")]
            }
            Load::Failed(why) => vec![one(Sty::Yellow, format!("  {why}"))],
            Load::Ready(ix) => {
                let m = ix.matching(&q);
                if m.is_empty() {
                    return vec![one(Sty::Dim, format!("  no file matches @{q}"))];
                }
                let (first, shown) = menu_window(m.len(), self.at.sel);
                let sel = self.at.sel.min(m.len() - 1);
                let mut rows: Vec<Line> = m
                    .iter()
                    .enumerate()
                    .skip(first)
                    .take(shown)
                    .map(|(i, p)| {
                        one(
                            if i == sel { Sty::Cyan } else { Sty::Dim },
                            format!("  @{p}"),
                        )
                    })
                    .collect();
                if m.len() > MENU_ROWS || ix.truncated {
                    let cut = if ix.truncated {
                        format!(" · the list stops at {MAX_FILES} files")
                    } else {
                        String::new()
                    };
                    rows.push(one(
                        Sty::Dim,
                        format!(
                            "  {} more · ↑/↓ scroll · tab picks · esc closes{cut}",
                            m.len().saturating_sub(MENU_ROWS)
                        ),
                    ));
                }
                rows
            }
        }
    }
}
