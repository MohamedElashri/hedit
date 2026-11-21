"""Install the offline documentation shared by source and release packages."""
import argparse
from pathlib import Path
import shutil


DOCUMENTATION_FILES = ['README.md', 'USER_GUIDE.md', 'DEVELOPER_GUIDE.md', 'CHANGELOG.md', 'LICENSE',
                     'vendor/egui_commonmark/PATCH.md', 'vendor/libghostty-vt-sys/PATCH.md',
                     'docs/extensions.md', 'docs/extension-security.md', 'docs/plugin-sdk.md',
                     'schemas/plugin-manifest.schema.json', 'schemas/hedit-sdk.lua',
                     'examples/extensions/sdk-markdown.json', 'examples/extensions/sdk-markdown.fixtures.json',
                     'examples/extensions/sdk-table.json', 'examples/extensions/sdk-table.fixtures.json',
                     'examples/extensions/document-assist.json', 'examples/extensions/document-assist.fixtures.json',
                     'docs/plugin-services.md', 'docs/plugin-broker.md', 'examples/extensions/demo-visuals.json',
                     'examples/extensions/sdk-trim.json', 'examples/extensions/sdk-trim.fixtures.json',
                     'examples/extensions/markdown-task.json',
                     'examples/extensions/markdown-quote.json',
                     'examples/extensions/scoped-json.json', 'examples/extensions/scoped-json.lua',
                     'examples/extensions/scoped-json.fixtures.json',
                     'examples/extensions/scoped-rename.json', 'examples/extensions/scoped-rename.lua',
                     'examples/extensions/scoped-rename.fixtures.json',
                     'examples/extensions/scoped-markdown-html.json', 'examples/extensions/scoped-markdown-html.lua',
                     'examples/extensions/scoped-markdown-html.fixtures.json']

def copy_documentation(root, destination):
    destination.mkdir(parents=True, exist_ok=True)
    for filename in DOCUMENTATION_FILES:
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
