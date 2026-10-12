#!/usr/bin/env bash
# Build the hand image (AWS design §3.3; step 40, theseus-mgw.6), and push it to the
# theseus-hands stack's ECR repository, theseus/hand.
#
#   infra/aws/hand/build.sh                      build theseus/hand:<commit> locally
#   infra/aws/hand/build.sh --push <region>      build, then push to this account's
#                                                repository, and print the image's
#                                                digest URI for the stack's HandImageUri
#   infra/aws/hand/build.sh --rootless ...       the same without Docker: rootless.py
#                                                builds the Dockerfile in a user
#                                                namespace, and crane assembles and
#                                                pushes the image (CRANE names crane)
#
# The binary is the static musl theseusd of scripts/build.sh's release-thin profile.
# Pushing reads the account from the AWS CLI's own credentials (the operator's shell,
# never a job: AGENTS.md's security posture). The repository's tags are immutable, so
# each commit pushes once.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../.." && pwd)
target=x86_64-unknown-linux-musl
push_region=""
builder=docker

while [ $# -gt 0 ]; do
    case "$1" in
        --push)
            push_region="${2:?--push takes the region}"
            shift 2
            ;;
        --rootless)
            builder=rootless
            shift
            ;;
        -h|--help)
            sed -n '2,18p' "$0"
            exit 0
            ;;
        *)
            echo "build.sh: unknown argument $1" >&2
            exit 2
            ;;
    esac
done

commit=$(git -C "$root" rev-parse --short=12 HEAD)
if [ -n "$(git -C "$root" status --porcelain)" ]; then
    echo "build.sh: the tree has uncommitted changes; an image is named by its commit" >&2
    exit 1
fi

"$root/scripts/build.sh" --profile release-thin --target "$target" -p theseusd
bin="${CARGO_TARGET_DIR:-$root/target}/$target/release-thin/theseusd"
# `file` says "statically linked" of a static binary, and "static-pie linked" of the
# static-pie one a Rust musl build makes.
kind=$(file "$bin")
if ! grep -qE 'statically linked|static-pie linked' <<<"$kind"; then
    echo "build.sh: $bin is not a static binary" >&2
    exit 1
fi

context=$(mktemp -d)
# The rootless build's tree has read-only directories of the base's.
trap 'chmod -R u+w "$context" 2>/dev/null; rm -rf "$context"' EXIT
cp "$here/Dockerfile" "$context/"
cp "$bin" "$context/theseusd"
image="theseus/hand:$commit"

if [ "$builder" = rootless ]; then
    crane="${CRANE:-crane}"
    if ! command -v "$crane" >/dev/null; then
        echo "build.sh: --rootless needs crane (go-containerregistry) on PATH, or CRANE" >&2
        exit 1
    fi
    export CRANE="$crane"
    python3 "$here/rootless.py" build --dockerfile "$here/Dockerfile" --context "$context" --work "$context/work"
    if [ -z "$push_region" ]; then
        out="${CARGO_TARGET_DIR:-$root/target}/hand/theseus-hand-$commit.tar"
        mkdir -p "$(dirname "$out")"
        python3 "$here/rootless.py" assemble --work "$context/work" --output "$out"
        echo "$image saved as $out"
        exit 0
    fi
    account=$(aws sts get-caller-identity --query Account --output text)
    registry="$account.dkr.ecr.$push_region.amazonaws.com"
    remote="$registry/theseus/hand:$commit"
    # crane's login lives in a scratch DOCKER_CONFIG, removed with the context.
    export DOCKER_CONFIG="$context/docker-config"
    mkdir -m 700 "$DOCKER_CONFIG"
    aws ecr get-login-password --region "$push_region" \
        | "$crane" auth login "$registry" --username AWS --password-stdin
    python3 "$here/rootless.py" assemble --work "$context/work" --tag "$remote"
    digest=$(aws ecr describe-images --region "$push_region" --repository-name theseus/hand \
        --image-ids imageTag="$commit" --query 'imageDetails[0].imageDigest' --output text)
    echo "HandImageUri=$registry/theseus/hand@$digest"
    exit 0
fi

docker build --platform linux/amd64 -t "$image" "$context"
# The image runs its role: a spec that is not there is its own error, not a crash.
# `theseusd hand` without its spec prints that and exits 1, so the output is taken first:
# a pipeline under pipefail would answer with docker's status.
out=$(docker run --rm --entrypoint /usr/local/bin/theseusd "$image" hand 2>&1 || true)
if grep -q 'THESEUS_HAND is not set' <<<"$out"; then
    echo "build.sh: $image answers as a hand"
else
    echo "build.sh: $image does not run theseusd hand" >&2
    exit 1
fi

if [ -z "$push_region" ]; then
    echo "$image"
    exit 0
fi

account=$(aws sts get-caller-identity --query Account --output text)
registry="$account.dkr.ecr.$push_region.amazonaws.com"
aws ecr get-login-password --region "$push_region" \
    | docker login --username AWS --password-stdin "$registry"
remote="$registry/theseus/hand:$commit"
docker tag "$image" "$remote"
docker push "$remote"
digest=$(aws ecr describe-images --region "$push_region" --repository-name theseus/hand \
    --image-ids imageTag="$commit" --query 'imageDetails[0].imageDigest' --output text)
echo "HandImageUri=$registry/theseus/hand@$digest"
