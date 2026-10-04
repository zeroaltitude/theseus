# The hand image (AWS design §3.3; step 40, theseus-mgw.6): a slim base, the static
# daemon binary as `theseusd hand`, git, Python, and the common CLIs. The same image
# serves Lambda (the binary is its custom runtime's bootstrap: ENTRYPOINT and CMD) and
# Fargate (the task's `hand` container). For a Fargate hand in another image, an init
# container of this one copies /usr/local/bin/theseusd into a shared volume.
#
# Built by infra/aws/hand/build.sh, which puts the musl binary beside this file first.
# The base is pinned by digest (bookworm-slim on 2026-10-04): move it with a reviewed
# `docker pull debian:bookworm-slim`, and its digest here.
FROM debian:bookworm-slim@sha256:3783cc01769c7b2b1b83a5c5ad96c815348e28ed7da68e2e3687004faa906251

# The AWS CLI v2, pinned by version and by the zip's sha256 (fetched 2026-10-04).
ARG AWSCLI_VERSION=2.27.50
ARG AWSCLI_SHA256=2ed5b5a1d92f3aa860034b0d9ec0902574852c3e60916e221414abb6f1504d9e

# hadolint ignore=DL3008
RUN set -eux; \
    apt-get update; \
    apt-get install -y --no-install-recommends \
        ca-certificates curl git jq less openssh-client python3 python3-venv unzip zip; \
    curl -fsSL "https://awscli.amazonaws.com/awscli-exe-linux-x86_64-${AWSCLI_VERSION}.zip" -o /tmp/awscli.zip; \
    printf '%s  /tmp/awscli.zip\n' "${AWSCLI_SHA256}" > /tmp/awscli.sha256; \
    sha256sum -c /tmp/awscli.sha256; \
    unzip -q /tmp/awscli.zip -d /tmp; \
    /tmp/aws/install --bin-dir /usr/local/bin --install-dir /usr/local/aws-cli; \
    rm -rf /tmp/aws /tmp/awscli.zip /tmp/awscli.sha256 /var/lib/apt/lists/*; \
    useradd --uid 1000 --create-home --home-dir /home/hand --shell /usr/sbin/nologin hand

COPY --chmod=0755 theseusd /usr/local/bin/theseusd

# A hand works under /tmp, the one place Lambda lets it write; HOME is there too.
ENV HOME=/tmp THESEUS_HAND_DIR=/tmp
USER 1000
WORKDIR /tmp
ENTRYPOINT ["/usr/local/bin/theseusd"]
CMD ["hand"]
