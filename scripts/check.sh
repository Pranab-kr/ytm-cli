#!/usr/bin/env bash
set -euo pipefail
cargo fmt --all -- --check
cargo clippy --all-targets --workspace -- -D warnings
cargo test --workspace
echo "gate: OK"
