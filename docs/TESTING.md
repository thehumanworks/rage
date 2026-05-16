# Testing

## Definition-Of-Done Gate

Run this before claiming code, script, or harness changes are done:

```sh
scripts/verify.sh
```

It runs `scripts/qa.sh`, `scripts/smoke-local.sh`, and
`scripts/harness-audit.sh`. It runs live Infisical only when
`RAGE_LIVE_INFISICAL=1` is set.

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

This uses a temporary config/cache and lets `rage init` create a temporary age identity. It proves:

- `rage init`
- native age identity generation, recipient derivation, cache encryption, and cache decrypt
- `rage load`
- `rage exec`
- Keychain identity source remains SSH-gated unless `--allow-ssh-keychain` is passed

It must not touch real Infisical, real Keychain items, or persistent config.

## Live Infisical Smoke

Run only when live Infisical verification is needed:

```sh
scripts/smoke-infisical.sh
```

The script requires either `INFISICAL_TOKEN` or
`INFISICAL_MACHINE_IDENTITY_CLIENT_ID` plus
`INFISICAL_MACHINE_IDENTITY_CLIENT_SECRET`. If the identity can see multiple
projects, set `INFISICAL_PROJECT_SLUG`, `INFISICAL_PROJECT_ID`, or
`RAGE_INFISICAL_PROJECT_ID`. It creates a disposable root key, verifies `set ->
sync -> get -> list`, then deletes the key and confirms cleanup.

Do not wire this script into default tests unless the environment is explicitly a disposable integration environment.

## Harness Audit

Run:

```sh
scripts/harness-audit.sh
```

This checks that the AI-facing docs, scripts, project skill, and safety phrases exist and that `.env` is not tracked.

## Change Surface Matrix

See `docs/DEFINITION_OF_DONE.md` for the required evidence by change type.

Integration tests under `tests/` are part of the default QA gate. Do not move
integration-impacting coverage behind a manual flag unless it requires live
external infrastructure; use fake services for deterministic coverage.

## Test Coverage Map

- `src/main.rs` unit tests: encoding, dotenv rendering/parsing, shell quoting, JSON escaping, merge precedence, SSH script rendering.
- `src/agent_auth.rs` unit tests: timestamp handling and token redaction for provider refresh errors.
- `tests/cli.rs` integration tests: native age init/encrypt/decrypt, fake Infisical round trips, fake security Keychain gate, fake SSH argument/script handling, output formats, `unset`, remote agent auth imports, fake OAuth refresh, and fake Grok/Codex child behavior.
- `scripts/smoke-local.sh`: release binary local end-to-end behavior.
- `scripts/smoke-infisical.sh`: live Infisical behavior.

## When To Add Tests

Add or update tests whenever changing:

- CLI arguments or output.
- Infisical HTTP request/response behavior.
- agent auth import, refresh, redaction, child environment, or managed auth-file behavior.
- cache path or encryption/decryption.
- shell quoting or dotenv parsing.
- SSH script generation.
- Keychain identity behavior.
- config defaults or migration behavior.
