#!/usr/bin/env sh
set -eu

cd "$(dirname "$0")/.."

need() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "missing required command: $1" >&2
    exit 127
  }
}

need cargo

project="${RAGE_GCP_PROJECT:-}"
[ -n "$project" ] || {
  echo "set RAGE_GCP_PROJECT" >&2
  exit 2
}

if [ -z "${RAGE_GCP_ACCESS_TOKEN:-}" ] && [ -z "${RAGE_GCP_SERVICE_ACCOUNT_JSON:-}" ]; then
  echo "set RAGE_GCP_ACCESS_TOKEN or RAGE_GCP_SERVICE_ACCOUNT_JSON" >&2
  exit 2
fi

cargo build --release >/dev/null

tmp="$(mktemp -d)"
prefix="rageit$(date +%s)"
bundle="integration/test"

cleanup() {
  RAGE_CONFIG_DIR="$tmp/config" RAGE_CACHE_DIR="$tmp/cache" \
    ./target/release/rage delete-bundle "$bundle" --yes >/dev/null 2>&1 || true
  rm -rf "$tmp"
}
trap cleanup EXIT INT TERM

export RAGE_CONFIG_DIR="$tmp/config"
export RAGE_CACHE_DIR="$tmp/cache"
mkdir -p "$RAGE_CONFIG_DIR" "$RAGE_CACHE_DIR"

./target/release/rage init \
  --gcp-project "$project" \
  --age-identity "$tmp/key.txt" \
  --secret-prefix "$prefix" >/dev/null

./target/release/rage set "$bundle" RAGE_TEST_VALUE ok >/dev/null
rm -f "$RAGE_CACHE_DIR/$prefix-aW50ZWdyYXRpb24vdGVzdA.env.age"
./target/release/rage sync "$bundle" >/dev/null

value="$(./target/release/rage get "$bundle" RAGE_TEST_VALUE)"
[ "$value" = "ok" ] || {
  echo "unexpected GCP smoke value: $value" >&2
  exit 1
}

./target/release/rage list | grep '^integration/test$' >/dev/null

cleanup
trap - EXIT

echo "smoke-gcp: ok"
