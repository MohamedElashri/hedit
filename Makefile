.DEFAULT_GOAL := help

CARGO ?= cargo
PYTHON ?= python3
ACTIONLINT ?= actionlint
NIDA ?= nida
PROFILE ?= dev
SHARED_BUILD ?= 0
CLEAN_KIND ?= builds
APPLY ?= 0
PREFIX ?= $(HOME)/.local
REMOTE_HELPER ?= target/release/hedit-remote
export RELEASE_TAG

DEVELOPMENT_OUTPUT = target/$(if $(filter dev test,$(PROFILE)),debug,$(PROFILE))
ifeq ($(SHARED_BUILD),1)
HEDIT_SHARED_BUILD_DIR := $(shell $(PYTHON) scripts/build-storage.py shared-dir)
ifeq ($(strip $(HEDIT_SHARED_BUILD_DIR)),)
$(error Cannot locate the repository shared build directory)
endif
export CARGO_BUILD_BUILD_DIR := $(HEDIT_SHARED_BUILD_DIR)
endif

.PHONY: build-space clean-dev clean-release clean-incremental clean-worktrees clean-shared

.PHONY: help dev build run fmt fmt-check policy docs docs-site docs-site-prepare test-docs-site scripts-check static clippy lint test test-scripts test-renderer test-macos test-remote test-remote-live check ci smoke package install release-check workflow-lint linux-packages smoke-linux-packages macos clean

help: ## Show available commands and configuration
	@printf 'hedit development commands\n\n'
	@awk 'BEGIN { FS = ":.*## " } /^[a-zA-Z0-9_-]+:.*## / { printf "  %-18s %s\n", $$1, $$2 }' $(MAKEFILE_LIST)
	@printf '\nOptions: PREFIX (default ~/.local), DESTDIR (installation staging), ARGS (run arguments)\n'
	@printf 'Overrides: CARGO, PYTHON, ACTIONLINT; RELEASE_TAG for release-check\n'
	@printf 'Local builds: PROFILE=debugging for full symbols; SHARED_BUILD=1 to share intermediates\n'
	@printf 'Broad cleanup: CLEAN_KIND=builds|dev|release|incremental; APPLY=1 deletes the preview\n'
	@printf '\nBefore committing: make static clippy, then make test (Linux) or make test-macos (macOS)\n'

dev: ## Build compact development output; PROFILE=debugging enables full symbols
	$(CARGO) build --locked --workspace --profile $(PROFILE)
	$(PYTHON) scripts/macos_plugin_helper.py --editor "$(DEVELOPMENT_OUTPUT)/hedit"

build: ## Build the optimized release executable
	$(CARGO) build --release --locked --workspace
	$(PYTHON) scripts/macos_plugin_helper.py --editor target/release/hedit

run: dev ## Launch a development session; pass arguments with ARGS='...'
	$(CARGO) run --locked --profile $(PROFILE) -- $(ARGS)

fmt: ## Format Rust source
	$(CARGO) fmt --all

fmt-check: ## Check Rust formatting without changing files
	$(CARGO) fmt --all --check

policy: ## Reject app frontend and Node files; browser scripts stay in the docs site
	$(PYTHON) scripts/check-native.py

docs: ## Check documentation links, images, and section anchors
	$(PYTHON) scripts/check-docs.py

docs-site: ## Assemble, check, and build the Nida docs site (requires Nida)
	$(PYTHON) scripts/build-docs.py --nida "$(NIDA)"

docs-site-prepare: ## Assemble web sources for nida serve --site target/docs-site
	$(PYTHON) scripts/build-docs.py --prepare-only

test-docs-site: ## Test documentation imports, rendered-link checks, and native policy
	$(PYTHON) scripts/test-docs-site.py

scripts-check: ## Check Python and installer shell syntax
	$(PYTHON) -m compileall -q scripts
	sh -n install.sh
	sh -n packaging/linux/install.sh

static: fmt-check policy docs scripts-check ## Run the quick checks that need no compiling

clippy: ## Run clippy with warnings denied
	$(CARGO) clippy --locked --workspace --all-targets -- -D warnings

lint: static clippy ## Run all static checks: quick checks first, then clippy

test: ## Run Linux workspace, renderer, installer, and script tests
	$(CARGO) test --locked --workspace
	$(MAKE) test-renderer
	$(MAKE) test-scripts
	$(PYTHON) scripts/test-install.py

test-macos: ## Run the tests that apply on macOS (Rust tests and the app bundle checks)
	$(CARGO) build --locked --bin hedit
	$(PYTHON) scripts/macos_plugin_helper.py --editor target/debug/hedit
	$(CARGO) test --locked --workspace --no-fail-fast
	$(MAKE) test-renderer
	$(MAKE) test-scripts

test-scripts: ## Run offline Python toolchain and package tests (Linux/macOS)
	$(PYTHON) scripts/test-build-storage.py
	$(PYTHON) scripts/test-native-trace.py
	$(PYTHON) scripts/test-remote-live-setup.py
	$(PYTHON) scripts/test-zig-setup.py
	$(PYTHON) scripts/test-run-zig.py
	$(PYTHON) scripts/test-release.py
	$(PYTHON) scripts/test-linux-package.py
	$(PYTHON) scripts/test-macos-package.py
	$(PYTHON) scripts/test-windows-package.py

test-renderer: ## Test the patched native text renderer and widget library
	$(CARGO) test --locked -p egui -p epaint --lib

test-remote: ## Test the SSH workspace helper and recovery engine
	$(CARGO) test --locked -p hedit-remote

# Keep the bundled test helper within the managed bootstrap's 64 MiB upload limit.
test-remote-live: ## Test real loopback SSH and deterministic process crashes (requires sshd)
	$(CARGO) rustc --locked -p hedit-remote --bin hedit-remote -- -Cstrip=debuginfo
	$(CARGO) build --locked --example remote_workspace_probe
	$(CARGO) test --locked --bin hedit --no-run --message-format=json > target/remote-editor-tests.jsonl
	$(CARGO) build --locked -p hedit-remote --features test-faults --target-dir target/remote-fault-tests
	$(PYTHON) scripts/test-remote-live.py --fault-binary target/remote-fault-tests/debug/hedit-remote

check: lint test ## Run Linux static checks and tests without a release build

ci: check package smoke ## Run Linux checks, tests, packaging, and installation smoke tests

package: build ## Create the Linux release archive and SHA-256 file in dist/
	$(PYTHON) scripts/package-linux.py --helper "$(REMOTE_HELPER)"

smoke: build ## Verify Linux package extraction, installation, checksums, and the executable
	$(PYTHON) scripts/smoke-package.py --native-broker --helper "$(REMOTE_HELPER)"

linux-packages: build ## Build Linux tarball, DEB, RPM, and AppImage (requires packaging tools)
	$(PYTHON) scripts/package-linux.py --helper "$(REMOTE_HELPER)" --formats tar.gz deb rpm appimage

smoke-linux-packages: ## Verify Linux formats and distro installation (requires Docker)
	$(PYTHON) scripts/smoke-linux-packages.py --helper "$(REMOTE_HELPER)" --containers

install: build ## Install the Linux build under PREFIX; supports DESTDIR staging
	install -Dm755 target/release/hedit "$(DESTDIR)$(PREFIX)/bin/hedit"
	install -Dm755 target/release/hedit-remote "$(DESTDIR)$(PREFIX)/bin/hedit-remote"
	install -Dm755 target/release/hedit-json-tools "$(DESTDIR)$(PREFIX)/bin/hedit-json-tools"
	install -Dm644 assets/hedit.svg "$(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/hedit.svg"
	install -Dm644 packaging/linux/hedit.desktop "$(DESTDIR)$(PREFIX)/share/applications/hedit.desktop"
	install -d "$(DESTDIR)$(PREFIX)/share/hedit/themes"
	install -m644 themes/*.json "$(DESTDIR)$(PREFIX)/share/hedit/themes/"
	$(PYTHON) scripts/package_resources.py --out-dir "$(DESTDIR)$(PREFIX)/share/doc/hedit"

release-check: ## Check RELEASE_TAG=vX.Y.Z against the version in Cargo.toml
	$(PYTHON) scripts/check-release.py

workflow-lint: ## Validate GitHub Actions workflows (requires actionlint)
	$(ACTIONLINT)

build-space: ## Report generated storage across registered worktrees
	$(PYTHON) scripts/build-storage.py report --all-worktrees

clean: ## Remove this worktree's compiler output; retain tools and validation evidence
	$(PYTHON) scripts/build-storage.py clean --apply

clean-dev: ## Remove this worktree's dev/test/debugging compiler output
	$(PYTHON) scripts/build-storage.py clean --kind dev --apply

clean-release: ## Remove this worktree's release output, including universal architecture slices
	$(PYTHON) scripts/build-storage.py clean --kind release --apply

clean-incremental: ## Remove only this worktree's incremental compiler caches
	$(PYTHON) scripts/build-storage.py clean --kind incremental --apply

clean-worktrees: ## Preview compiler cleanup across worktrees; APPLY=1 deletes it
	$(PYTHON) scripts/build-storage.py clean --all-worktrees --kind $(CLEAN_KIND) $(if $(filter 1,$(APPLY)),--apply,)

clean-shared: ## Preview shared intermediate cleanup; APPLY=1 deletes it
	$(PYTHON) scripts/build-storage.py clean --target-dir "$(shell $(PYTHON) scripts/build-storage.py shared-dir)" --kind $(CLEAN_KIND) $(if $(filter 1,$(APPLY)),--apply,)

macos: ## Build and package one universal macOS app (requires macOS and Xcode)
	MACOSX_DEPLOYMENT_TARGET=11.0 $(CARGO) build --release --locked --workspace --target aarch64-apple-darwin
	MACOSX_DEPLOYMENT_TARGET=11.0 $(CARGO) build --release --locked --workspace --target x86_64-apple-darwin
	$(PYTHON) scripts/package-macos.py
