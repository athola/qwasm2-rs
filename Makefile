.PHONY: help all build build-release wasm wasm-release bundle bundle-release \
        play play-release serve devserver clean clean-gamedata \
        check test test-browser lint fmt fmt-check \
        gamedata gamedata-check gamedata-demo \
        pak-web prereq-pak-repack \
        prereqs prereq-rust prereq-wasm-pack prereq-7z prereq-curl prereq-python3

# --- Config ---
.DEFAULT_GOAL := help
WASM_CRATE   := crates/q2-wasm
WASM_PKG     := $(WASM_CRATE)/pkg
DIST         := dist
GAMEDATA     := gamedata/baseq2
PORT         := 8080

# Demo pak0.pak source
DEMO_URL     := https://deponie.yamagi.org/quake2/idstuff/q2-314-demo-x86.exe

# Web pak: extension filter for future use when transcoding audio/textures (e.g. --allow bsp,cfg,opus,webp).
# Currently unused — pak-web copies all assets and Brotli-compresses for web delivery.
PAK_WEB_ALLOW :=

# Status markers (override for CI: make OK='[ok]' FAIL='[FAIL]')
OK           := ✓
FAIL         := ✗

# --- Default ---
help: ## Show available targets
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2}'

# --- Prerequisites ---
prereq-rust: ## Check Rust toolchain + wasm32 target
	@command -v cargo > /dev/null 2>&1 || \
		(echo "$(FAIL) cargo not found — install from https://rustup.rs"; exit 1)
	@rustup target list --installed | grep -q wasm32-unknown-unknown || \
		(echo "$(FAIL) wasm32-unknown-unknown target missing"; \
		 echo "  Fix: rustup target add wasm32-unknown-unknown"; exit 1)
	@echo "$(OK) cargo + wasm32 target"

prereq-wasm-pack: ## Check wasm-pack is installed
	@command -v wasm-pack > /dev/null 2>&1 || \
		(echo "$(FAIL) wasm-pack not found"; \
		 echo "  Fix: cargo install wasm-pack"; exit 1)
	@echo "$(OK) wasm-pack"

prereq-7z: ## Check 7z is installed (needed for gamedata-demo)
	@command -v 7z > /dev/null 2>&1 || \
		(echo "$(FAIL) 7z not found"; \
		 echo "  Fix: brew install p7zip"; exit 1)
	@echo "$(OK) 7z"

prereq-curl: ## Check curl is available
	@command -v curl > /dev/null 2>&1 || \
		(echo "$(FAIL) curl not found"; exit 1)
	@echo "$(OK) curl"

prereq-python3: ## Check python3 is available
	@command -v python3 > /dev/null 2>&1 || \
		(echo "$(FAIL) python3 not found"; exit 1)
	@echo "$(OK) python3"

prereq-pak-repack: ## Check q2-pak-repack builds
	@cargo build -p q2-pak-repack --quiet 2>&1 | grep -E "^error" && exit 1 || true
	@echo "$(OK) q2-pak-repack"

prereqs: prereq-rust prereq-wasm-pack prereq-curl prereq-7z prereq-python3 prereq-pak-repack ## Check all build prerequisites
	@echo ""
	@echo "All build prerequisites satisfied."

# --- Game data ---
gamedata-check: ## Ensure pak0.pak exists (auto-downloads demo if missing)
	@if [ ! -f "$(GAMEDATA)/pak0.pak" ]; then \
		echo "pak0.pak not found — downloading demo (~47 MB)..."; \
		$(MAKE) gamedata-demo || exit 1; \
	else \
		echo "$(OK) $(GAMEDATA)/pak0.pak ($$(du -h "$(GAMEDATA)/pak0.pak" | cut -f1))"; \
	fi

gamedata-demo: prereq-curl prereq-7z ## Download + extract demo pak0.pak (~47 MB)
	@set -e; tmpdir=$$(mktemp -d); trap 'rm -rf "$$tmpdir"' EXIT; \
	mkdir -p "$(GAMEDATA)"; \
	echo "Downloading Quake 2 demo installer..."; \
	curl -# -L -o "$$tmpdir/q2demo.exe" "$(DEMO_URL)"; \
	echo "Extracting pak0.pak..."; \
	(cd "$$tmpdir" && 7z e -y q2demo.exe Install/Data/baseq2/pak0.pak > /dev/null); \
	if [ -f "$$tmpdir/pak0.pak" ]; then \
		mv "$$tmpdir/pak0.pak" "$(GAMEDATA)/pak0.pak"; \
		echo "$(OK) $(GAMEDATA)/pak0.pak ($$(du -h "$(GAMEDATA)/pak0.pak" | cut -f1))"; \
	else \
		echo "$(FAIL) Extraction failed"; \
		echo "  Try manually: download q2-314-demo-x86.exe, extract with 7z,"; \
		echo "  copy Install/Data/baseq2/pak0.pak to $(GAMEDATA)/"; \
		exit 1; \
	fi

gamedata: gamedata-check ## Alias for gamedata-check

pak-web: prereq-pak-repack gamedata-check ## Build web pak: all assets, Brotli-compressed (~26 MB wire size)
	cargo run -p q2-pak-repack --release -- \
		--in "$(GAMEDATA)/pak0.pak" \
		--out "$(GAMEDATA)/pak0-web.pak" \
		--all \
		--brotli
	@echo "$(OK) $(GAMEDATA)/pak0-web.pak ready"

# --- Build pipeline ---
wasm: prereq-wasm-pack ## Build WASM module (wasm-pack, debug)
	wasm-pack build --dev --target web $(WASM_CRATE)

wasm-release: prereq-wasm-pack ## Build WASM module (release, smaller)
	wasm-pack build --target web --release $(WASM_CRATE)

bundle: wasm ## Build WASM + bundle into dist/qwasm2.html
	cargo run -p q2-bundler

bundle-release: wasm-release ## Release WASM + bundle into dist/qwasm2.html
	cargo run --release -p q2-bundler

# --- Play / Serve ---
play: prereqs bundle gamedata-check pak-web ## Build everything + launch devserver
	@echo ""
	@echo "  http://127.0.0.1:$(PORT)/qwasm2.html"
	@echo ""
	PORT=$(PORT) cargo run -p q2-devserver

play-release: prereqs bundle-release gamedata-check pak-web ## Release build + devserver
	@echo ""
	@echo "  http://127.0.0.1:$(PORT)/qwasm2.html"
	@echo ""
	PORT=$(PORT) cargo run --release -p q2-devserver

devserver: gamedata-check ## Run devserver only (no rebuild)
	PORT=$(PORT) cargo run -p q2-devserver

serve: wasm prereq-python3 ## Serve wasm-pack output on localhost (simple, no gamedata)
	@echo "Serving $(WASM_PKG) at http://localhost:$(PORT)"
	@cd "$(WASM_PKG)" && python3 -m http.server "$(PORT)"

# --- Native build ---
all: build ## Build all crates (alias for build)

build: prereq-rust ## Build all crates (native)
	cargo build

build-release: prereq-rust ## Build all crates (native, release)
	cargo build --release

# --- Quality ---
check: prereq-rust ## Type-check all crates (native + wasm)
	cargo check
	cargo check --target wasm32-unknown-unknown -p q2-wasm

test: prereq-rust ## Run native tests
	cargo test --workspace --lib --bins --tests --exclude q2-wasm

test-browser: bundle ## Run Playwright browser tests (requires npx)
	cd tests/browser && npx playwright install --with-deps chromium 2>/dev/null; \
	cd tests/browser && npx playwright test

lint: prereq-rust ## Run clippy lints
	cargo clippy --all-targets -- -D warnings

fmt: prereq-rust ## Format code
	cargo fmt

fmt-check: prereq-rust ## Check formatting
	cargo fmt -- --check

# --- Cleanup ---
clean: ## Remove build artifacts
	cargo clean
	rm -rf "$(WASM_PKG)" "$(DIST)"
	rm -f "$(GAMEDATA)/pak0-web.pak" "$(GAMEDATA)/pak0-web.pak.br"

clean-gamedata: ## Remove downloaded game data
	rm -rf gamedata/
