//! The shapes theseus-oyrt added: OpenAI, Stripe and Google keys, a
//! connection URL's password, and the prefixed tokens of Slack, GitHub,
//! GitLab, npm, PyPI, Hugging Face, SendGrid and others; each withheld, and
//! its look-alikes kept. Every secret is invented and built from parts at run
//! time, so no whole key-shaped literal is in the repository.

use super::corpus::{Gen, BASE62};
use super::Scrubber;

fn scrub(text: &str) -> (String, u32) {
    Scrubber::default().scrub(text)
}

fn kept(text: &str) {
    assert_eq!(scrub(text), (text.to_string(), 0), "{text}");
}

#[test]
fn openai_keys_are_withheld_and_anthropic_keys_stay_theirs() {
    let mut g = Gen::new(11);
    for head in ["", "proj-", "svcacct-", "admin-"] {
        let key = format!("{}{}{head}{}", "s", "k-", g.base62(48));
        let (out, n) = scrub(&format!("OPENAI_API_KEY={key}\n"));
        assert_eq!(
            (out.as_str(), n),
            ("OPENAI_API_KEY=[redacted:openai_key]\n", 1),
            "{head}"
        );
    }
    let ant = format!("{}{}api03-{}", "sk-", "ant-", g.base62(48));
    assert_eq!(scrub(&ant).0, "[redacted:anthropic_key]");
    // `sk-` in prose and names, a short body, one with no capital or digit,
    // and `sk-` inside a word.
    kept("the sk- prefix is OpenAI's; the sk-learn tutorial");
    kept(&format!("sk-{}", g.base62(20)));
    kept(&format!("sk-{}", "abcdefghijklmnopqrstuvwxyzabcdefghij"));
    kept(&format!("disk-{}", g.base62(40)));
}

#[test]
fn stripe_secret_and_restricted_keys_are_withheld_and_publishable_ones_kept() {
    let mut g = Gen::new(12);
    for (kind, mode) in [
        ("sk", "live"),
        ("sk", "test"),
        ("rk", "live"),
        ("rk", "test"),
    ] {
        let key = format!("{kind}_{mode}_{}", g.base62(99));
        let (out, n) = scrub(&format!("Stripe::api_key = \"{key}\""));
        assert_eq!(
            (out.as_str(), n),
            ("Stripe::api_key = \"[redacted:stripe_key]\"", 1)
        );
    }
    let hook = format!("{}{}", "whsec_", g.base62(32));
    assert_eq!(scrub(&hook).0, "[redacted:stripe_webhook_secret]");
    // A publishable key is public by design.
    kept(&format!("pk_{}_{}", "live", g.base62(99)));
    kept("sk_live_short and sk_test_ alone");
}

#[test]
fn google_api_keys_are_withheld_at_their_length_only() {
    let mut g = Gen::new(13);
    let token = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-";
    let key = format!("{}{}{}", "AI", "za", g.of(token, 35));
    let (out, n) = scrub(&format!("?key={key}&alt=json"));
    assert_eq!(
        (out.as_str(), n),
        ("?key=[redacted:google_api_key]&alt=json", 1)
    );
    // Too short, too long, and inside a word.
    kept(&format!("AIza{}", g.of(token, 20)));
    kept(&format!("AIza{}", g.base62(50)));
    kept(&format!("xAIza{}", g.of(BASE62, 35)));
}

#[test]
fn a_connection_urls_password_alone_is_withheld() {
    for (url, says) in [
        (
            "postgres://app:Inv3nted-pw!x@db:5432/x",
            "postgres://app:[redacted:url_password]@db:5432/x",
        ),
        (
            "DATABASE_URL=postgresql://report:p%40ss-W0rd@10.0.0.7/analytics?sslmode=require",
            "DATABASE_URL=postgresql://report:[redacted:url_password]@10.0.0.7/analytics?sslmode=require",
        ),
        ("mysql://root:hunter2hunter2@localhost/shop", "mysql://root:[redacted:url_password]@localhost/shop"),
        (
            "mongodb+srv://u:raw@at-sign@cluster0.example.net/app",
            "mongodb+srv://u:[redacted:url_password]@cluster0.example.net/app",
        ),
        ("redis://:only-a-password@cache:6379/0", "redis://:[redacted:url_password]@cache:6379/0"),
        ("rediss://default:Zk9x@cache:6380", "rediss://default:[redacted:url_password]@cache:6380"),
        ("amqp://worker:s3cret@rabbit:5672/vhost", "amqp://worker:[redacted:url_password]@rabbit:5672/vhost"),
        (
            "\"proxy\": \"http://deploy:tok3n@proxy.example.net:3128\"",
            "\"proxy\": \"http://deploy:[redacted:url_password]@proxy.example.net:3128\"",
        ),
    ] {
        assert_eq!(scrub(url), (says.to_string(), 1), "{url}");
    }
    // No password, a user alone, an empty one, one that names a value given
    // elsewhere, an `@` past the authority, and no scheme.
    kept("postgres://app@db.internal:5432/orders");
    kept("git+ssh://git@github.com/example/demo.git");
    kept("https://github.com/example/demo/pull/12");
    kept("ftp://anonymous:@files.example.net/pub");
    kept("postgres://app:${DB_PASSWORD}@db:5432/x");
    kept("mongodb://svc:<password>@mongo:27017");
    kept("amqp://worker:****@rabbit:5672");
    kept("http://host:8080/login?next=a:b@c");
    kept("user:pass@host is not a URL");
    kept("://:x@y");
}

#[test]
fn the_other_prefixed_tokens_are_withheld() {
    let mut g = Gen::new(14);
    let letters = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let hex = "0123456789abcdef";
    for (token, shape) in [
        (format!("{}{}", "gh", "u_") + &g.base62(36), "github_token"),
        (format!("{}{}", "gh", "s_") + &g.base62(36), "github_token"),
        (format!("{}{}", "gh", "r_") + &g.base62(36), "github_token"),
        (
            format!("{}{}", "xo", "xa-2-") + &g.base62(40),
            "slack_token",
        ),
        (format!("{}{}", "xo", "xr-") + &g.base62(40), "slack_token"),
        (format!("{}{}", "xo", "xs-") + &g.base62(40), "slack_token"),
        (
            format!("{}{}", "xa", "pp-1-") + &g.base62(60),
            "slack_token",
        ),
        (
            format!("{}{}", "gl", "pat-") + &g.base62(20),
            "gitlab_token",
        ),
        (format!("{}{}", "np", "m_") + &g.base62(36), "npm_token"),
        (
            format!("{}{}", "py", "pi-AgEIcHlwaS5vcmc") + &g.base62(120),
            "pypi_token",
        ),
        (
            format!("{}{}", "h", "f_") + &g.of(letters, 34),
            "huggingface_token",
        ),
        (
            format!("{}{}.{}", "S", "G.", g.base62(22)) + &g.base62(43),
            "sendgrid_key",
        ),
        (
            format!("{}{}", "dop", "_v1_") + &g.of(hex, 64),
            "digitalocean_token",
        ),
        (
            format!("{}{}", "shp", "at_") + &g.of(hex, 32),
            "shopify_token",
        ),
        (format!("{}{}", "gs", "k_") + &g.base62(52), "groq_key"),
    ] {
        let (out, n) = scrub(&format!("export TOKEN='{token}'"));
        assert_eq!(
            (out, n),
            (format!("export TOKEN='[redacted:{shape}]'"), 1),
            "{token}"
        );
    }
    kept("from huggingface_hub import hf_hub_download; npm_config_cache=/tmp; SG.example");
    kept("glpat- and xapp- alone, gsk_short, dop_v1_abc");
}

/// A shape an encoding hid: in base64 (a Kubernetes secret's data), at each
/// offset, wrapped; percent-encoded; JSON-escaped with `\u`; and folded in
/// YAML's double-quoted style.
#[test]
fn a_shape_is_withheld_inside_an_encoding() {
    let mut g = Gen::new(15);
    let url = format!("postgres://app:{}@db:5432/x", g.base62(16));
    let b64 = super::corpus::b64;
    let (out, n) = scrub(&format!("data:\n  DATABASE_URL: {}\n", b64(url.as_bytes())));
    assert_eq!(
        (out.as_str(), n),
        ("data:\n  DATABASE_URL: [redacted:url_password]\n", 1)
    );
    let key = format!("{}{}{}", "sk_", "live_", g.base62(40));
    for pad in ["", "=", "=:"] {
        let enc = b64(format!("{pad}{key}").as_bytes())
            .replace('+', "-")
            .replace('/', "_");
        assert_eq!(
            scrub(&format!("t={enc}")).0,
            "t=[redacted:stripe_key]",
            "{pad:?}"
        );
    }
    let percent: String = key.bytes().map(|c| format!("%{c:02X}")).collect();
    assert_eq!(
        scrub(&format!("?k={percent}&")).0,
        "?k=[redacted:stripe_key]&"
    );
    let escaped: String = key
        .chars()
        .map(|c| format!("\\u{:04x}", c as u32))
        .collect();
    assert_eq!(
        scrub(&format!("{{\"k\": \"{escaped}\"}}")).0,
        "{\"k\": \"[redacted:stripe_key]\"}"
    );
    let pw = format!("{}\u{e9}{}", g.base62(8), g.base62(8));
    let folded = format!(
        "url: \"Inv\\xE9nted-{}:redis://:{}\\xE9\\\n  {}@cache:6379\"\n",
        "x".repeat(70),
        &pw[..8],
        &pw[10..]
    );
    let (out, n) = scrub(&folded);
    assert_eq!(n, 1, "{out}");
    assert!(!out.contains(&pw[..8]) && !out.contains(&pw[10..]), "{out}");
}

/// The cost of a scrub per MiB of ordinary output, printed; its bound is
/// loose, for a debug build on a busy machine: the number is the record.
#[test]
fn a_mib_of_ordinary_output_scrubs_in_bounded_time() {
    let clean: String = super::corpus::clean(7)
        .into_iter()
        .map(|(_, t)| t)
        .collect();
    let mut text = String::new();
    while text.len() < 1 << 20 {
        text.push_str(&clean);
    }
    let s = Scrubber::with_values(vec![("Inv3nted/Value+For~Tests?x=1".into(), "demo".into())]);
    let start = std::time::Instant::now();
    let (out, n) = s.scrub(&text);
    let took = start.elapsed();
    eprintln!(
        "scrub: {} bytes in {:?} ({:.1} ms/MiB), {n} replaced",
        text.len(),
        took,
        took.as_secs_f64() * 1000.0 * f64::from(1 << 20) / text.len() as f64
    );
    assert_eq!(n, 0, "the clean corpus holds no secret");
    assert_eq!(out.len(), text.len());
    assert!(took < std::time::Duration::from_secs(20), "{took:?}");
}
