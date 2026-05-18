# Architecture

`rage` keeps the runtime path simple:

```text
GCP Secret Manager -> sync/write commands -> age-encrypted local cache -> shell/exec/ssh commands
```

## Components

- **Remote store**: GCP Secret Manager. A rage bundle is stored as one Secret Manager secret whose latest version contains a dotenv payload for that bundle.
- **GCP auth**: `GCP_ACCESS_TOKEN`, `GOOGLE_OAUTH_ACCESS_TOKEN`, or `CLOUDSDK_AUTH_ACCESS_TOKEN` is used directly as a bearer token. `rage` talks to Secret Manager over HTTPS and does not shell out to `gcloud`.
- **Project selection**: `gcp_project` is stored in config. `rage init` accepts `--gcp-project` or reads `RAGE_GCP_PROJECT`, `GOOGLE_CLOUD_PROJECT`, `GOOGLE_PROJECT_ID`, or `GCLOUD_PROJECT`.
- **Bundle naming**: User-facing bundle names are encoded into stable Secret Manager IDs using the same base64url shape as local cache filenames, for example `rage-cHJvamVjdC9mb28vZGV2`.
- **Local cache**: One age-encrypted `.env.age` file per bundle.
- **Identity source**: File identities by default; `rage init` can generate the file identity and derive its public recipient natively. macOS Keychain is explicit opt-in.
- **Command runner**: `exec` and `shell` inject decrypted variables into child process environments.
- **SSH runner**: `ssh` sends a remote shell script over stdin so secrets are not embedded in local process arguments.
- **Agent auth runner**: `rage import grok`, `rage import codex`, `rage grok`, and `rage codex` live in `src/agent_auth.rs`. Imported agent auth records are stored in the `agents` bundle as `AUTHLESS_<PROVIDER>_JSON`, refreshed through provider OAuth endpoints when stale, and written only to the matching provider auth cache or explicit child environment/managed auth file.
- **TUI**: `rage tui` (module `src/tui.rs`) is a ratatui presentation layer over the same remote/cache helpers. It validates the age identity before opening the alternate screen, refuses to start when stdout is not a TTY, masks values by default, and shows imported agent auth records as managed placeholders.

## Safety Invariants

- Do not make network fetches part of normal shell startup unless the user passes `--sync`.
- Do not persist plaintext dotenv payloads or agent auth anywhere except explicit provider auth caches managed by `rage grok` and `rage codex`.
- Do not leak secret values into process arguments.
- Do not print raw agent access tokens, refresh tokens, or full auth JSON in status or error output.
- Keep Keychain disabled by default for SSH-originated sessions.
- Keep live GCP tests disposable and separate from `cargo test`.

## External Tool Boundaries

The implementation talks to:

- GCP Secret Manager over HTTPS.
- `security` only when `age_identity_source = "keychain"`.
- `ssh` only for `rage ssh`.
- Grok/Codex OAuth token endpoints only when `rage grok` or `rage codex` must refresh a stale imported auth record.
- `grok` and `codex` only as explicit child commands of their matching subcommands.

Age key generation, recipient derivation, cache encryption, and cache decryption use the Rust `age` crate directly. Do not reintroduce required external age binary dependencies.

The integration suite uses a fake GCP Secret Manager HTTP server plus fake `security` and `ssh` binaries to keep default tests deterministic. If you change request shapes, command arguments, or output contracts, update `tests/cli.rs` and run `scripts/qa.sh`.

## Deliberate Non-Goals

- No daemon.
- No plaintext cache.
- No required `gcloud`, `age`, or `age-keygen` CLI dependency.
- No broad IAM helper that grants write/admin roles to remote machines.
- No implicit Keychain unlock or SSH Keychain behavior.
