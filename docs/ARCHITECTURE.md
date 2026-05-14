# Architecture

`rage` keeps the runtime path simple:

```text
GCP Secret Manager -> sync/write commands -> age-encrypted local cache -> shell/exec/ssh commands
```

## Components

- **Remote store**: GCP Secret Manager. Bundles are stored as dotenv payloads in versioned secrets.
- **Bundle naming**: User-facing bundle names are base64url encoded into GCP secret IDs under a configured prefix.
- **Local cache**: One age-encrypted `.env.age` file per bundle.
- **Identity source**: File identities by default; `rage init` can generate the file identity and derive its public recipient natively. macOS Keychain is explicit opt-in.
- **Command runner**: `exec` and `shell` inject decrypted variables into child process environments.
- **SSH runner**: `ssh` sends a remote shell script over stdin so secrets are not embedded in local process arguments.

## Safety Invariants

- Do not make network fetches part of normal shell startup unless the user passes `--sync`.
- Do not persist plaintext dotenv payloads.
- Do not leak secret values into process arguments.
- Keep Keychain disabled by default for SSH-originated sessions.
- Keep live GCP tests disposable and separate from `cargo test`.

## External Tool Boundaries

The implementation talks to:

- GCP Secret Manager over HTTPS.
- Google OAuth token exchange directly for service account JSON credentials.
- `security` only when `age_identity_source = "keychain"`.
- `ssh` only for `rage ssh`.

Age key generation, recipient derivation, cache encryption, and cache decryption use the Rust `age` crate directly. Do not reintroduce required external age binary dependencies.

The integration suite uses a fake Secret Manager HTTP server plus fake `security` and `ssh` binaries to keep default tests deterministic. If you change request shapes, command arguments, or output contracts, update `tests/cli.rs` and run `scripts/qa.sh`.

## Deliberate Non-Goals

- No daemon.
- No plaintext cache.
- No required `gcloud` CLI dependency.
- No broad IAM helper that grants write/admin roles to remote machines.
- No implicit Keychain unlock or SSH Keychain behavior.
