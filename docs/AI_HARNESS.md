# AI Harness

This repository is prepared for agentic maintenance by making the safe path explicit and executable.

## Future-Agent Entry Point

1. Read `AGENTS.md`.
2. Read `README.md`.
3. Read `docs/ARCHITECTURE.md`, `docs/TESTING.md`, and `docs/DEFINITION_OF_DONE.md`.
4. For workflow reminders, read `.codex/skills/rage-secrets/SKILL.md`.
5. Read any feature contract that applies, such as `contract.md` for TUI work.
6. Make scoped changes.
7. Run the verification script that matches the change, finishing with `scripts/verify.sh` for code or harness work.

## Common Agent Failure Modes This Harness Prevents

- Treating GCP as required for every test.
- Accidentally logging or committing plaintext secrets.
- Making Keychain access automatic over SSH.
- Putting secrets into SSH command-line arguments.
- Changing command output without updating black-box tests.
- Claiming live GCP behavior is verified from fake tests only.
- Forgetting to run Clippy or release build.
- Stopping after narrow tests without the local definition-of-done gate.

## Preferred Workflows

For code changes:

```sh
scripts/verify.sh
```

For GCP Secret Manager transport changes:

```sh
scripts/verify.sh
RAGE_LIVE_GCP=1 scripts/verify.sh
```

For docs/harness changes:

```sh
scripts/verify.sh
```

## Evidence Standard

Final answers should state:

- files changed,
- scripts/checks run,
- whether live GCP was run or skipped,
- any remaining risk.

Do not claim full live verification unless `scripts/smoke-gcp.sh` or an equivalent disposable live round trip ran successfully.
