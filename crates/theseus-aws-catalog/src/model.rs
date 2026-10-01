//! One service's model, decoded: its shapes, members, and operations, with
//! the traits the client and the gate read. Documentation is never kept.
//!
//! The data lives in a few flat vectors (members, symbols, error shapes) that
//! shapes and operations index by span, so decoding a service is a handful of
//! allocations, not one per member. The public types are borrowed views.

use crate::wire::{Reader, StrTable, Sym, Writer};
use crate::CatalogError;

/// A shape's index within its service.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ShapeId(pub(crate) u32);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Span {
    pub(crate) start: u32,
    pub(crate) len: u32,
}

impl Span {
    fn range(self) -> std::ops::Range<usize> {
        self.start as usize..(self.start + self.len) as usize
    }
}

/// The wire protocols in the models (botocore's names for them).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Protocol {
    /// `awsQuery`: form-encoded requests, XML responses.
    Query,
    /// `ec2Query`: EC2's variant of the query protocol.
    Ec2,
    /// `awsJson1_0` and `awsJson1_1` (the service's `jsonVersion` says which).
    Json,
    /// `restJson1`: HTTP bindings, JSON bodies.
    RestJson,
    /// `restXml`: HTTP bindings, XML bodies (S3, Route 53, CloudFront).
    RestXml,
    /// `rpcv2Cbor`: not implemented by the client yet (AWS design §3.1).
    RpcV2Cbor,
}

impl Protocol {
    pub fn from_botocore(s: &str) -> Option<Protocol> {
        Some(match s {
            "query" => Protocol::Query,
            "ec2" => Protocol::Ec2,
            "json" => Protocol::Json,
            "rest-json" => Protocol::RestJson,
            "rest-xml" => Protocol::RestXml,
            "smithy-rpc-v2-cbor" => Protocol::RpcV2Cbor,
            _ => return None,
        })
    }

    /// botocore's name.
    pub fn as_str(self) -> &'static str {
        match self {
            Protocol::Query => "query",
            Protocol::Ec2 => "ec2",
            Protocol::Json => "json",
            Protocol::RestJson => "rest-json",
            Protocol::RestXml => "rest-xml",
            Protocol::RpcV2Cbor => "smithy-rpc-v2-cbor",
        }
    }

    /// Whether the client speaks it: the six protocols of the design.
    pub fn is_supported(self) -> bool {
        self != Protocol::RpcV2Cbor
    }

    fn code(self) -> u8 {
        match self {
            Protocol::Query => 0,
            Protocol::Ec2 => 1,
            Protocol::Json => 2,
            Protocol::RestJson => 3,
            Protocol::RestXml => 4,
            Protocol::RpcV2Cbor => 5,
        }
    }

    fn from_code(c: u8) -> Result<Protocol, CatalogError> {
        Ok(match c {
            0 => Protocol::Query,
            1 => Protocol::Ec2,
            2 => Protocol::Json,
            3 => Protocol::RestJson,
            4 => Protocol::RestXml,
            5 => Protocol::RpcV2Cbor,
            _ => return Err(CatalogError::Corrupt("protocol")),
        })
    }
}

/// How a service's requests are signed (its model's `signatureVersion`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signature {
    /// SigV4.
    V4,
    /// SigV4 with S3's rules: the payload hash in `x-amz-content-sha256`, and
    /// no double encoding or normalization of the path.
    S3V4,
    /// SigV2 (SimpleDB, Import/Export): not supported.
    V2,
    /// A bearer token (CodeCatalyst): not supported.
    Bearer,
}

impl Signature {
    fn code(self) -> u8 {
        match self {
            Signature::V4 => 0,
            Signature::S3V4 => 1,
            Signature::V2 => 2,
            Signature::Bearer => 3,
        }
    }

    fn from_code(c: u8) -> Result<Signature, CatalogError> {
        Ok(match c {
            0 => Signature::V4,
            1 => Signature::S3V4,
            2 => Signature::V2,
            3 => Signature::Bearer,
            _ => return Err(CatalogError::Corrupt("signature")),
        })
    }
}

/// A shape's type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Structure,
    List,
    Map,
    String,
    Boolean,
    Integer,
    Long,
    Float,
    Double,
    Timestamp,
    Blob,
}

impl Kind {
    pub fn from_botocore(s: &str) -> Option<Kind> {
        Some(match s {
            "structure" => Kind::Structure,
            "list" => Kind::List,
            "map" => Kind::Map,
            "string" => Kind::String,
            "boolean" => Kind::Boolean,
            "integer" => Kind::Integer,
            "long" => Kind::Long,
            "float" => Kind::Float,
            "double" => Kind::Double,
            "timestamp" => Kind::Timestamp,
            "blob" => Kind::Blob,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Structure => "structure",
            Kind::List => "list",
            Kind::Map => "map",
            Kind::String => "string",
            Kind::Boolean => "boolean",
            Kind::Integer => "integer",
            Kind::Long => "long",
            Kind::Float => "float",
            Kind::Double => "double",
            Kind::Timestamp => "timestamp",
            Kind::Blob => "blob",
        }
    }

    const ALL: [Kind; 11] = [
        Kind::Structure,
        Kind::List,
        Kind::Map,
        Kind::String,
        Kind::Boolean,
        Kind::Integer,
        Kind::Long,
        Kind::Float,
        Kind::Double,
        Kind::Timestamp,
        Kind::Blob,
    ];

    fn code(self) -> u8 {
        Kind::ALL.iter().position(|&k| k == self).unwrap_or(0) as u8
    }

    fn from_code(c: u8) -> Result<Kind, CatalogError> {
        Kind::ALL
            .get(usize::from(c))
            .copied()
            .ok_or(CatalogError::Corrupt("shape kind"))
    }
}

/// Where a member travels in an HTTP-bound request or response.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Location {
    Body,
    Uri,
    Querystring,
    Header,
    /// A prefixed map of headers (`x-amz-meta-*`).
    Headers,
    StatusCode,
}

impl Location {
    pub fn from_botocore(s: &str) -> Option<Location> {
        Some(match s {
            "uri" => Location::Uri,
            "querystring" => Location::Querystring,
            "header" => Location::Header,
            "headers" => Location::Headers,
            "statusCode" => Location::StatusCode,
            _ => return None,
        })
    }

    const ALL: [Location; 6] = [
        Location::Body,
        Location::Uri,
        Location::Querystring,
        Location::Header,
        Location::Headers,
        Location::StatusCode,
    ];

    fn code(self) -> u8 {
        Location::ALL.iter().position(|&l| l == self).unwrap_or(0) as u8
    }

    fn from_code(c: u8) -> Result<Location, CatalogError> {
        Location::ALL
            .get(usize::from(c))
            .copied()
            .ok_or(CatalogError::Corrupt("location"))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimestampFormat {
    Iso8601,
    Rfc822,
    UnixTimestamp,
}

impl TimestampFormat {
    pub fn from_botocore(s: &str) -> Option<TimestampFormat> {
        Some(match s {
            "iso8601" => TimestampFormat::Iso8601,
            "rfc822" => TimestampFormat::Rfc822,
            "unixTimestamp" => TimestampFormat::UnixTimestamp,
            _ => return None,
        })
    }

    fn code(self) -> u8 {
        match self {
            TimestampFormat::Iso8601 => 0,
            TimestampFormat::Rfc822 => 1,
            TimestampFormat::UnixTimestamp => 2,
        }
    }

    fn from_code(c: u8) -> Result<TimestampFormat, CatalogError> {
        Ok(match c {
            0 => TimestampFormat::Iso8601,
            1 => TimestampFormat::Rfc822,
            2 => TimestampFormat::UnixTimestamp,
            _ => return Err(CatalogError::Corrupt("timestamp format")),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Get,
    Head,
    Post,
    Put,
    Delete,
    Patch,
}

impl Method {
    pub fn from_botocore(s: &str) -> Option<Method> {
        Some(match s {
            "GET" => Method::Get,
            "HEAD" => Method::Head,
            "POST" => Method::Post,
            "PUT" => Method::Put,
            "DELETE" => Method::Delete,
            "PATCH" => Method::Patch,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Head => "HEAD",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Delete => "DELETE",
            Method::Patch => "PATCH",
        }
    }

    const ALL: [Method; 6] = [
        Method::Get,
        Method::Head,
        Method::Post,
        Method::Put,
        Method::Delete,
        Method::Patch,
    ];

    fn code(self) -> u8 {
        Method::ALL.iter().position(|&m| m == self).unwrap_or(0) as u8
    }

    fn from_code(c: u8) -> Result<Method, CatalogError> {
        Method::ALL
            .get(usize::from(c))
            .copied()
            .ok_or(CatalogError::Corrupt("method"))
    }
}

/// How one operation authenticates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Auth {
    SigV4,
    /// Sent unsigned (Cognito's sign-in calls, STS's web identity).
    None,
    /// SigV4 with `UNSIGNED-PAYLOAD` as the body's hash.
    SigV4UnsignedBody,
    Bearer,
}

impl Auth {
    fn code(self) -> u8 {
        match self {
            Auth::SigV4 => 0,
            Auth::None => 1,
            Auth::SigV4UnsignedBody => 2,
            Auth::Bearer => 3,
        }
    }

    fn from_code(c: u8) -> Result<Auth, CatalogError> {
        Ok(match c {
            0 => Auth::SigV4,
            1 => Auth::None,
            2 => Auth::SigV4UnsignedBody,
            3 => Auth::Bearer,
            _ => return Err(CatalogError::Corrupt("auth")),
        })
    }
}

pub(crate) mod sf {
    //! Shape flags.
    pub(crate) const SENSITIVE: u32 = 1;
    pub(crate) const UNION: u32 = 1 << 1;
    pub(crate) const DOCUMENT: u32 = 1 << 2;
    pub(crate) const EVENT: u32 = 1 << 3;
    pub(crate) const EVENTSTREAM: u32 = 1 << 4;
    pub(crate) const STREAMING: u32 = 1 << 5;
    pub(crate) const EXCEPTION: u32 = 1 << 6;
    pub(crate) const FAULT: u32 = 1 << 7;
    pub(crate) const DEPRECATED: u32 = 1 << 8;
    pub(crate) const REQUIRES_LENGTH: u32 = 1 << 9;
    pub(crate) const WRAPPER: u32 = 1 << 10;
    pub(crate) const FLATTENED: u32 = 1 << 11;
    pub(crate) const SPARSE: u32 = 1 << 12;
    pub(crate) const SENDER_FAULT: u32 = 1 << 13;
    pub(crate) const RETRYABLE: u32 = 1 << 14;
    pub(crate) const THROTTLING: u32 = 1 << 15;
}

pub(crate) mod mf {
    //! Member flags.
    pub(crate) const REQUIRED: u32 = 1;
    pub(crate) const IDEMPOTENCY_TOKEN: u32 = 1 << 1;
    pub(crate) const JSONVALUE: u32 = 1 << 2;
    pub(crate) const HOST_LABEL: u32 = 1 << 3;
    pub(crate) const XML_ATTRIBUTE: u32 = 1 << 4;
    pub(crate) const FLATTENED: u32 = 1 << 5;
    pub(crate) const STREAMING: u32 = 1 << 6;
    pub(crate) const EVENTPAYLOAD: u32 = 1 << 7;
    pub(crate) const DEPRECATED: u32 = 1 << 8;
}

pub(crate) mod of {
    //! Operation flags.
    pub(crate) const READONLY: u32 = 1;
    pub(crate) const IDEMPOTENT: u32 = 1 << 1;
    pub(crate) const DEPRECATED: u32 = 1 << 2;
    pub(crate) const CHECKSUM_REQUIRED: u32 = 1 << 3;
    pub(crate) const UNSIGNED_PAYLOAD: u32 = 1 << 4;
    pub(crate) const ENDPOINT_DISCOVERY: u32 = 1 << 5;
    pub(crate) const ENDPOINT_DISCOVERY_REQUIRED: u32 = 1 << 6;
    pub(crate) const ENDPOINT_OPERATION: u32 = 1 << 7;
    pub(crate) const REQUEST_COMPRESSION: u32 = 1 << 8;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct XmlNs {
    pub(crate) prefix: Option<Sym>,
    pub(crate) uri: Sym,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ShapeData {
    pub(crate) name: Sym,
    pub(crate) kind: Kind,
    pub(crate) flags: u32,
    /// Structure members; a list's one member; a map's key then value.
    pub(crate) members: Span,
    /// The payload member's index within `members`.
    pub(crate) payload: Option<u32>,
    pub(crate) enums: Span,
    pub(crate) min: Option<i64>,
    pub(crate) max: Option<i64>,
    pub(crate) timestamp_format: Option<TimestampFormat>,
    pub(crate) location_name: Option<Sym>,
    pub(crate) xml_namespace: Option<XmlNs>,
    pub(crate) error_code: Option<Sym>,
    pub(crate) error_status: Option<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MemberData {
    pub(crate) name: Sym,
    pub(crate) shape: ShapeId,
    pub(crate) location: Location,
    pub(crate) location_name: Option<Sym>,
    pub(crate) query_name: Option<Sym>,
    pub(crate) xml_namespace: Option<XmlNs>,
    pub(crate) flags: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PaginatorData {
    pub(crate) input_tokens: Span,
    pub(crate) output_tokens: Span,
    pub(crate) result_keys: Span,
    pub(crate) limit_key: Option<Sym>,
    pub(crate) more_results: Option<Sym>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OperationData {
    pub(crate) name: Sym,
    pub(crate) method: Method,
    pub(crate) request_uri: Sym,
    pub(crate) response_code: Option<u16>,
    pub(crate) input: Option<ShapeId>,
    pub(crate) input_location_name: Option<Sym>,
    pub(crate) input_xml_namespace: Option<XmlNs>,
    pub(crate) output: Option<ShapeId>,
    pub(crate) result_wrapper: Option<Sym>,
    /// Into `ServiceData::shape_ids`.
    pub(crate) errors: Span,
    pub(crate) flags: u32,
    pub(crate) auth: Auth,
    pub(crate) host_prefix: Option<Sym>,
    pub(crate) checksum_algorithm_member: Option<Sym>,
    pub(crate) paginator: Option<PaginatorData>,
    /// Which of the service's endpoint variants the operation uses: 0, or
    /// the variant its static context parameters chose (Neptune Analytics'
    /// data plane, ARC's control plane).
    pub(crate) endpoint_variant: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HeaderData {
    pub(crate) name: Sym,
    pub(crate) full_name: Sym,
    pub(crate) service_id: Sym,
    pub(crate) api_version: Sym,
    pub(crate) endpoint_prefix: Sym,
    pub(crate) signing_name: Sym,
    pub(crate) target_prefix: Option<Sym>,
    pub(crate) json_version: Option<Sym>,
    pub(crate) xml_namespace: Option<Sym>,
    pub(crate) global_endpoint: Option<Sym>,
    /// Every protocol the model lists, in its order of preference.
    pub(crate) protocols: Vec<Protocol>,
    /// The first of them the client speaks.
    pub(crate) protocol: Protocol,
    pub(crate) signature: Signature,
    pub(crate) auth: Auth,
    pub(crate) query_compatible: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ServiceData {
    pub(crate) header: HeaderData,
    pub(crate) shapes: Vec<ShapeData>,
    pub(crate) members: Vec<MemberData>,
    pub(crate) syms: Vec<Sym>,
    pub(crate) shape_ids: Vec<ShapeId>,
    /// Sorted by name, for lookup.
    pub(crate) operations: Vec<OperationData>,
}

// ---- encoding ----------------------------------------------------------

fn w_xmlns(w: &mut Writer, ns: Option<XmlNs>) {
    match ns {
        None => w.u8(0),
        Some(ns) => {
            w.u8(1);
            w.opt_sym(ns.prefix);
            w.sym(ns.uri);
        }
    }
}

fn r_xmlns(r: &mut Reader<'_>) -> Result<Option<XmlNs>, CatalogError> {
    Ok(match r.u8()? {
        0 => None,
        _ => Some(XmlNs {
            prefix: r.opt_sym()?,
            uri: r.sym()?,
        }),
    })
}

fn w_opt_u(w: &mut Writer, v: Option<u64>) {
    w.uv(v.map_or(0, |v| v + 1));
}

fn r_opt_u(r: &mut Reader<'_>) -> Result<Option<u64>, CatalogError> {
    Ok(match r.uv()? {
        0 => None,
        v => Some(v - 1),
    })
}

fn w_syms(w: &mut Writer, syms: &[Sym], span: Span) {
    w.len(span.len as usize);
    for &s in &syms[span.range()] {
        w.sym(s);
    }
}

fn r_syms(r: &mut Reader<'_>, syms: &mut Vec<Sym>) -> Result<Span, CatalogError> {
    let n = r.len()?;
    let start = syms.len() as u32;
    for _ in 0..n {
        syms.push(r.sym()?);
    }
    Ok(Span {
        start,
        len: n as u32,
    })
}

// A member's first byte: its location in the low three bits, then which
// optional fields follow.
const M_LOC_NAME: u8 = 1 << 3;
const M_QUERY_NAME: u8 = 1 << 4;
const M_XMLNS: u8 = 1 << 5;
const M_FLAGS: u8 = 1 << 6;

// A shape's presence bits, after its kind and flags.
const S_PAYLOAD: u32 = 1;
const S_ENUMS: u32 = 1 << 1;
const S_MIN: u32 = 1 << 2;
const S_MAX: u32 = 1 << 3;
const S_TS: u32 = 1 << 4;
const S_LOC_NAME: u32 = 1 << 5;
const S_XMLNS: u32 = 1 << 6;
const S_ERROR_CODE: u32 = 1 << 7;
const S_ERROR_STATUS: u32 = 1 << 8;

impl ServiceData {
    pub(crate) fn encode(&self, w: &mut Writer) {
        let h = &self.header;
        w.sym(h.name);
        w.sym(h.full_name);
        w.sym(h.service_id);
        w.sym(h.api_version);
        w.sym(h.endpoint_prefix);
        w.sym(h.signing_name);
        w.opt_sym(h.target_prefix);
        w.opt_sym(h.json_version);
        w.opt_sym(h.xml_namespace);
        w.opt_sym(h.global_endpoint);
        w.len(h.protocols.len());
        for p in &h.protocols {
            w.u8(p.code());
        }
        w.u8(h.protocol.code());
        w.u8(h.signature.code());
        w.u8(h.auth.code());
        w.u8(u8::from(h.query_compatible));

        w.len(self.shapes.len());
        for s in &self.shapes {
            self.encode_shape(w, s);
        }
        w.len(self.operations.len());
        for o in &self.operations {
            self.encode_operation(w, o);
        }
    }

    fn encode_shape(&self, w: &mut Writer, s: &ShapeData) {
        w.sym(s.name);
        w.u8(s.kind.code());
        w.uv(u64::from(s.flags));
        let mut present = 0u32;
        let mut bit = |cond: bool, b: u32| {
            if cond {
                present |= b;
            }
        };
        bit(s.payload.is_some(), S_PAYLOAD);
        bit(s.enums.len > 0, S_ENUMS);
        bit(s.min.is_some(), S_MIN);
        bit(s.max.is_some(), S_MAX);
        bit(s.timestamp_format.is_some(), S_TS);
        bit(s.location_name.is_some(), S_LOC_NAME);
        bit(s.xml_namespace.is_some(), S_XMLNS);
        bit(s.error_code.is_some(), S_ERROR_CODE);
        bit(s.error_status.is_some(), S_ERROR_STATUS);
        w.uv(u64::from(present));
        w.len(s.members.len as usize);
        for m in &self.members[s.members.range()] {
            encode_member(w, m);
        }
        if let Some(p) = s.payload {
            w.uv(u64::from(p));
        }
        if s.enums.len > 0 {
            w_syms(w, &self.syms, s.enums);
        }
        if let Some(v) = s.min {
            w.iv(v);
        }
        if let Some(v) = s.max {
            w.iv(v);
        }
        if let Some(t) = s.timestamp_format {
            w.u8(t.code());
        }
        if let Some(n) = s.location_name {
            w.sym(n);
        }
        if s.xml_namespace.is_some() {
            w_xmlns(w, s.xml_namespace);
        }
        if let Some(c) = s.error_code {
            w.sym(c);
        }
        if let Some(st) = s.error_status {
            w.uv(u64::from(st));
        }
    }

    fn encode_operation(&self, w: &mut Writer, o: &OperationData) {
        w.sym(o.name);
        w.u8(o.method.code());
        w.sym(o.request_uri);
        w_opt_u(w, o.response_code.map(u64::from));
        w_opt_u(w, o.input.map(|s| u64::from(s.0)));
        w.opt_sym(o.input_location_name);
        w_xmlns(w, o.input_xml_namespace);
        w_opt_u(w, o.output.map(|s| u64::from(s.0)));
        w.opt_sym(o.result_wrapper);
        w.len(o.errors.len as usize);
        for e in &self.shape_ids[o.errors.range()] {
            w.uv(u64::from(e.0));
        }
        w.uv(u64::from(o.flags));
        w.u8(o.auth.code());
        w.opt_sym(o.host_prefix);
        w.opt_sym(o.checksum_algorithm_member);
        match &o.paginator {
            None => w.u8(0),
            Some(p) => {
                w.u8(1);
                w_syms(w, &self.syms, p.input_tokens);
                w_syms(w, &self.syms, p.output_tokens);
                w_syms(w, &self.syms, p.result_keys);
                w.opt_sym(p.limit_key);
                w.opt_sym(p.more_results);
            }
        }
        w.u8(o.endpoint_variant);
    }

    pub(crate) fn decode(r: &mut Reader<'_>) -> Result<ServiceData, CatalogError> {
        let name = r.sym()?;
        let full_name = r.sym()?;
        let service_id = r.sym()?;
        let api_version = r.sym()?;
        let endpoint_prefix = r.sym()?;
        let signing_name = r.sym()?;
        let target_prefix = r.opt_sym()?;
        let json_version = r.opt_sym()?;
        let xml_namespace = r.opt_sym()?;
        let global_endpoint = r.opt_sym()?;
        let np = r.len()?;
        let mut protocols = Vec::with_capacity(np);
        for _ in 0..np {
            protocols.push(Protocol::from_code(r.u8()?)?);
        }
        let header = HeaderData {
            name,
            full_name,
            service_id,
            api_version,
            endpoint_prefix,
            signing_name,
            target_prefix,
            json_version,
            xml_namespace,
            global_endpoint,
            protocols,
            protocol: Protocol::from_code(r.u8()?)?,
            signature: Signature::from_code(r.u8()?)?,
            auth: Auth::from_code(r.u8()?)?,
            query_compatible: r.u8()? != 0,
        };

        let nshapes = r.len()?;
        let mut d = ServiceData {
            header,
            shapes: Vec::with_capacity(nshapes),
            members: Vec::with_capacity(nshapes * 2),
            syms: Vec::new(),
            shape_ids: Vec::new(),
            operations: Vec::new(),
        };
        for _ in 0..nshapes {
            let s = d.decode_shape(r, nshapes as u32)?;
            d.shapes.push(s);
        }
        let nops = r.len()?;
        d.operations.reserve_exact(nops);
        for _ in 0..nops {
            let o = d.decode_operation(r, nshapes as u32)?;
            d.operations.push(o);
        }
        Ok(d)
    }

    fn decode_shape(
        &mut self,
        r: &mut Reader<'_>,
        nshapes: u32,
    ) -> Result<ShapeData, CatalogError> {
        let name = r.sym()?;
        let kind = Kind::from_code(r.u8()?)?;
        let flags = r.u32()?;
        let present = r.u32()?;
        let nm = r.len()?;
        let start = self.members.len() as u32;
        for _ in 0..nm {
            self.members.push(decode_member(r, nshapes)?);
        }
        let members = Span {
            start,
            len: nm as u32,
        };
        let has = |b: u32| present & b != 0;
        let payload = if has(S_PAYLOAD) {
            let p = r.u32()?;
            if p >= members.len {
                return Err(CatalogError::Corrupt("payload member"));
            }
            Some(p)
        } else {
            None
        };
        let enums = if has(S_ENUMS) {
            r_syms(r, &mut self.syms)?
        } else {
            Span {
                start: self.syms.len() as u32,
                len: 0,
            }
        };
        Ok(ShapeData {
            name,
            kind,
            flags,
            members,
            payload,
            enums,
            min: if has(S_MIN) { Some(r.iv()?) } else { None },
            max: if has(S_MAX) { Some(r.iv()?) } else { None },
            timestamp_format: if has(S_TS) {
                Some(TimestampFormat::from_code(r.u8()?)?)
            } else {
                None
            },
            location_name: if has(S_LOC_NAME) {
                Some(r.sym()?)
            } else {
                None
            },
            xml_namespace: if has(S_XMLNS) { r_xmlns(r)? } else { None },
            error_code: if has(S_ERROR_CODE) {
                Some(r.sym()?)
            } else {
                None
            },
            error_status: if has(S_ERROR_STATUS) {
                Some(r.u16()?)
            } else {
                None
            },
        })
    }

    fn decode_operation(
        &mut self,
        r: &mut Reader<'_>,
        nshapes: u32,
    ) -> Result<OperationData, CatalogError> {
        let shape = |v: Option<u64>| -> Result<Option<ShapeId>, CatalogError> {
            match v {
                None => Ok(None),
                Some(v) if v < u64::from(nshapes) => Ok(Some(ShapeId(v as u32))),
                Some(_) => Err(CatalogError::Corrupt("shape id")),
            }
        };
        let name = r.sym()?;
        let method = Method::from_code(r.u8()?)?;
        let request_uri = r.sym()?;
        let response_code = r_opt_u(r)?.map(|v| v as u16);
        let input = shape(r_opt_u(r)?)?;
        let input_location_name = r.opt_sym()?;
        let input_xml_namespace = r_xmlns(r)?;
        let output = shape(r_opt_u(r)?)?;
        let result_wrapper = r.opt_sym()?;
        let ne = r.len()?;
        let start = self.shape_ids.len() as u32;
        for _ in 0..ne {
            let id = r.u32()?;
            if id >= nshapes {
                return Err(CatalogError::Corrupt("error shape id"));
            }
            self.shape_ids.push(ShapeId(id));
        }
        let errors = Span {
            start,
            len: ne as u32,
        };
        let flags = r.u32()?;
        let auth = Auth::from_code(r.u8()?)?;
        let host_prefix = r.opt_sym()?;
        let checksum_algorithm_member = r.opt_sym()?;
        let paginator = match r.u8()? {
            0 => None,
            _ => Some(PaginatorData {
                input_tokens: r_syms(r, &mut self.syms)?,
                output_tokens: r_syms(r, &mut self.syms)?,
                result_keys: r_syms(r, &mut self.syms)?,
                limit_key: r.opt_sym()?,
                more_results: r.opt_sym()?,
            }),
        };
        let endpoint_variant = r.u8()?;
        Ok(OperationData {
            name,
            method,
            request_uri,
            response_code,
            input,
            input_location_name,
            input_xml_namespace,
            output,
            result_wrapper,
            errors,
            flags,
            auth,
            host_prefix,
            checksum_algorithm_member,
            paginator,
            endpoint_variant,
        })
    }
}

fn encode_member(w: &mut Writer, m: &MemberData) {
    let mut first = m.location.code();
    if m.location_name.is_some() {
        first |= M_LOC_NAME;
    }
    if m.query_name.is_some() {
        first |= M_QUERY_NAME;
    }
    if m.xml_namespace.is_some() {
        first |= M_XMLNS;
    }
    if m.flags != 0 {
        first |= M_FLAGS;
    }
    w.u8(first);
    w.sym(m.name);
    w.uv(u64::from(m.shape.0));
    if let Some(n) = m.location_name {
        w.sym(n);
    }
    if let Some(n) = m.query_name {
        w.sym(n);
    }
    if m.xml_namespace.is_some() {
        w_xmlns(w, m.xml_namespace);
    }
    if m.flags != 0 {
        w.uv(u64::from(m.flags));
    }
}

fn decode_member(r: &mut Reader<'_>, nshapes: u32) -> Result<MemberData, CatalogError> {
    let first = r.u8()?;
    let location = Location::from_code(first & 0x7)?;
    let name = r.sym()?;
    let shape = r.u32()?;
    if shape >= nshapes {
        return Err(CatalogError::Corrupt("member shape id"));
    }
    Ok(MemberData {
        name,
        shape: ShapeId(shape),
        location,
        location_name: if first & M_LOC_NAME != 0 {
            Some(r.sym()?)
        } else {
            None
        },
        query_name: if first & M_QUERY_NAME != 0 {
            Some(r.sym()?)
        } else {
            None
        },
        xml_namespace: if first & M_XMLNS != 0 {
            r_xmlns(r)?
        } else {
            None
        },
        flags: if first & M_FLAGS != 0 { r.u32()? } else { 0 },
    })
}

// ---- the decoded service and its views ---------------------------------

/// One service, decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    pub(crate) strings: StrTable,
    pub(crate) d: ServiceData,
}

impl Service {
    /// Decodes a service blob: the string table, then the service.
    pub(crate) fn decode(bytes: &[u8]) -> Result<Service, CatalogError> {
        let mut r = Reader::new(bytes);
        let strings = StrTable::read(&mut r)?;
        let d = ServiceData::decode(&mut r)?;
        if !r.done() {
            return Err(CatalogError::Corrupt("trailing bytes after a service"));
        }
        Ok(Service { strings, d })
    }

    pub(crate) fn s(&self, s: Sym) -> &str {
        self.strings.get(s)
    }

    fn os(&self, s: Option<Sym>) -> Option<&str> {
        s.map(|s| self.strings.get(s))
    }

    /// botocore's name for the service, which the CLI uses too (`s3`, `ec2`).
    pub fn name(&self) -> &str {
        self.s(self.d.header.name)
    }

    pub fn full_name(&self) -> &str {
        self.s(self.d.header.full_name)
    }

    /// The SDKs' service id (`S3`, `EC2`, `Lambda`).
    pub fn service_id(&self) -> &str {
        self.s(self.d.header.service_id)
    }

    pub fn api_version(&self) -> &str {
        self.s(self.d.header.api_version)
    }

    pub fn endpoint_prefix(&self) -> &str {
        self.s(self.d.header.endpoint_prefix)
    }

    /// The name SigV4 signs with; for nearly every service, IAM's action prefix too.
    pub fn signing_name(&self) -> &str {
        self.s(self.d.header.signing_name)
    }

    /// The JSON protocols' `X-Amz-Target` prefix.
    pub fn target_prefix(&self) -> Option<&str> {
        self.os(self.d.header.target_prefix)
    }

    /// `1.0` or `1.1`, for the JSON protocol.
    pub fn json_version(&self) -> Option<&str> {
        self.os(self.d.header.json_version)
    }

    pub fn xml_namespace(&self) -> Option<&str> {
        self.os(self.d.header.xml_namespace)
    }

    pub fn global_endpoint(&self) -> Option<&str> {
        self.os(self.d.header.global_endpoint)
    }

    /// The protocol the client uses: the model's first that it speaks.
    pub fn protocol(&self) -> Protocol {
        self.d.header.protocol
    }

    /// Every protocol the model lists, in its order.
    pub fn protocols(&self) -> &[Protocol] {
        &self.d.header.protocols
    }

    pub fn signature(&self) -> Signature {
        self.d.header.signature
    }

    /// The service's default authentication.
    pub fn auth(&self) -> Auth {
        self.d.header.auth
    }

    /// A JSON service that answers with the query protocol's error codes
    /// (`x-amzn-query-error`): SQS and CloudWatch.
    pub fn query_compatible(&self) -> bool {
        self.d.header.query_compatible
    }

    pub fn shape(&self, id: ShapeId) -> ShapeRef<'_> {
        ShapeRef {
            svc: self,
            id,
            d: &self.d.shapes[id.0 as usize],
        }
    }

    pub fn shape_count(&self) -> usize {
        self.d.shapes.len()
    }

    pub fn shapes(&self) -> impl Iterator<Item = ShapeRef<'_>> + '_ {
        (0..self.d.shapes.len() as u32).map(move |i| self.shape(ShapeId(i)))
    }

    /// A shape by its model name (a linear scan).
    pub fn shape_by_name(&self, name: &str) -> Option<ShapeRef<'_>> {
        self.shapes().find(|s| s.name() == name)
    }

    pub fn operation_count(&self) -> usize {
        self.d.operations.len()
    }

    pub fn operations(&self) -> impl Iterator<Item = OperationRef<'_>> + '_ {
        self.d
            .operations
            .iter()
            .map(move |d| OperationRef { svc: self, d })
    }

    /// An operation by name: exactly (`ListObjectsV2`), then without regard
    /// to case, then in the CLI's form (`list-objects-v2`).
    pub fn operation(&self, name: &str) -> Option<OperationRef<'_>> {
        let ops = &self.d.operations;
        if let Ok(i) = ops.binary_search_by(|o| self.s(o.name).cmp(name)) {
            return Some(OperationRef {
                svc: self,
                d: &ops[i],
            });
        }
        let folded: String = name.chars().filter(|c| *c != '-' && *c != '_').collect();
        ops.iter()
            .find(|o| self.s(o.name).eq_ignore_ascii_case(&folded))
            .map(|d| OperationRef { svc: self, d })
    }
}

/// A shape, borrowed from its service.
#[derive(Clone, Copy)]
pub struct ShapeRef<'a> {
    svc: &'a Service,
    id: ShapeId,
    d: &'a ShapeData,
}

impl std::fmt::Debug for ShapeRef<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.name(), self.kind().as_str())
    }
}

impl<'a> ShapeRef<'a> {
    pub fn id(&self) -> ShapeId {
        self.id
    }

    pub fn service(&self) -> &'a Service {
        self.svc
    }

    pub fn name(&self) -> &'a str {
        self.svc.s(self.d.name)
    }

    pub fn kind(&self) -> Kind {
        self.d.kind
    }

    fn flag(&self, f: u32) -> bool {
        self.d.flags & f != 0
    }

    /// The model marks the value sensitive (a secret, or personal data).
    pub fn is_sensitive(&self) -> bool {
        self.flag(sf::SENSITIVE)
    }

    pub fn is_union(&self) -> bool {
        self.flag(sf::UNION)
    }

    /// A free-form JSON document.
    pub fn is_document(&self) -> bool {
        self.flag(sf::DOCUMENT)
    }

    pub fn is_event(&self) -> bool {
        self.flag(sf::EVENT)
    }

    pub fn is_eventstream(&self) -> bool {
        self.flag(sf::EVENTSTREAM)
    }

    pub fn is_streaming(&self) -> bool {
        self.flag(sf::STREAMING)
    }

    pub fn is_exception(&self) -> bool {
        self.flag(sf::EXCEPTION)
    }

    pub fn is_fault(&self) -> bool {
        self.flag(sf::FAULT)
    }

    pub fn is_deprecated(&self) -> bool {
        self.flag(sf::DEPRECATED)
    }

    pub fn requires_length(&self) -> bool {
        self.flag(sf::REQUIRES_LENGTH)
    }

    /// The query protocol's output wrapper shape.
    pub fn is_wrapper(&self) -> bool {
        self.flag(sf::WRAPPER)
    }

    /// A list or map serialized without its wrapping element.
    pub fn is_flattened(&self) -> bool {
        self.flag(sf::FLATTENED)
    }

    pub fn is_sparse(&self) -> bool {
        self.flag(sf::SPARSE)
    }

    /// An error the caller caused (4xx), as the model says.
    pub fn sender_fault(&self) -> bool {
        self.flag(sf::SENDER_FAULT)
    }

    /// `Some(throttling)` when the model marks the error retryable.
    pub fn retryable(&self) -> Option<bool> {
        self.flag(sf::RETRYABLE).then(|| self.flag(sf::THROTTLING))
    }

    /// A structure's members, in declaration order.
    pub fn members(&self) -> impl Iterator<Item = MemberRef<'a>> + 'a {
        let svc = self.svc;
        let ms = &svc.d.members[self.d.members.range()];
        let structure = self.d.kind == Kind::Structure;
        ms.iter()
            .filter(move |_| structure)
            .map(move |d| MemberRef { svc, d })
    }

    pub fn member_count(&self) -> usize {
        if self.d.kind == Kind::Structure {
            self.d.members.len as usize
        } else {
            0
        }
    }

    pub fn member(&self, name: &str) -> Option<MemberRef<'a>> {
        self.members().find(|m| m.name() == name)
    }

    fn nth(&self, i: u32) -> Option<MemberRef<'a>> {
        (i < self.d.members.len).then(|| MemberRef {
            svc: self.svc,
            d: &self.svc.d.members[(self.d.members.start + i) as usize],
        })
    }

    /// A list's member.
    pub fn list_member(&self) -> Option<MemberRef<'a>> {
        (self.d.kind == Kind::List).then(|| self.nth(0)).flatten()
    }

    pub fn map_key(&self) -> Option<MemberRef<'a>> {
        (self.d.kind == Kind::Map).then(|| self.nth(0)).flatten()
    }

    pub fn map_value(&self) -> Option<MemberRef<'a>> {
        (self.d.kind == Kind::Map).then(|| self.nth(1)).flatten()
    }

    /// The structure's payload member: the body of an HTTP-bound message.
    pub fn payload(&self) -> Option<MemberRef<'a>> {
        self.d.payload.and_then(|i| self.nth(i))
    }

    pub fn enum_values(&self) -> impl Iterator<Item = &'a str> + 'a {
        let svc = self.svc;
        svc.d.syms[self.d.enums.range()]
            .iter()
            .map(move |&s| svc.s(s))
    }

    pub fn enum_count(&self) -> usize {
        self.d.enums.len as usize
    }

    pub fn min(&self) -> Option<i64> {
        self.d.min
    }

    pub fn max(&self) -> Option<i64> {
        self.d.max
    }

    pub fn timestamp_format(&self) -> Option<TimestampFormat> {
        self.d.timestamp_format
    }

    pub fn location_name(&self) -> Option<&'a str> {
        self.svc.os(self.d.location_name)
    }

    /// `(prefix, uri)`.
    pub fn xml_namespace(&self) -> Option<(Option<&'a str>, &'a str)> {
        self.d
            .xml_namespace
            .map(|ns| (self.svc.os(ns.prefix), self.svc.s(ns.uri)))
    }

    /// The error code the service sends, when it differs from the shape's name.
    pub fn error_code(&self) -> Option<&'a str> {
        self.svc.os(self.d.error_code)
    }

    pub fn error_status(&self) -> Option<u16> {
        self.d.error_status
    }
}

/// A member of a structure, a list, or a map, borrowed from its service.
#[derive(Clone, Copy)]
pub struct MemberRef<'a> {
    svc: &'a Service,
    d: &'a MemberData,
}

impl std::fmt::Debug for MemberRef<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {:?}", self.name(), self.shape())
    }
}

impl<'a> MemberRef<'a> {
    /// The member's name (empty for a list's member or a map's key and value).
    pub fn name(&self) -> &'a str {
        self.svc.s(self.d.name)
    }

    pub fn shape(&self) -> ShapeRef<'a> {
        self.svc.shape(self.d.shape)
    }

    pub fn location(&self) -> Location {
        self.d.location
    }

    pub fn location_name(&self) -> Option<&'a str> {
        self.svc.os(self.d.location_name)
    }

    /// The name on the wire: the location name, or the member's own.
    pub fn wire_name(&self) -> &'a str {
        self.location_name().unwrap_or_else(|| self.name())
    }

    /// EC2's query name.
    pub fn query_name(&self) -> Option<&'a str> {
        self.svc.os(self.d.query_name)
    }

    pub fn xml_namespace(&self) -> Option<(Option<&'a str>, &'a str)> {
        self.d
            .xml_namespace
            .map(|ns| (self.svc.os(ns.prefix), self.svc.s(ns.uri)))
    }

    fn flag(&self, f: u32) -> bool {
        self.d.flags & f != 0
    }

    pub fn is_required(&self) -> bool {
        self.flag(mf::REQUIRED)
    }

    /// Filled from the call's correlation id when the caller leaves it out.
    pub fn is_idempotency_token(&self) -> bool {
        self.flag(mf::IDEMPOTENCY_TOKEN)
    }

    /// A header holding JSON, base64-encoded.
    pub fn is_jsonvalue(&self) -> bool {
        self.flag(mf::JSONVALUE)
    }

    /// Substituted into the endpoint's host prefix.
    pub fn is_host_label(&self) -> bool {
        self.flag(mf::HOST_LABEL)
    }

    pub fn is_xml_attribute(&self) -> bool {
        self.flag(mf::XML_ATTRIBUTE)
    }

    /// Flattened at the member (a shape can be flattened too).
    pub fn is_flattened(&self) -> bool {
        self.flag(mf::FLATTENED) || self.shape().is_flattened()
    }

    pub fn is_streaming(&self) -> bool {
        self.flag(mf::STREAMING) || self.shape().is_streaming()
    }

    pub fn is_eventpayload(&self) -> bool {
        self.flag(mf::EVENTPAYLOAD)
    }

    pub fn is_deprecated(&self) -> bool {
        self.flag(mf::DEPRECATED)
    }
}

/// An operation, borrowed from its service.
#[derive(Clone, Copy)]
pub struct OperationRef<'a> {
    pub(crate) svc: &'a Service,
    pub(crate) d: &'a OperationData,
}

impl std::fmt::Debug for OperationRef<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.svc.name(), self.name())
    }
}

impl<'a> OperationRef<'a> {
    pub fn service(&self) -> &'a Service {
        self.svc
    }

    pub fn name(&self) -> &'a str {
        self.svc.s(self.d.name)
    }

    pub fn method(&self) -> Method {
        self.d.method
    }

    /// The request URI template (`/{Bucket}?list-type=2`).
    pub fn request_uri(&self) -> &'a str {
        self.svc.s(self.d.request_uri)
    }

    /// The success status the model names, if it names one.
    pub fn response_code(&self) -> Option<u16> {
        self.d.response_code
    }

    pub fn input(&self) -> Option<ShapeRef<'a>> {
        self.d.input.map(|id| self.svc.shape(id))
    }

    /// The root element name of a REST-XML request body.
    pub fn input_location_name(&self) -> Option<&'a str> {
        self.svc.os(self.d.input_location_name)
    }

    pub fn input_xml_namespace(&self) -> Option<(Option<&'a str>, &'a str)> {
        self.d
            .input_xml_namespace
            .map(|ns| (self.svc.os(ns.prefix), self.svc.s(ns.uri)))
    }

    pub fn output(&self) -> Option<ShapeRef<'a>> {
        self.d.output.map(|id| self.svc.shape(id))
    }

    /// The query protocol's result element (`ListRolesResult`).
    pub fn result_wrapper(&self) -> Option<&'a str> {
        self.svc.os(self.d.result_wrapper)
    }

    pub fn errors(&self) -> impl Iterator<Item = ShapeRef<'a>> + 'a {
        let svc = self.svc;
        svc.d.shape_ids[self.d.errors.range()]
            .iter()
            .map(move |&id| svc.shape(id))
    }

    fn flag(&self, f: u32) -> bool {
        self.d.flags & f != 0
    }

    /// Smithy's `@readonly`, where the model carries it.
    pub fn is_readonly(&self) -> bool {
        self.flag(of::READONLY)
    }

    /// Smithy's `@idempotent`.
    pub fn is_idempotent(&self) -> bool {
        self.flag(of::IDEMPOTENT)
    }

    pub fn is_deprecated(&self) -> bool {
        self.flag(of::DEPRECATED)
    }

    /// The request needs a body checksum (`Content-MD5`, or a flexible one).
    pub fn checksum_required(&self) -> bool {
        self.flag(of::CHECKSUM_REQUIRED)
    }

    pub fn unsigned_payload(&self) -> bool {
        self.flag(of::UNSIGNED_PAYLOAD) || self.d.auth == Auth::SigV4UnsignedBody
    }

    /// `Some(required)` when the operation uses endpoint discovery.
    pub fn endpoint_discovery(&self) -> Option<bool> {
        self.flag(of::ENDPOINT_DISCOVERY)
            .then(|| self.flag(of::ENDPOINT_DISCOVERY_REQUIRED))
    }

    pub fn is_endpoint_operation(&self) -> bool {
        self.flag(of::ENDPOINT_OPERATION)
    }

    pub fn request_compression(&self) -> bool {
        self.flag(of::REQUEST_COMPRESSION)
    }

    pub fn auth(&self) -> Auth {
        self.d.auth
    }

    /// The endpoint's host prefix (`data-`, `{AccountId}.`).
    pub fn host_prefix(&self) -> Option<&'a str> {
        self.svc.os(self.d.host_prefix)
    }

    /// The input member that names a flexible checksum's algorithm.
    pub fn checksum_algorithm_member(&self) -> Option<&'a str> {
        self.svc.os(self.d.checksum_algorithm_member)
    }

    pub fn paginator(&self) -> Option<Paginator<'a>> {
        self.d
            .paginator
            .as_ref()
            .map(|d| Paginator { svc: self.svc, d })
    }

    /// Which of the service's endpoint variants the operation uses (see
    /// [`crate::Catalog::operation_endpoint`]).
    pub fn endpoint_variant(&self) -> usize {
        usize::from(self.d.endpoint_variant)
    }

    /// The input member the model marks `idempotencyToken`.
    pub fn idempotency_token(&self) -> Option<MemberRef<'a>> {
        self.input()?.members().find(|m| m.is_idempotency_token())
    }

    /// The input or output streams a payload (S3 `PutObject`, `GetObject`).
    pub fn streams_input(&self) -> bool {
        self.input()
            .and_then(|s| s.payload())
            .is_some_and(|m| m.is_streaming())
    }

    pub fn streams_output(&self) -> bool {
        self.output()
            .and_then(|s| s.payload())
            .is_some_and(|m| m.is_streaming())
    }

    /// The input or the output is an event stream (the CLI's job, for now).
    pub fn has_event_stream(&self) -> bool {
        let es = |s: Option<ShapeRef<'_>>| {
            s.is_some_and(|s| s.members().any(|m| m.shape().is_eventstream()))
        };
        es(self.input()) || es(self.output())
    }
}

/// How an operation pages (`paginators-1.json`).
#[derive(Clone, Copy)]
pub struct Paginator<'a> {
    svc: &'a Service,
    d: &'a PaginatorData,
}

impl<'a> Paginator<'a> {
    fn list(&self, span: Span) -> Vec<&'a str> {
        let svc = self.svc;
        svc.d.syms[span.range()].iter().map(|&s| svc.s(s)).collect()
    }

    /// The input members a page's token goes into.
    pub fn input_tokens(&self) -> Vec<&'a str> {
        self.list(self.d.input_tokens)
    }

    /// Where each token comes from in the output: a member, a path
    /// (`Contents[-1].Key`), or alternatives (`NextMarker || Contents[-1].Key`).
    pub fn output_tokens(&self) -> Vec<&'a str> {
        self.list(self.d.output_tokens)
    }

    /// The members whose lists the pages concatenate.
    pub fn result_keys(&self) -> Vec<&'a str> {
        self.list(self.d.result_keys)
    }

    /// The input member that sets a page's size.
    pub fn limit_key(&self) -> Option<&'a str> {
        self.svc.os(self.d.limit_key)
    }

    /// The output member that says whether more pages follow.
    pub fn more_results(&self) -> Option<&'a str> {
        self.svc.os(self.d.more_results)
    }
}
