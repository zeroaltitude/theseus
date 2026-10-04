//! The tender's S3 calls, in its own session: a put with its SHA-256, the
//! multipart upload's four calls, and a head that reads an object's
//! checksum back. Each is one request through the account's client
//! (`Account::request`), unbound to any call: the tender's requests are the
//! core's own, as the budget's reads are.

use std::sync::Arc;

use base64::Engine as _;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::super::session::Kind;
use super::super::{Account, Failure, Request, Signer};
use super::cursor::Part;
use super::TENDER;

/// SHA-256 of `bytes`, raw.
pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// Base64, as S3's checksum headers carry a digest.
pub fn b64(raw: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(raw)
}

/// A multipart object's checksum as S3 states it: the SHA-256 of its parts'
/// SHA-256s, then `-<parts>`.
pub fn composite(parts: &[Part]) -> Option<String> {
    let mut h = Sha256::new();
    for p in parts {
        h.update(
            base64::engine::general_purpose::STANDARD
                .decode(&p.sha256)
                .ok()?,
        );
    }
    Some(format!("{}-{}", b64(&h.finalize()), parts.len()))
}

/// What a request failed with, as the tender acts on it.
#[derive(Debug)]
pub enum S3Error {
    /// No such object, or no such upload.
    Missing,
    Other(String),
}

impl std::fmt::Display for S3Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            S3Error::Missing => f.write_str("not there"),
            S3Error::Other(s) => f.write_str(s),
        }
    }
}

impl From<S3Error> for String {
    fn from(e: S3Error) -> String {
        e.to_string()
    }
}

/// The bucket, as the tender calls it.
pub struct Bucket {
    pub account: Arc<Account>,
    pub name: String,
    pub region: String,
}

impl Bucket {
    async fn call(&self, operation: &'static str, input: Value) -> Result<Value, S3Error> {
        let r = Request {
            service: "s3",
            operation,
            input: &input,
            region: &self.region,
            pages: 1,
            class: "write",
            signer: Signer::As(Kind::Tender(TENDER)),
        };
        match self.account.request(None, &r).await {
            Ok(out) => Ok(out.body),
            Err(Failure::Call(e))
                if e.aws().is_some_and(|a| {
                    a.status == 404 || matches!(a.code.as_str(), "NoSuchUpload" | "NoSuchKey")
                }) =>
            {
                Err(S3Error::Missing)
            }
            Err(f) => Err(S3Error::Other(format!("s3 {operation}: {f}"))),
        }
    }

    /// One object, whole, with its SHA-256 (base64): S3 refuses it if the
    /// bytes it received do not match.
    pub async fn put(&self, key: &str, bytes: &[u8], sha: &str) -> Result<(), S3Error> {
        self.call(
            "PutObject",
            json!({
                "Bucket": self.name, "Key": key,
                "Body": {"base64": b64(bytes)},
                "ChecksumAlgorithm": "SHA256", "ChecksumSHA256": sha,
                "ContentType": "application/octet-stream",
            }),
        )
        .await
        .map(drop)
    }

    /// The object's checksum as S3 holds it (a single put's SHA-256, or a
    /// multipart's composite); None when there is no such object.
    pub async fn checksum(&self, key: &str) -> Result<Option<String>, S3Error> {
        match self
            .call(
                "HeadObject",
                json!({"Bucket": self.name, "Key": key, "ChecksumMode": "ENABLED"}),
            )
            .await
        {
            Ok(b) => Ok(b["ChecksumSHA256"].as_str().map(String::from)),
            Err(S3Error::Missing) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Begin a multipart upload, its parts checked by SHA-256: its id.
    pub async fn create_upload(&self, key: &str) -> Result<String, S3Error> {
        let b = self
            .call(
                "CreateMultipartUpload",
                json!({
                    "Bucket": self.name, "Key": key,
                    "ChecksumAlgorithm": "SHA256",
                    "ContentType": "application/octet-stream",
                }),
            )
            .await?;
        b["UploadId"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| S3Error::Other("CreateMultipartUpload named no UploadId".into()))
    }

    /// One part: its ETag.
    pub async fn upload_part(
        &self,
        key: &str,
        id: &str,
        n: u32,
        bytes: &[u8],
        sha: &str,
    ) -> Result<String, S3Error> {
        let b = self
            .call(
                "UploadPart",
                json!({
                    "Bucket": self.name, "Key": key, "UploadId": id, "PartNumber": n,
                    "Body": {"base64": b64(bytes)},
                    "ChecksumAlgorithm": "SHA256", "ChecksumSHA256": sha,
                }),
            )
            .await?;
        b["ETag"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| S3Error::Other(format!("UploadPart {n} named no ETag")))
    }

    /// The parts S3 holds of an upload, in order: (number, ETag, checksum).
    pub async fn list_parts(
        &self,
        key: &str,
        id: &str,
    ) -> Result<Vec<(u32, String, Option<String>)>, S3Error> {
        let b = self
            .call(
                "ListParts",
                json!({"Bucket": self.name, "Key": key, "UploadId": id}),
            )
            .await?;
        let mut parts: Vec<_> = b["Parts"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| {
                Some((
                    u32::try_from(p["PartNumber"].as_u64()?).ok()?,
                    p["ETag"].as_str()?.to_string(),
                    p["ChecksumSHA256"].as_str().map(String::from),
                ))
            })
            .collect();
        parts.sort_by_key(|p| p.0);
        Ok(parts)
    }

    /// Finish an upload from its parts.
    pub async fn complete(&self, key: &str, id: &str, parts: &[Part]) -> Result<(), S3Error> {
        let parts: Vec<Value> = parts
            .iter()
            .map(|p| json!({"PartNumber": p.n, "ETag": p.etag, "ChecksumSHA256": p.sha256}))
            .collect();
        self.call(
            "CompleteMultipartUpload",
            json!({
                "Bucket": self.name, "Key": key, "UploadId": id,
                "MultipartUpload": {"Parts": parts},
            }),
        )
        .await
        .map(drop)
    }
}
