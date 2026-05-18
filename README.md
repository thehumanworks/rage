# rage

`rage` is a small Rust CLI for using GCP Secret Manager as a personal secrets store
while keeping day-to-day shell startup fast.

The model is:

```text
GCP Secret Manager -> rage sync -> age-encrypted local cache -> rage shell/exec/ssh
```

GCP Secret Manager is the source of truth. Local shells and commands load from the
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

### npm

`rage` is also packaged as a single npm package that downloads the matching
GitHub Release binary during install:

```sh
npm install -g @nothumanwork/rage
```

The npm installer supports macOS aarch64, Linux x86_64, Linux aarch64, and
Windows x86_64. For the private GitHub repository, set `GITHUB_TOKEN` or
`RAGE_GITHUB_TOKEN` to a token with `contents:read`; if neither is set, the
installer falls back to `gh auth token` when the GitHub CLI is signed in.

Useful overrides: `RAGE_VERSION=v0.1.1` pins a release tag,
`RAGE_REPO=owner/repo` selects another GitHub repository, and
`RAGE_NPM_SKIP_DOWNLOAD=1` skips the postinstall download for packaging tests.

## Requirements

- A GCP project with Secret Manager enabled.
- `GCP_ACCESS_TOKEN`, `GOOGLE_OAUTH_ACCESS_TOKEN`, or
  `CLOUDSDK_AUTH_ACCESS_TOKEN` set when using remote commands.

## Setup

Initialize the CLI. For the default file identity path, `rage` creates the age
identity if it does not exist and derives the public recipient automatically:

```sh
rage init \
  --gcp-project YOUR_GOOGLE_CLOUD_PROJECT \
  --age-identity ~/.config/rage/key.txt
```

If `--gcp-project` is omitted, `rage init` reads `RAGE_GCP_PROJECT`,
`GOOGLE_CLOUD_PROJECT`, `GOOGLE_PROJECT_ID`, or `GCLOUD_PROJECT`. You can pass
`--gcp-endpoint` or set `RAGE_GCP_ENDPOINT` for tests or non-default API
proxies.

This writes `~/Library/Application Support/rage/config.toml` on macOS unless
`RAGE_CONFIG_DIR` is set. The encrypted cache defaults to
`~/Library/Caches/rage` unless `RAGE_CACHE_DIR` is set.

Configs from the Infisical-backed version are migrated on first load when a
usable project value is available. New configs store `gcp_project`.

The generated `key.txt` contains the private age identity. Keep it local and do
not commit it. The config stores only the derived public `age_recipient`.

Check GCP Secret Manager auth visibility with a direct token:

```sh
GCP_ACCESS_TOKEN=ya29.x rage auth status
```

`rage` uses the access token directly and does not invoke `gcloud`.
`RAGE_GCP_PROJECT`/`GOOGLE_CLOUD_PROJECT` can override the stored project at
runtime.

By default, `rage` reads the age identity from a file. macOS Keychain identity
loading is opt-in and still needs an explicit public recipient:

```sh
rage init \
  --gcp-project YOUR_GOOGLE_CLOUD_PROJECT \
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
operations that go through the existing GCP Secret Manager write and cache paths. It honors
the same SSH/Keychain guard as the other commands and refuses to open when
stdout is not a terminal.
In the `agents` bundle, imported `AUTHLESS_*_JSON` auth records are shown as
managed placeholders so imports are visible without rendering raw auth JSON.

## Agent Auth

`rage` can import existing Grok and Codex auth files into GCP Secret Manager, refresh
them when needed, and launch the matching CLI with a refreshed provider auth
cache. These records are stored in the `agents` bundle as
`AUTHLESS_<TOOL>_JSON` secrets and are reserved from normal
`rage load/shell/exec` output.

Bootstrap Grok from a logged-in machine:

```sh
rage import grok ~/.grok/auth.json
rage grok -- -p "hello"
```

`rage grok` refreshes the stored record when it is expired or near expiry, then
writes a Grok-compatible auth cache to `~/.grok/auth.json` before launching the
child process. Pass `-e`/`--ephemeral` to avoid writing the auth cache and use
the child-process `GROK_CODE_XAI_API_KEY` environment variable instead.

Bootstrap Codex from a ChatGPT login:

```sh
rage import codex ~/.codex/auth.json
rage codex
```

`rage codex` refreshes the stored record when needed, then writes a
Codex-compatible `auth.json` under `${CODEX_HOME:-$HOME/.codex}` before
launching the child process. Pass `-e`/`--ephemeral` to remove the managed
`auth.json` on exit, restoring any file that existed before launch. `--force`
is retained as a compatibility alias for the same temporary Codex behavior.

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

Internally, each bundle maps to one GCP Secret Manager secret. The secret ID is
the configured prefix plus a base64url-encoded bundle name, and the latest
secret version stores that bundle as dotenv text.

## Security Notes

- GCP Secret Manager stores the remote secret values. Use a dedicated project
  and narrow IAM roles where possible.
- The local cache is encrypted with `age`.
- File identities are the default. macOS Keychain identities are explicit and
  require `--allow-ssh-keychain` when `rage` is itself running over SSH.
- `rage shell` and `rage exec` inject plaintext values into child process
  environments. That is convenient, but environment variables are still visible
  to processes with sufficient local access.
- `GCP_ACCESS_TOKEN` is a bearer credential. Prefer read-only access on remote
  machines unless writing is required, and rotate it.
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

Local end-to-end smoke without GCP Secret Manager:

```sh
scripts/smoke-local.sh
```

Disposable live GCP Secret Manager smoke:

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
