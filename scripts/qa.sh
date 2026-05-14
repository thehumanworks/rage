#!/usr/bin/env sh
set -eu

cd "$(dirname "$0")/.."

cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
./target/release/rage --help >/dev/null

echo "qa: ok"
