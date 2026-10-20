# Install hedit on Linux

You need a graphical Linux session using X11 or Wayland, with OpenGL available.
CUDA and NVIDIA drivers are optional; ordinary text editing does not use them.

## Build from your checkout

Install Rust 1.92 or newer. On Debian or Ubuntu, install the native build tools:

```sh
sudo apt-get install build-essential pkg-config libx11-dev libxkbcommon-dev libwayland-dev libegl1-mesa-dev
cargo build --release --locked
./target/release/hedit
```

Build dependencies are managed by Cargo. No Node.js or frontend package manager
is needed.

To install the executable, application icon, menu entry, themes, and user guides
under `~/.local`:

```sh
make install
```

Ensure `~/.local/bin` is on your `PATH`, then run `hedit`. You can also launch
`~/.local/bin/hedit` directly. The desktop entry uses `hedit` from your graphical
session's `PATH`. If it does not appear immediately, restart your application
launcher or sign out and back in.

Use an alternative prefix with `make install PREFIX=/absolute/path`. Installation
does not change or delete your session data.

## Portable Linux package

Create a package from your checkout with:

```sh
make package
```

This writes a versioned archive and SHA-256 checksum to `dist/`. The architecture
in the filename matches the machine used to build it. A package built on a newer
Linux distribution may need newer system libraries on the destination machine.

For an archive you already have, verify its checksum, extract it, then run the
included installer. For example, for the x86-64 package:

```sh
sha256sum -c hedit-0.1.0-linux-x86_64.tar.gz.sha256
tar -xzf hedit-0.1.0-linux-x86_64.tar.gz
cd hedit-0.1.0-linux-x86_64
./hedit --version
sh install.sh
```

You can instead run `./hedit` from the extracted directory. The optional argument
to `install.sh` is an absolute installation prefix, such as
`sh install.sh /home/you/.local`.

## File dialogs

On Linux, the Open and Save dialogs use your desktop's XDG portal. If the dialogs
fail to appear, see [troubleshooting](troubleshooting.md#open-or-save-dialog-does-not-appear).

## Uninstall

For the default installation, remove `~/.local/bin/hedit`,
`~/.local/share/applications/hedit.desktop`,
`~/.local/share/icons/hicolor/scalable/apps/hedit.svg`,
`~/.local/share/hedit/themes/`, and `~/.local/share/doc/hedit/`.

Your recovery data normally lives in `~/.local/share/hedit/session/`. Keep it if
you may need your unsaved buffers again. See [recovery](recovery.md) before
removing session data.
