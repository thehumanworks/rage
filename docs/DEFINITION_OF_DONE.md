# Definition Of Done

This repo is ready for handoff only when the implementation, tests, docs, and
verification evidence match the changed surface.

## Default Rule

Before a final answer for any code, script, or harness change, run:

```sh
scripts/verify.sh
```

This is the deterministic local completion gate. It runs formatting, Clippy,
unit tests, integration tests under `tests/`, a release build, CLI help smoke
tests, local runtime smoke, and the agent harness audit. It skips live
Infisical by default and prints that skip.

## Fast Inner Loops

Use narrower checks while iterating, then finish with the default gate:

- Parser or CLI output: `cargo test --locked <test-name>` and `./target/debug/rage <command> --help`
- Pure Rust logic: `cargo test --locked <module-or-test-name>`
- TUI rendering: `cargo test --locked tui::tests`
- Shell, SSH, cache, or Keychain behavior: `cargo test --locked --test cli`
- Formatting or lint cleanup: `cargo fmt --check` and `cargo clippy --locked --all-targets -- -D warnings`

## Change Surface Matrix

| Changed surface | Required evidence |
| --- | --- |
| CLI args, output, or command behavior | Integration test in `tests/cli.rs`, command help smoke via `scripts/qa.sh`, README update if user-facing |
| Cache, age identity, encryption, shell export, SSH, or Keychain behavior | Focused regression test plus `scripts/smoke-local.sh` |
| Infisical HTTP request/response behavior | Fake Infisical coverage in `tests/cli.rs`; `scripts/qa.sh` runs integration tests by default; run `scripts/smoke-infisical.sh` only with disposable live credentials |
| Agent auth import, refresh, redaction, or child launch behavior | Fake OAuth and fake child CLI coverage in `tests/cli.rs`; avoid live Grok/Codex auth in default tests |
| TUI state, key handling, or rendering | Pure `src/tui.rs` tests using `TestBackend`; CLI test for non-TTY or identity-gate behavior |
| Docs, AGENTS, skills, scripts, CI, or process | `scripts/harness-audit.sh` and `scripts/verify.sh` |
| Dependencies or toolchain | `cargo check --locked`, `scripts/qa.sh`, and CI workflow review |

## Live Infisical Claims

Do not claim live Infisical behavior was verified unless one of these ran
successfully against a disposable project:

```sh
RAGE_LIVE_INFISICAL=1 scripts/verify.sh
scripts/smoke-infisical.sh
```

If live Infisical was not configured, say it was skipped and keep the claim
limited to fake Infisical and local smoke coverage.

## Final Answer Evidence

Every completion report should include:

- the user-visible behavior changed,
- the main files changed,
- the exact checks run,
- whether live Infisical was run or skipped,
- any residual risk.
