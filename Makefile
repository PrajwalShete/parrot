# Parrot: the web UI is built first, then embedded into the Rust binary.

.PHONY: build web dev test check release clean

build: web
	cargo build --release

web:
	cd web && npm ci --no-audit --no-fund && npm run build

# Run the server on 4799 (`cargo run -- --port 4799 --no-open`) and the UI with hot reload here.
dev:
	cd web && npm run dev

test:
	cargo test
	cd web && npm test

check:
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings
	cd web && npm run typecheck

clean:
	cargo clean
	rm -rf web/dist web/node_modules
