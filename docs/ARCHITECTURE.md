# Architecture

`rage` keeps the runtime path simple:

```text
Infisical -> sync/write commands -> age-encrypted local cache -> shell/exec/ssh commands
```

## Components

- **Remote store**: Infisical. A rage bundle maps to an Infisical secret path, and each environment key is stored as one Infisical secret under that path.
- **Infisical auth**: `INFISICAL_TOKEN` is used directly when set. Otherwise `rage` exchanges `INFISICAL_MACHINE_IDENTITY_CLIENT_ID` and `INFISICAL_MACHINE_IDENTITY_CLIENT_SECRET` for a short-lived Universal Auth bearer token. `rage init` can infer the project ID from legacy service-token metadata, from `INFISICAL_PROJECT_SLUG`, or from the Projects API when exactly one project is visible. `INFISICAL_PROJECT_ID` or `--infisical-project-id` is required when the project cannot be inferred. `INFISICAL_API_URL` can select a non-default Infisical domain.
- **Bundle naming**: User-facing bundle names map directly to Infisical paths. `global` maps to `/`; nested names such as `project/foo/dev` map to `/project/foo/dev`. Local cache filenames still use a stable base64url ID.
- **Local cache**: One age-encrypted `.env.age` file per bundle.
- **Identity source**: File identities by default; `rage init` can generate the file identity and derive its public recipient natively. macOS Keychain is explicit opt-in.
- **Command runner**: `exec` and `shell` inject decrypted variables into child process environments.
- **SSH runner**: `ssh` sends a remote shell script over stdin so secrets are not embedded in local process arguments.
- **Agent auth runner**: `rage import grok`, `rage import codex`, `rage grok`, and `rage codex` live in `src/agent_auth.rs`. Imported agent auth records are stored in the Infisical `/agents` path as `AUTHLESS_<PROVIDER>_JSON`, refreshed through the provider OAuth endpoints when stale, and injected only into the intended child environment or managed auth file. Bundle operations reserve those keys and never sync them into shell caches, while bundle listing still shows `agents` so the TUI has a visible home for imported agent auth.
- **TUI**: `rage tui` (module `src/tui.rs`) is a ratatui presentation layer that reuses the Infisical and cache helpers above. It validates the age identity before opening the alternate screen so the SSH-Keychain guard fires before any terminal state is touched, refuses to start when stdout is not a TTY, and never renders raw values in the masked detail view.

## Safety Invariants

- Do not make network fetches part of normal shell startup unless the user passes `--sync`.
- Do not persist plaintext dotenv payloads.
- Do not leak secret values into process arguments.
- Do not print raw agent access tokens, refresh tokens, or full auth JSON in status or error output.
- Keep Keychain disabled by default for SSH-originated sessions.
- Keep live Infisical tests disposable and separate from `cargo test`.

## External Tool Boundaries

The implementation talks to:

- Infisical over HTTPS.
- `security` only when `age_identity_source = "keychain"`.
- `ssh` only for `rage ssh`.
- Grok/Codex OAuth token endpoints only when `rage grok` or `rage codex` must refresh a stale imported auth record.
- `grok` and `codex` only as explicit child commands of their matching subcommands.

Age key generation, recipient derivation, cache encryption, and cache decryption use the Rust `age` crate directly. Do not reintroduce required external age binary dependencies.

The integration suite uses a fake Infisical HTTP server plus fake `security` and `ssh` binaries to keep default tests deterministic. If you change request shapes, command arguments, or output contracts, update `tests/cli.rs` and run `scripts/qa.sh`.

## Deliberate Non-Goals

- No daemon.
- No plaintext cache.
- No required `infisical` or `gcloud` CLI dependency.
- No broad IAM helper that grants write/admin roles to remote machines.
- No implicit Keychain unlock or SSH Keychain behavior.
