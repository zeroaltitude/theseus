//! A credential recognised by the **characters of the value**, for the
//! masking walk's map arm (theseus-3i6d1).
//!
//! **Why anything here at all.** Everything else under `aws/` keys on a
//! name or a shape: [`super::secret::NAMED`], `ENDS`, `HELD`, and the
//! output-shape rule's reading of AWS's `sensitive` marking. One predicate
//! read a value before this module — [`super::presigned::is_presigned`] —
//! and it recognises signed URLs and nothing else. So a tag or an
//! environment variable whose value *is* a credential (a `ghp_…`, an
//! `AKIA…`, a JWT a human pasted into a label), under a key nobody names
//! and a shape nobody marks, went to the model whole.
//!
//! **This is a seam, not a second list.** The rule itself is
//! [`crate::rsi::mine::read::secret_shaped`], borrowed whole: a prefix from
//! its `SECRET_PREFIXES`, or a run of 24 or more token characters mixing
//! letters and digits that is not a git id. It is *not* restated here, and
//! that is deliberate. B6 carries the named pattern "a hand-maintained list
//! with no completeness test", found three times in one night; a second
//! copy of a credential vocabulary is exactly that pattern, and the copies
//! drift apart in the direction that lets a credential through.
//!
//! **What a seam costs, and what it buys.** The borrowed rule's negatives
//! were chosen for the RSI miner's corpus — its own test pins `main`,
//! `feat/job-3`, a 40-character hex sha, `--no-ff`, a repository path — and
//! none of them was measured against AWS answers. Importing it bare would
//! let a tuning change made for the miner move what an AWS call shows the
//! model, with no test in the AWS path to notice. So the import lives
//! behind this one named function, which carries its own table of AWS
//! values ([`tests::THE_AWS_CORPUS`]) and its own AWS-side negative guard.
//! A change to the miner's rule that moves an AWS answer now fails a test
//! in the AWS path.
//!
//! **The AWS-side guard, measured.** `secret_shaped` over 24 invented but
//! realistic AWS answer values (ARNs, account ids, regions, instance and
//! AMI ids, a commit sha, digests, timestamps, CIDR blocks, bucket names,
//! key ids, a queue URL, a user agent) called 4 of them credentials. Two
//! were digests: a 64-character SHA-256 and an image digest
//! `sha256:<64 hex>`. The miner's rule excludes a hex run only at git-id
//! lengths (7 to 40), and AWS answers are full of hex digests at other
//! lengths — an S3 ETag, a checksum, an image digest — none of which can be
//! a credential, because a digest of something is not a secret for anything.
//! [`only_digests`] is that guard, and it takes the 4 to 2.
//!
//! **The 2 that remain are reported, not tuned away**: a bucket name with
//! no separators (`exampleorgdatalakearchive2026`) and a base64 public-key
//! fingerprint. Both are indistinguishable from a token by their characters
//! alone, and the bar is left where it is: this rule only ever holds *more*,
//! so its error is a bland value masked, never a credential shown. Whether
//! that trade is right where a map value is concerned is the owner's call,
//! and the same call the grafana workspace `name` reading asks for.
//!
//! **Where it is read.** The map arm of [`super::secret::walk`], on a
//! scalar value of a `Kind::String` shape, after the name rules have not
//! fired. Nowhere else: a value rule on every unmarked scalar of every
//! answer is a far larger blast radius and was not taken.

use crate::rsi::mine::read::secret_shaped;

/// Whether `v`'s characters are a credential's, for an AWS answer.
///
/// [`crate::rsi::mine::read::secret_shaped`]'s rule, less this path's own
/// negative: a value whose only credential evidence is a long hexadecimal
/// run is a digest, not a secret (see the module doc).
pub(super) fn credential_shaped(v: &str) -> bool {
    secret_shaped(v) && !only_digests(v)
}

/// `v` holds at least one run of 24 or more token characters and every such
/// run is hexadecimal: a digest, a checksum, an ETag, an image digest.
///
/// The runs are split exactly as `secret_shaped` splits them, so this reads
/// the same evidence the borrowed rule read.
fn only_digests(v: &str) -> bool {
    let mut any = false;
    for run in
        v.split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+' | '/' | '=')))
    {
        if run.len() < 24 {
            continue;
        }
        any = true;
        if !run.chars().all(|c| c.is_ascii_hexdigit()) {
            return false;
        }
    }
    any
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The AWS corpus: invented but realistic values an ordinary AWS read
    /// answers with, each with what this seam must say about it. Every
    /// `true` here is a value that would be masked in a map, so a row that
    /// flips to `true` is a new over-mask and a row that flips to `false`
    /// on a credential is a leak. The miner's rule is tuned for its own
    /// corpus; this table is what notices when that tuning reaches AWS.
    pub(super) const THE_AWS_CORPUS: &[(&str, &str, bool)] = &[
        ("a role ARN", "arn:aws:iam::210987654321:role/example-app-role", false),
        ("a task ARN", "arn:aws:ecs:us-west-2:210987654321:task/example-cluster/8f3c1d2e4b5a6978", false),
        ("an account id", "210987654321", false),
        ("a region", "us-west-2", false),
        ("an instance id", "i-0a1b2c3d4e5f60718", false),
        ("an AMI id", "ami-0a1b2c3d4e5f60718", false),
        ("a commit sha", "9f1c0b2a3d4e5f60718293a4b5c6d7e8f9012345", false),
        ("a sha256 digest", "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855", false),
        ("an image digest", "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855", false),
        ("an S3 ETag", "d41d8cd98f00b204e9800998ecf8427e", false),
        ("a timestamp", "2026-11-10T04:05:06Z", false),
        ("a CIDR block", "10.21.0.0/16", false),
        ("a bucket name", "example-logs-archive-us-west-2", false),
        ("a KMS key id", "1a2b3c4d-5e6f-4071-8293-a4b5c6d7e8f9", false),
        ("a KMS key ARN", "arn:aws:kms:us-west-2:210987654321:key/1a2b3c4d-5e6f-4071-8293-a4b5c6d7e8f9", false),
        ("an IAM unique id", "AROAEXAMPLEIDEXAMPLE", false),
        ("a log group name", "/aws/lambda/example-order-intake-handler", false),
        ("a user agent string", "aws-sdk-rust/1.42.0 os/linux lang/rust/1.98.1", false),
        ("a stack id", "arn:aws:cloudformation:us-west-2:210987654321:stack/example-stack/1a2b3c4d-5e6f-4071-8293-a4b5c6d7e8f9", false),
        ("a queue URL", "https://sqs.us-west-2.amazonaws.com/210987654321/example-intake-queue", false),
        ("an availability zone id", "usw2-az3", false),
        ("a security group id", "sg-0a1b2c3d4e5f60718", false),
        // The two the characters cannot tell from a token. Both are
        // over-masks this seam accepts rather than lower its bar for; see
        // the module doc.
        ("a bucket name with no separators", "exampleorgdatalakearchive2026", true),
        ("a public key fingerprint", "MC4wLjAuMEV4YW1wbGVGaW5nZXJwcmludDAwMQ", true),
    ];

    /// The credentials the seam must catch: one of each family the borrowed
    /// rule knows, so a guard added here can never empty it.
    const THE_CREDENTIALS: &[(&str, &str)] = &[
        ("an access key id", "AKIAEXAMPLEACCESSKEY"),
        ("a session key id", "ASIAEXAMPLEACCESSKEY"),
        ("a forge token", "ghp_ExampleExampleExampleExampleEx01"),
        (
            "a JWT",
            "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIwMDAxIn0.exampleexamplesign",
        ),
        ("a PEM block", "-----BEGIN EXAMPLE PRIVATE KEY-----"),
        ("a long opaque token", "Zk3j9QpLx7Vn2Rt8Wm4Yc6Hb1Fs5Dg0A"),
    ];

    /// Every row of the AWS corpus reads as the table says, and the counts
    /// are asserted, so neither list can pass by being empty.
    #[test]
    fn the_aws_corpus_reads_as_the_table_says() {
        assert_eq!(THE_AWS_CORPUS.len(), 24, "the corpus is the measurement");
        let mut masked = Vec::new();
        for (what, v, want) in THE_AWS_CORPUS {
            assert_eq!(credential_shaped(v), *want, "{what}: {v}");
            if *want {
                masked.push(*what);
            }
        }
        // Measured at base 61e72bf6: `secret_shaped` alone called 4 of the
        // 24 credentials; this seam's digest guard takes it to 2. Both are
        // over-masks, named in the table and in the module doc. A change
        // that moves this number is a change to what an AWS answer shows.
        assert_eq!(
            masked,
            [
                "a bucket name with no separators",
                "a public key fingerprint"
            ]
        );
    }

    /// The positive control: the guard cannot be met by refusing everything.
    #[test]
    fn every_credential_family_is_still_caught() {
        assert_eq!(THE_CREDENTIALS.len(), 6);
        for (what, v) in THE_CREDENTIALS {
            assert!(credential_shaped(v), "{what}: {v}");
        }
    }

    /// The digest guard reads the same runs the borrowed rule reads, and
    /// only fires where every long run is hexadecimal.
    #[test]
    fn a_digest_is_not_a_credential_and_a_token_beside_one_still_is() {
        assert!(only_digests(
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        ));
        assert!(only_digests(
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        ));
        // No long run at all: the guard says nothing and the rule decides.
        assert!(!only_digests("us-west-2"));
        // A digest beside a token is still a credential.
        assert!(credential_shaped(
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855 ghp_ExampleExampleExampleExampleEx01"
        ));
    }
}
