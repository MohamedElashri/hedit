#!/bin/sh
# Download a hedit release and install it without a compiler or package manager.
# Keep execution inside main so a truncated piped download cannot start installing.
main() (
    set -eu

    fail() { printf 'hedit: %s\n' "$*" >&2; exit 1; }
    usage() {
        cat <<'HELP'
Usage: sh install.sh [--version vX.Y.Z] [--prefix /absolute/path]
                     [--app-dir /absolute/path] [--github-cli]

Install the latest stable hedit release, or select a release tag.
Linux: installs under the prefix (default ~/.local); requires tar and sha256sum.
macOS: installs hedit.app in the app directory (default ~/Applications) and a
hedit command under the prefix; requires ditto and shasum.
Both need curl, or authenticated gh with --github-cli for private repositories.
HELP
    }

    version=
    install_prefix=${HOME}/.local
    app_dir=${HOME}/Applications
    use_gh=false
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --version|--prefix|--app-dir)
                [ "$#" -ge 2 ] || fail "Missing value for $1."
                case "$1" in
                    --version) version=$2 ;;
                    --prefix) install_prefix=$2 ;;
                    --app-dir) app_dir=$2 ;;
                esac
                shift 2 ;;
            --github-cli) use_gh=true; shift ;;
            -h|--help) usage; exit 0 ;;
            *) fail "Unknown argument: $1. Use --help for usage." ;;
        esac
    done
    case "$install_prefix" in /*) ;; *) fail 'Use an absolute installation prefix.' ;; esac
    case "$app_dir" in /*) ;; *) fail 'Use an absolute application directory.' ;; esac
    # The macOS launcher embeds the app path in a quoted shell string.
    case "$app_dir" in *[\"\$\`\\]*) fail 'The application directory cannot contain quotes, $, `, or \.' ;; esac
    case "$(uname -m)" in
        x86_64|amd64) arch=x86_64 ;;
        aarch64|arm64) arch=aarch64 ;;
        *) fail 'Supported architectures: x86_64 and aarch64.' ;;
    esac
    case "$(uname -s)" in
        Linux) platform=linux; tools='tar sha256sum mktemp install' ;;
        # The macOS app is universal, so one archive serves both architectures.
        Darwin) platform=macos; tools='ditto shasum mktemp' ;;
        *) fail 'Release packages support Linux and macOS.' ;;
    esac
    for tool in $tools; do
        command -v "$tool" >/dev/null 2>&1 || fail "Required command not found: $tool."
    done
    repo=MohamedElashri/hedit
    if "$use_gh"; then
        command -v gh >/dev/null 2>&1 || fail 'Install GitHub CLI and run gh auth login first.'
        gh auth status --hostname github.com >/dev/null 2>&1 || fail 'Run gh auth login for an account with repository access.'
        if [ -z "$version" ]; then
            version=$(gh release view --repo "$repo" --json tagName --jq .tagName) || fail 'No accessible stable release found.'
        fi
    else
        command -v curl >/dev/null 2>&1 || fail 'Required command not found: curl.'
        if [ -z "$version" ]; then
            release_url=$(curl --fail --silent --show-error --location --retry 3 \
                --connect-timeout 15 --max-time 300 \
                --proto '=https' --proto-redir '=https' --output /dev/null \
                --write-out '%{url_effective}' "https://github.com/$repo/releases/latest") || \
                fail 'Cannot find a release. For a private repository, use --github-cli.'
            case "$release_url" in
                "https://github.com/$repo/releases/tag/"*) version=${release_url##*/} ;;
                *) fail 'GitHub did not return a release tag.' ;;
            esac
        fi
    fi
    case "$version" in
        v[0-9]*) ;;
        *) fail 'Expected a release tag such as v0.1.2.' ;;
    esac
    case "$version" in *[!a-zA-Z0-9.+-]*) fail 'Invalid release tag.' ;; esac

    if [ "$platform" = linux ]; then
        package=hedit-${version#v}-linux-$arch
        archive=$package.tar.gz
        label="Linux $arch"
    else
        package=hedit-${version#v}-macos-universal
        archive=$package.zip
        label='macOS (universal)'
    fi
    work_dir=$(mktemp -d "${TMPDIR:-/tmp}/hedit-install.XXXXXXXX")
    trap 'rm -rf "$work_dir"' EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM HUP
    printf 'Downloading hedit %s for %s…\n' "$version" "$label"
    if "$use_gh"; then
        gh release download "$version" --repo "$repo" --dir "$work_dir" \
            --pattern "$archive" --pattern "$archive.sha256" || fail 'Release download failed.'
    else
        for asset in "$archive" "$archive.sha256"; do
            curl --fail --silent --show-error --location --retry 3 \
                --connect-timeout 15 --max-time 300 \
                --proto '=https' --proto-redir '=https' \
                --output "$work_dir/$asset" \
                "https://github.com/$repo/releases/download/$version/$asset" || \
                fail 'Release download failed. Check the tag and architecture; private repositories need --github-cli.'
        done
    fi
    # Accept exactly one checksum for the expected archive, never paths from a manifest.
    checksum=$(cat "$work_dir/$archive.sha256")
    read -r expected filename extra <<CHECKSUM
$checksum
CHECKSUM
    [ -z "$extra" ] && [ "$filename" = "$archive" ] || fail 'Invalid checksum file.'
    [ "${#expected}" -eq 64 ] || fail 'Invalid SHA-256 checksum.'
    case "$expected" in *[!0-9a-f]*) fail 'Invalid SHA-256 checksum.' ;; esac
    [ "$checksum" = "$expected  $archive" ] || fail 'Invalid checksum file.'
    if [ "$platform" = linux ]; then
        actual=$(sha256sum "$work_dir/$archive")
    else
        actual=$(shasum -a 256 "$work_dir/$archive")
    fi
    [ "${actual%% *}" = "$expected" ] || fail 'Checksum mismatch; nothing was installed.'

    if [ "$platform" = macos ]; then
        install_macos
    else
        install_linux
    fi
    case ":${PATH}:" in
        *":$install_prefix/bin:"*) ;;
        *) printf 'Add %s/bin to your PATH to use the hedit command.\n' "$install_prefix" ;;
    esac
)

install_linux() {
    tar -xzf "$work_dir/$archive" --no-same-owner -C "$work_dir"
    # Check host library compatibility before replacing an existing installation.
    "$work_dir/$package/hedit" --version || fail 'This package cannot run on your system. See the user guide for source installation.'
    sh "$work_dir/$package/install.sh" "$install_prefix"
    printf '\nRun hedit with: %s/bin/hedit\n' "$install_prefix"
}

install_macos() {
    ditto -x -k "$work_dir/$archive" "$work_dir/unpacked" || fail 'Cannot extract the macOS archive.'
    app=$work_dir/unpacked/hedit.app
    [ -x "$app/Contents/MacOS/hedit" ] || fail 'The macOS archive does not contain hedit.app.'
    "$app/Contents/MacOS/hedit" --version || fail 'This package cannot run on your system.'
    if command -v codesign >/dev/null 2>&1; then
        codesign --verify --deep --strict "$app" || fail 'The app signature is invalid; nothing was installed.'
    fi
    mkdir -p "$app_dir" "$install_prefix/bin"
    target=$app_dir/hedit.app
    # Stage beside the target so the swap is a rename, and keep the old app until it succeeds.
    staged=$app_dir/.hedit.app.new
    rm -rf "$staged" "$app_dir/.hedit.app.old"
    ditto "$app" "$staged"
    if [ -e "$target" ]; then
        mv "$target" "$app_dir/.hedit.app.old"
    fi
    mv "$staged" "$target" || {
        [ -e "$app_dir/.hedit.app.old" ] && mv "$app_dir/.hedit.app.old" "$target"
        fail 'Cannot install hedit.app.'
    }
    rm -rf "$app_dir/.hedit.app.old"
    # A wrapper, not a symlink, so the app re-launches itself from inside its bundle.
    launcher=$install_prefix/bin/hedit
    rm -f "$launcher"
    printf '#!/bin/sh\nexec "%s/Contents/MacOS/hedit" "$@"\n' "$target" > "$launcher"
    chmod 755 "$launcher"
    printf 'Installed hedit.app in %s\n' "$app_dir"
    printf '\nOpen hedit from %s, or run: %s\n' "$target" "$launcher"
}
main "$@"
