# Testing

## Definition-Of-Done Gate

Run this before claiming code, script, or harness changes are done:

```sh
scripts/verify.sh
```

It runs `scripts/qa.sh`, `scripts/smoke-local.sh`, and
`scripts/harness-audit.sh`. It runs live GCP only when `RAGE_LIVE_GCP=1` is set.

## QA Gate

Run this for the deterministic Rust quality gate:

```sh
scripts/qa.sh
```

It runs:

- `cargo fmt --check`
- `cargo clippy --locked --all-targets -- -D warnings`
- `cargo test --locked --bins`
- `cargo test --locked --tests`
- `cargo build --locked --release`
- release binary help smoke for the root command and every subcommand

## Local End-to-End Smoke

Run:

```sh
scripts/smoke-local.sh
```

This uses a temporary config/cache and lets `rage init` create a temporary age identity. It proves native age identity generation, recipient derivation, cache encryption/decryption, `rage load`, `rage exec`, and SSH-gated Keychain behavior. It must not touch real GCP, real Keychain items, or persistent config.

## Live GCP Smoke

Run only when live GCP verification is needed:

```sh
scripts/smoke-gcp.sh
```

The script requires `GCP_ACCESS_TOKEN` or `GOOGLE_OAUTH_ACCESS_TOKEN` with Secret Manager access and a disposable project selected by `RAGE_GCP_PROJECT` or `GOOGLE_CLOUD_PROJECT`. It creates a disposable root key, verifies `set -> sync -> get -> list`, then deletes the key.

Do not wire this script into default tests unless the environment is explicitly disposable.

## Harness Audit

Run:

```sh
scripts/harness-audit.sh
```

This checks that the AI-facing docs, scripts, project skill, and safety phrases exist and that `.env` is not tracked.

## Change Surface Matrix

See `docs/DEFINITION_OF_DONE.md` for the required evidence by change type.

Integration tests under `tests/` are part of the default QA gate. Do not move integration-impacting coverage behind a manual flag unless it requires live external infrastructure; use fake services for deterministic coverage.

## Test Coverage Map

- `src/main.rs` unit tests: encoding, dotenv rendering/parsing, shell quoting, JSON escaping, merge precedence, SSH script rendering.
- `src/agent_auth.rs` unit tests: timestamp handling and token redaction for provider refresh errors.
- `tests/cli.rs` integration tests: native age init/encrypt/decrypt, fake GCP Secret Manager round trips, fake `security` Keychain gate, fake SSH argument/script handling, output formats, `unset`, remote agent auth imports, fake OAuth refresh, and fake Grok/Codex child behavior.
- `scripts/smoke-local.sh`: release binary local end-to-end behavior.
- `scripts/smoke-gcp.sh`: live GCP Secret Manager behavior.

## When To Add Tests

Add or update tests whenever changing:

- CLI arguments or output.
- GCP Secret Manager HTTP request/response behavior.
- agent auth import, refresh, redaction, child environment, or managed auth-file behavior.
- cache path or encryption/decryption.
- shell quoting or dotenv parsing.
- SSH script generation.
- Keychain identity behavior.
- config defaults or migration behavior.
