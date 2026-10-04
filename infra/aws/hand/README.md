# infra/aws/hand: the hand image

A hand is a job that runs in AWS and keeps the job wrapper's contract (the AWS design, `docs/design/aws-toolset.md`,
§3.3; step 40, theseus-mgw.6). This image is where it runs: a slim Debian base, the static musl `theseusd` as
`/usr/local/bin/theseusd`, git, Python 3, curl, jq, zip, and the AWS CLI v2.

- **Lambda.** The image is the `theseus-hand-basic` function's code. Its entry point is `theseusd hand`, which finds
  `AWS_LAMBDA_RUNTIME_API` and serves Lambda's custom runtime: each event is one hand's spec.
- **Fargate.** The task's `hand` container runs `theseusd hand` with its spec in `THESEUS_HAND`. For a hand in another
  image, an init container of this one copies the binary into a shared volume, and that image runs `/hand/theseusd
  hand` once the copy has succeeded.
- **What a hand does** (`crates/theseus-core/src/aws/hands/hand.rs`): it runs its argv in `/tmp/hand-<correlation
  id>` under its deadline, streams its output to `/theseus/hands` (`hands/<group>/<index>`), uploads `result.json`,
  `output.txt`, and every file under `out/` to `s3://theseus-<account>-<region>/hands/<correlation id>/`, and sends its
  completion envelope, signed with its own key, to `theseus-completions`. It never sees the daemon's key.

## Building and pushing

```bash
infra/aws/hand/build.sh                  # theseus/hand:<commit>, built and smoke-run locally
infra/aws/hand/build.sh --push us-west-2 # then pushed to theseus/hand in ECR; prints HandImageUri=…@sha256:…
```

The build needs Docker, `musl-gcc` (Debian's `musl-tools`), and the rustup target `x86_64-unknown-linux-musl`. The
push needs the AWS CLI with the operator's own credentials, from the operator's shell. The theseus-hands stack's
`HandImageUri` parameter then takes the printed URI, by digest; the stack makes the Lambda hand once it is set.

## What it costs

Nothing while idle: an image in ECR (about $0.10 a GB a month), and the Lambda function costs nothing until it runs.
Fargate hands need the hands VPC's NAT (`theseus-hands-network`'s `NatGateway`, about $36 a month while enabled) to
pull the image and reach the queue and the logs. Theseus never turns it on: a Fargate call with it off fails, saying
so, and the default backend for short work is Lambda, which needs no VPC.
