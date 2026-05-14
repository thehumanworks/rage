# AI Harness

This repository is prepared for agentic maintenance by making the safe path explicit and executable.

## Future-Agent Entry Point

1. Read `AGENTS.md`.
2. Read `README.md`.
3. Read `docs/ARCHITECTURE.md` and `docs/TESTING.md`.
4. For workflow reminders, read `.codex/skills/rage-secrets/SKILL.md`.
5. Make scoped changes.
6. Run the verification script that matches the change.

## Common Agent Failure Modes This Harness Prevents

- Treating GCP as required for every test.
- Accidentally logging or committing plaintext secrets.
- Making Keychain access automatic over SSH.
- Putting secrets into SSH command-line arguments.
- Changing command output without updating black-box tests.
- Claiming GCP behavior is verified from fake tests only.
- Forgetting to run Clippy or release build.

## Preferred Workflows

For code changes:

```sh
scripts/qa.sh
scripts/smoke-local.sh
```

For GCP transport changes:

```sh
scripts/qa.sh
scripts/smoke-local.sh
scripts/smoke-gcp.sh
```

For docs/harness changes:

```sh
scripts/harness-audit.sh
scripts/qa.sh
```

## Evidence Standard

Final answers should state:

- files changed,
- scripts/checks run,
- whether live GCP was run or skipped,
- any remaining risk.

Do not claim full live verification unless `scripts/smoke-gcp.sh` or an equivalent disposable live round trip ran successfully.
