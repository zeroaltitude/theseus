//! The AWS catalog (AWS design §3.1, §4): every operation of every AWS
//! service, compiled offline from the AWS CLI's botocore models into a compact
//! table, compressed with brotli (a pure-Rust decoder), and embedded in the
//! binary.
//!
//! Nothing here runs at start: the header decodes on the first lookup, and a
//! service decodes on its first use, then stays cached. Each operation's
//! class (Read, Write, Run), retry class, and flags (cost-bearing,
//! secret-bearing, IaC-only, idempotent through a token) come from the
//! model's traits, its HTTP binding, and its name, settled by the tables in
//! [`classify`] where the models say nothing.
//!
//! The generator is `theseus-aws-catalog-gen` (see [`compile`]).

use std::sync::{Arc, OnceLock};

mod classify;
pub mod compile;
mod describe;
mod endpoints;
mod json;
mod model;
mod rules;
mod tables;
mod wire;

pub use classify::{Class, ClassReason, Classification, RetryClass, SecretBearing};
pub use describe::{describe_operation, describe_service, describe_services, input_schema};
pub use endpoints::{Endpoint, Partition};
pub use model::{
    Auth, Kind, Location, MemberRef, Method, OperationRef, Paginator, Protocol, Service, ShapeId,
    ShapeRef, Signature, TimestampFormat,
};

use endpoints::EndpointRule;
use wire::{Reader, Writer};

pub(crate) const MAGIC: &[u8; 8] = b"THAWSCAT";
pub(crate) const FORMAT_VERSION: u8 = 1;

/// The catalog the binary carries, generated from the AWS CLI's models.
static EMBEDDED: &[u8] = include_bytes!("../data/aws-catalog.bin");

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CatalogError {
    #[error("the AWS catalog is corrupt: {0}")]
    Corrupt(&'static str),
    #[error("the AWS catalog's format is version {0}; this build reads version {FORMAT_VERSION}")]
    Version(u8),
    #[error("no AWS service is named {0:?} (aws.describe lists them)")]
    UnknownService(String),
    #[error("{0:?} is not a region name")]
    BadRegion(String),
}

/// A service's entry in the catalog's index: what is known without decoding
/// the service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceEntry {
    /// botocore's name, which the CLI uses (`s3`, `stepfunctions`).
    pub name: String,
    /// The SDKs' service id (`S3`, `SFN`).
    pub service_id: String,
    pub endpoint_prefix: String,
    pub signing_name: String,
    pub protocol: Protocol,
    pub operations: u32,
    /// The endpoint variants: 0 for operations without static context
    /// parameters, then one per distinct set of them.
    pub(crate) endpoints: Vec<EndpointRule>,
    pub(crate) offset: u32,
    pub(crate) len: u32,
    pub(crate) raw_len: u32,
}

impl ServiceEntry {
    pub(crate) fn encode(&self, w: &mut Writer) {
        w.str(&self.name);
        w.str(&self.service_id);
        w.str(&self.endpoint_prefix);
        w.str(&self.signing_name);
        w.str(self.protocol.as_str());
        w.uv(u64::from(self.operations));
        w.len(self.endpoints.len());
        for e in &self.endpoints {
            e.encode(w);
        }
        w.uv(u64::from(self.offset));
        w.uv(u64::from(self.len));
        w.uv(u64::from(self.raw_len));
    }

    fn decode(r: &mut Reader<'_>) -> Result<ServiceEntry, CatalogError> {
        let name = r.str()?.to_owned();
        let service_id = r.str()?.to_owned();
        let endpoint_prefix = r.str()?.to_owned();
        let signing_name = r.str()?.to_owned();
        let protocol =
            Protocol::from_botocore(r.str()?).ok_or(CatalogError::Corrupt("protocol"))?;
        let operations = r.u32()?;
        let n = r.len()?;
        if n == 0 {
            return Err(CatalogError::Corrupt("a service without an endpoint"));
        }
        let mut endpoints = Vec::with_capacity(n);
        for _ in 0..n {
            endpoints.push(EndpointRule::decode(r)?);
        }
        Ok(ServiceEntry {
            name,
            service_id,
            endpoint_prefix,
            signing_name,
            protocol,
            operations,
            endpoints,
            offset: r.u32()?,
            len: r.u32()?,
            raw_len: r.u32()?,
        })
    }

    /// The service's compressed size in the catalog.
    pub fn compressed_len(&self) -> usize {
        self.len as usize
    }
}

/// The catalog: an index of services, and their blobs, decoded on demand.
pub struct Catalog {
    bytes: Arc<[u8]>,
    blobs_at: usize,
    snapshot: String,
    partitions: Vec<Partition>,
    /// Sorted by name.
    entries: Vec<ServiceEntry>,
    cache: Vec<OnceLock<Result<Arc<Service>, CatalogError>>>,
}

impl std::fmt::Debug for Catalog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Catalog")
            .field("snapshot", &self.snapshot)
            .field("services", &self.entries.len())
            .field("bytes", &self.bytes.len())
            .finish()
    }
}

/// Decompresses one block, which must come to exactly `raw_len` bytes.
pub(crate) fn unbrotli(input: &[u8], raw_len: usize) -> Result<Vec<u8>, CatalogError> {
    use std::io::Read;
    let mut out = Vec::with_capacity(raw_len);
    brotli_decompressor::Decompressor::new(input, 16 * 1024)
        .take(raw_len as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|_| CatalogError::Corrupt("a compressed block does not decode"))?;
    if out.len() != raw_len {
        return Err(CatalogError::Corrupt("a block decodes to the wrong length"));
    }
    Ok(out)
}

impl Catalog {
    /// The catalog embedded in the binary, its index decoded on first use.
    pub fn embedded() -> Result<&'static Catalog, CatalogError> {
        static CATALOG: OnceLock<Result<Catalog, CatalogError>> = OnceLock::new();
        CATALOG
            .get_or_init(|| Catalog::from_bytes(Arc::from(EMBEDDED)))
            .as_ref()
            .map_err(Clone::clone)
    }

    /// The embedded catalog's size in the binary, compressed.
    pub fn embedded_len() -> usize {
        EMBEDDED.len()
    }

    /// A catalog from bytes the generator wrote.
    pub fn from_bytes(bytes: Arc<[u8]>) -> Result<Catalog, CatalogError> {
        let mut r = Reader::new(&bytes);
        if r.take(MAGIC.len())? != MAGIC {
            return Err(CatalogError::Corrupt("not an AWS catalog"));
        }
        let version = r.u8()?;
        if version != FORMAT_VERSION {
            return Err(CatalogError::Version(version));
        }
        let raw_len = r.uv()?;
        if raw_len > 64 << 20 {
            return Err(CatalogError::Corrupt("the header is implausibly large"));
        }
        let header = r.bytes()?;
        // What follows the header is the services' blobs.
        let blobs_at = r.pos();
        let raw = unbrotli(header, raw_len as usize)?;
        let mut h = Reader::new(&raw);
        let snapshot = h.str()?.to_owned();
        let np = h.len()?;
        let mut partitions = Vec::with_capacity(np);
        for _ in 0..np {
            partitions.push(Partition::decode(&mut h)?);
        }
        let ns = h.len()?;
        let mut entries = Vec::with_capacity(ns);
        for _ in 0..ns {
            let e = ServiceEntry::decode(&mut h)?;
            let end = blobs_at as u64 + u64::from(e.offset) + u64::from(e.len);
            if end > bytes.len() as u64 {
                return Err(CatalogError::Corrupt("a service lies past the end"));
            }
            entries.push(e);
        }
        if !h.done() {
            return Err(CatalogError::Corrupt("trailing bytes in the header"));
        }
        if !entries.windows(2).all(|w| w[0].name < w[1].name) {
            return Err(CatalogError::Corrupt("services out of order"));
        }
        let cache = (0..entries.len()).map(|_| OnceLock::new()).collect();
        Ok(Catalog {
            bytes,
            blobs_at,
            snapshot,
            partitions,
            entries,
            cache,
        })
    }

    /// Where the models came from (`aws-cli/2.34.15`).
    pub fn snapshot(&self) -> &str {
        &self.snapshot
    }

    pub fn partitions(&self) -> &[Partition] {
        &self.partitions
    }

    /// Every service, by name, without decoding any.
    pub fn services(&self) -> &[ServiceEntry] {
        &self.entries
    }

    fn index(&self, name: &str) -> Option<usize> {
        if let Ok(i) = self.entries.binary_search_by(|e| e.name.as_str().cmp(name)) {
            return Some(i);
        }
        // An alias: the service id (`SFN`, `Cost Explorer`), the signing
        // name (`states`), or the endpoint prefix (`monitoring`), when only
        // one service answers to it.
        let norm = |s: &str| s.to_ascii_lowercase().replace([' ', '_'], "-");
        let want = norm(name);
        let hits: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                norm(&e.service_id) == want || e.signing_name == name || e.endpoint_prefix == name
            })
            .map(|(i, _)| i)
            .collect();
        (hits.len() == 1).then(|| hits[0])
    }

    pub fn entry(&self, name: &str) -> Option<&ServiceEntry> {
        self.index(name).map(|i| &self.entries[i])
    }

    /// A service, decoded on its first use and cached after.
    pub fn service(&self, name: &str) -> Result<Arc<Service>, CatalogError> {
        let i = self
            .index(name)
            .ok_or_else(|| CatalogError::UnknownService(name.to_owned()))?;
        self.cache[i]
            .get_or_init(|| self.decode(i).map(Arc::new))
            .clone()
    }

    /// Decodes a service without the cache (for measuring it).
    pub fn decode_uncached(&self, name: &str) -> Result<Service, CatalogError> {
        let i = self
            .index(name)
            .ok_or_else(|| CatalogError::UnknownService(name.to_owned()))?;
        self.decode(i)
    }

    fn decode(&self, i: usize) -> Result<Service, CatalogError> {
        let e = &self.entries[i];
        let start = self.blobs_at + e.offset as usize;
        let raw = unbrotli(
            &self.bytes[start..start + e.len as usize],
            e.raw_len as usize,
        )?;
        let svc = Service::decode(&raw)?;
        if svc
            .operations()
            .any(|o| o.endpoint_variant() >= e.endpoints.len())
        {
            return Err(CatalogError::Corrupt("an operation's endpoint variant"));
        }
        Ok(svc)
    }

    /// Where a service's requests go in a region, and how they are signed:
    /// its operations' endpoint when they set no static context parameter,
    /// which is nearly every service's only one. A client resolves each
    /// operation with [`Catalog::operation_endpoint`].
    pub fn endpoint(&self, service: &str, region: &str) -> Result<Endpoint, CatalogError> {
        let e = self
            .entry(service)
            .ok_or_else(|| CatalogError::UnknownService(service.to_owned()))?;
        endpoints::resolve(
            &self.partitions,
            &e.endpoints[0],
            &e.endpoint_prefix,
            &e.signing_name,
            region,
        )
    }

    /// Where one operation's requests go in a region: its service's endpoint
    /// for the static context parameters the operation sets (Neptune
    /// Analytics' control and data planes are different hosts).
    pub fn operation_endpoint(
        &self,
        op: OperationRef<'_>,
        region: &str,
    ) -> Result<Endpoint, CatalogError> {
        let name = op.service().name();
        let e = self
            .entry(name)
            .ok_or_else(|| CatalogError::UnknownService(name.to_owned()))?;
        let rule = e
            .endpoints
            .get(op.endpoint_variant())
            .ok_or(CatalogError::Corrupt("an operation's endpoint variant"))?;
        endpoints::resolve(
            &self.partitions,
            rule,
            &e.endpoint_prefix,
            &e.signing_name,
            region,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::{compile_models, ModelFiles};
    use crate::json::J;
    use crate::rules::PartitionDef;

    /// The generator's encoder, for tests.
    pub(crate) fn brotli(raw: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut out = Vec::new();
        {
            let mut w = ::brotli::CompressorWriter::new(&mut out, 4096, 11, 20);
            w.write_all(raw).unwrap();
        }
        out
    }

    pub(crate) fn tiny_parts() -> Vec<PartitionDef> {
        PartitionDef::parse(
            &J::parse(
                r#"{"partitions": [{"id": "aws", "outputs": {"dnsSuffix": "amazonaws.com",
                "dualStackDnsSuffix": "api.aws", "implicitGlobalRegion": "us-east-1",
                "supportsFIPS": true, "supportsDualStack": true},
                "regions": {"us-east-1": {}, "us-west-2": {}}}]}"#,
            )
            .unwrap(),
        )
    }

    pub(crate) const TINY_MODEL: &str = r#"{
      "version": "2.0",
      "metadata": {"apiVersion": "2020-01-01", "endpointPrefix": "widgets", "protocol": "json",
        "jsonVersion": "1.1", "serviceFullName": "Example Widgets", "serviceId": "Widgets",
        "signatureVersion": "v4", "targetPrefix": "Widgets_20200101", "uid": "widgets-2020-01-01"},
      "operations": {
        "ListWidgets": {"name": "ListWidgets", "http": {"method": "POST", "requestUri": "/"},
          "input": {"shape": "ListWidgetsRequest"}, "output": {"shape": "ListWidgetsResponse"},
          "documentation": "<p>Lists.</p>"},
        "CreateWidget": {"name": "CreateWidget", "http": {"method": "POST", "requestUri": "/"},
          "input": {"shape": "CreateWidgetRequest"}, "output": {"shape": "Widget"},
          "errors": [{"shape": "Throttled"}]}
      },
      "shapes": {
        "ListWidgetsRequest": {"type": "structure", "members": {
          "NextToken": {"shape": "Token"}, "MaxResults": {"shape": "Count"}}},
        "ListWidgetsResponse": {"type": "structure", "members": {
          "Widgets": {"shape": "WidgetList"}, "NextToken": {"shape": "Token"}}},
        "CreateWidgetRequest": {"type": "structure", "required": ["Name"], "members": {
          "Name": {"shape": "Name"}, "ClientToken": {"shape": "Token", "idempotencyToken": true},
          "Color": {"shape": "Color"}}},
        "Widget": {"type": "structure", "members": {"Name": {"shape": "Name"}, "Secret": {"shape": "Secret"}}},
        "WidgetList": {"type": "list", "member": {"shape": "Widget"}},
        "Token": {"type": "string", "max": 64},
        "Count": {"type": "integer", "min": 1, "max": 100},
        "Name": {"type": "string", "documentation": "A name."},
        "Secret": {"type": "string", "sensitive": true},
        "Color": {"type": "string", "enum": ["red", "green"]},
        "Throttled": {"type": "structure", "members": {"message": {"shape": "Name"}},
          "error": {"code": "ThrottledCode", "httpStatusCode": 400, "senderFault": true},
          "exception": true, "retryable": {"throttling": true}}
      }
    }"#;

    pub(crate) const TINY_PAGINATORS: &str = r#"{"pagination": {"ListWidgets": {
      "input_token": "NextToken", "output_token": "NextToken", "limit_key": "MaxResults",
      "result_key": "Widgets"}}}"#;

    fn tiny_catalog() -> Catalog {
        let files = ModelFiles {
            service: TINY_MODEL.into(),
            paginators: Some(TINY_PAGINATORS.into()),
            ..Default::default()
        };
        let (bytes, report) = compile_models(
            &[("widgets".into(), files)],
            &tiny_parts(),
            "test-snapshot",
            &brotli,
        )
        .unwrap();
        assert_eq!(report.services, 1);
        assert_eq!(report.operations, 2);
        Catalog::from_bytes(bytes.into()).unwrap()
    }

    #[test]
    fn a_compiled_catalog_reads_back() {
        let c = tiny_catalog();
        assert_eq!(c.snapshot(), "test-snapshot");
        assert_eq!(c.services().len(), 1);
        let e = &c.services()[0];
        assert_eq!(
            (e.name.as_str(), e.protocol, e.operations),
            ("widgets", Protocol::Json, 2)
        );
        let svc = c.service("widgets").unwrap();
        assert_eq!(svc.target_prefix(), Some("Widgets_20200101"));
        assert_eq!(svc.json_version(), Some("1.1"));
        // Sorted for lookup, and found by any of its spellings.
        let names: Vec<&str> = svc.operations().map(|o| o.name()).collect();
        assert_eq!(names, ["CreateWidget", "ListWidgets"]);
        for spelling in ["ListWidgets", "listwidgets", "list-widgets"] {
            assert_eq!(svc.operation(spelling).unwrap().name(), "ListWidgets");
        }
        let create = svc.operation("CreateWidget").unwrap();
        let input = create.input().unwrap();
        let members: Vec<&str> = input.members().map(|m| m.name()).collect();
        assert_eq!(
            members,
            ["Name", "ClientToken", "Color"],
            "declaration order is kept"
        );
        assert!(input.member("Name").unwrap().is_required());
        assert_eq!(create.idempotency_token().unwrap().name(), "ClientToken");
        let color = input.member("Color").unwrap().shape();
        assert_eq!(color.enum_values().collect::<Vec<_>>(), ["red", "green"]);
        let throttled = create.errors().next().unwrap();
        assert_eq!(throttled.error_code(), Some("ThrottledCode"));
        assert_eq!(throttled.retryable(), Some(true));
        assert!(throttled.sender_fault());
        let p = svc.operation("ListWidgets").unwrap().paginator().unwrap();
        assert_eq!(p.input_tokens(), ["NextToken"]);
        assert_eq!(p.result_keys(), ["Widgets"]);
        assert_eq!(p.limit_key(), Some("MaxResults"));
        // The same Arc comes back from the cache.
        assert!(Arc::ptr_eq(&svc, &c.service("widgets").unwrap()));
        assert_eq!(c.service("Widgets").unwrap().name(), "widgets");
        assert!(matches!(
            c.service("nope"),
            Err(CatalogError::UnknownService(_))
        ));
    }

    #[test]
    fn the_default_endpoint_when_there_is_no_rule_set() {
        let c = tiny_catalog();
        let e = c.endpoint("widgets", "us-west-2").unwrap();
        assert_eq!(e.url, "https://widgets.us-west-2.amazonaws.com");
        assert_eq!(e.signing_region, "us-west-2");
        assert_eq!(e.signing_name, "widgets");
        assert_eq!(e.partition, "aws");
    }

    /// A rule set that sends the control plane and the data plane to
    /// different hosts, by a static context parameter (Neptune Analytics').
    const PLANES: &str = r#"{"version": "1.0",
      "parameters": {"Region": {"type": "String"}, "UseFIPS": {"type": "Boolean", "default": false},
                     "UseDualStack": {"type": "Boolean", "default": false},
                     "ApiType": {"type": "String", "required": true}},
      "rules": [
        {"conditions": [{"fn": "aws.partition", "argv": [{"ref": "Region"}], "assign": "P"}],
         "type": "tree", "rules": [
          {"conditions": [{"fn": "stringEquals", "argv": [{"ref": "ApiType"}, "ControlPlane"]}],
           "type": "endpoint", "endpoint": {"url": "https://graphs.{Region}.on.aws"}},
          {"conditions": [{"fn": "stringEquals", "argv": [{"ref": "ApiType"}, "DataPlane"]}],
           "type": "endpoint", "endpoint": {"url": "https://{Region}.graphs.{P#dnsSuffix}"}},
          {"conditions": [], "type": "error", "error": "Invalid Configuration: Unknown ApiType"}]}]}"#;

    #[test]
    fn an_operations_static_parameters_choose_its_endpoint() {
        let model = r#"{"version": "2.0",
          "metadata": {"apiVersion": "2020-01-01", "endpointPrefix": "graphs", "protocol": "rest-json",
            "serviceId": "Graphs", "signatureVersion": "v4", "uid": "graphs-2020-01-01"},
          "operations": {
            "ListGraphs": {"name": "ListGraphs", "http": {"method": "GET", "requestUri": "/graphs"},
              "staticContextParams": {"ApiType": {"value": "ControlPlane"}}},
            "ExecuteQuery": {"name": "ExecuteQuery", "http": {"method": "POST", "requestUri": "/queries"},
              "staticContextParams": {"ApiType": {"value": "DataPlane"}},
              "endpoint": {"hostPrefix": "{graphIdentifier}."}},
            "GetGraph": {"name": "GetGraph", "http": {"method": "GET", "requestUri": "/graphs/x"},
              "staticContextParams": {"ApiType": {"value": "ControlPlane"}}}
          },
          "shapes": {}}"#;
        let files = ModelFiles {
            service: model.into(),
            ruleset: Some(PLANES.into()),
            ..Default::default()
        };
        let (bytes, report) =
            compile_models(&[("graphs".into(), files)], &tiny_parts(), "t", &brotli).unwrap();
        // No operation goes without an `ApiType`, so the variant that would
        // fail to resolve is never resolved.
        assert!(
            report.endpoint_failures.is_empty(),
            "{:?}",
            report.endpoint_failures
        );
        let c = Catalog::from_bytes(bytes.into()).unwrap();
        let svc = c.service("graphs").unwrap();
        let ep = |op: &str| {
            c.operation_endpoint(svc.operation(op).unwrap(), "us-west-2")
                .unwrap()
                .url
        };
        assert_eq!(ep("ListGraphs"), "https://graphs.us-west-2.on.aws");
        assert_eq!(ep("GetGraph"), "https://graphs.us-west-2.on.aws");
        assert_eq!(ep("ExecuteQuery"), "https://us-west-2.graphs.amazonaws.com");
        // Two operations with the same parameters share a variant.
        let variant = |op: &str| svc.operation(op).unwrap().endpoint_variant();
        assert_eq!(variant("ListGraphs"), variant("GetGraph"));
        assert_ne!(variant("ListGraphs"), variant("ExecuteQuery"));
    }

    #[test]
    fn corrupt_bytes_are_an_error() {
        assert!(Catalog::from_bytes(Arc::from(&b"nope"[..])).is_err());
        let c = tiny_catalog();
        let mut bytes = c.bytes.to_vec();
        bytes[8] = 99;
        assert_eq!(
            Catalog::from_bytes(bytes.into()).err(),
            Some(CatalogError::Version(99))
        );
    }
}
