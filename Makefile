.PHONY: dev-up dev-down test lint fmt typecheck openapi-export staging-validate staging-plan

dev-up:
	bash scripts/dev-up

dev-down:
	bash scripts/dev-down

test:
	cargo test --workspace
	pnpm typecheck

lint:
	cargo clippy --workspace --all-targets -- -D warnings
	pnpm lint

fmt:
	cargo fmt --all
	pnpm format

typecheck:
	pnpm typecheck

openapi-export:
	bash scripts/export-openapi.sh

# Staging IaC — validate only (no AWS account required).
staging-validate:
	terraform -chdir=infrastructure/terraform/environments/staging fmt -check -recursive ../..
	terraform -chdir=infrastructure/terraform/environments/staging init -backend=false -input=false
	terraform -chdir=infrastructure/terraform/environments/staging validate
	@command -v checkov >/dev/null && checkov -d infrastructure/terraform --framework terraform --quiet \
		|| echo "checkov not installed; CI runs it"

# Staging plan — requires AWS creds + TF_VAR_* secrets. Never runs apply.
# See docs/ops/staging.md.
staging-plan: staging-validate
	@if [ -z "$${AWS_ACCESS_KEY_ID}$${AWS_PROFILE}$${AWS_ROLE_ARN}" ]; then \
		echo "No AWS credentials detected (AWS_ACCESS_KEY_ID / AWS_PROFILE / AWS_ROLE_ARN)."; \
		echo "Skipping terraform plan. Configure an account, then re-run."; \
		exit 0; \
	fi
	@test -n "$${TF_VAR_rds_master_password}" || { echo "Set TF_VAR_rds_master_password"; exit 1; }
	@test -n "$${TF_VAR_redis_auth_token}" || { echo "Set TF_VAR_redis_auth_token"; exit 1; }
	terraform -chdir=infrastructure/terraform/environments/staging plan \
		-var-file=terraform.tfvars.example -input=false
