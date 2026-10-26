.PHONY: help all build build-release wasm wasm-release bundle bundle-release \
        play play-release devserver clean clean-gamedata \
        check test test-browser lint fmt fmt-check ci \
        gamedata gamedata-check gamedata-demo pak-web \
        prereqs prereq-rust prereq-wasm-pack prereq-7z prereq-curl

# --- Config ---
.DEFAULT_GOAL := help
WASM_CRATE   := crates/q2-wasm
WASM_PKG     := $(WASM_CRATE)/pkg
DIST         := dist
GAMEDATA     := gamedata/baseq2
PORT         := 8080
WASM_TARGET  := wasm32-unknown-unknown

# Extra cargo flags. CI sets CARGO_FLAGS=--locked so a stale Cargo.lock fails.
CARGO_FLAGS  ?=
# CI sets --with-deps (installs system libraries via apt/sudo on Linux).
PLAYWRIGHT_INSTALL_FLAGS ?=

# Demo pak0.pak source. SHA-256 of the installer as downloaded on 2026-10-26;
# its extracted pak0.pak is byte-identical to the one the project was built with.
DEMO_URL     := https://deponie.yamagi.org/quake2/idstuff/q2-314-demo-x86.exe
DEMO_SHA256  := 7ace5a43983f10d6bdc9d9b6e17a1032ba6223118d389bd170df89b945a04a1e

WEB_PAK      := $(GAMEDATA)/pak0-web.pak
# Written last by q2-pak-repack, so it doubles as the "web pak is complete" marker.
WEB_PAK_BR   := $(WEB_PAK).br

# Status markers (override for CI: make OK='[ok]' FAIL='[FAIL]')
OK           := ✓
FAIL         := ✗

# --- Default ---
help: ## Show available targets
	@grep -E '^[a-zA-Z0-9_-]+:.*## .*$$' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*## "}; {printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2}'

# --- Prerequisites ---
prereq-rust: ## Check Rust toolchain + wasm32 target
	@command -v cargo > /dev/null 2>&1 || \
		(echo "$(FAIL) cargo not found — install from https://rustup.rs"; exit 1)
	@test -d "$$(rustc --print target-libdir --target $(WASM_TARGET))" || \
		(echo "$(FAIL) $(WASM_TARGET) target missing"; \
		 echo "  Fix: rustup target add $(WASM_TARGET)"; exit 1)
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

prereq-curl: ## Check curl is available (needed for gamedata-demo)
	@command -v curl > /dev/null 2>&1 || \
		(echo "$(FAIL) curl not found"; exit 1)
	@echo "$(OK) curl"

prereqs: prereq-rust prereq-wasm-pack prereq-curl prereq-7z ## Check all prerequisites (7z/curl only matter without pak0.pak)
	@echo ""
	@echo "All build prerequisites satisfied."

# --- Game data ---
$(GAMEDATA)/pak0.pak:
	@echo "pak0.pak not found — downloading demo (~39 MB installer)..."
	@$(MAKE) gamedata-demo

gamedata-check: $(GAMEDATA)/pak0.pak ## Ensure pak0.pak exists (auto-downloads demo if missing)
	@echo "$(OK) $(GAMEDATA)/pak0.pak ($$(du -h "$(GAMEDATA)/pak0.pak" | cut -f1))"

gamedata-demo: prereq-curl prereq-7z ## Download + extract demo pak0.pak (overwrites it)
	@set -e; tmpdir=$$(mktemp -d); trap 'rm -rf "$$tmpdir"' EXIT; \
	mkdir -p "$(GAMEDATA)"; \
	echo "Downloading Quake 2 demo installer..."; \
	curl --fail -# -L -o "$$tmpdir/q2demo.exe" "$(DEMO_URL)"; \
	sum=$$(shasum -a 256 "$$tmpdir/q2demo.exe" | cut -d' ' -f1); \
	if [ "$$sum" != "$(DEMO_SHA256)" ]; then \
		echo "$(FAIL) Checksum mismatch for $(DEMO_URL)"; \
		echo "  expected $(DEMO_SHA256)"; \
		echo "  got      $$sum"; \
		exit 1; \
	fi; \
	echo "Extracting pak0.pak..."; \
	if ! (cd "$$tmpdir" && 7z e -y q2demo.exe Install/Data/baseq2/pak0.pak > /dev/null) || \
	   [ ! -f "$$tmpdir/pak0.pak" ]; then \
		echo "$(FAIL) Extraction failed"; \
		echo "  Try manually: download q2-314-demo-x86.exe, extract with 7z,"; \
		echo "  copy Install/Data/baseq2/pak0.pak to $(GAMEDATA)/"; \
		exit 1; \
	fi; \
	mv "$$tmpdir/pak0.pak" "$(GAMEDATA)/pak0.pak"; \
	echo "$(OK) $(GAMEDATA)/pak0.pak ($$(du -h "$(GAMEDATA)/pak0.pak" | cut -f1))"

gamedata: gamedata-check ## Alias for gamedata-check

# Rebuilt only when pak0.pak or the repacker changes; Brotli level 11 takes ~1 min.
$(WEB_PAK_BR): $(GAMEDATA)/pak0.pak $(wildcard crates/q2-pak-repack/src/*.rs)
	cargo run $(CARGO_FLAGS) -p q2-pak-repack --release -- \
		--in "$(GAMEDATA)/pak0.pak" \
		--out "$(WEB_PAK)" \
		--brotli
	@echo "$(OK) $(WEB_PAK) ready"

pak-web: $(WEB_PAK_BR) ## Build web pak: all assets, Brotli-compressed (~28 MB wire size)

# --- Build pipeline ---
wasm: prereq-wasm-pack ## Build WASM module (wasm-pack, debug)
	wasm-pack build --dev --target web $(WASM_CRATE)

wasm-release: prereq-wasm-pack ## Build WASM module (release, smaller)
	wasm-pack build --target web --release $(WASM_CRATE)

bundle: wasm ## Build WASM + bundle into dist/qwasm2.html
	cargo run $(CARGO_FLAGS) -p q2-bundler

bundle-release: wasm-release ## Release WASM + bundle into dist/qwasm2.html
	cargo run $(CARGO_FLAGS) --release -p q2-bundler

# --- Play ---
play: prereq-rust bundle pak-web ## Build everything + launch devserver
	@echo ""
	@echo "  http://127.0.0.1:$(PORT)/qwasm2.html"
	@echo ""
	PORT=$(PORT) cargo run $(CARGO_FLAGS) -p q2-devserver

play-release: prereq-rust bundle-release pak-web ## Release build + devserver
	@echo ""
	@echo "  http://127.0.0.1:$(PORT)/qwasm2.html"
	@echo ""
	PORT=$(PORT) cargo run $(CARGO_FLAGS) --release -p q2-devserver

devserver: gamedata-check ## Run devserver only (no rebuild)
	PORT=$(PORT) cargo run $(CARGO_FLAGS) -p q2-devserver

# --- Native build ---
all: build ## Build all crates (alias for build)

build: prereq-rust ## Build all crates (native)
	cargo build $(CARGO_FLAGS)

build-release: prereq-rust ## Build all crates (native, release)
	cargo build $(CARGO_FLAGS) --release

# --- Quality (CI runs `make ci`) ---
check: prereq-rust ## Type-check all crates (native + wasm32 libraries)
	cargo check $(CARGO_FLAGS) --workspace --all-targets
	cargo check $(CARGO_FLAGS) --workspace --lib --target $(WASM_TARGET)

test: prereq-rust ## Run all workspace tests (unit, integration, doc)
	cargo test $(CARGO_FLAGS) --workspace

test-browser: bundle ## Run Playwright browser tests (requires npm)
	cd tests/browser && npm ci && \
		npx playwright install $(PLAYWRIGHT_INSTALL_FLAGS) chromium && \
		npx playwright test

lint: prereq-rust ## Run clippy (native all targets + wasm32 libraries)
	cargo clippy $(CARGO_FLAGS) --workspace --all-targets -- -D warnings
	cargo clippy $(CARGO_FLAGS) --workspace --lib --target $(WASM_TARGET) -- -D warnings

fmt: prereq-rust ## Format code
	cargo fmt --all

fmt-check: prereq-rust ## Check formatting
	cargo fmt --all -- --check

ci: fmt-check lint check test ## Run the Rust checks CI runs

# --- Cleanup ---
clean: ## Remove build artifacts
	cargo clean
	rm -rf "$(WASM_PKG)" "$(DIST)" tests/browser/test-results tests/browser/playwright-report
	rm -f "$(WEB_PAK)" "$(WEB_PAK_BR)"

clean-gamedata: ## Delete gamedata/baseq2 pak0.pak + web paks (includes a copied retail pak0.pak)
	rm -f "$(GAMEDATA)/pak0.pak" "$(WEB_PAK)" "$(WEB_PAK_BR)"
