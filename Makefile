.PHONY: qa smoke-local smoke-gcp harness-audit verify

qa:
	scripts/qa.sh

smoke-local:
	scripts/smoke-local.sh

smoke-gcp:
	scripts/smoke-gcp.sh

harness-audit:
	scripts/harness-audit.sh

verify:
	scripts/verify.sh
