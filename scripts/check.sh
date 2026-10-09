#!/usr/bin/env bash
# Full local gate, same steps as CI. Run before every push.
set -euo pipefail
cd "$(dirname "$0")/.."

echo "== frontend: install, typecheck, build"
npm ci --no-audit --no-fund
npm run check
npm run build

echo "== rust: format, lint, test"
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked

echo "== all checks passed"
