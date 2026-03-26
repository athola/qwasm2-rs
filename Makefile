.PHONY: help build wasm bundle play serve clean check test lint fmt

# --- Config ---
WASM_CRATE   := crates/q2-wasm
WASM_PKG     := $(WASM_CRATE)/pkg
DIST         := dist
OPEN         := open

# --- Default ---
help: ## Show available targets
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-14s\033[0m %s\n", $$1, $$2}'

# --- Build pipeline ---
wasm: ## Build WASM module (wasm-pack)
	wasm-pack build --target web $(WASM_CRATE)

wasm-release: ## Build WASM module (release, smaller)
	wasm-pack build --target web --release $(WASM_CRATE)

bundle: wasm ## Build WASM + bundle into dist/qwasm2.html
	cargo run -p q2-bundler

bundle-release: wasm-release ## Release WASM + bundle into dist/qwasm2.html
	cargo run --release -p q2-bundler

# --- Play / Serve ---
play: bundle ## Build, bundle, and open in browser
	$(OPEN) $(DIST)/qwasm2.html

play-release: bundle-release ## Release build, bundle, and open in browser
	$(OPEN) $(DIST)/qwasm2.html

serve: wasm ## Serve wasm-pack output on localhost:8080
	@echo "Serving $(WASM_PKG) at http://localhost:8080"
	@cd $(WASM_PKG) && python3 -m http.server 8080

# --- Native build ---
build: ## Build all crates (native)
	cargo build

build-release: ## Build all crates (native, release)
	cargo build --release

# --- Quality ---
check: ## Type-check all crates (native + wasm)
	cargo check
	cargo check --target wasm32-unknown-unknown -p q2-wasm

test: ## Run native tests
	cargo test

lint: ## Run clippy lints
	cargo clippy --all-targets -- -D warnings

fmt: ## Format code
	cargo fmt

fmt-check: ## Check formatting
	cargo fmt -- --check

# --- Cleanup ---
clean: ## Remove build artifacts
	cargo clean
	rm -rf $(WASM_PKG) $(DIST)
