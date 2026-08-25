.PHONY: dev-up dev-down test lint fmt typecheck openapi-export

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
