//! From an operation and its typed input to an unsigned request: the
//! protocol's serialization, at the operation's endpoint, with its host
//! prefix, S3's virtual hosting, and the checksums the model requires.

use base64::Engine as _;
use md5::{Digest as _, Md5};
use theseus_aws_catalog::{Auth, Catalog, CatalogError, OperationRef, Protocol, Signature};

use crate::error::CallError;
use crate::request::HttpRequest;
use crate::sign::SigningScope;
use crate::value::V;
use crate::{json, query, rest};

/// A request ready to sign, and how to sign it.
#[derive(Clone, Debug)]
pub(crate) struct Built {
    pub(crate) req: HttpRequest,
    pub(crate) scope: SigningScope,
    /// `false` for the few operations that go unsigned (Cognito's sign-in,
    /// STS's web identity).
    pub(crate) signed: bool,
}

/// Why the client cannot make a call that the catalog describes; the CLI
/// (`proc.run aws …`) can.
pub(crate) fn unsupported(op: OperationRef<'_>) -> Option<String> {
    let svc = op.service();
    match svc.signature() {
        Signature::V2 => return Some(format!("{} signs with SigV2; use the CLI", svc.name())),
        Signature::Bearer => {
            return Some(format!(
                "{} authenticates with a bearer token; use the CLI",
                svc.name()
            ))
        }
        Signature::V4 | Signature::S3V4 => {}
    }
    if op.auth() == Auth::Bearer {
        return Some(format!(
            "{}:{} authenticates with a bearer token; use the CLI",
            svc.name(),
            op.name()
        ));
    }
    if op.has_event_stream() {
        return Some(format!(
            "{}:{} streams events; use the CLI",
            svc.name(),
            op.name()
        ));
    }
    if op.endpoint_discovery() == Some(true) {
        return Some(format!(
            "{}:{} needs endpoint discovery; use the CLI",
            svc.name(),
            op.name()
        ));
    }
    if !svc.protocol().is_supported() {
        return Some(format!(
            "{} speaks only {}",
            svc.name(),
            svc.protocol().as_str()
        ));
    }
    None
}

/// A host label's value: one DNS label, so it cannot redirect the request.
fn host_label(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 63
        && v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && !v.starts_with('-')
}

/// Fills the operation's host prefix (`{AccountId}.`) from its host labels.
fn host_prefix(op: OperationRef<'_>, input: Option<&V<'_>>) -> Result<Option<String>, CallError> {
    let Some(template) = op.host_prefix() else {
        return Ok(None);
    };
    let mut out = String::new();
    let mut rest = template;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let end = rest[i..]
            .find('}')
            .map(|e| e + i)
            .ok_or_else(|| CallError::InvalidInput("a malformed host prefix".into()))?;
        let name = &rest[i + 1..end];
        let v = input
            .and_then(|i| i.member(name))
            .and_then(V::as_str)
            .ok_or_else(|| CallError::InvalidInput(format!("missing the host member {name:?}")))?;
        if !host_label(v) {
            return Err(CallError::InvalidInput(format!(
                "{name} must be one DNS label (letters, digits, and dashes), not {v:?}"
            )));
        }
        out.push_str(v);
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    Ok(Some(out))
}

/// A bucket that can be the first label of S3's host: virtual hosting, as
/// the SDKs prefer. A name with dots would break the TLS certificate's
/// wildcard, so it goes path-style.
fn virtual_hostable(bucket: &str) -> bool {
    (3..=63).contains(&bucket.len())
        && bucket
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && bucket.as_bytes()[0].is_ascii_alphanumeric()
        && bucket.as_bytes()[bucket.len() - 1].is_ascii_alphanumeric()
}

/// The checksums the model requires: a flexible one the caller chose (S3's
/// `ChecksumAlgorithm`), or `Content-MD5` for an operation that requires one.
fn checksums(
    op: OperationRef<'_>,
    input: Option<&V<'_>>,
    req: &mut HttpRequest,
) -> Result<(), CallError> {
    let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
    let chosen = op
        .checksum_algorithm_member()
        .and_then(|m| input.and_then(|i| i.member(m)))
        .and_then(V::as_str)
        .map(str::to_ascii_uppercase);
    if let Some(alg) = chosen {
        let (header, value) = match alg.as_str() {
            "CRC32" => (
                "x-amz-checksum-crc32",
                b64(&crc32fast::hash(&req.body).to_be_bytes()),
            ),
            "SHA256" => {
                let d = sha2::Sha256::digest(&req.body);
                ("x-amz-checksum-sha256", b64(&d))
            }
            other => {
                return Err(CallError::Unsupported(format!(
                    "the {other} checksum; use CRC32 or SHA256"
                )))
            }
        };
        if req.header(header).is_none() {
            req.set_header(header, value);
        }
        return Ok(());
    }
    let has_checksum = req.header("content-md5").is_some()
        || req
            .headers
            .iter()
            .any(|(k, _)| k.to_ascii_lowercase().starts_with("x-amz-checksum-"));
    if op.checksum_required() && !has_checksum {
        req.set_header("Content-MD5", b64(&Md5::digest(&req.body)));
    }
    Ok(())
}

fn bad_region(e: CatalogError) -> CallError {
    match e {
        CatalogError::BadRegion(r) => CallError::InvalidInput(format!("{r:?} is not a region")),
        other => CallError::InvalidInput(other.to_string()),
    }
}

/// Builds the unsigned request for one call. `endpoint_override` sends it
/// elsewhere (a test's fake endpoint): path-style S3, and no host prefix.
pub(crate) fn build(
    catalog: &Catalog,
    op: OperationRef<'_>,
    input: Option<&V<'_>>,
    region: &str,
    endpoint_override: Option<&str>,
) -> Result<Built, CallError> {
    if let Some(why) = unsupported(op) {
        return Err(CallError::Unsupported(why));
    }
    let svc = op.service();
    // S3's bucket is a dynamic context parameter of its rule set: the catalog
    // resolved each operation without one. A directory bucket needs S3
    // Express; any other bucket is served by S3's own endpoint, even for the
    // operations whose static parameters send a bucketless call (or a
    // directory bucket's) to the S3 Express control endpoint.
    let bucket = (svc.name() == "s3")
        .then(|| input.and_then(|i| i.member("Bucket")).and_then(V::as_str))
        .flatten();
    if let Some(b) = bucket {
        if b.ends_with("--x-s3") || b.ends_with("--xa-s3") {
            return Err(CallError::Unsupported(
                "S3 directory buckets need S3 Express session auth; use the CLI".into(),
            ));
        }
        if b.starts_with("arn:") {
            return Err(CallError::Unsupported(
                "S3 access point and Outposts ARNs; name the bucket, or use the CLI".into(),
            ));
        }
    }
    let ep = match bucket {
        Some(_) => catalog.endpoint(svc.name(), region),
        None => catalog.operation_endpoint(op, region),
    }
    .map_err(bad_region)?;
    let base = endpoint_override.unwrap_or(&ep.url).trim_end_matches('/');
    // An endpoint may carry a path, which every request's path follows.
    let (origin, base_path) = match base.split_once("://") {
        Some((scheme, rest)) => match rest.find('/') {
            Some(i) => (format!("{scheme}://{}", &rest[..i]), rest[i..].to_owned()),
            None => (base.to_owned(), String::new()),
        },
        None => {
            return Err(CallError::InvalidInput(format!(
                "the endpoint {base:?} has no scheme"
            )))
        }
    };
    let mut req = HttpRequest {
        method: op.method().as_str(),
        origin,
        path: "/".to_owned(),
        query: Vec::new(),
        headers: Vec::new(),
        body: Vec::new(),
    };
    match svc.protocol() {
        Protocol::Query | Protocol::Ec2 => {
            let form = query::form(op, input, svc.protocol() == Protocol::Ec2);
            req.body = crate::request::encode_query(&form).into_bytes();
            req.headers.push((
                "Content-Type".into(),
                "application/x-www-form-urlencoded; charset=utf-8".into(),
            ));
        }
        Protocol::Json => {
            let target = format!("{}.{}", svc.target_prefix().unwrap_or_default(), op.name());
            let version = svc.json_version().unwrap_or("1.0");
            req.headers.push(("X-Amz-Target".into(), target));
            req.headers.push((
                "Content-Type".into(),
                format!("application/x-amz-json-{version}"),
            ));
            if svc.query_compatible() {
                req.headers
                    .push(("x-amzn-query-mode".into(), "true".into()));
            }
            req.body = match input {
                Some(V::Struct(ms)) => {
                    serde_json::to_vec(&json::object(ms, false)).unwrap_or_default()
                }
                _ => b"{}".to_vec(),
            };
        }
        Protocol::RestJson | Protocol::RestXml => {
            let parts = rest::serialize(op, input, svc.protocol() == Protocol::RestXml)
                .map_err(CallError::InvalidInput)?;
            req.path = parts.path;
            req.query = parts.query;
            req.headers = parts.headers;
            req.body = parts.body;
        }
        Protocol::RpcV2Cbor => {
            return Err(CallError::Unsupported("the CBOR protocol".into()));
        }
    }
    if !base_path.is_empty() {
        req.path = format!("{base_path}{}", req.path);
    }
    if endpoint_override.is_none() {
        if let Some(prefix) = host_prefix(op, input)? {
            let (scheme, host) = req.origin.split_once("://").unwrap_or(("https", ""));
            req.origin = format!("{scheme}://{prefix}{host}");
        }
    }
    let s3 = svc.signature() == Signature::S3V4;
    if op.request_uri().starts_with("/{Bucket}") {
        if let Some(bucket) = bucket {
            if endpoint_override.is_none() && virtual_hostable(bucket) {
                // `/{bucket}/key` at `s3.region…` is `/key` at `{bucket}.s3.region…`.
                let prefix = format!("{base_path}/{bucket}");
                if let Some(rest) = req.path.strip_prefix(&prefix) {
                    if rest.is_empty() || rest.starts_with('/') || rest.starts_with('?') {
                        let rest = if rest.is_empty() { "/" } else { rest };
                        req.path = format!("{base_path}{rest}");
                        let (scheme, host) = req.origin.split_once("://").unwrap_or(("https", ""));
                        req.origin = format!("{scheme}://{bucket}.{host}");
                    }
                }
            }
        }
    }
    checksums(op, input, &mut req)?;
    let unsigned_payload = op.unsigned_payload();
    Ok(Built {
        scope: SigningScope {
            region: ep.signing_region,
            service: ep.signing_name,
            s3,
            unsigned_payload,
        },
        signed: op.auth() != Auth::None,
        req,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_labels_cannot_redirect() {
        assert!(host_label("123456789012"));
        assert!(host_label("my-graph-1"));
        assert!(!host_label("evil.example"));
        assert!(!host_label("a/b"));
        assert!(!host_label(""));
        assert!(!host_label("-x"));
    }

    #[test]
    fn which_buckets_are_virtual_hosted() {
        assert!(virtual_hostable("example-bucket-1"));
        assert!(!virtual_hostable("example.bucket"));
        assert!(!virtual_hostable("Example"));
        assert!(!virtual_hostable("ab"));
        assert!(!virtual_hostable("-bucket"));
    }
}
