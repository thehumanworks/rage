#!/usr/bin/env sh
set -eu

cd "$(dirname "$0")/.."

required_files="
AGENTS.md
README.md
docs/ARCHITECTURE.md
docs/TESTING.md
docs/AI_HARNESS.md
docs/DEFINITION_OF_DONE.md
.codex/skills/rage-secrets/SKILL.md
.codex/skills/rage-secrets/references/checklist.md
.github/workflows/ci.yml
Makefile
rust-toolchain.toml
scripts/qa.sh
scripts/verify.sh
scripts/smoke-local.sh
scripts/smoke-gcp.sh
scripts/harness-audit.sh
contract.md
"

for file in $required_files; do
  [ -f "$file" ] || {
    echo "missing harness file: $file" >&2
    exit 1
  }
done

for script in scripts/qa.sh scripts/verify.sh scripts/smoke-local.sh scripts/smoke-gcp.sh scripts/harness-audit.sh; do
  [ -x "$script" ] || {
    echo "script is not executable: $script" >&2
    exit 1
  }
done

grep 'File-based age identities are the default' AGENTS.md >/dev/null
grep 'Rust `age` crate' AGENTS.md docs/ARCHITECTURE.md .codex/skills/rage-secrets/SKILL.md >/dev/null
grep 'not require.*gcloud' AGENTS.md README.md .codex/skills/rage-secrets/SKILL.md >/dev/null
grep -- '--allow-ssh-keychain' AGENTS.md README.md .codex/skills/rage-secrets/SKILL.md >/dev/null
grep 'scripts/qa.sh' AGENTS.md README.md docs/TESTING.md >/dev/null
grep 'scripts/verify.sh' AGENTS.md README.md docs/TESTING.md docs/DEFINITION_OF_DONE.md .codex/skills/rage-secrets/SKILL.md >/dev/null
grep 'smoke-gcp' docs/TESTING.md >/dev/null
grep 'cargo test --locked --tests' scripts/qa.sh docs/TESTING.md >/dev/null
grep 'integration tests by default' docs/DEFINITION_OF_DONE.md >/dev/null
grep 'Definition Of Done' docs/DEFINITION_OF_DONE.md >/dev/null
grep 'Change Surface Matrix' docs/DEFINITION_OF_DONE.md >/dev/null
grep 'TUI Verification Contract' contract.md >/dev/null
grep 'scripts/verify.sh' .github/workflows/ci.yml Makefile >/dev/null

if grep -R 'need age\|Command::new("age"' tests src .github scripts/qa.sh scripts/smoke-local.sh scripts/smoke-gcp.sh 2>/dev/null; then
  echo "found a required external age binary dependency" >&2
  exit 1
fi

if grep -R 'age-keygen' tests src .github scripts/qa.sh scripts/smoke-local.sh scripts/smoke-gcp.sh 2>/dev/null; then
  echo "found a required external age keygen dependency" >&2
  exit 1
fi

if grep -R 'need gcloud\|Command::new("gcloud"\|active `gcloud`\|Install gcloud' tests src .github scripts/qa.sh scripts/smoke-local.sh scripts/smoke-gcp.sh 2>/dev/null; then
  echo "found a required external gcloud dependency" >&2
  exit 1
fi

if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  if git ls-files --error-unmatch .env >/dev/null 2>&1; then
    echo ".env is tracked; remove plaintext secret files from git" >&2
    exit 1
  fi
fi

echo "harness-audit: ok"
