"""Offline documentation shared by the Linux, macOS, and Windows packages."""
import shutil


def copy_documentation(root, destination):
    destination.mkdir(parents=True, exist_ok=True)
    for filename in ['README.md', 'USER_GUIDE.md', 'DEVELOPER_GUIDE.md', 'CHANGELOG.md', 'LICENSE',
                     'vendor/egui_commonmark/PATCH.md', 'vendor/libghostty-vt-sys/PATCH.md',
                     'docs/extensions.md', 'examples/extensions/markdown-task.json',
                     'examples/extensions/markdown-quote.json']:
        target = destination / filename
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(root / filename, target)
    for directory in ['assets', 'docs/images']:
        shutil.copytree(root / directory, destination / directory)
