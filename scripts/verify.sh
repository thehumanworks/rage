#!/usr/bin/env sh
set -eu

cd "$(dirname "$0")/.."

scripts/qa.sh
scripts/smoke-local.sh
scripts/harness-audit.sh

if [ "${RAGE_LIVE_GCP:-0}" = "1" ]; then
  scripts/smoke-gcp.sh
else
  echo "verify: skipped live GCP smoke; set RAGE_LIVE_GCP=1 with GCP_ACCESS_TOKEN to run it"
fi

echo "verify: ok"
