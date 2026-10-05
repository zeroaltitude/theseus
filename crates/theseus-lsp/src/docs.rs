//! Document sync: full text, one version per document.
//!
//! A document is opened with its whole text (`didOpen`, version 1), and each
//! change sends the whole text again with the next version (`didChange`).
//! The client remembers each open file's modification time and size, and
//! before every request about a document it checks them all
//! ([`Client::sync_disk`]): a file changed on disk is sent again, and one
//! that is gone is closed. So a request never reads a document older than
//! the file, whoever wrote it. After a write of its own (L3's `fs.write`,
//! `fs.edit`, `fs.patch`), a caller says so with [`Client::file_changed`],
//! which also tells the server's file watcher. For a server that checks
//! after a save (`Options::check_token`, rust-analyzer), it opens and saves a
//! file it has not opened, so the check covers that file's first edit too.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::json;

use crate::client::{Client, Error};
use crate::types::FileChangeType;
use crate::uri;

/// A `didSave`: the count of messages sent with it, and when.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Saved {
    pub(crate) sent: u64,
    pub(crate) at: tokio::time::Instant,
}

/// An open document.
#[derive(Debug, Clone)]
pub(crate) struct Doc {
    pub(crate) path: PathBuf,
    pub(crate) version: i32,
    pub(crate) text: String,
    /// The file's modification time and size when it was last read.
    stamp: Option<(SystemTime, u64)>,
    /// The count of messages sent when this version went out.
    pub(crate) sent_at: u64,
    /// The `didSave` of this version; the next change clears it.
    pub(crate) saved: Option<Saved>,
}

/// The language id LSP names a file's language by, from its extension.
pub fn language_id(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
    {
        "py" | "pyi" => "python",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "typescriptreact",
        "js" | "mjs" | "cjs" => "javascript",
        "jsx" => "javascriptreact",
        "rs" => "rust",
        "json" => "json",
        "toml" => "toml",
        _ => "plaintext",
    }
}

/// A file's text and stamp, read now. `Ok(None)`: it does not exist.
async fn read(path: &Path) -> Result<Option<(String, (SystemTime, u64))>, Error> {
    let err = |e: std::io::Error| Error::File {
        path: path.to_path_buf(),
        message: e.to_string(),
    };
    let meta = match tokio::fs::metadata(path).await {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(err(e)),
    };
    let text = tokio::fs::read_to_string(path).await.map_err(err)?;
    let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    Ok(Some((text, (mtime, meta.len()))))
}

async fn stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let m = tokio::fs::metadata(path).await.ok()?;
    Some((m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len()))
}

impl Client {
    /// The document's URI (normalized), or an error for a relative path.
    pub fn uri(&self, path: &Path) -> Result<String, Error> {
        let abs = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.shared().opts.root.join(path)
        };
        uri::from_path(&abs).ok_or_else(|| Error::File {
            path: path.to_path_buf(),
            message: "not a path a file URI can name".into(),
        })
    }

    fn abs(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.shared().opts.root.join(path)
        }
    }

    /// Open a file from disk, or, if it is open, bring it up to date with
    /// the disk. Returns its version.
    pub async fn open(&self, path: &Path) -> Result<i32, Error> {
        let _sync = self.shared().sync.lock().await;
        self.open_locked(path).await
    }

    async fn open_locked(&self, path: &Path) -> Result<i32, Error> {
        let path = self.abs(path);
        let uri = self.uri(&path)?;
        if self.shared().lock().docs.contains_key(&uri) {
            self.resync_locked(&uri).await?;
            return self.version(&uri).ok_or_else(|| Error::File {
                path: path.clone(),
                message: "the file is gone".into(),
            });
        }
        let Some((text, stamp)) = read(&path).await? else {
            return Err(Error::File {
                path,
                message: "no such file".into(),
            });
        };
        let params = json!({ "textDocument": {
            "uri": uri, "languageId": language_id(&path), "version": 1, "text": text,
        }});
        let s = self.shared();
        let sent = s.send(crate::jsonrpc::notification("textDocument/didOpen", params));
        s.lock().docs.insert(
            uri,
            Doc {
                path,
                version: 1,
                text,
                stamp: Some(stamp),
                sent_at: sent,
                saved: None,
            },
        );
        Ok(1)
    }

    fn version(&self, uri: &str) -> Option<i32> {
        self.shared().lock().docs.get(uri).map(|d| d.version)
    }

    /// Whether the file is open, and at which version.
    pub fn open_version(&self, path: &Path) -> Option<i32> {
        self.version(&self.uri(path).ok()?)
    }

    /// Send new text for a document, opening it first if it is not open.
    /// Returns the new version.
    pub async fn change(&self, path: &Path, text: String) -> Result<i32, Error> {
        let _sync = self.shared().sync.lock().await;
        let uri = self.uri(path)?;
        if !self.shared().lock().docs.contains_key(&uri) {
            self.open_locked(path).await?;
        }
        Ok(self.send_change(&uri, text, None))
    }

    /// `didChange` with the whole text, and the next version.
    fn send_change(&self, uri: &str, text: String, stamp: Option<(SystemTime, u64)>) -> i32 {
        let s = self.shared();
        let mut st = s.lock();
        let Some(doc) = st.docs.get_mut(uri) else {
            return 0;
        };
        doc.version += 1;
        let version = doc.version;
        let params = json!({
            "textDocument": { "uri": uri, "version": version },
            "contentChanges": [{ "text": text }],
        });
        doc.text = text;
        doc.saved = None;
        if stamp.is_some() {
            doc.stamp = stamp;
        }
        drop(st);
        let sent = s.send(crate::jsonrpc::notification(
            "textDocument/didChange",
            params,
        ));
        if let Some(doc) = s.lock().docs.get_mut(uri) {
            doc.sent_at = sent;
        }
        version
    }

    /// `didSave`, with the text when the server asked for it. The save is
    /// kept with the document's version, for the wait on its check.
    pub fn save(&self, path: &Path) -> Result<(), Error> {
        let uri = self.uri(path)?;
        let s = self.shared();
        let st = s.lock();
        let Some(doc) = st.docs.get(&uri) else {
            return Ok(());
        };
        let version = doc.version;
        let mut params = json!({ "textDocument": { "uri": uri } });
        if st.caps.save_include_text {
            params["text"] = json!(doc.text);
        }
        drop(st);
        let sent = s.send(crate::jsonrpc::notification("textDocument/didSave", params));
        let saved = Saved {
            sent,
            at: tokio::time::Instant::now(),
        };
        let mut st = s.lock();
        st.last_save = Some(saved);
        if let Some(d) = st.docs.get_mut(&uri).filter(|d| d.version == version) {
            d.saved = Some(saved);
        }
        Ok(())
    }

    /// `didClose`, and forget the document.
    pub fn close(&self, path: &Path) -> Result<(), Error> {
        let uri = self.uri(path)?;
        self.close_uri(&uri);
        Ok(())
    }

    fn close_uri(&self, uri: &str) {
        let s = self.shared();
        let was_open = s.lock().docs.remove(uri).is_some();
        if was_open {
            s.send(crate::jsonrpc::notification(
                "textDocument/didClose",
                json!({ "textDocument": { "uri": uri } }),
            ));
        }
    }

    /// `workspace/didChangeWatchedFiles`.
    pub fn watched_files_changed(
        &self,
        changes: &[(PathBuf, FileChangeType)],
    ) -> Result<(), Error> {
        let mut list = Vec::new();
        for (p, t) in changes {
            list.push(json!({ "uri": self.uri(p)?, "type": t }));
        }
        if !list.is_empty() {
            self.notify(
                "workspace/didChangeWatchedFiles",
                json!({ "changes": list }),
            );
        }
        Ok(())
    }

    /// A file was written (or removed) outside the server's sight: an open
    /// document is sent again and saved, a removed one closed, and the file
    /// watcher is told either way. For a server that checks after a save,
    /// a file not open is opened and saved, so its first edit is checked too.
    pub async fn file_changed(&self, path: &Path) -> Result<(), Error> {
        let _sync = self.shared().sync.lock().await;
        let path = self.abs(path);
        let uri = self.uri(&path)?;
        let open = self.shared().lock().docs.contains_key(&uri);
        let exists = stamp(&path).await.is_some();
        let kind = match (open, exists) {
            (_, false) => FileChangeType::Deleted,
            (true, true) => FileChangeType::Changed,
            (false, true) => FileChangeType::Created,
        };
        if open {
            self.resync_locked(&uri).await?;
            if exists {
                self.save(&path)?;
            }
        } else if exists && self.shared().opts.check_token.is_some() {
            self.open_locked(&path).await?;
            self.save(&path)?;
        }
        // A file this client has not seen may still be new to the server:
        // `Created` and `Changed` are both "read it again".
        self.watched_files_changed(&[(path, kind)])
    }

    /// Send again every open document whose file changed on disk (its
    /// modification time or size), and close any whose file is gone.
    /// Returns how many it sent or closed.
    pub async fn sync_disk(&self) -> Result<usize, Error> {
        let _sync = self.shared().sync.lock().await;
        let uris: Vec<String> = self.shared().lock().docs.keys().cloned().collect();
        let mut n = 0;
        for uri in uris {
            if self.resync_locked(&uri).await? {
                n += 1;
            }
        }
        Ok(n)
    }

    /// Bring one open document up to date with its file. Holds the sync
    /// lock (its caller does).
    async fn resync_locked(&self, uri: &str) -> Result<bool, Error> {
        let Some((path, old)) = self
            .shared()
            .lock()
            .docs
            .get(uri)
            .map(|d| (d.path.clone(), d.stamp))
        else {
            return Ok(false);
        };
        let now = stamp(&path).await;
        if now.is_some() && now == old {
            return Ok(false);
        }
        match read(&path).await? {
            None => {
                self.close_uri(uri);
                Ok(true)
            }
            Some((text, stamp)) => {
                let same = self
                    .shared()
                    .lock()
                    .docs
                    .get(uri)
                    .is_some_and(|d| d.text == text);
                if same {
                    if let Some(d) = self.shared().lock().docs.get_mut(uri) {
                        d.stamp = Some(stamp);
                    }
                    return Ok(false);
                }
                self.send_change(uri, text, Some(stamp));
                Ok(true)
            }
        }
    }

    /// Open the file if it is not open, and sync every open document with
    /// the disk: what a request about a document does first. Returns the
    /// document's URI.
    pub(crate) async fn prepare(&self, path: &Path) -> Result<String, Error> {
        let uri = self.uri(path)?;
        self.sync_disk().await?;
        if !self.shared().lock().docs.contains_key(&uri) {
            self.open(path).await?;
        }
        Ok(uri)
    }
}
