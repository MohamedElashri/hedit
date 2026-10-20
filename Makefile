PREFIX ?= $(HOME)/.local

.PHONY: build check install package
build:
	cargo build --release --locked

check:
	python3 scripts/check-native.py
	python3 scripts/check-docs.py
	cargo fmt --check
	cargo clippy --locked --all-targets -- -D warnings
	cargo test --locked

install: build
	install -Dm755 target/release/hedit "$(DESTDIR)$(PREFIX)/bin/hedit"
	install -Dm644 assets/hedit.svg "$(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/hedit.svg"
	install -Dm644 packaging/linux/hedit.desktop "$(DESTDIR)$(PREFIX)/share/applications/hedit.desktop"
	install -d "$(DESTDIR)$(PREFIX)/share/hedit/themes"
	install -m644 themes/*.json "$(DESTDIR)$(PREFIX)/share/hedit/themes/"
	install -d "$(DESTDIR)$(PREFIX)/share/doc/hedit"
	cp -R docs/. "$(DESTDIR)$(PREFIX)/share/doc/hedit/"
	install -m644 LICENSE NOTICE "$(DESTDIR)$(PREFIX)/share/doc/hedit/"

package: build
	python3 scripts/package-linux.py
