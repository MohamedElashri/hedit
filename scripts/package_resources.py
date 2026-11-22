"""Install the offline documentation shared by source and release packages."""
import argparse
from pathlib import Path
import shutil


def copy_documentation(root, destination):
    destination.mkdir(parents=True, exist_ok=True)
    for filename in ['README.md', 'USER_GUIDE.md', 'DEVELOPER_GUIDE.md', 'CHANGELOG.md', 'LICENSE',
                     'vendor/egui_commonmark/PATCH.md', 'vendor/libghostty-vt-sys/PATCH.md',
                     'docs/extensions.md', 'docs/extension-security.md', 'docs/plugin-sdk.md',
                     'schemas/plugin-manifest.schema.json', 'schemas/hedit-sdk.lua',
                     'examples/extensions/sdk-markdown.json', 'examples/extensions/sdk-markdown.fixtures.json',
                     'examples/extensions/sdk-table.json', 'examples/extensions/sdk-table.fixtures.json',
                     'examples/extensions/sdk-trim.json', 'examples/extensions/sdk-trim.fixtures.json',
                     'examples/extensions/markdown-task.json',
                     'examples/extensions/markdown-quote.json']:
        target = destination / filename
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(root / filename, target)
    for directory in ['assets', 'docs/images']:
        shutil.copytree(root / directory, destination / directory, dirs_exist_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out-dir', type=Path, required=True)
    args = parser.parse_args()
    copy_documentation(Path(__file__).resolve().parents[1], args.out_dir)


if __name__ == '__main__':
    main()
