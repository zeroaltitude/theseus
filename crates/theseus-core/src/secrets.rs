//! Secrets. All of them live in 1Password and are read through a service
//! account at startup. The only secret this process accepts by any other path
//! is the service-account token itself. Values live in zeroizing memory and
//! are never logged, stored, or written anywhere but the header they belong in.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::process::Stdio;

use anyhow::{bail, Context, Result};
use zeroize::Zeroizing;

pub const TOKEN_ENV: &str = "OP_SERVICE_ACCOUNT_TOKEN";

/// A secret value. Debug and Display never print it.
#[derive(Clone)]
pub struct Secret(Zeroizing<String>);

impl Secret {
    pub fn new(s: String) -> Self {
        Self(Zeroizing::new(s))
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Secret(<{} bytes>)", self.0.len())
    }
}

/// `op://vault/item/field` or `op://vault/item/section/field`, optionally
/// followed by `#label` to select one `label: value` line out of a multi-line
/// note (many hand-written Secure Notes look like `api key value: …`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretRef {
    pub vault: String,
    pub item: String,
    pub path: String,
    pub line_label: Option<String>,
    raw: String,
    op_ref: String,
}

impl SecretRef {
    pub fn parse(s: &str) -> Result<Self> {
        let (op_ref, line_label) = match s.split_once('#') {
            Some((r, l)) if !l.trim().is_empty() => (r, Some(l.trim().to_string())),
            _ => (s, None),
        };
        let rest = op_ref
            .strip_prefix("op://")
            .with_context(|| format!("not an op:// reference: {s:?}"))?;
        let mut parts = rest.splitn(3, '/');
        let vault = parts.next().unwrap_or_default().to_string();
        let item = parts.next().unwrap_or_default().to_string();
        let path = parts.next().unwrap_or_default().to_string();
        if vault.is_empty() || item.is_empty() || path.is_empty() {
            bail!("op:// reference needs vault/item/field: {s:?}");
        }
        Ok(Self {
            vault,
            item,
            path,
            line_label,
            raw: s.to_string(),
            op_ref: op_ref.to_string(),
        })
    }
    /// The full reference as written, including any `#label`.
    pub fn as_str(&self) -> &str {
        &self.raw
    }
    /// What `op read` receives (no fragment).
    pub fn op_ref(&self) -> &str {
        &self.op_ref
    }

    /// Apply the `#label` selection, if any, to a fetched value.
    pub fn select(&self, value: String) -> Result<String> {
        let Some(label) = &self.line_label else {
            return Ok(value);
        };
        let want = label.to_ascii_lowercase();
        for line in value.lines() {
            if let Some((k, v)) = line.split_once(':') {
                if k.trim().to_ascii_lowercase() == want {
                    let v = v.trim();
                    if v.is_empty() {
                        bail!("{}: line {label:?} is empty", self.raw);
                    }
                    return Ok(v.to_string());
                }
            }
        }
        let labels: Vec<String> = value
            .lines()
            .filter_map(|l| l.split_once(':').map(|(k, _)| k.trim().to_string()))
            .collect();
        bail!(
            "{}: no line labelled {label:?} (labels present: {})",
            self.raw,
            labels.join(", ")
        )
    }
}

/// Reads secrets by shelling out to the `op` CLI under the service-account
/// token. 1Password publishes no first-party Rust SDK; the community FFI
/// wrappers are the later candidate for removing this dependency.
#[derive(Clone)]
pub struct OpReader {
    token: Secret,
    op_bin: PathBuf,
}

impl OpReader {
    /// Token from `OP_SERVICE_ACCOUNT_TOKEN`, or from `token_file` if given.
    pub fn from_env(token_file: Option<&str>) -> Result<Self> {
        let token = match std::env::var(TOKEN_ENV) {
            Ok(v) if !v.trim().is_empty() => v.trim().to_string(),
            _ => {
                let path = token_file.with_context(|| {
                    format!("{TOKEN_ENV} is not set and no --op-token-file was given; refusing to start without 1Password access")
                })?;
                let path = crate::config::expand(path);
                let meta = std::fs::metadata(&path)
                    .with_context(|| format!("token file {} not readable", path.display()))?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mode = meta.permissions().mode() & 0o777;
                    if mode & 0o077 != 0 {
                        bail!(
                            "token file {} has mode {:o}; it must not be group- or world-readable",
                            path.display(),
                            mode
                        );
                    }
                }
                let _ = meta;
                std::fs::read_to_string(&path)?.trim().to_string()
            }
        };
        if token.is_empty() {
            bail!("empty 1Password service-account token");
        }
        let op_bin = which("op").context("the `op` CLI (1Password) is not on PATH")?;
        Ok(Self {
            token: Secret::new(token),
            op_bin,
        })
    }

    pub async fn read(&self, r: &SecretRef) -> Result<Secret> {
        let out = tokio::process::Command::new(&self.op_bin)
            .arg("read")
            .arg("--no-newline")
            .arg(r.op_ref())
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", std::env::var("HOME").unwrap_or_default())
            .env(TOKEN_ENV, self.token.expose())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .context("spawning op")?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            // op's stderr describes the failure; it does not echo values.
            bail!("op read {} failed: {}", r.as_str(), err.trim());
        }
        let value = String::from_utf8(out.stdout).context("op returned non-UTF-8")?;
        if value.is_empty() {
            bail!("op read {} returned an empty value", r.as_str());
        }
        Ok(Secret::new(r.select(value)?))
    }
}

/// Resolved secrets by config name. Fail closed: every reference resolves or
/// the process does not start.
#[derive(Clone, Default)]
pub struct Secrets {
    map: BTreeMap<String, Secret>,
}

impl Secrets {
    pub async fn resolve_all(refs: &BTreeMap<String, String>, op: &OpReader) -> Result<Self> {
        let mut map = BTreeMap::new();
        let mut failures = Vec::new();
        // Resolve concurrently: each `op read` is a network round trip.
        let futs = refs.iter().map(|(name, raw)| async move {
            let r = SecretRef::parse(raw)?;
            let v = op
                .read(&r)
                .await
                .map_err(|e| anyhow::anyhow!("{name}: {e}"));
            Ok::<_, anyhow::Error>((name.clone(), v))
        });
        for res in futures_util::future::join_all(futs).await {
            match res? {
                (name, Ok(v)) => {
                    tracing::info!(secret = %name, bytes = v.len(), "resolved");
                    map.insert(name, v);
                }
                (_, Err(e)) => failures.push(e.to_string()),
            }
        }
        if !failures.is_empty() {
            bail!(
                "refusing to start: {} secret(s) failed to resolve:\n  {}",
                failures.len(),
                failures.join("\n  ")
            );
        }
        Ok(Self { map })
    }

    pub fn get(&self, name: &str) -> Option<&Secret> {
        self.map.get(name)
    }
    pub fn names(&self) -> Vec<String> {
        self.map.keys().cloned().collect()
    }
}

impl fmt::Debug for Secrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Secrets")
            .field("names", &self.names())
            .finish()
    }
}

fn which(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(bin))
        .find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_refs_with_spaces() {
        let r = SecretRef::parse("op://Eddie-Tabitha/anthropic openclaw key/notesPlain").unwrap();
        assert_eq!(r.vault, "Eddie-Tabitha");
        assert_eq!(r.item, "anthropic openclaw key");
        assert_eq!(r.path, "notesPlain");
    }

    #[test]
    fn line_label_selects_one_line_of_a_note() {
        let r = SecretRef::parse("op://V/z.ai key/notesPlain#api key value").unwrap();
        assert_eq!(r.op_ref(), "op://V/z.ai key/notesPlain");
        let note = "name: zai\napi key id: abc\nApi Key Value:  id.secret \n".to_string();
        assert_eq!(r.select(note).unwrap(), "id.secret");
        let missing = SecretRef::parse("op://V/i/notesPlain#nope").unwrap();
        let e = missing.select("a: 1\nb: 2".into()).unwrap_err().to_string();
        assert!(e.contains("labels present: a, b"));
        let plain = SecretRef::parse("op://V/i/notesPlain").unwrap();
        assert_eq!(plain.select("raw".into()).unwrap(), "raw");
    }

    #[test]
    fn rejects_short_refs() {
        assert!(SecretRef::parse("op://vault/item").is_err());
        assert!(SecretRef::parse("vault/item/field").is_err());
    }

    #[test]
    fn secret_debug_hides_value() {
        let s = Secret::new("sk-ant-very-secret".into());
        assert!(!format!("{s:?}").contains("secret"));
    }
}
