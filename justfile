default:
    @just --list

dev:
    HOST_UID=$(id -u) HOST_GID=$(id -g) docker compose up --build

build:
    #!/usr/bin/env bash
    set -euo pipefail
    bun install --cwd website --frozen-lockfile
    bun run --cwd website build
    mkdir -p dist
    for pair in x86_64-unknown-linux-musl:amd64 aarch64-unknown-linux-musl:arm64; do
        target="${pair%%:*}"
        cargo build --release --target "$target"
        cp "target/$target/release/bacre" "dist/bacre-linux-${pair##*:}"
    done
    ls -lh dist

lint:
    cargo fmt --check
    cargo clippy --all-targets -- -D warnings
    bun install --cwd website
    bun run --cwd website lint

test:
    # in a zone with daylight saving, so the tests of the clock changes run everywhere
    TZ=Europe/Amsterdam cargo test
    bun install --cwd website
    bun run --cwd website test:unit
