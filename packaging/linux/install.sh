#!/bin/sh
# Install the unpacked portable package. No network or package manager needed.
set -eu
package_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
install_prefix=${1:-"${HOME}/.local"}
case "$install_prefix" in
    /*) ;;
    *) printf '%s\n' 'Use an absolute install prefix, for example /home/you/.local.' >&2; exit 2 ;;
esac
install -d "$install_prefix/bin" \
    "$install_prefix/share/icons/hicolor/scalable/apps" "$install_prefix/share/applications"
install -m755 "$package_dir/hedit" "$install_prefix/bin/hedit"
install -m755 "$package_dir/hedit-remote" "$install_prefix/bin/hedit-remote"
install -m755 "$package_dir/hedit-json-tools" "$install_prefix/bin/hedit-json-tools"
install -m644 "$package_dir/assets/hedit.svg" "$install_prefix/share/icons/hicolor/scalable/apps/hedit.svg"
install -m644 "$package_dir/hedit.desktop" "$install_prefix/share/applications/hedit.desktop"
install -d "$install_prefix/share/hedit/themes" "$install_prefix/share/doc/hedit"
install -m644 "$package_dir"/themes/*.json "$install_prefix/share/hedit/themes/"
install -m644 "$package_dir/README.md" "$package_dir/USER_GUIDE.md" \
    "$package_dir/DEVELOPER_GUIDE.md" "$package_dir/CHANGELOG.md" "$package_dir/LICENSE" \
    "$install_prefix/share/doc/hedit/"
cp -R "$package_dir/assets" "$package_dir/docs" "$package_dir/vendor" "$package_dir/schemas" "$install_prefix/share/doc/hedit/"
install -d "$install_prefix/share/doc/hedit/examples/extensions"
install -m644 "$package_dir"/examples/extensions/*.json "$package_dir"/examples/extensions/*.lua \
    "$install_prefix/share/doc/hedit/examples/extensions/"
printf 'Installed hedit in %s\n' "$install_prefix"
printf 'Launch: %s/bin/hedit\n' "$install_prefix"
