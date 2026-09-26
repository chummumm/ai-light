#!/usr/bin/env bash
set -Eeuo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
command -v cargo >/dev/null || { echo 'Install Rust with rustup first.' >&2; exit 1; }
[[ -f Cargo.lock ]] || cargo generate-lockfile
cargo test -p light-core -p light-agent --locked
cargo build -p light-agent --release --locked
printf '\nUbuntu agent: %s/target/release/light-agent\n' "$PWD"
