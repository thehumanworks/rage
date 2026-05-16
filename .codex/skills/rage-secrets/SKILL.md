---
name: rage-secrets
description: Use when working inside the rage repository, a Rust CLI for Infisical backed personal secrets with age-encrypted local cache, shell/exec/ssh loading, agent auth runners, and explicit macOS Keychain SSH gating.
---

# rage-secrets

Use this skill for changes under `/Users/mish/tools/rage`.

## First Reads

Read these before editing behavior:

- `AGENTS.md`
- `README.md`
- `docs/ARCHITECTURE.md`
- `docs/TESTING.md`
- `docs/DEFINITION_OF_DONE.md`
- `tests/cli.rs` for black-box contracts

For a compact completion checklist, read `references/checklist.md`.

## Core Rules

- Preserve the model: `Infisical -> rage sync -> age-encrypted local cache -> rage load/exec/shell/ssh`.
- Keep file-based age identities as the default.
- Keep age generation/encryption/decryption native through the Rust `age` crate; do not require the `age` or `age-keygen` binaries.
- Keep Infisical access native through HTTPS; do not require the `infisical` or `gcloud` CLI.
- Keep macOS Keychain identity loading explicit.
- If running over SSH, Keychain identity loading must require `--allow-ssh-keychain`.
- Never write plaintext secrets into repo files, tests, logs, or process arguments.
- Keep `cargo test` deterministic. Use the fake Infisical server and fake external tools there.
- Use `scripts/smoke-infisical.sh` only for explicit live Infisical verification.

## Verification

Run the narrowest sufficient set, but do not skip the required gate for changed surfaces:

- Default completion gate: `scripts/verify.sh`
- Normal code inner loop: `scripts/qa.sh` and `scripts/smoke-local.sh`
- Infisical request behavior: add `scripts/smoke-infisical.sh` or `RAGE_LIVE_INFISICAL=1 scripts/verify.sh`
- Harness/docs/scripts: `scripts/harness-audit.sh`

Report what ran and what was skipped.
