# The Rust server during development: the source is mounted in, cargo-watch rebuilds and
# restarts it on every change, and restic is the real thing. Keep the Rust version in step
# with the flake.
FROM rust:1.99-slim

# git, so build.rs can tell the version and commit (the checkout is mounted in, owned by
# someone else); curl and bzip2 to fetch restic
RUN apt-get update \
    && apt-get install -y --no-install-recommends git curl bzip2 \
    && rm -rf /var/lib/apt/lists/* \
    && git config --system --add safe.directory '*'

# restic, pinned and checked against the release's own checksums
ARG RESTIC_VERSION=0.19.1
ARG TARGETARCH
RUN case "$TARGETARCH" in \
        amd64) sum=f415415624dcc452f2a02b8c33641791a8c6d6d3b65bbb3543fcf9a25151585c ;; \
        arm64) sum=a5f64aaab53d51e311fa3829124c5b703f2d14cf187d8640b6be3b2b49376465 ;; \
        *) echo "no restic for $TARGETARCH" && exit 1 ;; \
    esac \
    && curl -fsSL -o /tmp/restic.bz2 "https://github.com/restic/restic/releases/download/v${RESTIC_VERSION}/restic_${RESTIC_VERSION}_linux_${TARGETARCH}.bz2" \
    && echo "$sum  /tmp/restic.bz2" | sha256sum -c - \
    && bunzip2 /tmp/restic.bz2 \
    && install -m 755 /tmp/restic /usr/local/bin/restic \
    && rm /tmp/restic

RUN cargo install cargo-watch --locked \
    && rm -rf /usr/local/cargo/registry

# The build output and the crate downloads live in named volumes, which start out as a copy
# of these folders: writable by whichever user the container runs as
RUN mkdir -p /cargo-target /usr/local/cargo/registry \
    && chmod -R a+rwX /cargo-target /usr/local/cargo/registry

ENV CARGO_TARGET_DIR=/cargo-target
WORKDIR /app
