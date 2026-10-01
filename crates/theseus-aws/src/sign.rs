//! SigV4, with AWS's own signer (`aws-sigv4`, pure Rust): a request's
//! `Authorization` header, or a presigned URL (AWS design §3.1, "Signing").

use std::time::{Duration, SystemTime};

use aws_sigv4::http_request::{
    sign, PayloadChecksumKind, PercentEncodingMode, SessionTokenMode, SignableBody,
    SignableRequest, SignatureLocation, SigningSettings, UriPathNormalizationMode,
};
use aws_sigv4::sign::v4;

use crate::creds::Credentials;
use crate::request::HttpRequest;

/// What a signature covers: the region and service it is scoped to, and
/// whether S3's rules apply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SigningScope {
    pub region: String,
    /// The signing name (`s3`, `execute-api`, `sagemaker`).
    pub service: String,
    /// S3's rules: the path is encoded once and never normalized, and the
    /// body's hash travels in `x-amz-content-sha256`.
    pub s3: bool,
    /// Sign `UNSIGNED-PAYLOAD` instead of the body's hash.
    pub unsigned_payload: bool,
}

/// The canonical request's rules, which the scope decides for the client
/// and the SigV4 test suite sets case by case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SigningRules {
    /// Encode the (already encoded) path once more, as every service but S3
    /// expects.
    pub double_encode: bool,
    /// Remove `.` and `..` segments and repeated slashes from the path.
    pub normalize_path: bool,
    /// Send the payload's hash in `x-amz-content-sha256`.
    pub content_sha256: bool,
    /// Sign the session token (`false` adds it after signing, as a few
    /// services require).
    pub sign_session_token: bool,
}

impl SigningRules {
    pub fn for_scope(scope: &SigningScope) -> SigningRules {
        SigningRules {
            double_encode: !scope.s3,
            normalize_path: !scope.s3,
            content_sha256: scope.s3 || scope.unsigned_payload,
            sign_session_token: true,
        }
    }

    fn settings(self) -> SigningSettings {
        let mut s = SigningSettings::default();
        s.percent_encoding_mode = if self.double_encode {
            PercentEncodingMode::Double
        } else {
            PercentEncodingMode::Single
        };
        s.uri_path_normalization_mode = if self.normalize_path {
            UriPathNormalizationMode::Enabled
        } else {
            UriPathNormalizationMode::Disabled
        };
        s.payload_checksum_kind = if self.content_sha256 {
            PayloadChecksumKind::XAmzSha256
        } else {
            PayloadChecksumKind::NoHeader
        };
        s.session_token_mode = if self.sign_session_token {
            SessionTokenMode::Include
        } else {
            SessionTokenMode::Exclude
        };
        s
    }
}

/// What signing adds: headers, or the query parameters of a presigned URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signed {
    pub headers: Vec<(String, String)>,
    pub params: Vec<(String, String)>,
    pub signature: String,
}

/// Signs one request's parts. `body` of `None` signs `UNSIGNED-PAYLOAD`;
/// `presign` puts the signature in the query, valid for that long. The URL
/// must already be encoded. Errors name the request, never the credentials.
#[allow(clippy::too_many_arguments)]
pub fn sign_parts(
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: Option<&[u8]>,
    creds: &Credentials,
    region: &str,
    service: &str,
    rules: SigningRules,
    time: SystemTime,
    presign: Option<Duration>,
) -> Result<Signed, String> {
    let identity = creds.identity();
    let mut settings = rules.settings();
    if let Some(expires) = presign {
        settings.signature_location = SignatureLocation::QueryParams;
        settings.expires_in = Some(expires);
    }
    let params = v4::SigningParams::builder()
        .identity(&identity)
        .region(region)
        .name(service)
        .time(time)
        .settings(settings)
        .build()
        .map_err(|e| format!("signing: {e}"))?
        .into();
    let body = match body {
        Some(b) => SignableBody::Bytes(b),
        None => SignableBody::UnsignedPayload,
    };
    let signable = SignableRequest::new(
        method,
        url,
        headers.iter().map(|(k, v)| (k.as_str(), v.as_str())),
        body,
    )
    .map_err(|e| format!("signing: {e}"))?;
    let (instructions, signature) = sign(signable, &params)
        .map_err(|e| format!("signing: {e}"))?
        .into_parts();
    let (headers, params) = instructions.into_parts();
    Ok(Signed {
        headers: headers
            .iter()
            .map(|h| (h.name().to_owned(), h.value().to_owned()))
            .collect(),
        params: params
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.into_owned()))
            .collect(),
        signature,
    })
}

/// Signs a request in place, adding `x-amz-date` and `authorization` (and,
/// as the rules say, `x-amz-content-sha256` and `x-amz-security-token`).
pub fn sign_request(
    req: &mut HttpRequest,
    creds: &Credentials,
    scope: &SigningScope,
    time: SystemTime,
) -> Result<String, String> {
    let body = (!scope.unsigned_payload).then_some(req.body.as_slice());
    let signed = sign_parts(
        req.method,
        &req.url(),
        &req.headers,
        body,
        creds,
        &scope.region,
        &scope.service,
        SigningRules::for_scope(scope),
        time,
        None,
    )?;
    for (k, v) in signed.headers {
        req.set_header(&k, v);
    }
    Ok(signed.signature)
}

/// The longest a presigned URL may live: SigV4's limit.
pub const MAX_PRESIGN: Duration = Duration::from_secs(7 * 24 * 3600);

/// A presigned URL for the request, valid for `expires` (at most seven days,
/// and no longer than the credentials' session). S3's payload is unsigned,
/// as its presigned URLs always are.
pub fn presign_request(
    req: &HttpRequest,
    creds: &Credentials,
    scope: &SigningScope,
    time: SystemTime,
    expires: Duration,
) -> Result<String, String> {
    if expires.is_zero() || expires > MAX_PRESIGN {
        return Err(format!(
            "a presigned URL lives between 1 second and 7 days, not {}s",
            expires.as_secs()
        ));
    }
    let mut rules = SigningRules::for_scope(scope);
    rules.content_sha256 = false;
    let body = (!(scope.s3 || scope.unsigned_payload)).then_some(req.body.as_slice());
    let signed = sign_parts(
        req.method,
        &req.url(),
        &req.headers,
        body,
        creds,
        &scope.region,
        &scope.service,
        rules,
        time,
        Some(expires),
    )?;
    let mut out = req.clone();
    out.query
        .extend(signed.params.into_iter().map(|(k, v)| (k, Some(v))));
    Ok(out.url())
}
