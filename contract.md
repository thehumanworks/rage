# TUI Verification Contract

Goal: add a `rage tui` subcommand that opens an interactive terminal UI for browsing and editing the bundles already managed by `rage`, without weakening any of the project invariants in `AGENTS.md`.

The TUI must be a thin presentation layer over the existing helpers in `src/main.rs` (`gcp_list_bundles`, `gcp_read_bundle`, `gcp_write_bundle`, `gcp_delete_bundle`, `sync_bundle`, `read_cache`, `write_cache`, `validate_env_key`). No new GCP transport. No plaintext on disk. No daemon.

A change set is "done" when every criterion below is satisfied.

## Criteria

1. **Subcommand exists.** `src/main.rs` declares a `Tui(TuiArgs)` variant on `Commands` and dispatches it to `tui::run`. `rage tui --help` exits 0 and lists `--allow-ssh-keychain`.

2. **Dependencies present.** `Cargo.toml` `[dependencies]` lists `ratatui` and `crossterm` at compatible versions (latest 0.x). `cargo build` exits 0.

3. **Module shape.** `src/tui.rs` exists and exports:
   - `pub struct AppState` with fields covering bundle list, selection, current bundle env, mask flag, and status line.
   - `pub fn draw(state: &AppState, frame: &mut ratatui::Frame)` (pure: takes `&AppState`, mutates only the frame).
   - `pub fn handle_key(state: &mut AppState, key: crossterm::event::KeyEvent) -> tui::Action` (pure: no I/O).
   - `pub fn run(cfg: &Config, allow_ssh_keychain: bool) -> anyhow::Result<()>` for the event loop.

4. **Mask is the default and is honoured by the renderer.** A `#[test] fn detail_view_masks_values_by_default` in `src/tui.rs` uses `ratatui::backend::TestBackend`, prepares an `AppState` with a key whose value is `"sk-abcdef"`, draws into a 60x10 buffer, and asserts the buffer contains `"••••••"` (or similar non-empty mask glyphs) and does NOT contain `"sk-abcdef"`.

5. **Mask toggle reveals.** A `#[test] fn mask_toggle_reveals_values` flips the mask flag via `handle_key` on the `'m'` key, redraws, and asserts the buffer now contains `"sk-abcdef"`.

6. **Bundle list renders.** A `#[test] fn bundle_list_renders_names_and_selection` renders an `AppState` seeded with two bundles (`global`, `project/foo/dev`), asserts both names appear, and that the selected row is marked.

7. **SSH+Keychain guard still applies.** A new test in `tests/cli.rs` named `tui_refuses_keychain_identity_over_ssh` configures a keychain identity and `SSH_CONNECTION`, runs `rage tui` (in a mode that returns before opening a TTY — see criterion 8), and asserts the stderr contains `"refusing to read macOS Keychain identity from an SSH session"`.

8. **Non-TTY safety.** When stdout is not a TTY, `rage tui` exits non-zero with an error mentioning `"requires a terminal"` rather than panicking or corrupting the terminal. This is exercised by the test in criterion 7 (assert_cmd pipes stdout, so it is not a TTY).

9. **QA passes.** `scripts/qa.sh` exits 0 (formatting, clippy with `-D warnings`, `cargo test`, debug build). `scripts/harness-audit.sh` exits 0.

## Out of scope (for this iteration)

- Live GCP smoke test for the TUI (`scripts/smoke-gcp.sh`).
- Mouse support, theming, search/filter, paging through GCP list responses inside the UI.
- A separate background thread for long-running fetches; v1 is allowed to block the UI thread during a sync with a "Syncing…" status line.

# Release Distribution Verification Contract

Goal: ship signed-by-tag prebuilt binaries of `rage` for the four supported host
combinations and a smart installer script users can curl-pipe. No new runtime
behavior, no changes to the GCP/age invariants in `AGENTS.md`.

## Criteria

R1. **Release workflow exists.** `.github/workflows/release.yml` is present and
    declares `on:` triggers for `push` of tags matching `v*` and a manual
    `workflow_dispatch` with a `tag` input. `actionlint` (or `python -c "import
    yaml; yaml.safe_load(open('.github/workflows/release.yml'))"`) exits 0.

R2. **Matrix covers four targets.** The build job's strategy matrix includes
    exactly these `target` values:
    - `aarch64-apple-darwin`
    - `x86_64-unknown-linux-gnu`
    - `aarch64-unknown-linux-gnu`
    - `x86_64-pc-windows-msvc`
    Each entry is pinned to a concrete `runs-on` value (no `ubuntu-latest` for
    aarch64 Linux unless `cross` is invoked).

R3. **Artifacts are named deterministically.** The workflow uploads one archive
    per target named `rage-<version>-<target>.tar.gz` (or `.zip` for the
    Windows target), where `<version>` is the tag minus the `v` prefix. A
    `SHA256SUMS` file with one line per archive is uploaded alongside.

R4. **Artifacts attach to a GitHub Release.** The workflow uses
    `softprops/action-gh-release@v2` (or `gh release create/upload`) so that on
    a tag push, all four archives plus `SHA256SUMS` end up attached to the
    release named after the tag. The release is created if missing and updated
    if present.

R5. **install.sh exists at repo root and is executable.** `ls -l install.sh`
    shows mode `0755`. `sh -n install.sh` exits 0. The script:
    - detects OS via `uname -s` (`Darwin`, `Linux`) and arch via `uname -m`
      (`arm64`/`aarch64`, `x86_64`),
    - maps each supported pair to one of the three Unix targets in R2,
    - errors with a clear message on Windows or any unmapped pair,
    - resolves the latest release tag from
      `api.github.com/repos/<owner>/<repo>/releases/latest` unless `VERSION` is
      set,
    - downloads the archive + `SHA256SUMS` with `curl -fsSL` (falling back to
      `wget` if `curl` is missing),
    - verifies the archive against `SHA256SUMS` using `shasum -a 256` or
      `sha256sum`,
    - installs the extracted `rage` binary into `/usr/local/bin` when writable
      or via `sudo`, otherwise into `$HOME/.local/bin` (creating it if needed),
    - prints the installed path and version on success.

R6. **install.sh is documented in README.** README.md gains a short "Install"
    section that shows a one-liner of the form
    `curl -fsSL https://raw.githubusercontent.com/<owner>/<repo>/main/install.sh | sh`
    and points to the Releases page for manual downloads.

R7. **Existing CI is untouched in spirit.** `.github/workflows/ci.yml` still
    runs `scripts/verify.sh` on push/PR. `scripts/qa.sh` exits 0 on the host
    after changes (no Rust source files modified by this task except possibly
    a `Cargo.toml` `[profile.release]` tuning section). `scripts/harness-audit.sh`
    exits 0.
