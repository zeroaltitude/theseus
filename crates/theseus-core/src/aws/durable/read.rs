//! The restore's reads (step 16), beside the tender's writes: `GetObject`
//! with its checksum checked, and `Query` of a deployment's rows over every
//! page, signed in the restore's own session, `theseus-restore`.
//!
//! The session is the owner role narrowed by an inline policy that only
//! reads ([`policy`]): `s3:GetObject` under the deployment's prefix,
//! `s3:ListBucket` on that prefix alone (so a missing object answers 404,
//! not 403), and `dynamodb:Query` of rows whose partition key starts with
//! the deployment. Its name is its own, so CloudTrail names the restore.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{json, Map, Value};
use theseus_aws::Credentials;

use super::super::session::{self, Kind};
use super::super::{Account, Failure, Request, Signer};
use super::s3::{b64, sha256};
use super::{bucket, prefix, TABLE};

/// The restore's name: its session is `theseus-restore`.
pub const RESTORE: &str = "restore";

/// The most bytes one `GetObject` reads: past it, an object is read in
/// ranges (the client holds a response whole, up to 16 MiB).
pub const CHUNK: u64 = 8 << 20;

/// The restore session's inline policy (§3.5): reads of the deployment's
/// objects and rows, and nothing that writes.
pub fn policy(account: &str, region: &str, deployment: &str) -> Value {
    let b = bucket(account, region);
    let p = prefix(deployment);
    json!({
        "Version": "2012-10-17",
        "Statement": [
            {
                "Sid": "ReadItsPrefix",
                "Effect": "Allow",
                "Action": "s3:GetObject",
                "Resource": format!("arn:aws:s3:::{b}/{p}*"),
            },
            {
                "Sid": "ListItsPrefix",
                "Effect": "Allow",
                "Action": "s3:ListBucket",
                "Resource": format!("arn:aws:s3:::{b}"),
                "Condition": {"StringLike": {"s3:prefix": [format!("{p}*")]}},
            },
            {
                "Sid": "ItsIndexRows",
                "Effect": "Allow",
                "Action": "dynamodb:Query",
                "Resource": format!("arn:aws:dynamodb:{region}:{account}:table/{TABLE}"),
                "Condition": {"ForAllValues:StringLike": {
                    "dynamodb:LeadingKeys": [format!("{deployment}#*")],
                }},
            },
        ],
    })
}

/// The restore's session: the owner role, under [`policy`] for
/// `deployment`, named `theseus-restore`. Never the key: an account with no
/// owner role yet has no restore.
pub async fn session(account: &Arc<Account>, deployment: &str) -> Result<Credentials, String> {
    if account.cfg.owner_role.is_none() {
        return Err(format!(
            "AWS account {} has no owner_role, so it has no restore session (the restore reads \
             in a session of its own, never with the key)",
            account.id
        ));
    }
    let p = policy(&account.id, &account.cfg.region, deployment);
    let want = session::Want {
        kind: Kind::Tender(RESTORE),
        name: session::session_name(&format!("theseus-{RESTORE}")),
        execution: None,
        lasts: session::LONGEST,
        inline: Some(&p),
    };
    account.session(&want, None, None).await
}

/// The digest an object must have: a sealed segment's row holds its
/// SHA-256 in hex, a tail's in base64, and a blob's name is its hex.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Digest {
    Hex(String),
    Base64(String),
}

impl Digest {
    fn matches(&self, raw: &[u8; 32]) -> bool {
        match self {
            Digest::Hex(h) => hex::encode(raw) == *h,
            Digest::Base64(b) => b64(raw) == *b,
        }
    }
}

impl std::fmt::Display for Digest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Digest::Hex(h) | Digest::Base64(h) => f.write_str(h),
        }
    }
}

/// Why a read failed.
#[derive(Debug)]
pub enum ReadError {
    /// No such object.
    Missing,
    /// Its bytes are not the ones its row names.
    Checksum(String),
    Other(String),
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::Missing => f.write_str("not there"),
            ReadError::Checksum(s) | ReadError::Other(s) => f.write_str(s),
        }
    }
}

/// A row as the restore reads it: its attributes, a string as a string and
/// a number as a number.
pub type Row = BTreeMap<String, Value>;

/// The bucket and the table, as the restore reads them.
pub struct Reader {
    pub account: Arc<Account>,
    pub bucket: String,
    pub region: String,
    pub creds: Credentials,
    /// The most bytes one `GetObject` reads ([`CHUNK`]; tests make it
    /// small, so objects are read in ranges).
    pub chunk: u64,
    /// Rows asked for in one `Query` page (DynamoDB's own cap is 1 MB);
    /// tests make it small, so the pages are followed.
    pub page: Option<u32>,
}

impl Reader {
    async fn call(&self, service: &str, operation: &str, input: Value) -> Result<Value, ReadError> {
        let r = Request {
            service,
            operation,
            input: &input,
            region: &self.region,
            pages: 1,
            class: "read",
            signer: Signer::With(&self.creds),
        };
        match self.account.request(None, &r).await {
            Ok(out) => Ok(out.body),
            Err(Failure::Call(e))
                if e.aws()
                    .is_some_and(|a| a.status == 404 || a.code == "NoSuchKey") =>
            {
                Err(ReadError::Missing)
            }
            Err(f) => Err(ReadError::Other(format!("{service} {operation}: {f}"))),
        }
    }

    /// An object whole, `len` bytes, checked against `want`: in one
    /// `GetObject` (with S3's own SHA-256 checked too, when it keeps one
    /// for the whole object), or in ranges of `chunk`.
    pub async fn get(&self, key: &str, len: u64, want: &Digest) -> Result<Vec<u8>, ReadError> {
        let mut bytes = Vec::with_capacity(usize::try_from(len).unwrap_or(0));
        if len <= self.chunk {
            let body = self
                .call(
                    "s3",
                    "GetObject",
                    json!({"Bucket": self.bucket, "Key": key, "ChecksumMode": "ENABLED"}),
                )
                .await?;
            bytes = super::super::s3::bytes_of(&body["Body"]);
            if let Some(s3) = body["ChecksumSHA256"].as_str().filter(|c| !c.contains('-')) {
                let got = b64(&sha256(&bytes));
                if got != s3 {
                    return Err(ReadError::Checksum(format!(
                        "{key}: its SHA-256 is {got}, not the {s3} S3 keeps for it"
                    )));
                }
            }
        } else {
            let mut from = 0u64;
            while from < len {
                let to = (from + self.chunk).min(len) - 1;
                let body = self
                    .call(
                        "s3",
                        "GetObject",
                        json!({"Bucket": self.bucket, "Key": key, "Range": format!("bytes={from}-{to}")}),
                    )
                    .await?;
                let part = super::super::s3::bytes_of(&body["Body"]);
                if part.is_empty() {
                    break;
                }
                from += part.len() as u64;
                bytes.extend(part);
            }
        }
        let raw = sha256(&bytes);
        if bytes.len() as u64 != len || !want.matches(&raw) {
            return Err(ReadError::Checksum(format!(
                "{key}: {} bytes whose SHA-256 is {}, not the {len} bytes and {want} its row names",
                bytes.len(),
                match want {
                    Digest::Hex(_) => hex::encode(raw),
                    Digest::Base64(_) => b64(&raw),
                }
            )));
        }
        Ok(bytes)
    }

    /// Every row under the partition key `pk`, over every page, in sort
    /// key order.
    pub async fn query(&self, pk: &str) -> Result<Vec<Row>, String> {
        let mut rows = Vec::new();
        let mut start: Option<Value> = None;
        loop {
            let mut input = json!({
                "TableName": TABLE,
                "KeyConditionExpression": "pk = :pk",
                "ExpressionAttributeValues": {":pk": {"S": pk}},
                "ConsistentRead": true,
            });
            if let Some(n) = self.page {
                input["Limit"] = json!(n);
            }
            if let Some(s) = start.take() {
                input["ExclusiveStartKey"] = s;
            }
            let body = self
                .call("dynamodb", "Query", input)
                .await
                .map_err(|e| e.to_string())?;
            for item in body["Items"].as_array().into_iter().flatten() {
                if let Value::Object(m) = item {
                    rows.push(plain(m));
                }
            }
            match body.get("LastEvaluatedKey") {
                Some(k @ Value::Object(m)) if !m.is_empty() => start = Some(k.clone()),
                _ => break,
            }
        }
        Ok(rows)
    }
}

/// An item's attributes as plain values: `S` a string, `N` a number (an
/// unsigned integer, as the tender writes them), `BOOL` a bool.
fn plain(item: &Map<String, Value>) -> Row {
    item.iter()
        .filter_map(|(k, v)| {
            let v = if let Some(s) = v["S"].as_str() {
                json!(s)
            } else if let Some(n) = v["N"].as_str() {
                json!(n.parse::<u64>().ok()?)
            } else {
                json!(v["BOOL"].as_bool()?)
            };
            Some((k.clone(), v))
        })
        .collect()
}
