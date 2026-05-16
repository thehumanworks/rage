#!/usr/bin/env sh
set -eu

cd "$(dirname "$0")/.."

scripts/qa.sh
scripts/smoke-local.sh
scripts/harness-audit.sh

if [ "${RAGE_LIVE_INFISICAL:-0}" = "1" ]; then
  scripts/smoke-infisical.sh
else
  echo "verify: skipped live Infisical smoke; set RAGE_LIVE_INFISICAL=1 with INFISICAL_TOKEN to run it"
fi

echo "verify: ok"
