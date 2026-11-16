.PHONY: test test-unit test-integration coverage coverage-html e2e up down lint

test: lint test-unit test-integration

lint:
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings

test-unit:
	cargo test --lib

test-integration:
	cargo test --test exporter

coverage:
	cargo llvm-cov --summary-only --fail-under-lines 90

coverage-html:
	cargo llvm-cov --html --open

# Starts the compose stack, runs the e2e tests, then always tears it down.
e2e:
	cargo test --features e2e --test e2e; status=$$?; docker compose down -v; exit $$status

up:
	docker compose up -d --build --wait

down:
	docker compose down -v
