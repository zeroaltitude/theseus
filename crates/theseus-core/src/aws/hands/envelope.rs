//! What a hand is given, and what it sends home (AWS design §3.3; spec
//! §3.16's `Completion` envelope and its `signature`).
//!
//! - **The key.** Each dispatch has its own HMAC key, derived with HKDF
//!   (SHA-256) from the vault's AWS secret and the hand's correlation id
//!   ([`derive_key`]). Nothing new is stored: the daemon derives it again to
//!   check a completion, and the hand holds only its own, so a hand can sign
//!   for itself and for no other.
//! - **The spec** ([`HandSpec`]) is the hand's whole input: its argv, its
//!   deadline, where its result and logs go, the queue, and its key. It
//!   rides in the Lambda event, or in the Fargate task's environment.
//! - **The envelope** ([`Envelope`]) is the completion a hand sends to the
//!   queue: the spec's ids, the outcome, the exit code, where the result is,
//!   and the signature over all of it ([`Envelope::signing_bytes`]).

use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

/// The envelope's and the spec's version.
pub const VERSION: u32 = 1;

/// HKDF's salt: names what the key is for, so the same secret derives
/// nothing else's key.
const SALT: &[u8] = b"theseus-hands/1";

/// The signature's scheme, before its hex.
const SCHEME: &str = "hmac-sha256:";

/// The most of a hand's output its envelope carries; the rest is in S3.
pub const TAIL_BYTES: usize = 4096;

/// The per-dispatch key: HKDF-SHA256 of the account's secret access key,
/// with the correlation id as its info.
pub fn derive_key(secret: &[u8], correlation_id: &str) -> Zeroizing<[u8; 32]> {
    let mut okm = Zeroizing::new([0u8; 32]);
    Hkdf::<Sha256>::new(Some(SALT), secret)
        .expand(correlation_id.as_bytes(), okm.as_mut())
        .expect("32 bytes is a valid length for HKDF-SHA256");
    okm
}

/// Everything a hand needs, and nothing else of Theseus's.
#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct HandSpec {
    pub v: u32,
    pub correlation_id: String,
    /// The group's id: the correlation id of the `aws.hands.run` call.
    pub group: String,
    pub index: u32,
    /// This hand's input: an item of the call's `inputs`, or null.
    #[serde(default)]
    pub input: Value,
    pub argv: Vec<String>,
    /// The wrapper's own deadline (§3.3's first TTL layer).
    pub deadline_secs: u64,
    pub region: String,
    pub bucket: String,
    pub queue_url: String,
    pub log_group: String,
    /// `lambda` or `fargate`: the envelope's producer.
    pub backend: String,
    /// The per-dispatch key, in hex.
    pub key: String,
    /// Every request goes here instead of AWS's endpoint: a test's fake.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
}

impl std::fmt::Debug for HandSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HandSpec")
            .field("correlation_id", &self.correlation_id)
            .field("group", &self.group)
            .field("index", &self.index)
            .field("argv", &self.argv)
            .field("backend", &self.backend)
            .field("key", &"<withheld>")
            .finish_non_exhaustive()
    }
}

impl HandSpec {
    /// Its key's bytes, or why it has none.
    pub fn key_bytes(&self) -> Result<Zeroizing<Vec<u8>>, String> {
        hex::decode(&self.key)
            .map(Zeroizing::new)
            .map_err(|e| format!("the hand's key is not hex: {e}"))
    }

    /// The S3 prefix its result goes under.
    pub fn prefix(&self) -> String {
        format!("hands/{}/", self.correlation_id)
    }

    /// Its CloudWatch Logs stream: `hands/<group>/<index>`.
    pub fn log_stream(&self) -> String {
        format!("hands/{}/{}", self.group, self.index)
    }
}

/// A hand's completion, as it sends it to the queue.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Envelope {
    pub v: u32,
    pub correlation_id: String,
    pub group: String,
    pub index: u32,
    /// `succeeded`, `failed`, or `unknown`.
    pub outcome: String,
    #[serde(default)]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub timed_out: bool,
    /// `s3://<bucket>/hands/<correlation id>/`, once its result is there.
    #[serde(default)]
    pub result_ref: Option<String>,
    /// The Lambda request id, or the ECS task's ARN.
    #[serde(default)]
    pub external_op_id: Option<String>,
    pub started_at_ms: u64,
    pub finished_at_ms: u64,
    /// `hand:lambda` or `hand:fargate`.
    pub producer: String,
    /// The end of its output.
    #[serde(default)]
    pub tail: String,
    /// What did not go as it should, beside the job's own outcome: a result
    /// that could not be uploaded, say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// `hmac-sha256:<hex>` over [`Envelope::signing_bytes`].
    #[serde(default)]
    pub signature: String,
}

impl Envelope {
    /// The bytes the signature covers: every field but the signature, each
    /// length-prefixed so no field's text can pass for another's, the tail
    /// by its digest.
    pub fn signing_bytes(&self) -> Vec<u8> {
        let tail = hex::encode(Sha256::digest(self.tail.as_bytes()));
        let fields: [&str; 14] = [
            "theseus-hand-envelope",
            &self.v.to_string(),
            &self.correlation_id,
            &self.group,
            &self.index.to_string(),
            &self.outcome,
            &self.exit_code.map_or(String::new(), |c| c.to_string()),
            if self.timed_out { "1" } else { "0" },
            self.result_ref.as_deref().unwrap_or_default(),
            self.external_op_id.as_deref().unwrap_or_default(),
            &format!("{}-{}", self.started_at_ms, self.finished_at_ms),
            &self.producer,
            &tail,
            self.note.as_deref().unwrap_or_default(),
        ];
        let mut out = Vec::new();
        for f in fields {
            out.extend_from_slice(f.len().to_string().as_bytes());
            out.push(b':');
            out.extend_from_slice(f.as_bytes());
            out.push(b'\n');
        }
        out
    }

    fn mac(&self, key: &[u8]) -> Hmac<Sha256> {
        let mut m = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("HMAC takes any key");
        m.update(&self.signing_bytes());
        m
    }

    /// Sign it with `key`.
    pub fn sign(&mut self, key: &[u8]) {
        self.signature = format!(
            "{SCHEME}{}",
            hex::encode(self.mac(key).finalize().into_bytes())
        );
    }

    /// Whether its signature is `key`'s, compared in constant time.
    pub fn verify(&self, key: &[u8]) -> bool {
        let Some(hexed) = self.signature.strip_prefix(SCHEME) else {
            return false;
        };
        let Ok(bytes) = hex::decode(hexed) else {
            return false;
        };
        self.mac(key).verify_slice(&bytes).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope() -> Envelope {
        Envelope {
            v: VERSION,
            correlation_id: "act_example_1".into(),
            group: "act_group_1".into(),
            index: 3,
            outcome: "succeeded".into(),
            exit_code: Some(0),
            timed_out: false,
            result_ref: Some("s3://example-bucket/hands/act_example_1/".into()),
            external_op_id: Some("req-1".into()),
            started_at_ms: 1_000,
            finished_at_ms: 2_000,
            producer: "hand:lambda".into(),
            tail: "done\n".into(),
            note: None,
            signature: String::new(),
        }
    }

    /// A key is the secret's and the correlation id's: another of either
    /// derives another key, and the same derive the same.
    #[test]
    fn each_dispatch_derives_its_own_key() {
        let a = derive_key(b"secret-one", "act_1");
        assert_eq!(*a, *derive_key(b"secret-one", "act_1"));
        assert_ne!(*a, *derive_key(b"secret-one", "act_2"));
        assert_ne!(*a, *derive_key(b"secret-two", "act_1"));
    }

    /// A signed envelope checks with its key, and with no other; a change
    /// to any field it carries, or a missing signature, fails the check.
    #[test]
    fn a_signature_checks_only_with_its_key_and_its_fields() {
        let key = derive_key(b"secret-one", "act_example_1");
        let mut e = envelope();
        assert!(!e.verify(key.as_ref()), "unsigned");
        e.sign(key.as_ref());
        assert!(e.verify(key.as_ref()));
        assert!(!e.verify(derive_key(b"secret-one", "act_example_2").as_ref()));
        let changes: [&dyn Fn(&mut Envelope); 7] = [
            &|e| e.outcome = "failed".into(),
            &|e| e.exit_code = Some(1),
            &|e| e.correlation_id = "act_example_2".into(),
            &|e| e.tail.push('x'),
            &|e| e.timed_out = true,
            &|e| e.result_ref = None,
            &|e| e.note = Some("x".into()),
        ];
        for change in changes {
            let mut f = e.clone();
            change(&mut f);
            assert!(!f.verify(key.as_ref()), "{f:?}");
        }
        // Survives the wire.
        let back: Envelope = serde_json::from_str(&serde_json::to_string(&e).unwrap()).unwrap();
        assert!(back.verify(key.as_ref()));
    }

    /// One field's text cannot pass for two: the lengths prefix each.
    #[test]
    fn fields_cannot_be_shifted_between_each_other() {
        let mut a = envelope();
        a.group = "g\n5:x".into();
        let mut b = envelope();
        b.group = "g".into();
        assert_ne!(a.signing_bytes(), b.signing_bytes());
    }

    /// A spec's debug form never shows its key.
    #[test]
    fn a_specs_debug_withholds_its_key() {
        let s = HandSpec {
            v: VERSION,
            correlation_id: "act_1".into(),
            group: "act_g".into(),
            index: 0,
            input: Value::Null,
            argv: vec!["true".into()],
            deadline_secs: 60,
            region: "us-west-2".into(),
            bucket: "example-bucket".into(),
            queue_url: "https://sqs.example/q".into(),
            log_group: "/theseus/hands".into(),
            backend: "lambda".into(),
            key: "00ff".repeat(16),
            endpoint: None,
        };
        let d = format!("{s:?}");
        assert!(!d.contains("00ff"), "{d}");
        assert_eq!(s.key_bytes().unwrap().len(), 32);
    }
}
