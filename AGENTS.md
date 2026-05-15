# AGENTS.md

This file governs the whole `rage` repository.

## Project Contract

`rage` is a fast Rust CLI for personal secrets:

```text
GCP Secret Manager -> rage sync -> age-encrypted local cache -> rage load/exec/shell/ssh
```

Preserve these invariants:

- GCP is the remote source of truth; normal shell startup must not require a network fetch.
- The local cache must remain age-encrypted.
- Plaintext secrets must not be written to repo files, test fixtures, logs, command lines, or docs.
- File-based age identities are the default.
- macOS Keychain identity loading is opt-in.
- When running over SSH, Keychain identity loading must require an explicit `--allow-ssh-keychain` style flag.
- `rage ssh` must not put secret values in local process arguments. Prefer stdin or environment on a child only when intentional.
- Tests must be deterministic by default. Live GCP tests must be explicit and disposable.

## Required First Reads

Before changing behavior, read:

- `README.md`
- `docs/ARCHITECTURE.md`
- `docs/TESTING.md`
- `docs/DEFINITION_OF_DONE.md`
- Relevant command implementation in `src/main.rs`
- Relevant integration tests in `tests/cli.rs`

For AI-harness work, also read:

- `.codex/skills/rage-secrets/SKILL.md`

## Change Discipline

- Keep the CLI small and boring. Prefer explicit commands and clear error messages over background magic.
- Do not add a long-running daemon unless the user explicitly asks for one.
- Keep age operations native through the Rust `age` crate; do not require users or CI to install external age binaries.
- Keep GCP access native through HTTPS/OAuth calls; do not require users or CI to install `gcloud`.
- Do not broaden IAM assumptions. Remote-machine examples should stay read-only unless writing is required.
- Do not silently make Keychain the default path for SSH.
- Keep live external behavior behind scripts or explicit flags. Never make `cargo test` require GCP, SSH hosts, or a real Keychain item.
- If command output may include secrets, assert on structure or sentinel values only.

## Verification Contract

For the default local definition of done, run:

```sh
scripts/verify.sh
```

For faster inner loops while iterating, use the narrow checks in
`docs/DEFINITION_OF_DONE.md`, then finish with `scripts/verify.sh`.

For normal code changes, the minimum component checks are:

```sh
scripts/qa.sh
scripts/smoke-local.sh
```

For changes touching GCP command construction or Secret Manager behavior, also run:

```sh
scripts/smoke-gcp.sh
```

only when a disposable GCP project/account is configured.

For harness/docs/script changes, run:

```sh
scripts/harness-audit.sh
```

Before claiming completion, report exactly which scripts/checks ran and which external checks were skipped.

## File Map

- `src/main.rs`: CLI implementation and unit tests.
- `tests/cli.rs`: black-box integration tests with fake Secret Manager, `ssh`, and `security`.
- `scripts/qa.sh`: deterministic Rust quality gate.
- `scripts/smoke-local.sh`: local end-to-end smoke using temporary age identity/cache.
- `scripts/smoke-gcp.sh`: disposable live GCP smoke with cleanup.
- `scripts/harness-audit.sh`: validates the AI harness files and safety assumptions.
- `scripts/verify.sh`: deterministic local definition-of-done gate.
- `docs/`: architecture, testing, and agent-oriented context.
- `contract.md`: feature-level verification contract for the current TUI work.
- `.codex/skills/rage-secrets/`: project-local skill for future agents.
