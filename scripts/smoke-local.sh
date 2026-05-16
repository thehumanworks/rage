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

cargo build --release >/dev/null

tmp="$(mktemp -d)"
cleanup() {
  rm -rf "$tmp"
}
trap cleanup EXIT INT TERM

export RAGE_CONFIG_DIR="$tmp/config"
export RAGE_CACHE_DIR="$tmp/cache"
unset INFISICAL_TOKEN RAGE_INFISICAL_ENDPOINT RAGE_INFISICAL_PROJECT_ID RAGE_INFISICAL_ENVIRONMENT
mkdir -p "$RAGE_CONFIG_DIR" "$RAGE_CACHE_DIR"

./target/release/rage init \
  --infisical-project-id local-smoke \
  --age-identity "$tmp/key.txt" >/dev/null

test -f "$tmp/key.txt"
grep 'AGE-SECRET-KEY-' "$tmp/key.txt" >/dev/null
grep 'age_recipient = "age1' "$RAGE_CONFIG_DIR/rage/config.toml" >/dev/null

./target/release/rage auth status | grep '^auth: not-configured$' >/dev/null

fake_bin="$tmp/fake-bin"
mkdir -p "$fake_bin"
cat >"$fake_bin/security" <<EOF
#!/usr/bin/env sh
cat "$tmp/key.txt"
EOF
chmod +x "$fake_bin/security"

recipient="$(sed -n 's/^age_recipient = "\\(.*\\)"/\\1/p' "$RAGE_CONFIG_DIR/rage/config.toml")"

./target/release/rage init \
  --infisical-project-id local-smoke \
  --age-recipient "$recipient" \
  --age-identity acct \
  --age-identity-source keychain \
  --keychain-service rage-smoke \
  --keychain-account acct >/dev/null

if PATH="$fake_bin:$PATH" SSH_CONNECTION="127.0.0.1 1 127.0.0.1 2" \
  ./target/release/rage load global >/tmp/rage-smoke-out 2>"$tmp/keychain.err"; then
  echo "expected Keychain load over SSH to fail without explicit flag" >&2
  exit 1
fi

grep 'refusing to read macOS Keychain identity from an SSH session' "$tmp/keychain.err" >/dev/null

echo "smoke-local: ok"
