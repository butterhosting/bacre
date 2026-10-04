default:
    @just --list

dev:
    HOST_UID=$(id -u) HOST_GID=$(id -g) docker compose -f compose.dev.yaml up --build

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
    bun install --cwd e2e
    bun run --cwd e2e tc

test:
    # in a zone with daylight saving, so the tests of the clock changes run everywhere
    TZ=Europe/Amsterdam cargo test
    bun install --cwd website
    bun run --cwd website test:unit

e2e: e2e-prepare
    bun run --cwd e2e start

e2e-headless: e2e-prepare
    bun run --cwd e2e start:headless

[private]
e2e-prepare: build
    bun install --cwd e2e
    bun run --cwd e2e install:browsers
    docker compose -f compose.e2e.yaml down
    @if curl --silent --output /dev/null --max-time 2 http://localhost:3000/; then \
        echo "Port 3000 is in use; stop the dev stack (just dev) first" >&2; \
        exit 1; \
    fi
