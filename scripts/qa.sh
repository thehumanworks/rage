#!/usr/bin/env sh
set -eu

cd "$(dirname "$0")/.."

cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --bins
cargo test --locked --tests
cargo build --locked --release
./target/release/rage --help >/dev/null
for command in init auth config list set unset delete-bundle get sync load exec shell ssh tui; do
  ./target/release/rage "$command" --help >/dev/null
done

echo "qa: ok"
