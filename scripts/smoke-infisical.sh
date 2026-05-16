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

if [ -f .env ]; then
  set -a
  . ./.env
  set +a
fi

if [ -z "${INFISICAL_TOKEN:-}" ] &&
  { [ -z "${INFISICAL_MACHINE_IDENTITY_CLIENT_ID:-}" ] ||
    [ -z "${INFISICAL_MACHINE_IDENTITY_CLIENT_SECRET:-}" ]; }; then
  echo "set INFISICAL_TOKEN or INFISICAL_MACHINE_IDENTITY_CLIENT_ID and INFISICAL_MACHINE_IDENTITY_CLIENT_SECRET" >&2
  exit 2
fi

project_arg=""
project_id="${RAGE_INFISICAL_PROJECT_ID:-${INFISICAL_PROJECT_ID:-}}"
if [ -n "$project_id" ]; then
  project_arg="--infisical-project-id $project_id"
fi

cargo build --release >/dev/null

tmp="$(mktemp -d)"
stamp="$(date +%s)-$$"
bundle="global"
key="RAGE_SMOKE_${stamp}"
key="$(printf '%s' "$key" | tr '-' '_')"

cleanup() {
  RAGE_CONFIG_DIR="$tmp/config" RAGE_CACHE_DIR="$tmp/cache" \
    ./target/release/rage unset "$bundle" "$key" >/dev/null 2>&1 || true
  rm -rf "$tmp"
}
trap cleanup EXIT INT TERM

export RAGE_CONFIG_DIR="$tmp/config"
export RAGE_CACHE_DIR="$tmp/cache"
mkdir -p "$RAGE_CONFIG_DIR" "$RAGE_CACHE_DIR"

# shellcheck disable=SC2086
./target/release/rage init \
  $project_arg \
  --age-identity "$tmp/key.txt" >/dev/null

./target/release/rage set "$bundle" "$key" ok >/dev/null
rm -f "$RAGE_CACHE_DIR"/rage-*.env.age
./target/release/rage sync "$bundle" >/dev/null

value="$(./target/release/rage get "$bundle" "$key")"
[ "$value" = "ok" ] || {
  echo "unexpected Infisical smoke value: $value" >&2
  exit 1
}

./target/release/rage list | grep "^$bundle$" >/dev/null

cleanup
trap - EXIT

echo "smoke-infisical: ok"
