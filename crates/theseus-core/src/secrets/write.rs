//! Writing a secret back (the Google design, §2.2: the minting step): the
//! refresh token `theseus google auth` earns goes where `[secrets]` points.
//!
//! - `op://` is minted through the same `op` and token the reads use: `op item
//!   create` when the item is not there, `op item edit` when it is, the item
//!   JSON on **stdin**, so the value is never on argv (where `/proc` shows it)
//!   and never in a temp file. It is read back with `op read` and compared by
//!   sha256 before anything is called a success.
//! - `file:` is an atomic write at mode 0600, read back the same way.
//! - `env:` is refused: a process's environment cannot be written.
//!
//! A refused write is an error with the reason; the caller holds the value in
//! memory only and says so.

use std::io::Write as _;

use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use super::{op_error, OpReader, Secret, SecretRef, Source, OP_TIMEOUT};

/// Where a written value went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kept {
    Op,
    File,
}

impl Kept {
    pub fn as_str(self) -> &'static str {
        match self {
            Kept::Op => "op",
            Kept::File => "file",
        }
    }
}

pub const ENV_REFUSED: &str = "an env ref can't be written; point it at op:// or file:";

fn digest(s: &str) -> [u8; 32] {
    Sha256::digest(s.as_bytes()).into()
}

/// `reference`'s kind, for the places that must not call a write by its name.
pub fn kind_of(reference: &str) -> &'static str {
    match Source::of(reference) {
        Source::Vault => "op",
        Source::File => "file",
        Source::Env => "env",
    }
}

impl OpReader {
    /// `op` with `args`, `stdin` piped to it. Its stdout, or why it failed
    /// (op's own words, which describe the failure and never echo a value).
    async fn run(&self, args: &[&str], stdin: Option<&str>) -> Result<Vec<u8>, String> {
        if let Some(why) = &self.absent {
            return Err(format!("no 1Password access: {why}"));
        }
        let mut cmd = self.op();
        cmd.args(args);
        if stdin.is_some() {
            cmd.stdin(std::process::Stdio::piped());
        }
        let mut child = Self::start(&mut cmd).map_err(|e| format!("spawning op: {e}"))?;
        let run = async {
            if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
                match pipe.write_all(text.as_bytes()).await {
                    Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {}
                    r => r?,
                }
                drop(pipe);
            }
            child.wait_with_output().await
        };
        let out = match tokio::time::timeout(OP_TIMEOUT, run).await {
            Err(_) => {
                return Err(format!(
                    "op did not answer within {} s",
                    OP_TIMEOUT.as_secs()
                ))
            }
            Ok(Err(e)) => return Err(format!("running op: {e}")),
            Ok(Ok(o)) => o,
        };
        if out.status.success() {
            Ok(out.stdout)
        } else {
            Err(op_error(&out.stderr))
        }
    }

    /// Whether `item` is in `vault`.
    async fn has_item(&self, vault: &str, item: &str) -> Result<bool, String> {
        match self
            .run(
                &["item", "get", item, "--vault", vault, "--format", "json"],
                None,
            )
            .await
        {
            Ok(_) => Ok(true),
            Err(e)
                if e.contains("isn't an item")
                    || e.contains("not found")
                    || e.contains("No item") =>
            {
                Ok(false)
            }
            Err(e) => Err(e),
        }
    }

    /// `value` into the one concealed field `reference` names, minted as the
    /// module says. Read back and compared before it returns.
    pub async fn write_field(&self, reference: &str, value: &Secret) -> Result<(), String> {
        let r = SecretRef::parse(reference).map_err(|e| format!("{e:#}"))?;
        if r.line_label.is_some() {
            return Err("a written secret names a field, not a line of a note".into());
        }
        let field = r.path.rsplit('/').next().unwrap_or(&r.path);
        let exists = self.has_item(&r.vault, &r.item).await?;
        self.put(&r.vault, &r.item, field, value.expose(), exists)
            .await?;
        let back = self.read_raw(r.op_ref()).await?;
        if digest(&back) != digest(value.expose()) {
            return Err("the stored value did not read back as written".into());
        }
        Ok(())
    }

    async fn put(
        &self,
        vault: &str,
        item: &str,
        field: &str,
        value: &str,
        exists: bool,
    ) -> Result<(), String> {
        let concealed =
            serde_json::json!({"id": field, "label": field, "type": "CONCEALED", "value": value});
        if exists {
            let body = serde_json::json!({"fields": [concealed]}).to_string();
            self.run(
                &["item", "edit", item, "--vault", vault, "--format", "json"],
                Some(&body),
            )
            .await?;
        } else {
            let body = serde_json::json!({"title": item, "category": "API_CREDENTIAL", "fields": [concealed]})
                .to_string();
            // `-`: op reads a template from stdin only when told (op 2.x).
            self.run(
                &["item", "create", "-", "--vault", vault, "--format", "json"],
                Some(&body),
            )
            .await?;
        }
        Ok(())
    }

    /// The field emptied (a revoke clears what it stored).
    pub async fn clear_field(&self, reference: &str) -> Result<(), String> {
        let r = SecretRef::parse(reference).map_err(|e| format!("{e:#}"))?;
        let field = r.path.rsplit('/').next().unwrap_or(&r.path);
        if !self.has_item(&r.vault, &r.item).await? {
            return Ok(());
        }
        self.put(&r.vault, &r.item, field, "", true).await
    }
}

/// `value` into the `file:` entry's path, atomically at mode 0600, and read back.
pub fn write_file(reference: &str, value: &Secret) -> Result<(), String> {
    let path = crate::config::expand(
        reference
            .strip_prefix("file:")
            .ok_or("not a file: reference")?,
    );
    let dir = path.parent().ok_or("the path has no directory")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let tmp = dir.join(format!(".{}.new", uuid::Uuid::now_v7().simple()));
    let written = (|| -> std::io::Result<()> {
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(value.expose().as_bytes())?;
        f.write_all(b"\n")?;
        f.sync_all()?;
        std::fs::rename(&tmp, &path)
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("{}: {e}", path.display()));
    }
    match super::local_value(reference) {
        Some(Ok(back)) if digest(back.expose()) == digest(value.expose()) => Ok(()),
        Some(Ok(_)) => Err("the stored value did not read back as written".into()),
        Some(Err(e)) => Err(e),
        None => Err("not a local reference".into()),
    }
}

/// A `file:` entry's file removed (a revoke); none is not an error.
pub fn clear_file(reference: &str) -> Result<(), String> {
    let path = crate::config::expand(
        reference
            .strip_prefix("file:")
            .ok_or("not a file: reference")?,
    );
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

impl OpReader {
    /// A reader that runs `op_bin` under `token`: a fake `op` script in a test.
    #[cfg(test)]
    pub(crate) fn with_bin(token: &str, op_bin: std::path::PathBuf) -> Self {
        Self {
            token: Secret::new(token.to_string()),
            op_bin,
            token_file: None,
            absent: None,
        }
    }
}

/// Write `value` where `reference` points, by its kind: the caller's one call.
pub async fn store(reader: &OpReader, reference: &str, value: &Secret) -> Result<Kept, String> {
    match Source::of(reference) {
        Source::Env => Err(ENV_REFUSED.into()),
        Source::File => write_file(reference, value).map(|()| Kept::File),
        Source::Vault => reader
            .write_field(reference, value)
            .await
            .map(|()| Kept::Op),
    }
}

/// What a revoke clears: the field emptied, or the file deleted.
pub async fn clear(reader: &OpReader, reference: &str) -> Result<Kept, String> {
    match Source::of(reference) {
        Source::Env => Err(ENV_REFUSED.into()),
        Source::File => clear_file(reference).map(|()| Kept::File),
        Source::Vault => reader.clear_field(reference).await.map(|()| Kept::Op),
    }
}
