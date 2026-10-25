# Bug Hunt RS pilot: the hunt task's image. Built locally (never pushed): the seeded
# tree, its dependencies vendored, the pinned toolchain's components, nextest, and a
# warm target/ for the visible suite.
#
# Build context (kept beside the pilot, not in this repository): cargo-nextest, vendor/,
# cargo-config.toml and seeded/ (the planted tree). `build-and-check.sh` builds it.
FROM rust:1.98.1-slim-bookworm
RUN apt-get update && apt-get install -y --no-install-recommends git util-linux procps python3 \
      ca-certificates curl nodejs npm diffutils && rm -rf /var/lib/apt/lists/*
RUN rustup component add rustfmt clippy && rustup target add x86_64-unknown-linux-musl
COPY cargo-nextest /usr/local/cargo/bin/cargo-nextest
COPY vendor /opt/vendor
COPY cargo-config.toml /usr/local/cargo/config.toml
COPY seeded/ /app/
WORKDIR /app
ENV CARGO_NET_OFFLINE=true CARGO_TERM_COLOR=never
# The last step, after the planted files are in place and the build is done: every path under
# /app, the warm target/ and the directories too, takes the stamp the untouched files carry.
# A planted file must not be told from its neighbours by its time (theseus-8qx1i): the 10-17
# pilot found its bugs with `find -newermt` over the planted files' later stamp, and Claude Code
# did with `ls -l --time-style=full-iso` on 10-24. Keep this in the same RUN as the build: a
# layer of its own would store /app a second time.
RUN cargo nextest run --locked -p theseus-store -p theseus-kernel -p theseus-tools --no-run \
 && cargo build --locked -p theseus-store -p theseus-kernel -p theseus-tools --tests \
 && chmod -R a+rwX /app \
 && find /app -exec touch -h -d '2026-10-17 09:04:31' {} +
