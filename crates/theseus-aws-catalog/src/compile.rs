//! The generator's side: botocore's JSON models in, the compact catalog out.
//!
//! Run offline by `theseus-aws-catalog-gen` (an `xtask`-style binary, never a
//! `build.rs`), from the AWS CLI's bundled models: `service-2.json`,
//! `paginators-1.json` (each with its `sdk-extras` merged as botocore's
//! loader merges them), `endpoint-rule-set-1.json`, and `partitions.json`.
//! [`botocore_data_dir`] finds those models.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use crate::endpoints::{EndpointRule, Exception, Partition, Template};
use crate::json::J;
use crate::model::{
    mf, of, sf, Auth, HeaderData, Kind, Location, MemberData, Method, OperationData, PaginatorData,
    Protocol, ServiceData, ShapeData, ShapeId, Signature, Span, TimestampFormat, XmlNs,
};
use crate::rules::{Evaluator, PartitionDef};
use crate::wire::{Interner, Sym, Writer};
use crate::{CatalogError, ServiceEntry, FORMAT_VERSION, MAGIC};

#[derive(Debug, thiserror::Error)]
pub enum CompileError {
    #[error("{0}: {1}")]
    Io(String, std::io::Error),
    #[error("{0}: invalid JSON: {1}")]
    Json(String, serde_json::Error),
    #[error("{service}: {what}")]
    Model { service: String, what: String },
    #[error("catalog: {0}")]
    Catalog(#[from] CatalogError),
    #[error("the botocore models: {0}")]
    Source(String),
}

fn bad(service: &str, what: impl Into<String>) -> CompileError {
    CompileError::Model {
        service: service.to_owned(),
        what: what.into(),
    }
}

/// One service's model files, as text.
#[derive(Debug, Default, Clone)]
pub struct ModelFiles {
    pub service: String,
    pub service_extras: Option<String>,
    pub paginators: Option<String>,
    pub paginator_extras: Option<String>,
    pub ruleset: Option<String>,
}

fn parse(service: &str, what: &str, text: &str) -> Result<J, CompileError> {
    J::parse(text).map_err(|e| CompileError::Json(format!("{service}/{what}"), e))
}

/// Applies an `sdk-extras` file's `merge` section.
fn merge_extras(service: &str, base: &mut J, extras: Option<&str>) -> Result<(), CompileError> {
    if let Some(text) = extras {
        let x = parse(service, "sdk-extras", text)?;
        if let Some(m) = x.get("merge") {
            base.merge(m);
        }
    }
    Ok(())
}

fn xmlns(it: &mut Interner, v: Option<&J>) -> Option<XmlNs> {
    match v? {
        J::Str(uri) => Some(XmlNs {
            prefix: None,
            uri: it.sym(uri),
        }),
        o @ J::Obj(_) => Some(XmlNs {
            prefix: o.str_at("prefix").map(|p| it.sym(p)),
            uri: it.sym(o.str_at("uri")?),
        }),
        _ => None,
    }
}

/// The first authentication scheme in a Smithy `auth` list that the client
/// knows.
fn auth_from_list(list: &J) -> Option<Auth> {
    list.arr().iter().filter_map(J::str).find_map(|a| match a {
        "aws.auth#sigv4" | "aws.auth#sigv4a" => Some(Auth::SigV4),
        "smithy.api#noAuth" => Some(Auth::None),
        "smithy.api#httpBearerAuth" => Some(Auth::Bearer),
        _ => None,
    })
}

/// A set of static context parameters (`ApiType = DataPlane`), sorted by name.
pub(crate) type StaticParams = Vec<(String, J)>;

/// Compiles one service's model into its decoded form and string table, and
/// the distinct static context parameter sets its operations use: each is an
/// endpoint variant, and variant 0 is the empty set.
pub(crate) fn build_service(
    name: &str,
    files: &ModelFiles,
) -> Result<(Interner, ServiceData, Vec<StaticParams>), CompileError> {
    let mut model = parse(name, "service-2.json", &files.service)?;
    merge_extras(name, &mut model, files.service_extras.as_deref())?;
    let md = model
        .get("metadata")
        .ok_or_else(|| bad(name, "no metadata"))?;
    let mut it = Interner::default();

    // The header.
    let protocols: Vec<Protocol> = match md.get("protocols") {
        Some(list) => list
            .arr()
            .iter()
            .filter_map(J::str)
            .filter_map(Protocol::from_botocore)
            .collect(),
        None => md
            .str_at("protocol")
            .and_then(Protocol::from_botocore)
            .into_iter()
            .collect(),
    };
    let protocols = if protocols.is_empty() {
        md.str_at("protocol")
            .and_then(Protocol::from_botocore)
            .into_iter()
            .collect::<Vec<_>>()
    } else {
        protocols
    };
    let protocol = protocols
        .iter()
        .copied()
        .find(|p| p.is_supported())
        .or_else(|| protocols.first().copied())
        .ok_or_else(|| bad(name, "no protocol"))?;
    let signature = match md.str_at("signatureVersion").unwrap_or("v4") {
        "v4" => Signature::V4,
        "s3" | "s3v4" => Signature::S3V4,
        "v2" => Signature::V2,
        "bearer" => Signature::Bearer,
        other => return Err(bad(name, format!("signature version {other}"))),
    };
    let auth = md
        .get("auth")
        .and_then(auth_from_list)
        .unwrap_or(match signature {
            Signature::Bearer => Auth::Bearer,
            _ => Auth::SigV4,
        });
    let prefix = md
        .str_at("endpointPrefix")
        .ok_or_else(|| bad(name, "no endpointPrefix"))?;
    let header = HeaderData {
        name: it.sym(name),
        full_name: it.sym(md.str_at("serviceFullName").unwrap_or(name)),
        service_id: it.sym(md.str_at("serviceId").unwrap_or(name)),
        api_version: it.sym(md.str_at("apiVersion").unwrap_or_default()),
        endpoint_prefix: it.sym(prefix),
        signing_name: it.sym(md.str_at("signingName").unwrap_or(prefix)),
        target_prefix: md.str_at("targetPrefix").map(|s| it.sym(s)),
        json_version: md.str_at("jsonVersion").map(|s| it.sym(s)),
        xml_namespace: md.str_at("xmlNamespace").map(|s| it.sym(s)),
        global_endpoint: md.str_at("globalEndpoint").map(|s| it.sym(s)),
        protocols,
        protocol,
        signature,
        auth,
        query_compatible: md.get("awsQueryCompatible").is_some(),
    };

    // Shapes, in the model's order; ids are their positions.
    let shapes_j = model.get("shapes").map(J::obj).unwrap_or(&[]);
    let ids: HashMap<&str, u32> = shapes_j
        .iter()
        .enumerate()
        .map(|(i, (n, _))| (n.as_str(), i as u32))
        .collect();
    let shape_id = |n: Option<&str>| -> Result<ShapeId, CompileError> {
        let n = n.ok_or_else(|| bad(name, "a reference without a shape"))?;
        ids.get(n)
            .map(|&i| ShapeId(i))
            .ok_or_else(|| bad(name, format!("unknown shape {n}")))
    };
    let mut d = ServiceData {
        header,
        shapes: Vec::with_capacity(shapes_j.len()),
        members: Vec::new(),
        syms: Vec::new(),
        shape_ids: Vec::new(),
        operations: Vec::new(),
    };
    for (sname, s) in shapes_j {
        let kind = s
            .str_at("type")
            .and_then(Kind::from_botocore)
            .ok_or_else(|| bad(name, format!("shape {sname}: type {:?}", s.str_at("type"))))?;
        let mut flags = 0u32;
        for (key, f) in [
            ("sensitive", sf::SENSITIVE),
            ("union", sf::UNION),
            ("document", sf::DOCUMENT),
            ("event", sf::EVENT),
            ("eventstream", sf::EVENTSTREAM),
            ("streaming", sf::STREAMING),
            ("exception", sf::EXCEPTION),
            ("fault", sf::FAULT),
            ("deprecated", sf::DEPRECATED),
            ("requiresLength", sf::REQUIRES_LENGTH),
            ("wrapper", sf::WRAPPER),
            ("flattened", sf::FLATTENED),
            ("sparse", sf::SPARSE),
        ] {
            if s.bool_at(key) {
                flags |= f;
            }
        }
        let (mut error_code, mut error_status) = (None, None);
        if let Some(e) = s.get("error") {
            error_code = e.str_at("code").map(|c| it.sym(c));
            error_status = e.get("httpStatusCode").and_then(J::int).map(|v| v as u16);
            if e.bool_at("senderFault") {
                flags |= sf::SENDER_FAULT;
            }
        }
        if let Some(r) = s.get("retryable") {
            flags |= sf::RETRYABLE;
            if r.bool_at("throttling") {
                flags |= sf::THROTTLING;
            }
        }

        let start = d.members.len() as u32;
        let member = |mname: &str,
                      mj: &J,
                      required: bool,
                      it: &mut Interner|
         -> Result<MemberData, CompileError> {
            let mut mflags = if required { mf::REQUIRED } else { 0 };
            for (key, f) in [
                ("idempotencyToken", mf::IDEMPOTENCY_TOKEN),
                ("jsonvalue", mf::JSONVALUE),
                ("hostLabel", mf::HOST_LABEL),
                ("xmlAttribute", mf::XML_ATTRIBUTE),
                ("flattened", mf::FLATTENED),
                ("streaming", mf::STREAMING),
                ("eventpayload", mf::EVENTPAYLOAD),
                ("deprecated", mf::DEPRECATED),
            ] {
                if mj.bool_at(key) {
                    mflags |= f;
                }
            }
            let location = match mj.str_at("location") {
                None => Location::Body,
                Some(l) => {
                    Location::from_botocore(l).ok_or_else(|| bad(name, format!("location {l}")))?
                }
            };
            Ok(MemberData {
                name: it.sym(mname),
                shape: shape_id(mj.str_at("shape"))?,
                location,
                location_name: mj.str_at("locationName").map(|n| it.sym(n)),
                query_name: mj.str_at("queryName").map(|n| it.sym(n)),
                xml_namespace: xmlns(it, mj.get("xmlNamespace")),
                flags: mflags,
            })
        };
        let mut payload = None;
        match kind {
            Kind::Structure => {
                let required: Vec<&str> = s
                    .get("required")
                    .map(J::arr)
                    .unwrap_or(&[])
                    .iter()
                    .filter_map(J::str)
                    .collect();
                let pay = s.str_at("payload");
                for (i, (mname, mj)) in s
                    .get("members")
                    .map(J::obj)
                    .unwrap_or(&[])
                    .iter()
                    .enumerate()
                {
                    let m = member(mname, mj, required.contains(&mname.as_str()), &mut it)?;
                    d.members.push(m);
                    if pay == Some(mname.as_str()) {
                        payload = Some(i as u32);
                    }
                }
                if pay.is_some() && payload.is_none() {
                    return Err(bad(name, format!("shape {sname}: payload names no member")));
                }
            }
            Kind::List => {
                let mj = s
                    .get("member")
                    .ok_or_else(|| bad(name, format!("list {sname} without a member")))?;
                let m = member("", mj, false, &mut it)?;
                d.members.push(m);
            }
            Kind::Map => {
                for key in ["key", "value"] {
                    let mj = s
                        .get(key)
                        .ok_or_else(|| bad(name, format!("map {sname} without a {key}")))?;
                    let m = member("", mj, false, &mut it)?;
                    d.members.push(m);
                }
            }
            _ => {}
        }
        let members = Span {
            start,
            len: d.members.len() as u32 - start,
        };
        let enum_start = d.syms.len() as u32;
        for v in s
            .get("enum")
            .map(J::arr)
            .unwrap_or(&[])
            .iter()
            .filter_map(J::str)
        {
            let sym = it.sym(v);
            d.syms.push(sym);
        }
        let enums = Span {
            start: enum_start,
            len: d.syms.len() as u32 - enum_start,
        };
        let timestamp_format = match s.str_at("timestampFormat") {
            None => None,
            Some(t) => Some(
                TimestampFormat::from_botocore(t)
                    .ok_or_else(|| bad(name, format!("timestamp format {t}")))?,
            ),
        };
        d.shapes.push(ShapeData {
            name: it.sym(sname),
            kind,
            flags,
            members,
            payload,
            enums,
            min: s.get("min").and_then(J::int),
            max: s.get("max").and_then(J::int),
            timestamp_format,
            location_name: s.str_at("locationName").map(|n| it.sym(n)),
            xml_namespace: xmlns(&mut it, s.get("xmlNamespace")),
            error_code,
            error_status,
        });
    }

    // Paginators, by operation.
    let mut pag = match &files.paginators {
        Some(text) => parse(name, "paginators-1.json", text)?,
        None => J::Obj(Vec::new()),
    };
    merge_extras(name, &mut pag, files.paginator_extras.as_deref())?;
    let pagination = pag.get("pagination");

    // Operations, sorted by name for lookup.
    let mut variants: Vec<StaticParams> = vec![Vec::new()];
    let mut ops: Vec<&(String, J)> = model
        .get("operations")
        .map(J::obj)
        .unwrap_or(&[])
        .iter()
        .collect();
    ops.sort_by(|a, b| a.0.cmp(&b.0));
    for (oname, o) in ops {
        let http = o
            .get("http")
            .ok_or_else(|| bad(name, format!("{oname}: no http")))?;
        let method = http
            .str_at("method")
            .and_then(Method::from_botocore)
            .ok_or_else(|| bad(name, format!("{oname}: method {:?}", http.str_at("method"))))?;
        let input = o.get("input");
        let output = o.get("output");
        let mut flags = 0u32;
        for (key, f) in [
            ("readonly", of::READONLY),
            ("idempotent", of::IDEMPOTENT),
            ("deprecated", of::DEPRECATED),
            ("httpChecksumRequired", of::CHECKSUM_REQUIRED),
            ("unsignedPayload", of::UNSIGNED_PAYLOAD),
            ("endpointoperation", of::ENDPOINT_OPERATION),
        ] {
            if o.bool_at(key) {
                flags |= f;
            }
        }
        if o.get("httpChecksum")
            .is_some_and(|c| c.bool_at("requestChecksumRequired"))
        {
            flags |= of::CHECKSUM_REQUIRED;
        }
        if let Some(ed) = o.get("endpointdiscovery") {
            flags |= of::ENDPOINT_DISCOVERY;
            if ed.bool_at("required") {
                flags |= of::ENDPOINT_DISCOVERY_REQUIRED;
            }
        }
        if o.get("requestcompression").is_some() {
            flags |= of::REQUEST_COMPRESSION;
        }
        let op_auth = match (o.get("auth"), o.str_at("authtype")) {
            (Some(list), _) => auth_from_list(list).unwrap_or(auth),
            (None, Some("none")) => Auth::None,
            (None, Some("v4-unsigned-body")) => Auth::SigV4UnsignedBody,
            (None, Some("bearer")) => Auth::Bearer,
            _ => auth,
        };
        let errors_start = d.shape_ids.len() as u32;
        for e in o.get("errors").map(J::arr).unwrap_or(&[]) {
            let id = shape_id(e.str_at("shape"))?;
            d.shape_ids.push(id);
        }
        let errors = Span {
            start: errors_start,
            len: d.shape_ids.len() as u32 - errors_start,
        };
        let paginator = match pagination.and_then(|p| p.get(oname)) {
            None => None,
            Some(p) => {
                let list = |v: Option<&J>, it: &mut Interner, syms: &mut Vec<Sym>| -> Span {
                    let start = syms.len() as u32;
                    for s in v.map(J::strs).unwrap_or_default() {
                        syms.push(it.sym(s));
                    }
                    Span {
                        start,
                        len: syms.len() as u32 - start,
                    }
                };
                let input_tokens = list(p.get("input_token"), &mut it, &mut d.syms);
                let output_tokens = list(p.get("output_token"), &mut it, &mut d.syms);
                let result_keys = list(p.get("result_key"), &mut it, &mut d.syms);
                Some(PaginatorData {
                    input_tokens,
                    output_tokens,
                    result_keys,
                    limit_key: p.str_at("limit_key").map(|s| it.sym(s)),
                    more_results: p.str_at("more_results").map(|s| it.sym(s)),
                })
            }
        };
        let input_shape = match input {
            Some(i) => Some(shape_id(i.str_at("shape"))?),
            None => None,
        };
        let output_shape = match output {
            Some(i) => Some(shape_id(i.str_at("shape"))?),
            None => None,
        };
        let mut statics: StaticParams = o
            .get("staticContextParams")
            .map(J::obj)
            .unwrap_or(&[])
            .iter()
            .filter_map(|(k, p)| p.get("value").map(|v| (k.clone(), v.clone())))
            .collect();
        statics.sort_by(|a, b| a.0.cmp(&b.0));
        let endpoint_variant = match variants.iter().position(|v| *v == statics) {
            Some(i) => i,
            None => {
                variants.push(statics);
                variants.len() - 1
            }
        };
        let endpoint_variant = u8::try_from(endpoint_variant)
            .map_err(|_| bad(name, "more than 255 endpoint variants"))?;
        d.operations.push(OperationData {
            name: it.sym(oname),
            method,
            request_uri: it.sym(http.str_at("requestUri").unwrap_or("/")),
            response_code: http.get("responseCode").and_then(J::int).map(|c| c as u16),
            input: input_shape,
            input_location_name: input
                .and_then(|i| i.str_at("locationName"))
                .map(|n| it.sym(n)),
            input_xml_namespace: xmlns(&mut it, input.and_then(|i| i.get("xmlNamespace"))),
            output: output_shape,
            result_wrapper: output
                .and_then(|o| o.str_at("resultWrapper"))
                .map(|n| it.sym(n)),
            errors,
            flags,
            auth: op_auth,
            host_prefix: o
                .get("endpoint")
                .and_then(|e| e.str_at("hostPrefix"))
                .map(|h| it.sym(h)),
            checksum_algorithm_member: o
                .get("httpChecksum")
                .and_then(|c| c.str_at("requestAlgorithmMember"))
                .map(|m| it.sym(m)),
            paginator,
            endpoint_variant,
        });
    }
    Ok((it, d, variants))
}

/// A service's blob before compression: its string table, then the service.
pub(crate) fn encode_service(it: &Interner, d: &ServiceData) -> Vec<u8> {
    let mut body = Writer::default();
    d.encode(&mut body);
    let mut w = Writer::default();
    it.write(&mut w);
    w.buf.extend_from_slice(&body.buf);
    w.buf
}

/// Compiles one service and decodes it again: for tests that build a
/// service from a model fixture, and for the round-trip check.
pub fn compile_service(name: &str, files: &ModelFiles) -> Result<crate::Service, CompileError> {
    let (it, d, _) = build_service(name, files)?;
    Ok(crate::Service::decode(&encode_service(&it, &d))?)
}

/// What the generator reports.
#[derive(Debug, Default, Clone)]
pub struct CompileReport {
    pub services: usize,
    pub operations: usize,
    pub shapes: usize,
    pub raw_bytes: usize,
    pub compressed_bytes: usize,
    /// Services whose endpoint is not the default somewhere.
    pub endpoint_rules: usize,
    /// `(service, partition, region, error)` where a rule set did not resolve
    /// (the default endpoint stands).
    pub endpoint_failures: Vec<(String, String, String, String)>,
    /// The largest service, uncompressed: `(name, bytes)`.
    pub largest: (String, usize),
}

/// A partition's endpoint for one service: the URL with `{region}`, and the
/// signing region with it, when every region of the partition agrees.
fn summarize(
    prefix: &str,
    part: &PartitionDef,
    results: &BTreeMap<String, (String, Option<String>)>,
) -> (Option<Template>, Vec<Exception>) {
    // Patterns, most common first.
    let mut counts: BTreeMap<(String, Option<String>), usize> = BTreeMap::new();
    let pattern = |region: &str, url: &str, sr: &Option<String>| {
        let u = url.replace(region, "{region}");
        let s = sr.as_ref().map(|s| {
            if s == region {
                "{region}".to_owned()
            } else {
                s.clone()
            }
        });
        (u, s)
    };
    for (region, (url, sr)) in results {
        *counts.entry(pattern(region, url, sr)).or_default() += 1;
    }
    let Some(((tu, ts), _)) = counts.iter().max_by_key(|(_, n)| **n) else {
        return (None, Vec::new());
    };
    let default_url = format!("https://{prefix}.{{region}}.{}", part.dns_suffix);
    let signing = match ts.as_deref() {
        None | Some("{region}") => None,
        Some(s) => Some(s.to_owned()),
    };
    let template = if *tu == default_url && signing.is_none() {
        None
    } else {
        Some(Template {
            partition: 0, // set by the caller
            url: (*tu != default_url).then(|| tu.clone()),
            signing_region: signing,
        })
    };
    let mut exceptions = Vec::new();
    for (region, (url, sr)) in results {
        if pattern(region, url, sr) != (tu.clone(), ts.clone()) {
            exceptions.push(Exception {
                region: region.clone(),
                url: url.clone(),
                signing_region: sr.clone().filter(|s| s != region),
            });
        }
    }
    (template, exceptions)
}

/// Resolves a service's rule set for every region of every partition, with
/// one endpoint variant's static context parameters set.
fn endpoint_rule(
    name: &str,
    prefix: &str,
    signing_name: &str,
    ruleset: Option<&J>,
    parts: &[PartitionDef],
    params: &[(String, J)],
    report: &mut CompileReport,
) -> EndpointRule {
    let mut rule = EndpointRule::default();
    let Some(rs) = ruleset else {
        return rule;
    };
    let ev = Evaluator::new(parts);
    // A failure names the variant: `neptune-graph [ApiType=DataPlane]`.
    let label = if params.is_empty() {
        name.to_owned()
    } else {
        let set: Vec<String> = params
            .iter()
            .map(|(k, v)| match v {
                J::Str(s) => format!("{k}={s}"),
                J::Bool(b) => format!("{k}={b}"),
                other => format!("{k}={other:?}"),
            })
            .collect();
        format!("{name} [{}]", set.join(", "))
    };
    let mut names: BTreeMap<String, usize> = BTreeMap::new();
    for (pi, part) in parts.iter().enumerate() {
        let mut results = BTreeMap::new();
        for region in &part.regions {
            // Pseudo-regions (`aws-global`) are not places to send requests.
            if !region.bytes().any(|b| b.is_ascii_digit()) {
                continue;
            }
            match ev.resolve_with(rs, region, params) {
                Ok(r) => {
                    if let Some(n) = &r.signing_name {
                        *names.entry(n.clone()).or_default() += 1;
                    }
                    let url = r.url.trim_end_matches('/').to_owned();
                    results.insert(region.clone(), (url, r.signing_region));
                }
                Err(e) => report.endpoint_failures.push((
                    label.clone(),
                    part.id.clone(),
                    region.clone(),
                    e,
                )),
            }
        }
        let (template, exceptions) = summarize(prefix, part, &results);
        if let Some(mut t) = template {
            t.partition = pi as u32;
            rule.templates.push(t);
        }
        rule.exceptions.extend(exceptions);
    }
    if let Some((n, _)) = names.iter().max_by_key(|(_, c)| **c) {
        if n != signing_name {
            rule.signing_name = Some(n.clone());
        }
    }
    rule
}

fn read(path: &Path) -> Result<String, CompileError> {
    std::fs::read_to_string(path).map_err(|e| CompileError::Io(path.display().to_string(), e))
}

fn read_opt(path: &Path) -> Result<Option<String>, CompileError> {
    if path.exists() {
        read(path).map(Some)
    } else {
        Ok(None)
    }
}

/// The model files of every service under botocore's `data` directory, at
/// each service's newest API version.
pub fn read_models(dir: &Path) -> Result<Vec<(String, ModelFiles)>, CompileError> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map_err(|e| CompileError::Io(dir.display().to_string(), e))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let mut out = Vec::new();
    for name in names {
        let sdir = dir.join(&name);
        let mut versions: Vec<String> = std::fs::read_dir(&sdir)
            .map_err(|e| CompileError::Io(sdir.display().to_string(), e))?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().join("service-2.json").exists())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        versions.sort();
        let Some(v) = versions.pop() else { continue };
        let vdir = sdir.join(v);
        out.push((
            name,
            ModelFiles {
                service: read(&vdir.join("service-2.json"))?,
                service_extras: read_opt(&vdir.join("service-2.sdk-extras.json"))?,
                paginators: read_opt(&vdir.join("paginators-1.json"))?,
                paginator_extras: read_opt(&vdir.join("paginators-1.sdk-extras.json"))?,
                ruleset: read_opt(&vdir.join("endpoint-rule-set-1.json"))?,
            },
        ));
    }
    Ok(out)
}

/// The variable that names a botocore `data` directory, ahead of the `aws`
/// on PATH.
pub const BOTOCORE_DATA: &str = "THESEUS_BOTOCORE_DATA";

/// The botocore `data` directory to read: the one `THESEUS_BOTOCORE_DATA`
/// names, else that of the `aws` on PATH ([`cli_data_dir`]).
pub fn botocore_data_dir() -> Result<PathBuf, CompileError> {
    if let Some(dir) = std::env::var_os(BOTOCORE_DATA).filter(|d| !d.is_empty()) {
        let dir = PathBuf::from(dir);
        if !dir.is_dir() {
            return Err(CompileError::Source(format!(
                "{BOTOCORE_DATA} names {}, which is not a directory",
                dir.display()
            )));
        }
        return Ok(dir);
    }
    let aws = aws_on_path().ok_or_else(|| {
        CompileError::Source(format!("no aws on PATH, and {BOTOCORE_DATA} is not set"))
    })?;
    cli_data_dir(&aws)
}

/// The first `aws` on PATH, as PATH names it.
pub fn aws_on_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join("aws"))
        .find(|a| a.is_file())
}

/// The botocore `data` directory of the AWS CLI `aws`, from its real path.
/// The CLI's own Python says where it imports `awscli.botocore` from: the
/// interpreter on the `#!` line, or Homebrew's `libexec/bin/python3` when the
/// CLI is a wrapper. AWS's installer has no Python to ask (the CLI is a
/// frozen binary), and keeps the models in the `dist/awscli` above it.
pub fn cli_data_dir(aws: &Path) -> Result<PathBuf, CompileError> {
    let real =
        std::fs::canonicalize(aws).map_err(|e| CompileError::Io(aws.display().to_string(), e))?;
    let homebrew = real
        .parent()
        .and_then(Path::parent)
        .map(|keg| keg.join("libexec/bin/python3"));
    shebang(&real)
        .into_iter()
        .chain(homebrew)
        .find_map(|python| ask_python(&python))
        .or_else(|| {
            real.ancestors()
                .skip(1)
                .map(|d| d.join("dist/awscli/botocore/data"))
                .find(|d| d.is_dir())
        })
        .ok_or_else(|| {
            CompileError::Source(format!(
                "{} ({}): no Python of its own imports awscli.botocore, and no dist/awscli is above it",
                aws.display(),
                real.display()
            ))
        })
}

/// The interpreter on a script's `#!` line, when it is a Python.
fn shebang(script: &Path) -> Option<PathBuf> {
    use std::io::Read;
    // The kernel reads no more of a `#!` line than this.
    let mut head = [0u8; 256];
    let n = std::fs::File::open(script).ok()?.read(&mut head).ok()?;
    let line = head[..n].strip_prefix(b"#!")?;
    let line = &line[..line.iter().position(|&c| c == b'\n')?];
    let python = PathBuf::from(std::str::from_utf8(line).ok()?.split_whitespace().next()?);
    python
        .file_name()?
        .to_str()?
        .starts_with("python")
        .then_some(python)
}

/// The `data` directory beside the `awscli.botocore` that `python` imports.
/// `-I` keeps PYTHONPATH and the user's packages out of the answer.
fn ask_python(python: &Path) -> Option<PathBuf> {
    let out = std::process::Command::new(python)
        .args([
            "-I",
            "-c",
            "import os, awscli.botocore as b; print(os.path.join(os.path.dirname(b.__file__), 'data'))",
        ])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let dir = PathBuf::from(String::from_utf8(out.stdout).ok()?.trim_end());
    dir.is_dir().then_some(dir)
}

/// The CLI's version, the catalog's snapshot label: the first word of
/// `aws --version` (`aws-cli/2.34.15`).
pub fn cli_label(aws: &Path) -> Result<String, CompileError> {
    let out = std::process::Command::new(aws)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| CompileError::Io(format!("{} --version", aws.display()), e))?;
    let said = String::from_utf8_lossy(&out.stdout);
    match said.split_whitespace().next() {
        Some(label) if out.status.success() && label.starts_with("aws-cli/") => {
            Ok(label.to_owned())
        }
        _ => Err(CompileError::Source(format!(
            "{} --version said {:?} ({})",
            aws.display(),
            said.trim(),
            String::from_utf8_lossy(&out.stderr).trim()
        ))),
    }
}

/// The brotli encoder, which the generator brings: the library carries only
/// the decoder, so the encoder never reaches the binary.
pub type Compress<'a> = &'a dyn Fn(&[u8]) -> Vec<u8>;

/// Compiles every service under `dir` into the catalog's bytes.
pub fn compile_dir(
    dir: &Path,
    snapshot: &str,
    compress: Compress<'_>,
) -> Result<(Vec<u8>, CompileReport), CompileError> {
    let partitions_j = parse(
        "partitions",
        "partitions.json",
        &read(&dir.join("partitions.json"))?,
    )?;
    let parts = PartitionDef::parse(&partitions_j);
    if parts.is_empty() {
        return Err(bad("partitions", "no partitions"));
    }
    let models = read_models(dir)?;
    compile_models(&models, &parts, snapshot, compress)
}

/// Compiles model files (a test's fixtures, say) with a one-partition table
/// (`aws`, `us-east-1` and `us-west-2`): no rule set, so default endpoints.
pub fn compile_fixtures(
    models: &[(String, ModelFiles)],
    snapshot: &str,
    compress: Compress<'_>,
) -> Result<Vec<u8>, CompileError> {
    let parts = PartitionDef::parse(
        &J::parse(
            r#"{"partitions": [{"id": "aws", "outputs": {"dnsSuffix": "amazonaws.com",
            "dualStackDnsSuffix": "api.aws", "implicitGlobalRegion": "us-east-1",
            "supportsFIPS": true, "supportsDualStack": true},
            "regions": {"us-east-1": {}, "us-west-2": {}}}]}"#,
        )
        .map_err(|e| CompileError::Json("fixture partitions".into(), e))?,
    );
    compile_models(models, &parts, snapshot, compress).map(|(bytes, _)| bytes)
}

pub(crate) fn compile_models(
    models: &[(String, ModelFiles)],
    parts: &[PartitionDef],
    snapshot: &str,
    compress: Compress<'_>,
) -> Result<(Vec<u8>, CompileReport), CompileError> {
    let mut report = CompileReport::default();
    let mut entries = Vec::new();
    let mut blobs = Vec::new();
    for (name, files) in models {
        let (it, d, variants) = build_service(name, files)?;
        let raw = encode_service(&it, &d);
        // Every blob must decode to what was encoded.
        let back = crate::Service::decode(&raw)?;
        if back.d != d {
            return Err(bad(name, "the encoded service decodes differently"));
        }
        let packed = compress(&raw);
        if crate::unbrotli(&packed, raw.len())? != raw {
            return Err(bad(name, "the compressed service decompresses differently"));
        }
        let prefix = it.get(d.header.endpoint_prefix).to_owned();
        let signing = it.get(d.header.signing_name).to_owned();
        let ruleset = match &files.ruleset {
            Some(text) => Some(parse(name, "endpoint-rule-set-1.json", text)?),
            None => None,
        };
        // Variant 0 (no static parameters) is resolved only when an
        // operation uses it: Neptune Analytics' rule set has no endpoint
        // without its `ApiType`.
        let plain = d.operations.is_empty() || d.operations.iter().any(|o| o.endpoint_variant == 0);
        let endpoints: Vec<EndpointRule> = variants
            .iter()
            .enumerate()
            .map(|(i, params)| {
                if i == 0 && !plain {
                    EndpointRule::default()
                } else {
                    endpoint_rule(
                        name,
                        &prefix,
                        &signing,
                        ruleset.as_ref(),
                        parts,
                        params,
                        &mut report,
                    )
                }
            })
            .collect();
        if endpoints.iter().any(|e| {
            !e.templates.is_empty() || !e.exceptions.is_empty() || e.signing_name.is_some()
        }) {
            report.endpoint_rules += 1;
        }
        report.services += 1;
        report.operations += d.operations.len();
        report.shapes += d.shapes.len();
        report.raw_bytes += raw.len();
        if raw.len() > report.largest.1 {
            report.largest = (name.clone(), raw.len());
        }
        entries.push(ServiceEntry {
            name: name.clone(),
            service_id: it.get(d.header.service_id).to_owned(),
            endpoint_prefix: prefix,
            signing_name: signing,
            protocol: d.header.protocol,
            operations: d.operations.len() as u32,
            endpoints,
            offset: blobs.len() as u32,
            len: packed.len() as u32,
            raw_len: raw.len() as u32,
        });
        blobs.extend_from_slice(&packed);
    }

    let partitions: Vec<Partition> = parts
        .iter()
        .map(|p| Partition {
            id: p.id.clone(),
            dns_suffix: p.dns_suffix.clone(),
            implicit_global_region: p.implicit_global_region.clone(),
            regions: p.regions.clone(),
        })
        .collect();
    let mut h = Writer::default();
    h.str(snapshot);
    h.len(partitions.len());
    for p in &partitions {
        p.encode(&mut h);
    }
    h.len(entries.len());
    for e in &entries {
        e.encode(&mut h);
    }
    let header = compress(&h.buf);

    let mut out = Writer::default();
    out.buf.extend_from_slice(MAGIC);
    out.u8(FORMAT_VERSION);
    out.len(h.buf.len());
    out.bytes(&header);
    out.buf.extend_from_slice(&blobs);
    report.compressed_bytes = out.buf.len();
    Ok((out.buf, report))
}
