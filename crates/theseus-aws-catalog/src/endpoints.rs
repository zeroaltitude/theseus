//! Endpoints, from the partition table and each service's rule set as the
//! generator resolved them (AWS design §3.1). At run time this is a lookup,
//! never a rules engine: a URL template per partition when the service's
//! endpoint is not the default `https://{prefix}.{region}.{dnsSuffix}`, a
//! fixed signing region for global services, and the regions that differ.

use crate::wire::{Reader, Writer};
use crate::CatalogError;

/// One partition: its DNS suffix and its regions at the snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Partition {
    pub id: String,
    pub dns_suffix: String,
    pub implicit_global_region: String,
    pub regions: Vec<String>,
}

/// A service's endpoint in one partition, when it is not the default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Template {
    pub(crate) partition: u32,
    /// The URL with the region as `{region}`, or `None` for the default.
    pub(crate) url: Option<String>,
    /// The region every request signs for (a global service's), if fixed.
    pub(crate) signing_region: Option<String>,
}

/// A region whose endpoint differs from its partition's template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Exception {
    pub(crate) region: String,
    pub(crate) url: String,
    pub(crate) signing_region: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct EndpointRule {
    /// The rule set's signing name, when it differs from the model's.
    pub(crate) signing_name: Option<String>,
    pub(crate) templates: Vec<Template>,
    pub(crate) exceptions: Vec<Exception>,
}

/// Where a request goes, and how it is signed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// `https://host` with no trailing slash.
    pub url: String,
    pub signing_region: String,
    pub signing_name: String,
    pub partition: String,
}

impl Endpoint {
    /// The host, without the scheme.
    pub fn host(&self) -> &str {
        let rest = self
            .url
            .split_once("://")
            .map_or(self.url.as_str(), |(_, r)| r);
        rest.split('/').next().unwrap_or(rest)
    }
}

fn w_opt(w: &mut Writer, s: &Option<String>) {
    match s {
        None => w.u8(0),
        Some(s) => {
            w.u8(1);
            w.str(s);
        }
    }
}

fn r_opt(r: &mut Reader<'_>) -> Result<Option<String>, CatalogError> {
    Ok(match r.u8()? {
        0 => None,
        _ => Some(r.str()?.to_owned()),
    })
}

impl Partition {
    pub(crate) fn encode(&self, w: &mut Writer) {
        w.str(&self.id);
        w.str(&self.dns_suffix);
        w.str(&self.implicit_global_region);
        w.len(self.regions.len());
        for r in &self.regions {
            w.str(r);
        }
    }

    pub(crate) fn decode(r: &mut Reader<'_>) -> Result<Partition, CatalogError> {
        let id = r.str()?.to_owned();
        let dns_suffix = r.str()?.to_owned();
        let implicit_global_region = r.str()?.to_owned();
        let n = r.len()?;
        let mut regions = Vec::with_capacity(n);
        for _ in 0..n {
            regions.push(r.str()?.to_owned());
        }
        Ok(Partition {
            id,
            dns_suffix,
            implicit_global_region,
            regions,
        })
    }
}

impl EndpointRule {
    pub(crate) fn encode(&self, w: &mut Writer) {
        w_opt(w, &self.signing_name);
        w.len(self.templates.len());
        for t in &self.templates {
            w.uv(u64::from(t.partition));
            w_opt(w, &t.url);
            w_opt(w, &t.signing_region);
        }
        w.len(self.exceptions.len());
        for e in &self.exceptions {
            w.str(&e.region);
            w.str(&e.url);
            w_opt(w, &e.signing_region);
        }
    }

    pub(crate) fn decode(r: &mut Reader<'_>) -> Result<EndpointRule, CatalogError> {
        let signing_name = r_opt(r)?;
        let nt = r.len()?;
        let mut templates = Vec::with_capacity(nt);
        for _ in 0..nt {
            templates.push(Template {
                partition: r.u32()?,
                url: r_opt(r)?,
                signing_region: r_opt(r)?,
            });
        }
        let ne = r.len()?;
        let mut exceptions = Vec::with_capacity(ne);
        for _ in 0..ne {
            exceptions.push(Exception {
                region: r.str()?.to_owned(),
                url: r.str()?.to_owned(),
                signing_region: r_opt(r)?,
            });
        }
        Ok(EndpointRule {
            signing_name,
            templates,
            exceptions,
        })
    }
}

/// The partition of a region: its own list first, then the partition whose
/// regions share the longest prefix with it (a region newer than the
/// snapshot), then `aws`.
pub(crate) fn partition_index(parts: &[Partition], region: &str) -> Option<usize> {
    if let Some(i) = parts
        .iter()
        .position(|p| p.regions.iter().any(|r| r == region))
    {
        return Some(i);
    }
    let common = |a: &str, b: &str| a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count();
    let best = parts
        .iter()
        .enumerate()
        .map(|(i, p)| {
            (
                p.regions
                    .iter()
                    .map(|r| common(r, region))
                    .max()
                    .unwrap_or(0),
                p.id == "aws",
                i,
            )
        })
        .max();
    match best {
        Some((n, _, i)) if n >= 3 => Some(i),
        _ => parts.iter().position(|p| p.id == "aws"),
    }
}

/// A region name is lowercase letters, digits, and dashes; anything else
/// would be pasted into a host name.
pub(crate) fn valid_region(region: &str) -> bool {
    !region.is_empty()
        && region.len() <= 64
        && region
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

pub(crate) fn resolve(
    parts: &[Partition],
    rule: &EndpointRule,
    endpoint_prefix: &str,
    signing_name: &str,
    region: &str,
) -> Result<Endpoint, CatalogError> {
    if !valid_region(region) {
        return Err(CatalogError::BadRegion(region.to_owned()));
    }
    let pi =
        partition_index(parts, region).ok_or_else(|| CatalogError::BadRegion(region.to_owned()))?;
    let part = &parts[pi];
    let signing_name = rule
        .signing_name
        .clone()
        .unwrap_or_else(|| signing_name.to_owned());
    if let Some(e) = rule.exceptions.iter().find(|e| e.region == region) {
        return Ok(Endpoint {
            url: e.url.clone(),
            signing_region: e
                .signing_region
                .clone()
                .unwrap_or_else(|| region.to_owned()),
            signing_name,
            partition: part.id.clone(),
        });
    }
    let t = rule.templates.iter().find(|t| t.partition as usize == pi);
    let url = match t.and_then(|t| t.url.as_deref()) {
        Some(u) => u.replace("{region}", region),
        None => format!("https://{endpoint_prefix}.{region}.{}", part.dns_suffix),
    };
    Ok(Endpoint {
        url,
        signing_region: t
            .and_then(|t| t.signing_region.clone())
            .unwrap_or_else(|| region.to_owned()),
        signing_name,
        partition: part.id.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts() -> Vec<Partition> {
        vec![
            Partition {
                id: "aws".into(),
                dns_suffix: "amazonaws.com".into(),
                implicit_global_region: "us-east-1".into(),
                regions: vec!["us-east-1".into(), "us-west-2".into()],
            },
            Partition {
                id: "aws-cn".into(),
                dns_suffix: "amazonaws.com.cn".into(),
                implicit_global_region: "cn-northwest-1".into(),
                regions: vec!["cn-north-1".into()],
            },
        ]
    }

    #[test]
    fn the_default_and_a_global_service() {
        let parts = parts();
        let none = EndpointRule::default();
        let e = resolve(&parts, &none, "lambda", "lambda", "us-west-2").unwrap();
        assert_eq!(e.url, "https://lambda.us-west-2.amazonaws.com");
        assert_eq!(e.signing_region, "us-west-2");
        assert_eq!(e.host(), "lambda.us-west-2.amazonaws.com");
        let cn = resolve(&parts, &none, "lambda", "lambda", "cn-north-1").unwrap();
        assert_eq!(cn.url, "https://lambda.cn-north-1.amazonaws.com.cn");

        let iam = EndpointRule {
            signing_name: None,
            templates: vec![Template {
                partition: 0,
                url: Some("https://iam.amazonaws.com".into()),
                signing_region: Some("us-east-1".into()),
            }],
            exceptions: vec![],
        };
        let e = resolve(&parts, &iam, "iam", "iam", "us-west-2").unwrap();
        assert_eq!(e.url, "https://iam.amazonaws.com");
        assert_eq!(e.signing_region, "us-east-1");
    }

    #[test]
    fn a_region_that_is_not_a_name_is_refused() {
        let parts = parts();
        let none = EndpointRule::default();
        for bad in ["", "us-west-2.evil.example", "US-WEST-2", "a/b"] {
            assert!(resolve(&parts, &none, "s3", "s3", bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_rule_round_trips() {
        let rule = EndpointRule {
            signing_name: Some("execute-api".into()),
            templates: vec![Template {
                partition: 1,
                url: None,
                signing_region: Some("cn-northwest-1".into()),
            }],
            exceptions: vec![Exception {
                region: "us-east-1".into(),
                url: "https://x.example".into(),
                signing_region: None,
            }],
        };
        let mut w = Writer::default();
        rule.encode(&mut w);
        let mut r = Reader::new(&w.buf);
        assert_eq!(EndpointRule::decode(&mut r).unwrap(), rule);
        assert!(r.done());
    }
}
