#!/bin/sh
# Install the unpacked portable package. No network or package manager needed.
set -eu
package_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
install_prefix=${1:-"${HOME}/.local"}
case "$install_prefix" in
    /*) ;;
    *) printf '%s\n' 'Use an absolute install prefix, for example /home/you/.local.' >&2; exit 2 ;;
esac
install -Dm755 "$package_dir/hedit" "$install_prefix/bin/hedit"
install -Dm644 "$package_dir/assets/hedit.svg" "$install_prefix/share/icons/hicolor/scalable/apps/hedit.svg"
install -Dm644 "$package_dir/hedit.desktop" "$install_prefix/share/applications/hedit.desktop"
install -d "$install_prefix/share/hedit/themes" "$install_prefix/share/doc/hedit"
install -m644 "$package_dir"/themes/*.json "$install_prefix/share/hedit/themes/"
cp -R "$package_dir/docs/." "$install_prefix/share/doc/hedit/"
install -m644 "$package_dir/LICENSE" "$package_dir/NOTICE" "$install_prefix/share/doc/hedit/"
printf 'Installed hedit in %s\n' "$install_prefix"
printf 'Launch: %s/bin/hedit\n' "$install_prefix"
