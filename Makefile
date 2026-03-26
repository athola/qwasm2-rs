.PHONY: help build wasm bundle play serve clean check test lint fmt \
       gamedata gamedata-check gamedata-demo devserver \
       prereqs prereq-rust prereq-wasm-pack prereq-7z prereq-curl

# --- Config ---
WASM_CRATE   := crates/q2-wasm
WASM_PKG     := $(WASM_CRATE)/pkg
DIST         := dist
GAMEDATA     := gamedata/baseq2
OPEN         := open
PORT         := 8080

# Demo pak0.pak source
DEMO_URL     := https://deponie.yamagi.org/quake2/idstuff/q2-314-demo-x86.exe

# --- Default ---
help: ## Show available targets
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2}'

# --- Prerequisites ---
prereq-rust: ## Check Rust toolchain + wasm32 target
	@command -v cargo > /dev/null 2>&1 || \
		(echo "✗ cargo not found — install from https://rustup.rs"; exit 1)
	@rustup target list --installed | grep -q wasm32-unknown-unknown || \
		(echo "✗ wasm32-unknown-unknown target missing"; \
		 echo "  Fix: rustup target add wasm32-unknown-unknown"; exit 1)
	@echo "✓ cargo + wasm32 target"

prereq-wasm-pack: ## Check wasm-pack is installed
	@command -v wasm-pack > /dev/null 2>&1 || \
		(echo "✗ wasm-pack not found"; \
		 echo "  Fix: cargo install wasm-pack"; exit 1)
	@echo "✓ wasm-pack"

prereq-7z: ## Check 7z is installed (needed for gamedata-demo)
	@command -v 7z > /dev/null 2>&1 || \
		(echo "✗ 7z not found"; \
		 echo "  Fix: brew install p7zip"; exit 1)
	@echo "✓ 7z"

prereq-curl: ## Check curl is available
	@command -v curl > /dev/null 2>&1 || \
		(echo "✗ curl not found"; exit 1)
	@echo "✓ curl"

prereqs: prereq-rust prereq-wasm-pack prereq-curl prereq-7z ## Check all build prerequisites
	@echo ""
	@echo "All build prerequisites satisfied."

# --- Game data ---
gamedata-check: ## Ensure pak0.pak exists (auto-downloads demo if missing)
	@if [ ! -f $(GAMEDATA)/pak0.pak ]; then \
		echo "pak0.pak not found — downloading demo (~47 MB)..."; \
		$(MAKE) gamedata-demo; \
	else \
		echo "✓ $(GAMEDATA)/pak0.pak ($$(du -h $(GAMEDATA)/pak0.pak | cut -f1))"; \
	fi

gamedata-demo: prereq-curl prereq-7z ## Download + extract demo pak0.pak (~47 MB)
	@mkdir -p $(GAMEDATA) /tmp/q2demo
	@echo "Downloading Quake 2 demo installer..."
	@curl -# -L -o /tmp/q2demo/q2demo.exe "$(DEMO_URL)"
	@echo "Extracting pak0.pak..."
	@cd /tmp/q2demo && 7z e -y q2demo.exe Install/Data/baseq2/pak0.pak > /dev/null 2>&1
	@if [ -f /tmp/q2demo/pak0.pak ]; then \
		mv /tmp/q2demo/pak0.pak $(GAMEDATA)/pak0.pak; \
		echo "✓ $(GAMEDATA)/pak0.pak ($$(du -h $(GAMEDATA)/pak0.pak | cut -f1))"; \
	else \
		echo "✗ Extraction failed"; \
		echo "  Try manually: download q2-314-demo-x86.exe, extract with 7z,"; \
		echo "  copy Install/Data/baseq2/pak0.pak to $(GAMEDATA)/"; \
		rm -rf /tmp/q2demo; exit 1; \
	fi
	@rm -rf /tmp/q2demo

gamedata: gamedata-check ## Alias for gamedata-check

# --- Build pipeline ---
wasm: prereq-wasm-pack ## Build WASM module (wasm-pack)
	wasm-pack build --target web $(WASM_CRATE)

wasm-release: prereq-wasm-pack ## Build WASM module (release, smaller)
	wasm-pack build --target web --release $(WASM_CRATE)

bundle: wasm ## Build WASM + bundle into dist/qwasm2.html
	cargo run -p q2-bundler

bundle-release: wasm-release ## Release WASM + bundle into dist/qwasm2.html
	cargo run --release -p q2-bundler

# --- Play / Serve ---
play: prereqs bundle gamedata-check ## Build everything + launch devserver
	@echo ""
	@echo "  http://127.0.0.1:$(PORT)/qwasm2.html"
	@echo ""
	PORT=$(PORT) cargo run -p q2-devserver

play-release: prereqs bundle-release gamedata-check ## Release build + devserver
	PORT=$(PORT) cargo run --release -p q2-devserver

devserver: gamedata-check ## Run devserver only (no rebuild)
	PORT=$(PORT) cargo run -p q2-devserver

serve: wasm ## Serve wasm-pack output on localhost:8080 (simple, no gamedata)
	@echo "Serving $(WASM_PKG) at http://localhost:8080"
	@cd $(WASM_PKG) && python3 -m http.server 8080

# --- Native build ---
build: prereq-rust ## Build all crates (native)
	cargo build

build-release: prereq-rust ## Build all crates (native, release)
	cargo build --release

# --- Quality ---
check: ## Type-check all crates (native + wasm)
	cargo check
	cargo check --target wasm32-unknown-unknown -p q2-wasm

test: ## Run native tests
	cargo test --workspace --lib --bins --tests --exclude q2-wasm

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

clean-gamedata: ## Remove downloaded game data
	rm -rf gamedata/
