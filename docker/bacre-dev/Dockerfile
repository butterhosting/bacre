# The Rust server during development: the source is mounted in, cargo-watch rebuilds and
# restarts it on every change. Keep the Rust version in step with the flake.
FROM rust:1.99-slim

# git, so build.rs can tell the version and commit; the checkout is mounted in, owned by someone else
RUN apt-get update \
    && apt-get install -y --no-install-recommends git \
    && rm -rf /var/lib/apt/lists/* \
    && git config --system --add safe.directory '*'

RUN cargo install cargo-watch --locked \
    && rm -rf /usr/local/cargo/registry

# The build output and the crate downloads live in named volumes, which start out as a copy
# of these folders: writable by whichever user the container runs as
RUN mkdir -p /cargo-target /usr/local/cargo/registry \
    && chmod -R a+rwX /cargo-target /usr/local/cargo/registry

ENV CARGO_TARGET_DIR=/cargo-target
WORKDIR /app
