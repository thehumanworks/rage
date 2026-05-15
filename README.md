# rage

`rage` is a small Rust CLI for using GCP Secret Manager as a personal secrets
store while keeping day-to-day shell startup fast.

The model is:

```text
GCP Secret Manager -> rage sync -> age-encrypted local cache -> rage shell/exec/ssh
```

GCP is the source of truth. Local shells and commands load from the encrypted
cache, so they do not pay a network round trip on every shell init.

## Install

Prebuilt binaries are published on every tag push under
[GitHub Releases](https://github.com/thehumanworks/rage/releases). The supported
host matrix is macOS aarch64, Linux x86_64, Linux aarch64, and Windows x86_64.

For Unix-like systems, the bundled installer detects your platform, downloads
the matching archive, verifies its SHA-256 checksum, and drops the `rage`
binary into a system-wide location:

```sh
curl -fsSL https://raw.githubusercontent.com/thehumanworks/rage/main/install.sh | sh
```

Useful environment overrides: `VERSION=v0.1.0` pins a specific release,
`INSTALL_DIR=$HOME/.local/bin` selects a writable install dir, and
`RAGE_NO_SUDO=1` keeps the script entirely within `$HOME` when run on a
locked-down machine.

For Windows, download `rage-<version>-x86_64-pc-windows-msvc.zip` from the
Releases page and add the extracted directory to `PATH`.

## Requirements

- A GCP project with Secret Manager enabled.
- A GCP credential for `rage`: either an imported service account JSON
  encrypted by `rage`, `RAGE_GCP_SERVICE_ACCOUNT_JSON`, or
  `RAGE_GCP_ACCESS_TOKEN`.
- IAM permission to create/read Secret Manager secrets. For remote read-only
  machines, grant only `roles/secretmanager.secretAccessor`.

## Setup

Initialize the CLI. For the default file identity path, `rage` creates the age
identity if it does not exist and derives the public recipient automatically:

```sh
rage init \
  --gcp-project YOUR_GCP_PROJECT \
  --age-identity ~/.config/rage/key.txt
```

This writes `~/Library/Application Support/rage/config.toml` on macOS unless
`RAGE_CONFIG_DIR` is set. The encrypted cache defaults to
`~/Library/Caches/rage` unless `RAGE_CACHE_DIR` is set.

The generated `key.txt` contains the private age identity. Keep it local and do
not commit it. The config stores only the derived public `age_recipient`.

Configure GCP auth without installing `gcloud`:

```sh
rage auth import-service-account < gcp-service-account.json
rage auth status
```

The imported service account JSON is stored under the `rage` config directory
encrypted with your local age recipient. For ephemeral environments, you can
instead set `RAGE_GCP_SERVICE_ACCOUNT_JSON` to the raw JSON credential or
`RAGE_GCP_ACCESS_TOKEN` to a short-lived OAuth access token.

By default, `rage` reads the age identity from a file. macOS Keychain identity
loading is opt-in and still needs an explicit public recipient:

```sh
rage init \
  --gcp-project YOUR_GCP_PROJECT \
  --age-recipient "$recipient" \
  --age-identity acct \
  --age-identity-source keychain \
  --keychain-service rage-age-identity \
  --keychain-account acct
```

When the configured identity source is Keychain and `rage` detects that it is
running inside an SSH session, it refuses to read Keychain unless the command
passes `--allow-ssh-keychain` explicitly:

```sh
rage shell --allow-ssh-keychain global
```

## Daily Use

Create or update a bundle:

```sh
rage set global OPENAI_API_KEY sk-...
rage set project/foo/dev DATABASE_URL postgres://...
```

Fetch remote bundles into the encrypted local cache:

```sh
rage sync global project/foo/dev
```

Start a shell without more network fetches:

```sh
rage shell global project/foo/dev
```

Run one command:

```sh
rage exec global project/foo/dev -- cargo test
```

Export cached values into the current shell:

```sh
source <(rage load global project/foo/dev)
```

The default export output also installs a small shell wrapper so a later
`rage unset global OPENAI_API_KEY` updates the remote bundle, updates the local
cache, and removes `OPENAI_API_KEY` from the current shell. If you only want
plain export statements, pass `--no-shell-hook`.

Open the interactive terminal UI:

```sh
rage tui
```

The TUI is a thin presentation layer over the same commands documented above.
It lists remote bundles, shows the keys in the selected bundle with values
masked by default (toggle with `m`), and supports `a`dd, `e`dit, `d`elete
operations that go through the existing GCP write and cache paths. It honors
the same SSH/Keychain guard as the other commands and refuses to open when
stdout is not a terminal.

Forward selected cached secrets over SSH:

```sh
rage ssh myhost project/foo/dev -- printenv DATABASE_URL
```

The SSH command sends the remote script over stdin (`ssh host sh -s`) so secrets
are not embedded in the local `ssh` process arguments.

## Bundle Naming

Bundles are arbitrary names:

```text
global
global/openai
project/foo/dev
machine/macbook
ssh/build-box
```

Internally, bundle names are base64url encoded into GCP Secret Manager IDs under
the configured prefix, so slashes and nested namespaces are safe.

## Security Notes

- GCP stores the remote secret values. Use a dedicated project and narrow IAM.
- The local cache is encrypted with `age`.
- File identities are the default. macOS Keychain identities are explicit and
  require `--allow-ssh-keychain` when `rage` is itself running over SSH.
- `rage shell` and `rage exec` inject plaintext values into child process
  environments. That is convenient, but environment variables are still visible
  to processes with sufficient local access.
- Remote machines can use a service account JSON key for simplicity, but that
  key is a bearer credential. Prefer read-only access and rotate it.
- `rage` does not require the `gcloud`, `age`, or `age-keygen` binaries.

## Development

Default quality gate:

```sh
scripts/verify.sh
```

Narrow Rust-only gate:

```sh
scripts/qa.sh
```

Local end-to-end smoke without GCP:

```sh
scripts/smoke-local.sh
```

Disposable live GCP smoke:

```sh
scripts/smoke-gcp.sh
```

AI harness audit:

```sh
scripts/harness-audit.sh
```

Read `AGENTS.md`, `docs/ARCHITECTURE.md`, `docs/TESTING.md`,
`docs/DEFINITION_OF_DONE.md`, and `docs/AI_HARNESS.md` before making
behavioral changes.
