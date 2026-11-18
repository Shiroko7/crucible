# `make` builds the web app and starts the server: one command, then open
# http://127.0.0.1:8000. Everything else is for working on it.

PORT ?= 8000

.PHONY: run web server dev-web check test e2e

# Build the front end, then serve it and the API.
run: web server

# The Svelte app, built into web/build for the server to hand out.
web: web/node_modules
	cd web && npm run build

web/node_modules: web/package.json
	cd web && npm install

server:
	cargo run --release -p crucible-server -- --port $(PORT)

# The front end with hot reload, on its own port, talking to a server that
# is already running (`make server` in another terminal).
dev-web: web/node_modules
	cd web && npm run dev

# What has to pass before a push.
check: web/node_modules
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets -- -D warnings
	cargo test --workspace
	cd web && npm run check

test:
	cargo test --workspace

# The web app in a real browser, driven from Rust: needs Chrome or Edge.
e2e: web
	cargo test -p crucible-server --test web -- --ignored
