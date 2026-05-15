# rage Completion Checklist

Use this checklist before finalizing changes.

## Safety

- No plaintext secrets committed or printed.
- `.env` remains ignored and untracked.
- Local cache remains age-encrypted.
- Age key generation/encryption/decryption uses the Rust crate, not required external `age` binaries.
- Shell/exec secrets are only injected into child process environments.
- SSH forwarding does not place secret values in local process arguments.
- Keychain identity source is not the default.
- Keychain over SSH requires `--allow-ssh-keychain`.

## Tests

- `scripts/verify.sh` passed before finalizing code, script, or harness work.
- `scripts/qa.sh` passed for code changes.
- Integration-impacting changes are covered under `tests/` and run by the default QA gate.
- `scripts/smoke-local.sh` passed for local runtime changes.
- `scripts/smoke-gcp.sh` passed for live GCP claims, or the final answer says it was skipped.
- `scripts/harness-audit.sh` passed for harness/docs/script changes.

## Final Answer Evidence

- Mention changed files at a high level.
- Mention exact checks run.
- Mention live GCP status.
- Mention any residual risk or untested external environment.
