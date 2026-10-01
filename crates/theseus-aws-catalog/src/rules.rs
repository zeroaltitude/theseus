//! A small evaluator for botocore's endpoint rule sets, run by the generator
//! only (AWS design §3.1: no rules engine at run time).
//!
//! The generator evaluates each service's rule set for every region of every
//! partition, with FIPS and dual-stack off and no endpoint override, and the
//! catalog keeps only the result: a URL template per partition and the
//! regions that differ. It covers the twelve functions the 2.34.15 models use.

use std::collections::HashMap;

use crate::json::J;

/// One partition from `partitions.json`.
#[derive(Debug, Clone)]
pub(crate) struct PartitionDef {
    pub(crate) id: String,
    pub(crate) dns_suffix: String,
    pub(crate) dual_stack_dns_suffix: String,
    pub(crate) implicit_global_region: String,
    pub(crate) supports_fips: bool,
    pub(crate) supports_dual_stack: bool,
    pub(crate) regions: Vec<String>,
}

impl PartitionDef {
    pub(crate) fn parse(partitions_json: &J) -> Vec<PartitionDef> {
        partitions_json
            .get("partitions")
            .map(J::arr)
            .unwrap_or(&[])
            .iter()
            .filter_map(|p| {
                let out = p.get("outputs")?;
                Some(PartitionDef {
                    id: p.str_at("id")?.to_owned(),
                    dns_suffix: out.str_at("dnsSuffix")?.to_owned(),
                    dual_stack_dns_suffix: out
                        .str_at("dualStackDnsSuffix")
                        .unwrap_or_default()
                        .to_owned(),
                    implicit_global_region: out
                        .str_at("implicitGlobalRegion")
                        .unwrap_or_default()
                        .to_owned(),
                    supports_fips: out.bool_at("supportsFIPS"),
                    supports_dual_stack: out.bool_at("supportsDualStack"),
                    regions: p
                        .get("regions")
                        .map(J::obj)
                        .unwrap_or(&[])
                        .iter()
                        .map(|(r, _)| r.clone())
                        .collect(),
                })
            })
            .collect()
    }

    fn outputs(&self) -> J {
        J::Obj(vec![
            ("name".into(), J::Str(self.id.clone())),
            ("dnsSuffix".into(), J::Str(self.dns_suffix.clone())),
            (
                "dualStackDnsSuffix".into(),
                J::Str(self.dual_stack_dns_suffix.clone()),
            ),
            ("supportsFIPS".into(), J::Bool(self.supports_fips)),
            (
                "supportsDualStack".into(),
                J::Bool(self.supports_dual_stack),
            ),
            (
                "implicitGlobalRegion".into(),
                J::Str(self.implicit_global_region.clone()),
            ),
        ])
    }
}

/// The partition a region belongs to: its own list first, then the
/// partition with the region sharing the longest prefix (a region newer than
/// the snapshot), then `aws`.
pub(crate) fn partition_of<'p>(
    parts: &'p [PartitionDef],
    region: &str,
) -> Option<&'p PartitionDef> {
    if let Some(p) = parts.iter().find(|p| p.regions.iter().any(|r| r == region)) {
        return Some(p);
    }
    let common = |a: &str, b: &str| a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count();
    let best = parts
        .iter()
        .map(|p| {
            (
                p.regions
                    .iter()
                    .map(|r| common(r, region))
                    .max()
                    .unwrap_or(0),
                p,
            )
        })
        .max_by_key(|(n, p)| (*n, p.id == "aws"));
    match best {
        Some((n, p)) if n >= 3 => Some(p),
        _ => parts.iter().find(|p| p.id == "aws"),
    }
}

/// What a rule set resolved to.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Resolved {
    pub(crate) url: String,
    pub(crate) signing_region: Option<String>,
    pub(crate) signing_name: Option<String>,
}

pub(crate) struct Evaluator<'a> {
    parts: &'a [PartitionDef],
}

type Env = HashMap<String, J>;

impl<'a> Evaluator<'a> {
    pub(crate) fn new(parts: &'a [PartitionDef]) -> Self {
        Evaluator { parts }
    }

    /// Resolves the endpoint for `region` with every other parameter at its
    /// default (FIPS and dual-stack off, no override).
    #[cfg(test)]
    pub(crate) fn resolve(&self, ruleset: &J, region: &str) -> Result<Resolved, String> {
        self.resolve_with(ruleset, region, &[])
    }

    /// Resolves the endpoint for `region` with an operation's static context
    /// parameters (`ApiType = DataPlane`) set, and every other parameter at
    /// its default.
    pub(crate) fn resolve_with(
        &self,
        ruleset: &J,
        region: &str,
        params: &[(String, J)],
    ) -> Result<Resolved, String> {
        let mut env = Env::new();
        for (name, spec) in ruleset.get("parameters").map(J::obj).unwrap_or(&[]) {
            if let Some(d) = spec.get("default") {
                env.insert(name.clone(), d.clone());
            }
        }
        for (name, v) in params {
            env.insert(name.clone(), v.clone());
        }
        env.insert("Region".into(), J::Str(region.into()));
        env.insert("UseFIPS".into(), J::Bool(false));
        env.insert("UseDualStack".into(), J::Bool(false));
        let rules = ruleset.get("rules").map(J::arr).unwrap_or(&[]);
        match self.rules(rules, &env)? {
            Some(ep) => Ok(ep),
            None => Err("no rule matched".into()),
        }
    }

    fn rules(&self, rules: &[J], env: &Env) -> Result<Option<Resolved>, String> {
        for rule in rules {
            let mut scope = env.clone();
            if !self.conditions(rule, &mut scope)? {
                continue;
            }
            return match rule.str_at("type") {
                Some("endpoint") => self
                    .endpoint(rule.get("endpoint").unwrap_or(&J::Null), &scope)
                    .map(Some),
                Some("error") => Err(match rule.get("error") {
                    Some(e) => self.text(e, &scope)?,
                    None => "error rule".into(),
                }),
                Some("tree") => {
                    match self.rules(rule.get("rules").map(J::arr).unwrap_or(&[]), &scope)? {
                        Some(ep) => Ok(Some(ep)),
                        None => Err("a tree rule matched, but none of its rules".into()),
                    }
                }
                other => Err(format!("unknown rule type {other:?}")),
            };
        }
        Ok(None)
    }

    fn conditions(&self, rule: &J, scope: &mut Env) -> Result<bool, String> {
        for c in rule.get("conditions").map(J::arr).unwrap_or(&[]) {
            let v = self.value(c, scope)?;
            let truthy = !matches!(v, J::Null | J::Bool(false));
            if !truthy {
                return Ok(false);
            }
            if let Some(name) = c.str_at("assign") {
                scope.insert(name.to_owned(), v);
            }
        }
        Ok(true)
    }

    fn endpoint(&self, ep: &J, scope: &Env) -> Result<Resolved, String> {
        let url = match ep.get("url") {
            Some(u) => self.text(u, scope)?,
            None => return Err("an endpoint without a url".into()),
        };
        let mut signing_region = None;
        let mut signing_name = None;
        if let Some(props) = ep.get("properties") {
            let props = self.render(props, scope)?;
            let schemes = props.get("authSchemes").map(J::arr).unwrap_or(&[]);
            // SigV4's scheme; SigV4a's names a region set, not a region.
            let scheme = schemes
                .iter()
                .find(|s| s.str_at("name") == Some("sigv4"))
                .or_else(|| {
                    schemes
                        .iter()
                        .find(|s| s.str_at("name") == Some("sigv4-s3express"))
                });
            if let Some(s) = scheme {
                signing_region = s.str_at("signingRegion").map(str::to_owned);
                signing_name = s.str_at("signingName").map(str::to_owned);
            }
        }
        Ok(Resolved {
            url,
            signing_region,
            signing_name,
        })
    }

    /// A value: a reference, a function call, or a literal (strings are
    /// templates).
    fn value(&self, v: &J, scope: &Env) -> Result<J, String> {
        match v {
            J::Obj(_) if v.get("ref").is_some() => {
                let name = v.str_at("ref").unwrap_or_default();
                Ok(scope.get(name).cloned().unwrap_or(J::Null))
            }
            J::Obj(_) if v.get("fn").is_some() => self.call(v, scope),
            J::Str(s) => Ok(J::Str(Self::template(s, scope)?)),
            other => Ok(other.clone()),
        }
    }

    fn text(&self, v: &J, scope: &Env) -> Result<String, String> {
        match self.value(v, scope)? {
            J::Str(s) => Ok(s),
            other => Err(format!("expected a string, got {other:?}")),
        }
    }

    /// Renders every template inside a properties value.
    fn render(&self, v: &J, scope: &Env) -> Result<J, String> {
        Ok(match v {
            J::Str(s) => J::Str(Self::template(s, scope)?),
            J::Arr(items) => J::Arr(
                items
                    .iter()
                    .map(|i| self.render(i, scope))
                    .collect::<Result<_, _>>()?,
            ),
            J::Obj(_) if v.get("ref").is_some() || v.get("fn").is_some() => self.value(v, scope)?,
            J::Obj(entries) => J::Obj(
                entries
                    .iter()
                    .map(|(k, x)| Ok((k.clone(), self.render(x, scope)?)))
                    .collect::<Result<_, String>>()?,
            ),
            other => other.clone(),
        })
    }

    /// `{Name}` and `{Name#path}` interpolation; `{{` and `}}` escape braces.
    fn template(s: &str, scope: &Env) -> Result<String, String> {
        if !s.contains('{') && !s.contains('}') {
            return Ok(s.to_owned());
        }
        let mut out = String::with_capacity(s.len());
        let mut rest = s;
        while let Some(i) = rest.find(['{', '}']) {
            out.push_str(&rest[..i]);
            let tail = &rest[i..];
            if let Some(t) = tail.strip_prefix("{{") {
                out.push('{');
                rest = t;
            } else if let Some(t) = tail.strip_prefix("}}") {
                out.push('}');
                rest = t;
            } else if tail.starts_with('{') {
                let end = tail
                    .find('}')
                    .ok_or_else(|| format!("unclosed template in {s:?}"))?;
                let expr = &tail[1..end];
                let v = match expr.split_once('#') {
                    Some((name, path)) => get_attr(scope.get(name).unwrap_or(&J::Null), path),
                    None => scope.get(expr).cloned().unwrap_or(J::Null),
                };
                match v {
                    J::Str(x) => out.push_str(&x),
                    other => return Err(format!("template {expr} is {other:?}")),
                }
                rest = &tail[end + 1..];
            } else {
                out.push('}');
                rest = &tail[1..];
            }
        }
        out.push_str(rest);
        Ok(out)
    }

    fn call(&self, f: &J, scope: &Env) -> Result<J, String> {
        let name = f.str_at("fn").unwrap_or_default();
        let args = f
            .get("argv")
            .map(J::arr)
            .unwrap_or(&[])
            .iter()
            .map(|a| self.value(a, scope))
            .collect::<Result<Vec<J>, String>>()?;
        let arg = |i: usize| args.get(i).cloned().unwrap_or(J::Null);
        let s = |i: usize| match args.get(i) {
            Some(J::Str(x)) => Some(x.as_str()),
            _ => None,
        };
        let b = |i: usize| matches!(args.get(i), Some(J::Bool(true)));
        Ok(match name {
            "isSet" => J::Bool(!matches!(arg(0), J::Null)),
            "not" => J::Bool(!b(0)),
            "booleanEquals" => {
                J::Bool(matches!((arg(0), arg(1)), (J::Bool(x), J::Bool(y)) if x == y))
            }
            "stringEquals" => J::Bool(matches!((s(0), s(1)), (Some(x), Some(y)) if x == y)),
            "getAttr" => match s(1) {
                Some(path) => get_attr(&arg(0), path),
                None => J::Null,
            },
            "aws.partition" => match s(0) {
                Some(region) => {
                    partition_of(self.parts, region).map_or(J::Null, PartitionDef::outputs)
                }
                None => J::Null,
            },
            "substring" => match (
                s(0),
                args.get(1).and_then(J::int),
                args.get(2).and_then(J::int),
            ) {
                (Some(text), Some(start), Some(stop)) => substring(text, start, stop, b(3)),
                _ => J::Null,
            },
            "uriEncode" => s(0).map_or(J::Null, |x| J::Str(uri_encode(x))),
            "isValidHostLabel" => J::Bool(s(0).is_some_and(|x| valid_host_label(x, b(1)))),
            "parseURL" => s(0).map_or(J::Null, parse_url),
            "aws.parseArn" => s(0).map_or(J::Null, parse_arn),
            "aws.isVirtualHostableS3Bucket" => {
                J::Bool(s(0).is_some_and(|x| virtual_hostable_bucket(x, b(1))))
            }
            other => return Err(format!("unknown function {other}")),
        })
    }
}

/// `getAttr`: dotted names, each optionally indexed (`authSchemes[0]`).
fn get_attr(v: &J, path: &str) -> J {
    let mut cur = v.clone();
    for part in path.split('.') {
        let (name, index) = match part.find('[') {
            Some(i) => (
                &part[..i],
                part[i + 1..].trim_end_matches(']').parse::<usize>().ok(),
            ),
            None => (part, None),
        };
        if !name.is_empty() {
            cur = cur.get(name).cloned().unwrap_or(J::Null);
        }
        if let Some(i) = index {
            cur = cur.arr().get(i).cloned().unwrap_or(J::Null);
        }
    }
    cur
}

fn substring(text: &str, start: i64, stop: i64, reverse: bool) -> J {
    if !text.is_ascii() || start < 0 || stop <= start || stop as usize > text.len() {
        return J::Null;
    }
    let (start, stop) = (start as usize, stop as usize);
    let (a, b) = if reverse {
        (text.len() - stop, text.len() - start)
    } else {
        (start, stop)
    };
    J::Str(text[a..b].to_owned())
}

pub(crate) fn uri_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn valid_host_label(s: &str, allow_subdomains: bool) -> bool {
    let label = |l: &str| {
        !l.is_empty()
            && l.len() <= 63
            && l.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
            && !l.starts_with('-')
    };
    if allow_subdomains {
        s.split('.').all(label)
    } else {
        label(s)
    }
}

fn virtual_hostable_bucket(s: &str, allow_subdomains: bool) -> bool {
    let ok_label = |l: &str| {
        (3..=63).contains(&l.len())
            && l.bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
            && l.as_bytes()[0].is_ascii_alphanumeric()
            && l.as_bytes()[l.len() - 1].is_ascii_alphanumeric()
    };
    let looks_like_ip = s.split('.').count() == 4 && s.split('.').all(|p| p.parse::<u8>().is_ok());
    if looks_like_ip {
        return false;
    }
    if allow_subdomains {
        s.split('.').all(|l| !l.is_empty() && l.len() <= 63) && (3..=63).contains(&s.len()) && {
            let flat: String = s.replace('.', "");
            ok_label(s.split('.').next().unwrap_or("")) || ok_label(&flat)
        }
    } else {
        ok_label(s)
    }
}

fn parse_url(s: &str) -> J {
    let Some((scheme, rest)) = s.split_once("://") else {
        return J::Null;
    };
    if scheme != "http" && scheme != "https" || rest.contains('?') || rest.contains('#') {
        return J::Null;
    }
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let host = authority.rsplit_once(':').map_or(authority, |(h, _)| h);
    let is_ip = host.starts_with('[')
        || host.split('.').count() == 4 && host.split('.').all(|p| p.parse::<u8>().is_ok());
    let normalized = if path.ends_with('/') {
        path.to_owned()
    } else {
        format!("{path}/")
    };
    J::Obj(vec![
        ("scheme".into(), J::Str(scheme.into())),
        ("authority".into(), J::Str(authority.into())),
        ("path".into(), J::Str(path.into())),
        ("normalizedPath".into(), J::Str(normalized)),
        ("isIp".into(), J::Bool(is_ip)),
    ])
}

fn parse_arn(s: &str) -> J {
    let parts: Vec<&str> = s.splitn(6, ':').collect();
    if parts.len() != 6
        || parts[0] != "arn"
        || parts[1].is_empty()
        || parts[2].is_empty()
        || parts[5].is_empty()
    {
        return J::Null;
    }
    let resource: Vec<J> = parts[5]
        .split([':', '/'])
        .map(|p| J::Str(p.into()))
        .collect();
    J::Obj(vec![
        ("partition".into(), J::Str(parts[1].into())),
        ("service".into(), J::Str(parts[2].into())),
        ("region".into(), J::Str(parts[3].into())),
        ("accountId".into(), J::Str(parts[4].into())),
        ("resourceId".into(), J::Arr(resource)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts() -> Vec<PartitionDef> {
        let p = J::parse(
            r#"{"partitions": [
              {"id": "aws", "outputs": {"dnsSuffix": "amazonaws.com", "dualStackDnsSuffix": "api.aws",
                "implicitGlobalRegion": "us-east-1", "supportsFIPS": true, "supportsDualStack": true},
               "regions": {"us-east-1": {}, "us-west-2": {}, "aws-global": {}}},
              {"id": "aws-cn", "outputs": {"dnsSuffix": "amazonaws.com.cn", "dualStackDnsSuffix": "api.amazonwebservices.com.cn",
                "implicitGlobalRegion": "cn-northwest-1", "supportsFIPS": true, "supportsDualStack": true},
               "regions": {"cn-north-1": {}}}]}"#,
        )
        .unwrap();
        PartitionDef::parse(&p)
    }

    // The shape of a typical regional rule set, and of a global one.
    const REGIONAL: &str = r#"{"version": "1.0",
      "parameters": {"Region": {"type": "String"}, "UseFIPS": {"type": "Boolean", "default": false},
                     "UseDualStack": {"type": "Boolean", "default": false}, "Endpoint": {"type": "String"}},
      "rules": [
        {"conditions": [{"fn": "isSet", "argv": [{"ref": "Endpoint"}]}], "type": "endpoint",
         "endpoint": {"url": {"ref": "Endpoint"}}},
        {"conditions": [{"fn": "isSet", "argv": [{"ref": "Region"}]}], "type": "tree", "rules": [
          {"conditions": [{"fn": "aws.partition", "argv": [{"ref": "Region"}], "assign": "PartitionResult"}],
           "type": "tree", "rules": [
             {"conditions": [{"fn": "stringEquals", "argv": [{"fn": "getAttr", "argv": [{"ref": "PartitionResult"}, "name"]}, "aws"]},
                             {"fn": "booleanEquals", "argv": [{"ref": "UseFIPS"}, false]}],
              "type": "endpoint",
              "endpoint": {"url": "https://iam.amazonaws.com",
                "properties": {"authSchemes": [{"name": "sigv4", "signingRegion": "{PartitionResult#implicitGlobalRegion}"}]}}},
             {"conditions": [], "type": "endpoint",
              "endpoint": {"url": "https://iam.{Region}.{PartitionResult#dnsSuffix}"}}]}]},
        {"conditions": [], "type": "error", "error": "Invalid Configuration: Missing Region"}]}"#;

    #[test]
    fn a_global_service_resolves_to_its_global_host_and_scope() {
        let parts = parts();
        let ev = Evaluator::new(&parts);
        let rs = J::parse(REGIONAL).unwrap();
        let r = ev.resolve(&rs, "us-west-2").unwrap();
        assert_eq!(r.url, "https://iam.amazonaws.com");
        assert_eq!(r.signing_region.as_deref(), Some("us-east-1"));
        let cn = ev.resolve(&rs, "cn-north-1").unwrap();
        assert_eq!(cn.url, "https://iam.cn-north-1.amazonaws.com.cn");
        assert_eq!(cn.signing_region, None);
    }

    #[test]
    fn helpers_follow_the_rules_spec() {
        assert_eq!(substring("abcdef", 0, 2, false), J::Str("ab".into()));
        assert_eq!(substring("abcdef", 0, 2, true), J::Str("ef".into()));
        assert_eq!(substring("ab", 0, 3, false), J::Null);
        assert!(valid_host_label("my-bucket", false));
        assert!(!valid_host_label("a.b", false));
        assert!(valid_host_label("a.b", true));
        assert!(virtual_hostable_bucket("my-bucket-1", false));
        assert!(!virtual_hostable_bucket("My_Bucket", false));
        assert!(!virtual_hostable_bucket("192.168.1.1", true));
        assert_eq!(uri_encode("a b/c"), "a%20b%2Fc");
        let arn = parse_arn("arn:aws:s3:us-west-2:123456789012:accesspoint/ap");
        assert_eq!(get_attr(&arn, "resourceId[1]"), J::Str("ap".into()));
        assert_eq!(get_attr(&arn, "region"), J::Str("us-west-2".into()));
        assert_eq!(parse_arn("not-an-arn"), J::Null);
    }

    #[test]
    fn an_unknown_region_falls_to_the_partition_it_resembles() {
        let parts = parts();
        assert_eq!(partition_of(&parts, "us-west-9").unwrap().id, "aws");
        assert_eq!(partition_of(&parts, "cn-south-1").unwrap().id, "aws-cn");
        assert_eq!(partition_of(&parts, "zz").unwrap().id, "aws");
    }
}
