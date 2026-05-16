# rage

`rage` is a small Rust CLI for using Infisical as a personal secrets store
while keeping day-to-day shell startup fast.

The model is:

```text
Infisical -> rage sync -> age-encrypted local cache -> rage shell/exec/ssh
```

Infisical is the source of truth. Local shells and commands load from the
encrypted cache, so they do not pay a network round trip on every shell init.

## Install

Prebuilt binaries are published on every tag push under
[GitHub Releases](https://github.com/thehumanworks/rage/releases). The supported
host matrix is macOS aarch64, Linux x86_64, Linux aarch64, and Windows x86_64.

While `thehumanworks/rage` is a private repository, the installer needs a
GitHub token with `contents:read` scope on the repo (a fine-grained PAT or any
PAT with `repo` scope works). The script also falls back to `gh auth token` if
the `gh` CLI is already signed in.

For Unix-like systems, the bundled installer detects your platform, downloads
the matching archive via the authenticated GitHub Releases API, verifies its
SHA-256 checksum, and drops the `rage` binary into a system-wide location:

```sh
export GITHUB_TOKEN=ghp_xxx   # or: gh auth login --scopes 'repo'
curl -fsSL -H "Authorization: Bearer $GITHUB_TOKEN" \
  https://raw.githubusercontent.com/thehumanworks/rage/main/install.sh | sh
```

Once the repo is made public, the token is no longer required and the
one-liner becomes:

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

- An Infisical project and a machine identity or token with access to the
  target environment.
- Either `INFISICAL_TOKEN`, or both
  `INFISICAL_MACHINE_IDENTITY_CLIENT_ID` and
  `INFISICAL_MACHINE_IDENTITY_CLIENT_SECRET`, set when using remote commands.

## Setup

Initialize the CLI. For the default file identity path, `rage` creates the age
identity if it does not exist and derives the public recipient automatically:

```sh
rage init \
  --age-identity ~/.config/rage/key.txt
```

If `INFISICAL_TOKEN` is a legacy service token, `rage init` reads token metadata
and stores the project ID automatically. For machine identity access tokens or
client ID/secret auth, `rage init` can infer the project when the identity can
see exactly one project. If it can see multiple projects, set
`INFISICAL_PROJECT_SLUG`, set `INFISICAL_PROJECT_ID`, or pass
`--infisical-project-id`. You can also pass `--infisical-environment` or
`--infisical-endpoint` explicitly.

This writes `~/Library/Application Support/rage/config.toml` on macOS unless
`RAGE_CONFIG_DIR` is set. The encrypted cache defaults to
`~/Library/Caches/rage` unless `RAGE_CACHE_DIR` is set.

The generated `key.txt` contains the private age identity. Keep it local and do
not commit it. The config stores only the derived public `age_recipient`.

Check Infisical auth visibility with a direct token:

```sh
INFISICAL_TOKEN=st.x rage auth status
```

Or use Universal Auth credentials for a machine identity:

```sh
export INFISICAL_MACHINE_IDENTITY_CLIENT_ID=...
export INFISICAL_MACHINE_IDENTITY_CLIENT_SECRET=...
export INFISICAL_PROJECT_ID=...
rage auth status
```

`INFISICAL_TOKEN` takes precedence when it is set. Without `INFISICAL_TOKEN`,
`rage` exchanges the machine identity client ID and secret for a short-lived
Bearer token through Infisical's Universal Auth endpoint.

For non-default Infisical domains, set `RAGE_INFISICAL_ENDPOINT` or
`INFISICAL_API_URL`. `RAGE_INFISICAL_PROJECT_ID`/`INFISICAL_PROJECT_ID` and
`RAGE_INFISICAL_ENVIRONMENT`/`INFISICAL_ENVIRONMENT` can override the stored
project or environment at runtime.

By default, `rage` reads the age identity from a file. macOS Keychain identity
loading is opt-in and still needs an explicit public recipient:

```sh
rage init \
  --infisical-project-id YOUR_INFISICAL_PROJECT_ID \
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
operations that go through the existing Infisical write and cache paths. It honors
the same SSH/Keychain guard as the other commands and refuses to open when
stdout is not a terminal.

## Agent Auth

`rage` can import existing Grok and Codex auth files into Infisical, refresh
them when needed, and launch the matching CLI without keeping the original auth
cache on disk. These records are stored as root-level `AUTHLESS_<TOOL>_JSON`
secrets and are reserved from normal `rage load/shell/exec` bundle output.

Bootstrap Grok from a logged-in machine:

```sh
rage import grok ~/.grok/auth.json
rage grok -- -p "hello"
```

`rage grok` refreshes the stored record when it is expired or near expiry, then
sets only `GROK_CODE_XAI_API_KEY` for the child process.

Bootstrap Codex from a ChatGPT login:

```sh
rage import codex ~/.codex/auth.json
rage codex
```

`rage codex` writes a temporary Codex-compatible `auth.json` under
`${CODEX_HOME:-$HOME/.codex}` for the child process. If the file did not exist
before launch, it is removed on exit. Pass `--force` to temporarily overwrite an
existing `auth.json` and restore it after the child exits.

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

Internally, bundle names map directly to Infisical secret paths. `global` uses
the Infisical root path `/`; `project/foo/dev` stores environment keys under
`/project/foo/dev`. Nested bundles require a token that can create or use those
folders.

## Security Notes

- Infisical stores the remote secret values. Use a dedicated project/token and
  narrow token scopes where possible.
- The local cache is encrypted with `age`.
- File identities are the default. macOS Keychain identities are explicit and
  require `--allow-ssh-keychain` when `rage` is itself running over SSH.
- `rage shell` and `rage exec` inject plaintext values into child process
  environments. That is convenient, but environment variables are still visible
  to processes with sufficient local access.
- `INFISICAL_TOKEN` is a bearer credential. Prefer read-only access on remote
  machines unless writing is required, and rotate it.
- `rage` does not require the `infisical`, `gcloud`, `age`, or `age-keygen`
  binaries.

## Development

Default quality gate:

```sh
scripts/verify.sh
```

Narrow Rust-only gate:

```sh
scripts/qa.sh
```

Local end-to-end smoke without Infisical:

```sh
scripts/smoke-local.sh
```

Disposable live Infisical smoke:

```sh
scripts/smoke-infisical.sh
```

AI harness audit:

```sh
scripts/harness-audit.sh
```

Read `AGENTS.md`, `docs/ARCHITECTURE.md`, `docs/TESTING.md`,
`docs/DEFINITION_OF_DONE.md`, and `docs/AI_HARNESS.md` before making
behavioral changes.
