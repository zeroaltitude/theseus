//! The evaluator of a guardrail's input condition (its `when`) over typed input: an API call's JSON, or
//! a template resource's resolved properties.
//!
//! Answers have three values. A template can leave a value unknown (an import, an `Fn::If` on a
//! parameter), and a guardrail that might hit is asked about, as one that hits is: a maybe is a hit.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::cidr;
use crate::node::{has_mark, Node, MARK};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Truth {
    No,
    Maybe,
    Yes,
}

impl Truth {
    fn not(self) -> Truth {
        match self {
            Truth::Yes => Truth::No,
            Truth::No => Truth::Yes,
            Truth::Maybe => Truth::Maybe,
        }
    }

    /// Anything but a no is asked about.
    pub fn hits(self) -> bool {
        self != Truth::No
    }
}

/// What the evaluator knows beyond the input.
#[derive(Clone, Debug)]
pub struct Context {
    /// The account the call is made in: a principal or a share naming any other is outside it.
    pub account: String,
    /// The region, for a template's `AWS::Region`.
    pub region: String,
}

/// An answer, and the value that gave it, for the floor's confirm.
#[derive(Clone, Debug, PartialEq)]
pub struct Fired {
    pub truth: Truth,
    pub detail: Option<String>,
}

impl Fired {
    fn no() -> Fired {
        Fired {
            truth: Truth::No,
            detail: None,
        }
    }

    fn yes(detail: String) -> Fired {
        Fired {
            truth: Truth::Yes,
            detail: Some(detail),
        }
    }

    pub(crate) fn capped(mut self, cap: Truth) -> Fired {
        self.truth = self.truth.min(cap);
        self
    }
}

/// A member path: names joined by dots, matched without case. Lists are walked through, `*` is any
/// one member, and `**` any depth, none included.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(try_from = "String")]
pub struct Path {
    raw: String,
    segs: Vec<Seg>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Seg {
    Key(String),
    Any,
    Deep,
}

impl TryFrom<String> for Path {
    type Error = String;

    fn try_from(raw: String) -> Result<Path, String> {
        let mut segs = Vec::new();
        for s in raw.split('.') {
            segs.push(match s {
                "" => return Err(format!("path {raw:?} has an empty member")),
                "**" => Seg::Deep,
                "*" => Seg::Any,
                k if k
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') =>
                {
                    Seg::Key(k.to_string())
                }
                k => return Err(format!("path {raw:?}: {k:?} is not a member name")),
            });
        }
        if segs.last() == Some(&Seg::Deep) {
            return Err(format!("path {raw:?} ends in `**`"));
        }
        Ok(Path { raw, segs })
    }
}

impl Path {
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// The first member's name, unless the path starts at any depth.
    pub fn first(&self) -> Option<&str> {
        match self.segs.first() {
            Some(Seg::Key(k)) => Some(k),
            _ => None,
        }
    }
}

/// A value the path reached. `certain` is false under an `Fn::If` branch; `through` is true when the
/// walk stopped at an unknown value before the path's end.
struct Found<'n> {
    node: &'n Node,
    certain: bool,
    through: bool,
    at: String,
}

fn select<'n>(root: &'n Node, path: &Path) -> Vec<Found<'n>> {
    let mut out = Vec::new();
    walk(root, &path.segs, true, String::new(), &mut out);
    out
}

fn join(at: &str, key: &str) -> String {
    if at.is_empty() {
        key.to_string()
    } else {
        format!("{at}.{key}")
    }
}

fn walk<'n>(node: &'n Node, segs: &[Seg], certain: bool, at: String, out: &mut Vec<Found<'n>>) {
    match node {
        Node::List(items) => {
            for (i, item) in items.iter().enumerate() {
                walk(item, segs, certain, format!("{at}[{i}]"), out);
            }
            return;
        }
        Node::Either(alts) => {
            for alt in alts {
                walk(alt, segs, false, at.clone(), out);
            }
            return;
        }
        Node::Absent | Node::Null => return,
        _ => {}
    }
    let Some((seg, rest)) = segs.split_first() else {
        out.push(Found {
            node,
            certain,
            through: false,
            at,
        });
        return;
    };
    let unknown = matches!(node, Node::Unresolved(_) | Node::OwnRef(_));
    match (seg, node) {
        (Seg::Key(k), Node::Map(m)) => {
            if let Some((key, child)) = m.iter().find(|(key, _)| key.eq_ignore_ascii_case(k)) {
                walk(child, rest, certain, join(&at, key), out);
            }
        }
        (Seg::Any, Node::Map(m)) => {
            for (key, child) in m {
                walk(child, rest, certain, join(&at, key), out);
            }
        }
        (Seg::Deep, Node::Map(m)) => {
            walk(node, rest, certain, at.clone(), out);
            for (key, child) in m {
                walk(child, segs, certain, join(&at, key), out);
            }
        }
        // At any depth, an unknown value on the way is not followed: an import in one property must not
        // make every deep path a maybe. A named path into an unknown value is a maybe.
        (Seg::Key(_) | Seg::Any, _) if unknown => out.push(Found {
            node,
            certain,
            through: true,
            at,
        }),
        _ => {}
    }
}

/// The strongest answer `test` gives over the values the path reached.
fn over(found: &[Found], test: impl Fn(&Node) -> Truth) -> Fired {
    let mut best = Fired::no();
    for v in found {
        let mut t = test(v.node);
        if t == Truth::Yes && !v.certain {
            t = Truth::Maybe;
        }
        if t > best.truth {
            best = Fired {
                truth: t,
                detail: Some(format!("{} = {}", v.at, v.node.brief())),
            };
            if t == Truth::Yes {
                break;
            }
        }
    }
    best
}

fn absent(found: &[Found], path: &Path) -> Fired {
    if found.is_empty() {
        return Fired::yes(format!("{} is absent", path.as_str()));
    }
    if found.iter().any(|v| v.certain && !v.through) {
        return Fired::no();
    }
    Fired {
        truth: Truth::Maybe,
        detail: Some(format!("{} may be absent", path.as_str())),
    }
}

/// One value, or several, in the list's TOML.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl OneOrMany {
    pub fn values(&self) -> &[String] {
        match self {
            OneOrMany::One(s) => std::slice::from_ref(s),
            OneOrMany::Many(v) => v,
        }
    }
}

/// A guardrail's input condition: a set of tests, which holds when any test holds.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct When {
    #[serde(default)]
    pub public_cidr: Vec<Path>,
    #[serde(default)]
    pub external_principal: Vec<Path>,
    #[serde(default)]
    pub foreign_account: Vec<Path>,
    #[serde(default)]
    pub own_account: Vec<Path>,
    /// A template's reference to a resource it does not make: a literal id, a parameter's value, an
    /// import (a maybe). A `Ref` or `Fn::GetAtt` of its own resource is not.
    #[serde(default)]
    pub not_own: Vec<Path>,
    #[serde(default, rename = "true")]
    pub is_true: Vec<Path>,
    #[serde(default, rename = "false")]
    pub is_false: Vec<Path>,
    #[serde(default)]
    pub equals: BTreeMap<Path, OneOrMany>,
    #[serde(default)]
    pub matches: BTreeMap<Path, OneOrMany>,
    #[serde(default)]
    pub present: Vec<Path>,
    #[serde(default)]
    pub absent: Vec<Path>,
    #[serde(default)]
    pub any: Vec<When>,
    #[serde(default)]
    pub all: Vec<When>,
    #[serde(default)]
    pub none: Vec<When>,
}

/// The strongest answer so far; a yes ends the search.
struct Best(Fired);

impl Best {
    fn take(&mut self, f: Fired) -> bool {
        if f.truth > self.0.truth {
            self.0 = f;
        }
        self.0.truth == Truth::Yes
    }
}

impl When {
    /// The tests of where a value points: an address, a principal, an account, a resource. True once
    /// one holds for certain.
    fn eval_reach(&self, input: &Node, ctx: &Context, best: &mut Best) -> bool {
        for p in &self.public_cidr {
            if best.take(over(&select(input, p), cidr_truth)) {
                return true;
            }
        }
        for p in &self.external_principal {
            if best.take(external_principal(&select(input, p), ctx)) {
                return true;
            }
        }
        for p in &self.foreign_account {
            if best.take(over(&select(input, p), |n| foreign(n, ctx))) {
                return true;
            }
        }
        for p in &self.own_account {
            if best.take(over(&select(input, p), |n| own(n, ctx))) {
                return true;
            }
        }
        for p in &self.not_own {
            if best.take(over(&select(input, p), not_own)) {
                return true;
            }
        }
        false
    }

    pub fn eval(&self, input: &Node, ctx: &Context) -> Fired {
        let mut best = Best(Fired::no());
        if self.eval_reach(input, ctx, &mut best) {
            return best.0;
        }
        for p in &self.is_true {
            if best.take(over(&select(input, p), |n| boolean(n, true))) {
                return best.0;
            }
        }
        for p in &self.is_false {
            if best.take(over(&select(input, p), |n| boolean(n, false))) {
                return best.0;
            }
        }
        for (p, vals) in &self.equals {
            if best.take(over(&select(input, p), |n| equals(n, vals.values()))) {
                return best.0;
            }
        }
        for (p, globs) in &self.matches {
            if best.take(over(&select(input, p), |n| matches(n, globs.values()))) {
                return best.0;
            }
        }
        for p in &self.present {
            let truth = absent(&select(input, p), p).truth.not();
            if truth != Truth::No
                && best.take(Fired {
                    truth,
                    detail: Some(format!("{} is present", p.as_str())),
                })
            {
                return best.0;
            }
        }
        for p in &self.absent {
            if best.take(absent(&select(input, p), p)) {
                return best.0;
            }
        }
        for w in &self.any {
            if best.take(w.eval(input, ctx)) {
                return best.0;
            }
        }
        if !self.all.is_empty() {
            let mut truth = Truth::Yes;
            let mut details = Vec::new();
            for w in &self.all {
                let f = w.eval(input, ctx);
                truth = truth.min(f.truth);
                if truth == Truth::No {
                    break;
                }
                details.extend(f.detail);
            }
            if truth != Truth::No
                && best.take(Fired {
                    truth,
                    detail: Some(details.join(", and ")),
                })
            {
                return best.0;
            }
        }
        if !self.none.is_empty() {
            let mut truth = Truth::Yes;
            for w in &self.none {
                truth = truth.min(w.eval(input, ctx).truth.not());
            }
            if truth != Truth::No {
                best.take(Fired {
                    truth,
                    detail: None,
                });
            }
        }
        best.0
    }

    /// Every path the condition names, nested tests included.
    pub fn paths(&self) -> Vec<&Path> {
        let mut out: Vec<&Path> = Vec::new();
        for list in [
            &self.public_cidr,
            &self.external_principal,
            &self.foreign_account,
            &self.own_account,
            &self.not_own,
            &self.is_true,
            &self.is_false,
            &self.present,
            &self.absent,
        ] {
            out.extend(list.iter());
        }
        out.extend(self.equals.keys());
        out.extend(self.matches.keys());
        for w in self.any.iter().chain(&self.all).chain(&self.none) {
            out.extend(w.paths());
        }
        out
    }

    /// No test at all, at this level or any nested one: such a condition never holds, and the list
    /// rejects it.
    pub fn is_empty(&self) -> bool {
        self.paths().is_empty()
    }
}

fn unknown(n: &Node) -> bool {
    matches!(n, Node::Unresolved(_) | Node::OwnRef(_)) || n.as_str().is_some_and(has_mark)
}

fn cidr_truth(n: &Node) -> Truth {
    if unknown(n) {
        return Truth::Maybe;
    }
    match n.as_str().and_then(cidr::is_public) {
        Some(true) => Truth::Yes,
        Some(false) => Truth::No,
        // Not an address: AWS refuses it anyway, so asking costs nothing.
        None => Truth::Maybe,
    }
}

/// A resource the template does not make: anything but its own (`OwnRef`). An unknown value may be one.
fn not_own(n: &Node) -> Truth {
    match n {
        Node::OwnRef(_) => Truth::No,
        Node::Unresolved(_) => Truth::Maybe,
        Node::Str(s) if has_mark(s) => Truth::Maybe,
        Node::Str(s) if s.is_empty() => Truth::No,
        Node::Str(_) | Node::Num(_) => Truth::Yes,
        _ => Truth::No,
    }
}

fn boolean(n: &Node, want: bool) -> Truth {
    if unknown(n) {
        return Truth::Maybe;
    }
    let got = match n {
        Node::Bool(b) => Some(*b),
        Node::Str(s) if s.eq_ignore_ascii_case("true") => Some(true),
        Node::Str(s) if s.eq_ignore_ascii_case("false") => Some(false),
        _ => None,
    };
    if got == Some(want) {
        Truth::Yes
    } else {
        Truth::No
    }
}

fn scalar_text(n: &Node) -> Option<String> {
    match n {
        Node::Str(s) => Some(s.clone()),
        Node::Num(s) => Some(s.clone()),
        Node::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn equals(n: &Node, values: &[String]) -> Truth {
    if unknown(n) {
        return Truth::Maybe;
    }
    match scalar_text(n) {
        Some(s) if values.iter().any(|v| v.eq_ignore_ascii_case(&s)) => Truth::Yes,
        _ => Truth::No,
    }
}

fn matches(n: &Node, globs: &[String]) -> Truth {
    if unknown(n) {
        return Truth::Maybe;
    }
    match scalar_text(n) {
        Some(s) if globs.iter().any(|g| glob(g, &s)) => Truth::Yes,
        _ => Truth::No,
    }
}

/// `*` is any run of characters and `?` any one, compared without case: the list's match, and IAM's
/// for actions.
pub fn glob(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ti));
            pi += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

/// What an account-ish string names.
#[derive(Debug, PartialEq)]
enum Named {
    Account(String),
    /// An organization or an organizational unit: other accounts, by definition.
    Organization,
    Service,
    Anyone,
    Unknown,
    Other,
}

fn account_id(s: &str) -> bool {
    s.len() == 12 && s.bytes().all(|b| b.is_ascii_digit())
}

/// Each unknown span as one mark, so an unknown with colons in it (`Fn::ImportValue`) cannot shift an
/// ARN's fields.
fn collapse_marks(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut inside = false;
    for c in s.chars() {
        if c == MARK {
            if !inside {
                out.push(MARK);
            }
            inside = !inside;
        } else if !inside {
            out.push(c);
        }
    }
    out
}

/// What an account-ish string names. An ARN is judged by its service and account fields alone, so a
/// name a template leaves unknown in the rest of it (a user's, a role's) is no maybe.
fn named(s: &str) -> Named {
    let s = collapse_marks(s.trim());
    if s == "*" {
        return Named::Anyone;
    }
    if let Some(rest) = s.strip_prefix("arn:") {
        let parts: Vec<&str> = rest.splitn(5, ':').collect();
        if parts.len() < 5 || has_mark(parts[1]) || has_mark(parts[3]) {
            return if has_mark(&s) {
                Named::Unknown
            } else {
                Named::Other
            };
        }
        if parts[1] == "organizations" {
            return Named::Organization;
        }
        if account_id(parts[3]) {
            return Named::Account(parts[3].to_string());
        }
        return Named::Other;
    }
    if has_mark(&s) {
        return Named::Unknown;
    }
    if account_id(&s) {
        return Named::Account(s);
    }
    let lower = s.to_ascii_lowercase();
    if lower.starts_with("o-") || lower.starts_with("ou-") {
        return Named::Organization;
    }
    if lower.ends_with(".amazonaws.com") || lower.ends_with(".amazonaws.com.cn") {
        return Named::Service;
    }
    Named::Other
}

fn foreign(n: &Node, ctx: &Context) -> Truth {
    match n {
        Node::Unresolved(_) => Truth::Maybe,
        Node::OwnRef(_) => Truth::No,
        _ => match scalar_text(n).map(|s| named(&s)) {
            Some(Named::Account(a)) if a != ctx.account => Truth::Yes,
            Some(Named::Organization) => Truth::Yes,
            Some(Named::Unknown) => Truth::Maybe,
            _ => Truth::No,
        },
    }
}

fn own(n: &Node, ctx: &Context) -> Truth {
    match n {
        Node::Unresolved(_) => Truth::Maybe,
        Node::OwnRef(_) => Truth::Yes,
        _ => match scalar_text(n).map(|s| named(&s)) {
            Some(Named::Account(a)) if a == ctx.account => Truth::Yes,
            Some(Named::Unknown) => Truth::Maybe,
            _ => Truth::No,
        },
    }
}

/// Does any policy at these paths allow a principal outside the account?
fn external_principal(found: &[Found], ctx: &Context) -> Fired {
    let mut best = Fired::no();
    for v in found {
        let mut f = policy(v.node, ctx, 0);
        if f.truth == Truth::Yes && !v.certain {
            f.truth = Truth::Maybe;
        }
        if f.truth > best.truth {
            best = Fired {
                truth: f.truth,
                detail: Some(match f.detail {
                    Some(d) => format!("{}: {d}", v.at),
                    None => v.at.clone(),
                }),
            };
            if best.truth == Truth::Yes {
                break;
            }
        }
    }
    best
}

/// A policy document, as JSON text or a mapping; a single statement or a list of them is read too.
fn policy(doc: &Node, ctx: &Context, depth: usize) -> Fired {
    match doc {
        Node::Unresolved(why) => Fired {
            truth: Truth::Maybe,
            detail: Some(format!("the policy is unresolved ({why})")),
        },
        Node::Str(text) if depth == 0 => match serde_json::from_str::<serde_json::Value>(text) {
            Ok(v) => policy(&Node::from_json(&v), ctx, depth + 1),
            Err(_) => Fired {
                truth: Truth::Maybe,
                detail: Some("the policy is not JSON".into()),
            },
        },
        Node::Map(_) => {
            if let Some(statements) = doc.get("Statement") {
                statements_of(statements, ctx)
            } else if doc.get("Effect").is_some() {
                statement(doc, ctx)
            } else {
                // Not a policy: a wrapper around one, which its own path reads.
                Fired::no()
            }
        }
        Node::List(_) | Node::Either(_) => statements_of(doc, ctx),
        _ => Fired::no(),
    }
}

fn statements_of(n: &Node, ctx: &Context) -> Fired {
    let mut best = Fired::no();
    let mut each = |f: Fired| {
        if f.truth > best.truth {
            best = f;
        }
    };
    match n {
        Node::List(items) => items.iter().for_each(|s| each(statements_of(s, ctx))),
        Node::Either(alts) => alts
            .iter()
            .for_each(|s| each(statements_of(s, ctx).capped(Truth::Maybe))),
        Node::Unresolved(why) => each(Fired {
            truth: Truth::Maybe,
            detail: Some(format!("a statement is unresolved ({why})")),
        }),
        _ => each(statement(n, ctx)),
    }
    best
}

fn statement(s: &Node, ctx: &Context) -> Fired {
    let cap = match s.get("Effect") {
        Some(Node::Str(e)) if e.eq_ignore_ascii_case("Deny") => return Fired::no(),
        Some(Node::Str(e)) if e.eq_ignore_ascii_case("Allow") => Truth::Yes,
        // An effect nobody can read may be an allow.
        _ => Truth::Maybe,
    };
    if s.get("NotPrincipal").is_some() {
        return Fired::yes("an Allow with NotPrincipal allows everyone else".into()).capped(cap);
    }
    let Some(p) = s.get("Principal") else {
        return Fired::no();
    };
    principal(p, s.get("Condition"), ctx).capped(cap)
}

fn principal(p: &Node, cond: Option<&Node>, ctx: &Context) -> Fired {
    match p {
        Node::Str(_) | Node::Num(_) => aws_principal(p, cond, ctx),
        Node::Map(m) => {
            let mut best = Fired::no();
            for (kind, v) in m {
                let f = match kind.to_ascii_lowercase().as_str() {
                    "aws" => aws_principal(v, cond, ctx),
                    "service" => Fired::no(),
                    "federated" => Fired::yes(format!("a federated principal, {}", v.brief())),
                    "canonicaluser" => Fired::yes("an S3 canonical user".into()),
                    _ => Fired::yes(format!("a {kind} principal")),
                };
                if f.truth > best.truth {
                    best = f;
                }
            }
            best
        }
        Node::Either(alts) => {
            let mut best = Fired::no();
            for alt in alts {
                let f = principal(alt, cond, ctx).capped(Truth::Maybe);
                if f.truth > best.truth {
                    best = f;
                }
            }
            best
        }
        Node::Unresolved(why) => Fired {
            truth: Truth::Maybe,
            detail: Some(format!("the principal is unresolved ({why})")),
        },
        _ => Fired::no(),
    }
}

/// An `AWS` principal: one value or a list of them.
fn aws_principal(v: &Node, cond: Option<&Node>, ctx: &Context) -> Fired {
    let mut best = Fired::no();
    let mut values = Vec::new();
    flatten(v, true, &mut values);
    for (value, certain) in values {
        let f = match value {
            Node::OwnRef(_) => Fired::no(),
            Node::Unresolved(why) => Fired {
                truth: Truth::Maybe,
                detail: Some(format!("a principal is unresolved ({why})")),
            },
            _ => match scalar_text(value).map(|s| (named(&s), s)) {
                Some((Named::Anyone, _)) => match pinned(cond, ctx) {
                    Truth::Yes => Fired::no(),
                    Truth::Maybe => Fired {
                        truth: Truth::Maybe,
                        detail: Some("Principal * with a condition that may not pin it".into()),
                    },
                    Truth::No => {
                        Fired::yes("Principal * with no condition pinning this account".into())
                    }
                },
                Some((Named::Account(a), s)) if a != ctx.account => {
                    Fired::yes(format!("another account's principal, {s}"))
                }
                Some((Named::Organization, s)) => Fired::yes(format!("an organization, {s}")),
                Some((Named::Unknown, s)) => Fired {
                    truth: Truth::Maybe,
                    detail: Some(format!(
                        "a principal that may be outside, {}",
                        s.replace(MARK, "?")
                    )),
                },
                Some((Named::Account(_) | Named::Service, _)) => Fired::no(),
                Some((Named::Other, s)) => Fired {
                    truth: Truth::Maybe,
                    detail: Some(format!("a principal that names no account, {s}")),
                },
                None => Fired::no(),
            },
        };
        let f = if certain { f } else { f.capped(Truth::Maybe) };
        if f.truth > best.truth {
            best = f;
        }
    }
    best
}

fn flatten<'n>(n: &'n Node, certain: bool, out: &mut Vec<(&'n Node, bool)>) {
    match n {
        Node::List(items) => items.iter().for_each(|i| flatten(i, certain, out)),
        Node::Either(alts) => alts.iter().for_each(|a| flatten(a, false, out)),
        Node::Null | Node::Absent => {}
        _ => out.push((n, certain)),
    }
}

/// Does a statement's condition pin `Principal: *` to this account, its own resources, a VPC, or an
/// AWS service? An `…IfExists` operator pins nothing: a request without the key passes it.
fn pinned(cond: Option<&Node>, ctx: &Context) -> Truth {
    let Some(Node::Map(ops)) = cond else {
        return Truth::No;
    };
    let mut best = Truth::No;
    for (op, keys) in ops {
        let op = op.to_ascii_lowercase();
        let op = op
            .strip_prefix("foranyvalue:")
            .or_else(|| op.strip_prefix("forallvalues:"))
            .unwrap_or(&op);
        if op.ends_with("ifexists") {
            continue;
        }
        let Node::Map(keys) = keys else { continue };
        for (key, vals) in keys {
            let mut values = Vec::new();
            flatten(vals, true, &mut values);
            if values.is_empty() {
                continue;
            }
            let key = key.to_ascii_lowercase();
            let t = match (op, key.as_str()) {
                (
                    "stringequals" | "stringlike" | "stringequalsignorecase",
                    "aws:principalaccount"
                    | "aws:sourceaccount"
                    | "aws:sourceowner"
                    | "kms:calleraccount",
                )
                | (
                    "stringequals" | "stringlike" | "arnequals" | "arnlike",
                    "aws:principalarn" | "aws:sourcearn",
                ) => values
                    .iter()
                    .map(|(v, _)| own(v, ctx))
                    .min()
                    .unwrap_or(Truth::No),
                (
                    "stringequals" | "stringlike",
                    "aws:sourcevpc" | "aws:sourcevpce" | "aws:principalservicename",
                ) => Truth::Yes,
                ("bool", "aws:principalisawsservice") => values
                    .iter()
                    .map(|(v, _)| boolean(v, true))
                    .min()
                    .unwrap_or(Truth::No),
                _ => Truth::No,
            };
            best = best.max(t);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx() -> Context {
        Context {
            account: "111122223333".into(),
            region: "us-west-2".into(),
        }
    }

    fn when(toml_text: &str) -> When {
        toml::from_str(toml_text).unwrap()
    }

    fn truth(w: &When, input: serde_json::Value) -> Truth {
        w.eval(&Node::from_json(&input), &ctx()).truth
    }

    #[test]
    fn paths_walk_lists_any_depth_and_case() {
        let w = when(r#"public_cidr = ["**.CidrIp"]"#);
        let input = json!({"IpPermissions": [{"IpRanges": [{"CidrIp": "10.0.0.0/8"}, {"cidrip": "0.0.0.0/0"}]}]});
        let f = w.eval(&Node::from_json(&input), &ctx());
        assert_eq!(f.truth, Truth::Yes);
        assert_eq!(
            f.detail.as_deref(),
            Some("IpPermissions[0].IpRanges[1].cidrip = 0.0.0.0/0")
        );
        assert_eq!(truth(&w, json!({"CidrIp": "0.0.0.0/0"})), Truth::Yes);
        assert_eq!(truth(&w, json!({"CidrIp": "10.0.0.0/8"})), Truth::No);
        assert!(Path::try_from("a..b".to_string()).is_err());
        assert!(Path::try_from("a.**".to_string()).is_err());
    }

    #[test]
    fn principals_inside_and_outside_the_account() {
        let w = when(r#"external_principal = ["Policy"]"#);
        let doc = |principal: serde_json::Value| json!({"Policy": json!({"Statement": [{"Effect": "Allow", "Principal": principal, "Action": "sts:AssumeRole"}]}).to_string()});
        assert_eq!(
            truth(&w, doc(json!({"AWS": "arn:aws:iam::111122223333:root"}))),
            Truth::No
        );
        assert_eq!(truth(&w, doc(json!({"AWS": "111122223333"}))), Truth::No);
        assert_eq!(
            truth(&w, doc(json!({"AWS": "arn:aws:iam::444455556666:root"}))),
            Truth::Yes
        );
        assert_eq!(
            truth(&w, doc(json!({"AWS": ["111122223333", "444455556666"]}))),
            Truth::Yes
        );
        assert_eq!(
            truth(&w, doc(json!({"Service": "lambda.amazonaws.com"}))),
            Truth::No
        );
        assert_eq!(
            truth(
                &w,
                doc(json!({"Federated": "cognito-identity.amazonaws.com"}))
            ),
            Truth::Yes
        );
        assert_eq!(truth(&w, doc(json!("*"))), Truth::Yes);
        assert_eq!(truth(&w, json!({"Policy": "{not json"})), Truth::Maybe);
        // A deny never grants, and `*` pinned to this account grants no one outside.
        let deny = json!({"Policy": {"Statement": {"Effect": "Deny", "Principal": "*", "Action": "s3:*",
            "Condition": {"Bool": {"aws:SecureTransport": "false"}}}}});
        assert_eq!(truth(&w, deny), Truth::No);
        let pinned = json!({"Policy": {"Statement": [{"Effect": "Allow", "Principal": {"AWS": "*"}, "Action": "kms:Decrypt",
            "Condition": {"StringEquals": {"kms:CallerAccount": "111122223333"}}}]}});
        assert_eq!(truth(&w, pinned), Truth::No);
        let pinned_elsewhere = json!({"Policy": {"Statement": [{"Effect": "Allow", "Principal": "*", "Action": "sqs:SendMessage",
            "Condition": {"StringEqualsIfExists": {"aws:SourceAccount": "111122223333"}}}]}});
        assert_eq!(truth(&w, pinned_elsewhere), Truth::Yes);
        // An ARN is judged by its account: an unknown user name in this account is inside, and an
        // unknown account, even one whose reason has colons in it, may be outside.
        let unknown_name =
            format!("arn:aws:iam::111122223333:user/{MARK}parameter Name has no value{MARK}");
        assert_eq!(truth(&w, doc(json!({"AWS": unknown_name}))), Truth::No);
        let unknown_account = format!("arn:aws:iam::{MARK}Fn::ImportValue{MARK}:root");
        assert_eq!(
            truth(&w, doc(json!({"AWS": unknown_account}))),
            Truth::Maybe
        );
        let other_account = format!("arn:aws:iam::444455556666:role/{MARK}Fn::ImportValue{MARK}");
        assert_eq!(truth(&w, doc(json!({"AWS": other_account}))), Truth::Yes);
    }

    #[test]
    fn combinators_and_absence() {
        let w = when(
            r#"
equals = { Scheme = "internet-facing" }
all = [{ absent = ["Scheme"] }, { none = [{ equals = { Type = "gateway" } }] }]
"#,
        );
        assert_eq!(truth(&w, json!({"Scheme": "internet-facing"})), Truth::Yes);
        assert_eq!(truth(&w, json!({"Scheme": "internal"})), Truth::No);
        assert_eq!(truth(&w, json!({"Name": "lb"})), Truth::Yes);
        assert_eq!(truth(&w, json!({"Type": "gateway"})), Truth::No);
        assert_eq!(truth(&w, json!({"Type": "network"})), Truth::Yes);
    }

    #[test]
    fn unknown_values_are_maybes() {
        let w = when(r#"public_cidr = ["CidrIp"]"#);
        let n = Node::Map(vec![(
            "CidrIp".into(),
            Node::Unresolved("Fn::ImportValue".into()),
        )]);
        assert_eq!(w.eval(&n, &ctx()).truth, Truth::Maybe);
        let either = Node::Map(vec![(
            "CidrIp".into(),
            Node::Either(vec![
                Node::Str("10.0.0.0/8".into()),
                Node::Str("0.0.0.0/0".into()),
            ]),
        )]);
        assert_eq!(w.eval(&either, &ctx()).truth, Truth::Maybe);
        let a = when(r#"absent = ["Scheme"]"#);
        let maybe_absent = Node::Map(vec![(
            "Scheme".into(),
            Node::Either(vec![Node::Str("internal".into()), Node::Absent]),
        )]);
        assert_eq!(a.eval(&maybe_absent, &ctx()).truth, Truth::Maybe);
    }

    #[test]
    fn globs() {
        assert!(glob("theseus-trail-*", "theseus-trail-logs"));
        assert!(glob("*AllUsers*", "uri=\"acs/groups/global/AllUsers\""));
        assert!(glob(
            "arn:*:iam::*:policy/theseus-deny-spend",
            "arn:aws:iam::111122223333:policy/theseus-deny-spend"
        ));
        assert!(!glob("theseus-trail-*", "theseus-data"));
        assert!(glob("a?c", "abc"));
        assert!(!glob("a?c", "ac"));
    }
}
