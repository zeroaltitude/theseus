//! Reading an answer: a success through the operation's output shape, or
//! AWS's error, per protocol, as botocore reads them.

use serde_json::{json, Map, Value};
use theseus_aws_catalog::{OperationRef, Protocol};

use crate::error::{parse_denial, AwsError, ErrorRetry};
use crate::json as js;
use crate::rest;
use crate::xml;

/// What came back, before it is read.
#[derive(Clone, Debug)]
pub(crate) struct RawResponse {
    pub(crate) status: u16,
    /// Names in lowercase.
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Vec<u8>,
}

impl RawResponse {
    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// AWS's request id from the headers, where all but the query protocols
    /// put it.
    pub(crate) fn request_id(&self) -> Option<String> {
        ["x-amzn-requestid", "x-amz-request-id", "x-amzn-request-id"]
            .iter()
            .find_map(|h| self.header(h))
            .map(str::to_owned)
    }
}

pub(crate) enum Answer {
    Ok {
        body: Value,
        request_id: Option<String>,
    },
    Err(AwsError),
    /// A success that does not read as the model says.
    Unreadable(String),
}

pub(crate) fn read(op: OperationRef<'_>, resp: &RawResponse) -> Answer {
    if !(200..300).contains(&resp.status) || s3_error_in_success(op, resp) {
        return Answer::Err(error(op, resp));
    }
    match success(op, resp) {
        Ok((body, id)) => Answer::Ok {
            body,
            request_id: resp.request_id().or(id),
        },
        Err(e) => Answer::Unreadable(e),
    }
}

/// The first element's name, without reading the document.
fn root_tag(body: &[u8]) -> Option<&str> {
    let mut rest = body;
    loop {
        let i = rest.iter().position(|&b| b == b'<')?;
        rest = &rest[i + 1..];
        if matches!(rest.first(), Some(b'?' | b'!')) {
            continue;
        }
        let end = rest
            .iter()
            .position(|&b| b == b'>' || b == b'/' || b.is_ascii_whitespace())?;
        return std::str::from_utf8(&rest[..end]).ok();
    }
}

/// S3 can answer 200 with an error in the body (`CopyObject`,
/// `CompleteMultipartUpload`, `UploadPartCopy`) when the output is not a
/// stream.
fn s3_error_in_success(op: OperationRef<'_>, resp: &RawResponse) -> bool {
    op.service().name() == "s3"
        && !op.streams_output()
        && root_tag(&resp.body).is_some_and(|t| t == "Error")
}

fn blank(body: &[u8]) -> bool {
    body.iter().all(u8::is_ascii_whitespace)
}

fn success(op: OperationRef<'_>, resp: &RawResponse) -> Result<(Value, Option<String>), String> {
    let protocol = op.service().protocol();
    match protocol {
        Protocol::Query | Protocol::Ec2 if blank(&resp.body) => Ok((json!({}), None)),
        Protocol::Query => xml::read_query(&resp.body, op.output(), op.result_wrapper()),
        Protocol::Ec2 => xml::read_ec2(&resp.body, op.output()),
        Protocol::Json => {
            let Some(shape) = op.output() else {
                return Ok((json!({}), None));
            };
            if blank(&resp.body) {
                return Ok((json!({}), None));
            }
            let j: Value = serde_json::from_slice(&resp.body).map_err(|e| e.to_string())?;
            Ok((js::from_json(shape, &j, false), None))
        }
        Protocol::RestJson => {
            rest::parse(op, resp.status, &resp.headers, &resp.body, false).map(|v| (v, None))
        }
        Protocol::RestXml => {
            if op.service().name() == "s3" && op.name() == "GetBucketLocation" {
                // `<LocationConstraint>us-west-2</LocationConstraint>`, empty
                // for us-east-1: the root is the member.
                let region = xml::root(&resp.body)
                    .map(|(_, t)| t)
                    .filter(|t| !t.is_empty());
                return Ok((json!({ "LocationConstraint": region }), None));
            }
            rest::parse(op, resp.status, &resp.headers, &resp.body, true).map(|v| (v, None))
        }
        Protocol::RpcV2Cbor => Err("the CBOR protocol is not supported".into()),
    }
}

/// Codes that mean AWS refused the request to slow the caller down (the
/// SDKs' list): the request did not run.
const THROTTLES: &[&str] = &[
    "Throttling",
    "ThrottlingException",
    "ThrottledException",
    "RequestThrottledException",
    "TooManyRequestsException",
    "ProvisionedThroughputExceededException",
    "TransactionInProgressException",
    "RequestLimitExceeded",
    "BandwidthLimitExceeded",
    "LimitExceededException",
    "RequestThrottled",
    "SlowDown",
    "PriorRequestNotComplete",
    "EC2ThrottledException",
];

/// Codes of a server-side failure, which a retry may fix.
const TRANSIENT: &[&str] = &[
    "RequestTimeout",
    "RequestTimeoutException",
    "InternalError",
    "InternalFailure",
    "InternalServerError",
    "InternalServiceError",
    "ServiceUnavailable",
    "ServiceUnavailableException",
    "IDPCommunicationError",
];

pub(crate) fn retry_of(op: OperationRef<'_>, status: u16, code: &str) -> ErrorRetry {
    if status == 429 || THROTTLES.contains(&code) {
        return ErrorRetry::Throttle;
    }
    // The model's own `retryable` trait on the operation's errors.
    if let Some(e) = op
        .errors()
        .find(|e| e.error_code().unwrap_or_else(|| e.name()) == code || e.name() == code)
    {
        if let Some(throttling) = e.retryable() {
            return if throttling {
                ErrorRetry::Throttle
            } else {
                ErrorRetry::Transient
            };
        }
    }
    if TRANSIENT.contains(&code) || matches!(status, 500 | 502 | 503 | 504) {
        return ErrorRetry::Transient;
    }
    ErrorRetry::No
}

fn reason(status: u16) -> String {
    reqwest::StatusCode::from_u16(status)
        .ok()
        .and_then(|s| s.canonical_reason())
        .unwrap_or("")
        .to_owned()
}

/// A JSON error's code: `com.amazon…#ResourceNotFoundException:http://…` is
/// `ResourceNotFoundException`.
fn clean_code(code: &str) -> String {
    let c = code.split(':').next().unwrap_or(code);
    c.rsplit('#').next().unwrap_or(c).trim().to_owned()
}

/// Reads AWS's error from a failed answer.
pub(crate) fn error(op: OperationRef<'_>, resp: &RawResponse) -> AwsError {
    let svc = op.service();
    let status = resp.status;
    let (code, mut message, body_request_id) = match svc.protocol() {
        Protocol::Json | Protocol::RestJson | Protocol::RpcV2Cbor => {
            let body: Value = if blank(&resp.body) {
                Value::Object(Map::new())
            } else {
                serde_json::from_slice(&resp.body).unwrap_or_else(|_| {
                    let text = String::from_utf8_lossy(&resp.body);
                    json!({ "message": text.chars().take(512).collect::<String>() })
                })
            };
            let field = |k: &str| body.get(k).and_then(Value::as_str);
            let message = field("message")
                .or_else(|| field("Message"))
                .or_else(|| field("errorMessage"))
                .unwrap_or_default()
                .to_owned();
            let query_code = resp
                .header("x-amzn-query-error")
                .and_then(|q| q.split_once(';'))
                .map(|(c, _)| c)
                .filter(|c| !c.is_empty());
            let code = query_code
                .map(str::to_owned)
                .or_else(|| resp.header("x-amzn-errortype").map(clean_code))
                .or_else(|| {
                    field("__type")
                        .or_else(|| field("code"))
                        .or_else(|| field("Code"))
                        .map(clean_code)
                })
                .filter(|c| !c.is_empty())
                .unwrap_or_else(|| status.to_string());
            (code, message, None)
        }
        Protocol::Query | Protocol::Ec2 | Protocol::RestXml => match xml::read_error(&resp.body) {
            Some(f) if f.code.is_some() => {
                let mut message = f.message.unwrap_or_default();
                if let Some(region) = resp
                    .header("x-amz-bucket-region")
                    .map(str::to_owned)
                    .or(f.region)
                {
                    message.push_str(&format!(" (the bucket is in {region})"));
                } else if let Some(endpoint) = f.endpoint {
                    message.push_str(&format!(" (use the endpoint {endpoint})"));
                }
                (f.code.unwrap_or_default(), message, f.request_id)
            }
            _ => {
                // No error body (a HEAD, say): the status says what it can.
                let mut message = reason(status);
                if let Some(region) = resp.header("x-amz-bucket-region") {
                    message.push_str(&format!(" (the bucket is in {region})"));
                }
                (status.to_string(), message, None)
            }
        },
    };
    if message.is_empty() {
        message = reason(status);
    }
    let denial = parse_denial(&message);
    AwsError {
        status,
        retry: retry_of(op, status, &code),
        code,
        message,
        request_id: resp.request_id().or(body_request_id),
        denial,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_tags_skip_the_prolog() {
        assert_eq!(
            root_tag(b"<?xml version=\"1.0\"?>\n<!-- c --><Error><Code>x</Code></Error>"),
            Some("Error")
        );
        assert_eq!(root_tag(b"<CopyObjectResult>"), Some("CopyObjectResult"));
        assert_eq!(root_tag(b"<a/>"), Some("a"));
        assert_eq!(root_tag(b"{}"), None);
    }

    #[test]
    fn json_codes_lose_their_namespace() {
        assert_eq!(
            clean_code("com.amazonaws.dynamodb.v20120810#ResourceNotFoundException"),
            "ResourceNotFoundException"
        );
        assert_eq!(
            clean_code(
                "AccessDeniedException:http://internal.amazon.com/coral/com.amazon.coral.service/"
            ),
            "AccessDeniedException"
        );
    }
}
