//! The planted-secret corpus (theseus-oyrt): one invented secret of each
//! shape the scrubber knows, each in every encoding it claims (as written,
//! base64 standard and URL-safe at each of the three offsets, wrapped,
//! percent-encoded, JSON-escaped, YAML double-quoted and folded); and a clean
//! corpus of ordinary output (code, logs, prose, hashes, ids, base64 images)
//! that holds none. `tests_corpus.rs` scrubs both; theseusd's
//! `tests/planted_secrets.rs` includes this file and sends the planted one
//! through a daemon end to end.
//!
//! Every secret is built at run time from parts and a seeded generator, so
//! no whole key-shaped literal is in the repository. Depends on std and
//! serde_json alone, so the daemon's test can include it by path.

/// One secret as a tool prints it.
pub struct Secret {
    /// The stand-in's name the scrubber gives it.
    pub shape: &'static str,
    /// The line (or lines) a tool prints, holding the secret.
    pub line: String,
    /// The byte ranges of `line` that are secret: none may come through.
    pub cores: Vec<std::ops::Range<usize>>,
}

/// One secret in one encoding.
pub struct Planted {
    pub shape: &'static str,
    pub encoding: &'static str,
    /// The text a tool prints.
    pub text: String,
    /// The pieces of `text` that carry the secret: none may come through.
    pub carries: Vec<String>,
}

/// A small seeded generator (xorshift64*), so each run plants the same.
pub struct Gen(u64);

impl Gen {
    pub fn new(seed: u64) -> Self {
        Gen(seed | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// `n` characters of `alphabet`.
    pub fn of(&mut self, alphabet: &str, n: usize) -> String {
        let a = alphabet.as_bytes();
        (0..n)
            .map(|_| a[(self.next() % a.len() as u64) as usize] as char)
            .collect()
    }

    /// `n` characters of base62, with at least one capital, small letter and
    /// digit, as a random token of that length has.
    pub fn base62(&mut self, n: usize) -> String {
        loop {
            let s = self.of(BASE62, n);
            let b = s.as_bytes();
            if b.iter().any(u8::is_ascii_uppercase)
                && b.iter().any(u8::is_ascii_lowercase)
                && b.iter().any(u8::is_ascii_digit)
            {
                return s;
            }
        }
    }

    pub fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.next() as u8).collect()
    }
}

pub const BASE62: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
const TOKEN: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-";
const B64: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const HEX: &str = "0123456789abcdef";
const UPPER: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
const LETTERS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// `before`, the secret, `after`: its line and its range.
fn secret(shape: &'static str, before: &str, value: &str, after: &str) -> Secret {
    Secret {
        shape,
        line: format!("{before}{value}{after}"),
        cores: std::iter::once(before.len()..before.len() + value.len()).collect(),
    }
}

/// A prefixed token: `head` and `tail` joined at run time.
fn token(shape: &'static str, label: &str, head: &[&str], body: String) -> Secret {
    secret(
        shape,
        &format!("{label}="),
        &format!("{}{body}", head.concat()),
        "",
    )
}

/// A token's body, from the generator.
type Body = fn(&mut Gen) -> String;

const DIGITS: &str = "0123456789";

/// The prefixed tokens: each one's shape, the variable a `.env` names it by,
/// its prefix in parts, and its body.
const TOKENS: &[(&str, &str, &[&str], Body)] = &[
    (
        "anthropic_key",
        "ANTHROPIC_API_KEY",
        &["sk-", "ant-", "api03-"],
        |g| g.of(TOKEN, 90),
    ),
    ("github_token", "GH_TOKEN", &["gh", "p_"], |g| g.base62(36)),
    ("github_token", "GITHUB_PAT", &["github", "_pat_"], |g| {
        format!("{}_{}", g.base62(22), g.base62(59))
    }),
    ("github_token", "GH_OAUTH", &["gh", "o_"], |g| g.base62(36)),
    ("github_token", "GH_USER", &["gh", "u_"], |g| g.base62(36)),
    ("github_token", "GH_SERVER", &["gh", "s_"], |g| g.base62(36)),
    ("github_token", "GH_REFRESH", &["gh", "r_"], |g| {
        g.base62(36)
    }),
    (
        "op_service_account",
        "OP_SERVICE_ACCOUNT_TOKEN",
        &["op", "s_"],
        |g| g.base62(80),
    ),
    ("slack_token", "SLACK_BOT", &["xo", "xb-"], |g| {
        format!("{}-{}-{}", g.of(DIGITS, 12), g.of(DIGITS, 13), g.base62(24))
    }),
    ("slack_token", "SLACK_USER", &["xo", "xp-"], |g| {
        format!("{}-{}-{}", g.of(DIGITS, 12), g.of(DIGITS, 13), g.base62(32))
    }),
    ("slack_token", "SLACK_APP_CFG", &["xo", "xa-"], |g| {
        format!("2-{}", g.base62(40))
    }),
    ("slack_token", "SLACK_REFRESH", &["xo", "xr-"], |g| {
        g.base62(48)
    }),
    ("slack_token", "SLACK_LEGACY", &["xo", "xs-"], |g| {
        g.base62(48)
    }),
    (
        "slack_token",
        "SLACK_ROTATING",
        &["xo", "xe-", "xoxp-"],
        |g| g.base62(48),
    ),
    ("slack_token", "SLACK_APP", &["xa", "pp-"], |g| {
        format!("1-{}-{}", g.base62(11), g.base62(64))
    }),
    ("openai_key", "OPENAI_API_KEY", &["s", "k-"], |g| {
        g.base62(48)
    }),
    (
        "openai_key",
        "OPENAI_PROJECT_KEY",
        &["s", "k-", "proj-"],
        |g| format!("{}_{}", g.of(TOKEN, 74), g.base62(80)),
    ),
    (
        "openai_key",
        "OPENAI_SVC_KEY",
        &["s", "k-", "svcacct-"],
        |g| g.base62(120),
    ),
    (
        "openai_key",
        "OPENAI_ADMIN_KEY",
        &["s", "k-", "admin-"],
        |g| g.base62(120),
    ),
    ("stripe_key", "STRIPE_SECRET", &["sk", "_li", "ve_"], |g| {
        g.base62(99)
    }),
    ("stripe_key", "STRIPE_TEST", &["sk", "_te", "st_"], |g| {
        g.base62(99)
    }),
    (
        "stripe_key",
        "STRIPE_RESTRICTED",
        &["rk", "_li", "ve_"],
        |g| g.base62(99),
    ),
    (
        "stripe_key",
        "STRIPE_RESTRICTED_TEST",
        &["rk", "_te", "st_"],
        |g| g.base62(99),
    ),
    (
        "stripe_webhook_secret",
        "STRIPE_WEBHOOK",
        &["wh", "sec_"],
        |g| g.base62(32),
    ),
    ("google_api_key", "GOOGLE_API_KEY", &["AI", "za"], |g| {
        g.of(TOKEN, 35)
    }),
    ("gitlab_token", "GITLAB_TOKEN", &["gl", "pat-"], |g| {
        g.of(TOKEN, 20)
    }),
    ("npm_token", "NPM_TOKEN", &["np", "m_"], |g| g.base62(36)),
    (
        "pypi_token",
        "PYPI_TOKEN",
        &["py", "pi-", "AgEIcHlwaS5vcmc"],
        |g| g.of(TOKEN, 150),
    ),
    ("huggingface_token", "HF_TOKEN", &["h", "f_"], |g| {
        g.of(LETTERS, 34)
    }),
    ("sendgrid_key", "SENDGRID_API_KEY", &["S", "G."], |g| {
        format!("{}.{}", g.of(TOKEN, 22), g.of(TOKEN, 43))
    }),
    ("digitalocean_token", "DO_TOKEN", &["dop", "_v1_"], |g| {
        g.of(HEX, 64)
    }),
    ("shopify_token", "SHOPIFY_TOKEN", &["shp", "at_"], |g| {
        g.of(HEX, 32)
    }),
    ("shopify_token", "SHOPIFY_SECRET", &["shp", "ss_"], |g| {
        g.of(HEX, 32)
    }),
    ("groq_key", "GROQ_API_KEY", &["gs", "k_"], |g| g.base62(52)),
];

/// One secret of every shape the scrubber knows.
pub fn secrets(seed: u64) -> Vec<Secret> {
    let mut g = Gen::new(seed);
    let mut out: Vec<Secret> = TOKENS
        .iter()
        .map(|(shape, label, head, body)| token(shape, label, head, body(&mut g)))
        .collect();
    out.extend(url_passwords(&mut g));
    out.extend(aws(&mut g));
    out.extend(blocks(&mut g));
    out
}

/// Passwords in connection URLs, one per scheme; the password alone is
/// secret, and holds characters a URL takes as they are.
fn url_passwords(g: &mut Gen) -> Vec<Secret> {
    let mut out = Vec::new();
    for (scheme, user, host) in [
        ("postgres", "app", "db.internal:5432/orders"),
        ("postgresql", "report", "10.0.0.7/analytics?sslmode=require"),
        ("mysql", "root", "mysql.local:3306/shop"),
        ("mongodb", "svc", "mongo-0.mongo:27017/admin"),
        (
            "mongodb+srv",
            "cluster-user",
            "cluster0.example.net/app?retryWrites=true",
        ),
        ("redis", "", "cache:6379/0"),
        ("rediss", "default", "cache.example.net:6380"),
        ("amqp", "worker", "rabbit:5672/vhost"),
        ("https", "deploy", "registry.example.net/v2/"),
    ] {
        // One holds a character past ASCII, which YAML and JSON escape.
        let accent = if scheme == "postgresql" { "\u{e9}" } else { "" };
        let pw = format!("{}-{}{accent}!{}", g.base62(10), g.base62(6), g.base62(4));
        out.push(secret(
            "url_password",
            &format!("DATABASE_URL={scheme}://{user}:"),
            &pw,
            &format!("@{host}"),
        ));
    }
    out
}

/// AWS: an id alone; an id and its secret on one line, as the console's CSV
/// puts them; a labeled secret; a labeled session token.
fn aws(g: &mut Gen) -> Vec<Secret> {
    let id = format!("{}{}", "AK", "IA") + &g.of(UPPER, 16);
    let mut out = vec![secret("aws_access_key_id", "aws_access_key_id = ", &id, "")];
    let sts_id = format!("{}{}", "AS", "IA") + &g.of(UPPER, 16);
    let key = g.of(B64, 39) + "Q";
    let csv = format!("{sts_id},{key}");
    let at = "Access key ID,Secret access key\n".len();
    out.push(Secret {
        shape: "aws_secret_access_key",
        line: format!("Access key ID,Secret access key\n{csv}"),
        cores: vec![at..at + sts_id.len(), at + sts_id.len() + 1..at + csv.len()],
    });
    let key = g.of(B64, 38) + "Zq";
    out.push(secret(
        "aws_secret_access_key",
        "aws_secret_access_key = ",
        &key,
        "",
    ));
    let session = format!("IQoJb3JpZ2luX2Vj{}", g.of(B64, 300));
    out.push(secret(
        "aws_session_token",
        "AWS_SESSION_TOKEN=",
        &session,
        "",
    ));
    out
}

/// A private key block, its body the secret; and a JWT, its claims and
/// signature the secret.
fn blocks(g: &mut Gen) -> Vec<Secret> {
    let body: Vec<String> = (0..6).map(|_| g.of(B64, 64)).collect();
    let head = "-----BEGIN OPENSSH PRIVATE KEY-----\n";
    let key = format!(
        "{head}{}\n-----END OPENSSH PRIVATE KEY-----",
        body.join("\n")
    );
    let cores = body
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let at = head.len() + i * 65;
            at..at + l.len()
        })
        .collect();
    let claims = format!("{{\"sub\":\"{}\",\"exp\":1900000000}}", g.base62(24));
    let jwt = format!(
        "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.{}.{}",
        b64url(claims.as_bytes()),
        g.of(TOKEN, 43)
    );
    vec![
        Secret {
            shape: "private_key",
            line: key,
            cores,
        },
        secret("jwt", "Authorization: Bearer ", &jwt, ""),
    ]
}

/// Every secret in every encoding.
pub fn planted(seed: u64) -> Vec<Planted> {
    let mut out = Vec::new();
    for s in secrets(seed) {
        for (encoding, (text, carries)) in encodings(&s) {
            out.push(Planted {
                shape: s.shape,
                encoding,
                text,
                carries,
            });
        }
    }
    out
}

/// The encodings, each a text and the pieces of it that carry the secret.
fn encodings(s: &Secret) -> Vec<(&'static str, (String, Vec<String>))> {
    let mut out = vec![("verbatim", charwise(s, |c| c.to_string(), None))];
    for k in 0..3 {
        let name = ["base64", "base64+1", "base64+2"][k];
        out.push((name, base64(s, k, false, None)));
        let name = ["base64url", "base64url+1", "base64url+2"][k];
        out.push((name, base64(s, k, true, None)));
    }
    out.push(("base64 wrapped", base64(s, 1, false, Some(76))));
    out.push(("percent", charwise(s, |c| percent(c, false), None)));
    out.push((
        "percent every byte",
        charwise(s, |c| percent(c, true), None),
    ));
    out.push(("json", charwise(s, |c| json_char(c, false), None)));
    out.push(("json ascii \\u", charwise(s, |c| json_char(c, true), None)));
    out.push(("yaml double-quoted", charwise(s, yaml_char, None)));
    // Past the width from its first line, so PyYAML folds at the first
    // space or just after the first escape: inside a secret that holds one.
    out.push((
        "yaml folded",
        charwise(&behind(FOLD_FILL, s), yaml_char, Some(80)),
    ));
    // Each text on its own lines, labeled, as a tool's output holds it.
    for (name, (text, _)) in &mut out {
        let quoted = name.starts_with("json") || name.starts_with("yaml");
        *text = if quoted {
            format!("{}: \"{text}\"\n", s.shape)
        } else {
            format!("{}:\n{text}\n", s.shape)
        };
    }
    out
}

/// What the folded form's line begins with: a character YAML escapes, which
/// makes PyYAML choose the double-quoted style, then enough to pass the
/// width.
const FOLD_FILL: &str =
    "Inv\u{e9}nted-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx:";

/// The secret with `before` ahead of its line.
fn behind(before: &str, s: &Secret) -> Secret {
    Secret {
        shape: s.shape,
        line: format!("{before}{}", s.line),
        cores: s
            .cores
            .iter()
            .map(|r| r.start + before.len()..r.end + before.len())
            .collect(),
    }
}

/// The secret's line with each character encoded by `enc`, folded as PyYAML
/// folds a double-quoted scalar when `fold` gives a width: at a space, or
/// just after an escape, past the width, the line ends in `\` and the next
/// begins with two spaces of indentation and a space as `\ `.
fn charwise(
    s: &Secret,
    enc: impl Fn(char) -> String,
    fold: Option<usize>,
) -> (String, Vec<String>) {
    let mut text = String::new();
    // Where each byte of the line starts in the text.
    let mut at = vec![0usize; s.line.len() + 1];
    let mut column = 0;
    let mut after_escape = false;
    // Room for the label PyYAML writes first.
    if fold.is_some() {
        column = s.shape.len() + 3;
    }
    for (i, c) in s.line.char_indices() {
        let e = enc(c);
        if let Some(width) = fold {
            if column + e.len() > width && (c == ' ' || after_escape) {
                text.push_str("\\\n  ");
                column = 2;
            }
        }
        let e = if fold.is_some() && c == ' ' && column == 2 {
            "\\ ".to_string()
        } else {
            e
        };
        at[i..i + c.len_utf8()].fill(text.len());
        after_escape = e.starts_with('\\');
        column += e.len();
        text.push_str(&e);
    }
    at[s.line.len()] = text.len();
    let carries = s
        .cores
        .iter()
        .flat_map(|r| pieces(&text[at[r.start]..at[r.end]]))
        .collect();
    (text, carries)
}

/// The pieces of an encoded secret as they lie on the text's lines: split at
/// each line break, a fold's `\` and indentation dropped, each at least 8
/// characters.
fn pieces(t: &str) -> Vec<String> {
    t.split('\n')
        .map(|p| p.trim_start_matches(' ').trim_end_matches('\\'))
        .filter(|p| p.len() >= 8)
        .map(str::to_string)
        .collect()
}

fn percent(c: char, every: bool) -> String {
    let mut buf = [0u8; 4];
    c.encode_utf8(&mut buf)
        .bytes()
        .map(|b| {
            if !every && (b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~')) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// A character as JSON writes it: as serde_json does, or, `ascii`, every
/// character a `\u` escape.
fn json_char(c: char, ascii: bool) -> String {
    if ascii {
        let mut units = [0u16; 2];
        return c
            .encode_utf16(&mut units)
            .iter()
            .map(|u| format!("\\u{u:04x}"))
            .collect();
    }
    let s = serde_json::to_string(&c.to_string()).unwrap();
    s[1..s.len() - 1].to_string()
}

/// A character as PyYAML writes it in a double-quoted scalar.
fn yaml_char(c: char) -> String {
    match c {
        '"' => "\\\"".into(),
        '\\' => "\\\\".into(),
        '\n' => "\\n".into(),
        '\t' => "\\t".into(),
        c if (c as u32) < 0x20 => format!("\\x{:02X}", c as u32),
        c if c.is_ascii() => c.to_string(),
        c if (c as u32) < 0x100 => format!("\\x{:02X}", c as u32),
        c => format!("\\u{:04X}", c as u32),
    }
}

/// The secret's line after `k` bytes of filler, in base64 (URL-safe and
/// unpadded, `url`), wrapped at `wrap` columns; the pieces are the characters
/// that hold the secret's bytes alone, as they lie on the lines.
fn base64(s: &Secret, k: usize, url: bool, wrap: Option<usize>) -> (String, Vec<String>) {
    let mut plain = vec![b'#'; k];
    plain.extend_from_slice(s.line.as_bytes());
    let mut enc = b64(&plain);
    if url {
        enc = enc.replace('+', "-").replace('/', "_").replace('=', "");
    }
    let width = wrap.unwrap_or(usize::MAX);
    let lines: Vec<(usize, &str)> = enc
        .as_bytes()
        .chunks(width.min(enc.len().max(1)))
        .enumerate()
        .map(|(i, c)| {
            (
                i * width.min(enc.len().max(1)),
                std::str::from_utf8(c).unwrap(),
            )
        })
        .collect();
    let mut carries = Vec::new();
    for r in &s.cores {
        // Character j holds bits 6j to 6j+6.
        let (a, b) = ((8 * (k + r.start)).div_ceil(6), 8 * (k + r.end) / 6);
        for (from, line) in &lines {
            let (lo, hi) = (a.max(*from), b.min(from + line.len()));
            if hi >= lo + 8 {
                carries.push(line[lo - from..hi - from].to_string());
            }
        }
    }
    let text: Vec<&str> = lines.iter().map(|(_, l)| *l).collect();
    (text.join("\n"), carries)
}

/// Standard base64, padded.
pub fn b64(bytes: &[u8]) -> String {
    let a = B64.as_bytes();
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.len();
        let v = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for j in 0..4 {
            if j <= n {
                out.push(a[((v >> (18 - 6 * j)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn b64url(bytes: &[u8]) -> String {
    b64(bytes)
        .replace('+', "-")
        .replace('/', "_")
        .replace('=', "")
}

/// Ordinary output that holds no secret, each with what it is: every
/// stand-in the scrubber puts in one is a false positive.
pub fn clean(seed: u64) -> Vec<(&'static str, String)> {
    let mut g = Gen::new(seed);
    let sha = |g: &mut Gen| g.of(HEX, 40);
    let uuid = |g: &mut Gen| {
        format!(
            "{}-{}-4{}-a{}-{}",
            g.of(HEX, 8),
            g.of(HEX, 4),
            g.of(HEX, 3),
            g.of(HEX, 3),
            g.of(HEX, 12)
        )
    };
    let mut out: Vec<(&'static str, String)> = vec![
        (
            "rust",
            "fn disk_usage(task: &Task) -> Result<u64> {\n    // The ask-sk-ip path: run the risk-check, then mask-key the rest.\n    let desk-top = task.sk_id;\n    let a = \"sk-\"; // OpenAI's prefix, written alone\n    Ok(a.len() as u64)\n}\n".into(),
        ),
        (
            "prose",
            "Keys that start with sk- belong to OpenAI, and sk_live_ ones to Stripe; never paste one.\nA Google key begins AIza and runs 39 characters: AIzaShort is not one.\nUse hf_hub_download from huggingface_hub, and npm_config_cache for npm.\nSee SG.example and the xoxb- prefix in Slack's docs. The sk-learn tutorial is fine.\n".into(),
        ),
        (
            "urls",
            "postgres://app@db.internal:5432/orders\nmysql://localhost/shop\nhttps://github.com/example/theseus-demo/pull/12\ngit+ssh://git@github.com/example/demo.git\nhttps://user@registry.example.net/v2/\nredis://cache:6379/0\npostgres://app:${DB_PASSWORD}@db:5432/x\nmongodb://svc:<password>@mongo:27017\namqp://worker:****@rabbit:5672\nftp://anonymous:@files.example.net/pub\nhttp://[::1]:8080/health?next=a@b\n".into(),
        ),
        (
            "public key",
            format!("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI{} demo@example\n-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----\nSTRIPE_PUBLISHABLE={}{}\n", g.of(B64, 43), g.of(B64, 64), "pk_li", "ve_".to_string() + &g.base62(99)),
        ),
    ];
    out.extend(logs(&mut g));
    let image = b64(&g.bytes(6000));
    let wrapped: Vec<&str> = image
        .as_bytes()
        .chunks(76)
        .map(|c| std::str::from_utf8(c).unwrap())
        .collect();
    out.push((
        "base64 image",
        format!(
            "<img src=\"data:image/png;base64,{}\">\n{}\n",
            b64(&g.bytes(3000)),
            wrapped.join("\n")
        ),
    ));
    out.push((
        "json",
        serde_json::to_string_pretty(&serde_json::json!({
            "id": uuid(&mut g),
            "name": "Invénted widget \"deluxe\"",
            "path": "C:\\Users\\demo\\file.txt",
            "url": "https:\\/\\/example.net\\/a?b=c%20d",
            "nested": serde_json::to_string(&serde_json::json!({"k": "v\nw", "n": 3})).unwrap(),
            "sha": sha(&mut g),
            "config": b64(br#"{"debug":true,"level":"info","retries":3}"#),
        }))
        .unwrap(),
    ));
    out.push((
        "yaml",
        format!(
            "apiVersion: v1\nkind: ConfigMap\ndata:\n  greeting: \"Inv\\xE9nted hello with many words with many words with many words with\\\n    \\ many words\"\n  checksum: {}\n  config.json: {}\n",
            g.of(HEX, 64),
            b64(br#"{"listen":"0.0.0.0:8080","workers":4,"log":"json"}"#)
        ),
    ));
    out.push((
        "git log",
        (0..10)
            .map(|i| {
                format!(
                    "commit {}\nAuthor: Collaborator <collaborator@example.net>\nDate:   Sat Oct 10 12:0{i}:00 2026\n\n    tool: fix the disk-check and the task-sk-ip ({})\n\n",
                    sha(&mut g),
                    g.of("abcdefghijklmnopqrstuvwxyz0123456789", 4)
                )
            })
            .collect(),
    ));
    out
}

/// A server's log and a build's hashes, as tools print them.
fn logs(g: &mut Gen) -> Vec<(&'static str, String)> {
    let sha = |g: &mut Gen| g.of(HEX, 40);
    let uuid = |g: &mut Gen| {
        format!(
            "{}-{}-4{}-a{}-{}",
            g.of(HEX, 8),
            g.of(HEX, 4),
            g.of(HEX, 3),
            g.of(HEX, 3),
            g.of(HEX, 12)
        )
    };
    let mut out = Vec::new();
    let log: String = (0..40)
        .map(|i| {
            format!(
                "2026-10-10T12:{:02}:{:02}.{:03}Z INFO request_id={} trace={} commit={} took={}ms path=/api/v1/items/{}?page={}&sort=desc%2Cname\n",
                i % 60,
                (i * 7) % 60,
                i * 13 % 1000,
                uuid(g),
                g.of(HEX, 32),
                sha(g),
                i * 3 + 1,
                g.of("0123456789", 6),
                i
            )
        })
        .collect();
    out.push(("log", log));
    let hashes: String = (0..20)
        .map(|_| {
            format!(
                "{}  target/release/{}\nsha256:{}\nsha512-{}\n",
                g.of(HEX, 64),
                g.of("abcdefghijklmnopqrstuvwxyz", 8),
                g.of(HEX, 64),
                b64(&g.bytes(64))
            )
        })
        .collect();
    out.push(("hashes", hashes));
    out
}
