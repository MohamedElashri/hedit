.DEFAULT_GOAL := help

CARGO ?= cargo
PYTHON ?= python3
ACTIONLINT ?= actionlint
PREFIX ?= $(HOME)/.local
export RELEASE_TAG

.PHONY: help dev build run fmt fmt-check policy docs scripts-check static clippy lint test test-macos check ci smoke package install release-check workflow-lint macos clean

help: ## Show available commands and configuration
	@printf 'hedit development commands\n\n'
	@awk 'BEGIN { FS = ":.*## " } /^[a-zA-Z0-9_-]+:.*## / { printf "  %-18s %s\n", $$1, $$2 }' $(MAKEFILE_LIST)
	@printf '\nOptions: PREFIX (default ~/.local), DESTDIR (installation staging), ARGS (run arguments)\n'
	@printf 'Overrides: CARGO, PYTHON, ACTIONLINT; RELEASE_TAG for release-check\n'
	@printf '\nBefore committing: make ci\n'

dev: ## Build a debug executable
	$(CARGO) build --locked

build: ## Build the optimized release executable
	$(CARGO) build --release --locked

run: ## Launch a development session; pass arguments with ARGS='...'
	$(CARGO) run --locked -- $(ARGS)

fmt: ## Format Rust source
	$(CARGO) fmt --all

fmt-check: ## Check Rust formatting without changing files
	$(CARGO) fmt --all --check

policy: ## Check that the project stays free of Node/frontend files
	$(PYTHON) scripts/check-native.py

docs: ## Check documentation links, images, and section anchors
	$(PYTHON) scripts/check-docs.py

scripts-check: ## Check Python and installer shell syntax
	$(PYTHON) -m compileall -q scripts
	sh -n install.sh
	sh -n packaging/linux/install.sh

static: fmt-check policy docs scripts-check ## Run the quick checks that need no compiling

clippy: ## Run clippy with warnings denied
	$(CARGO) clippy --locked --all-targets -- -D warnings

lint: static clippy ## Run all static checks: quick checks first, then clippy

test: ## Run Rust tests and offline release-installer tests
	$(CARGO) test --locked
	$(PYTHON) scripts/test-zig-setup.py
	$(PYTHON) scripts/test-release.py
	$(PYTHON) scripts/test-install.py
	$(PYTHON) scripts/test-macos-package.py
	$(PYTHON) scripts/test-windows-package.py

test-macos: ## Run the tests that apply on macOS (Rust tests and the app bundle checks)
	$(CARGO) test --locked --no-fail-fast
	$(PYTHON) scripts/test-zig-setup.py
	$(PYTHON) scripts/test-release.py
	$(PYTHON) scripts/test-macos-package.py

check: lint test ## Run static checks and tests without a release build

ci: check package smoke ## Run the same full validation as GitHub CI; use before committing

package: build ## Create the Linux release archive and SHA-256 file in dist/
	$(PYTHON) scripts/package-linux.py

smoke: build ## Verify package extraction, installation, checksums, and the executable
	$(PYTHON) scripts/smoke-package.py

install: build ## Install under PREFIX; supports a DESTDIR staging directory
	install -Dm755 target/release/hedit "$(DESTDIR)$(PREFIX)/bin/hedit"
	install -Dm644 assets/hedit.svg "$(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/hedit.svg"
	install -Dm644 packaging/linux/hedit.desktop "$(DESTDIR)$(PREFIX)/share/applications/hedit.desktop"
	install -d "$(DESTDIR)$(PREFIX)/share/hedit/themes"
	install -m644 themes/*.json "$(DESTDIR)$(PREFIX)/share/hedit/themes/"
	install -d "$(DESTDIR)$(PREFIX)/share/doc/hedit"
	install -m644 README.md USER_GUIDE.md DEVELOPER_GUIDE.md LICENSE "$(DESTDIR)$(PREFIX)/share/doc/hedit/"
	cp -R assets "$(DESTDIR)$(PREFIX)/share/doc/hedit/"

release-check: ## Check RELEASE_TAG=vX.Y.Z against the version in Cargo.toml
	$(PYTHON) scripts/check-release.py

workflow-lint: ## Validate GitHub Actions workflows (requires actionlint)
	$(ACTIONLINT)

clean: ## Remove Cargo build artifacts; keep installed files and session data
	$(CARGO) clean

macos: ## Build and package one universal macOS app (requires macOS and Xcode)
	MACOSX_DEPLOYMENT_TARGET=11.0 $(CARGO) build --release --locked --target aarch64-apple-darwin
	MACOSX_DEPLOYMENT_TARGET=11.0 $(CARGO) build --release --locked --target x86_64-apple-darwin
	$(PYTHON) scripts/package-macos.py
