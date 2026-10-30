#!/usr/bin/env python3
"""Check version validation and the changelog text used in GitHub releases."""
from contextlib import redirect_stdout
import importlib.util
import io
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('check_release', Path(__file__).with_name('check-release.py'))
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class ReleaseNotes(unittest.TestCase):
    def test_extracts_only_requested_release_and_preserves_markdown(self):
        changelog = '''# Changelog

## [Unreleased]

- Future work.

## [0.1.11] - 2026-10-30

### Changed

- Compact editor. **Keep formatting.**
  Preserve continuation lines and Unicode: λ →.

### Fixed

- Close unsaved notes.

## [0.1.10] - 2026-10-30

- Markdown rendering.
'''
        notes = release.release_notes(changelog, '0.1.11')
        self.assertEqual(notes, '''### Changed

- Compact editor. **Keep formatting.**
  Preserve continuation lines and Unicode: λ →.

### Fixed

- Close unsaved notes.
''')
        self.assertNotIn('Future work', notes)
        self.assertNotIn('Markdown rendering', notes)

    def test_rejects_missing_undated_empty_duplicate_and_invalid_date_entries(self):
        valid = '## [0.1.11] - 2026-10-30\n\n- A fix.\n'
        for changelog in ['', '## [0.1.11]\n- A fix.\n',
                          '## [0.1.11] - 2026-10-30\n\n### Fixed\n',
                          valid + '\n' + valid,
                          '## [0.1.11] - 2026-02-30\n- A fix.\n']:
            with self.subTest(changelog=changelog), self.assertRaises(ValueError):
                release.release_notes(changelog, '0.1.11')

    def test_cli_checks_metadata_before_writing_notes(self):
        with tempfile.TemporaryDirectory() as work:
            root = Path(work)
            (root / 'Cargo.toml').write_text('[package]\nversion = "0.1.11"\n', encoding='utf-8')
            (root / 'CHANGELOG.md').write_text('## [0.1.11] - 2026-10-30\n\n- A fix.\n', encoding='utf-8')
            notes = root / 'notes.md'
            with patch.object(release, 'ROOT', root), redirect_stdout(io.StringIO()):
                for tag in ['', 'v0.1.10']:
                    with patch.dict(release.os.environ, {'RELEASE_TAG': tag}), self.assertRaises(SystemExit):
                        release.main(['--notes-file', str(notes)])
                    self.assertFalse(notes.exists())
                with patch.dict(release.os.environ, {'RELEASE_TAG': 'v0.1.11'}):
                    release.main(['--notes-file', str(notes)])
                    self.assertEqual(notes.read_text(encoding='utf-8'), '- A fix.\n')
                    (root / 'CHANGELOG.md').write_text('# Missing release entry\n', encoding='utf-8')
                    with self.assertRaises(SystemExit):
                        release.main(['--notes-file', str(notes)])
                    self.assertEqual(notes.read_text(encoding='utf-8'), '- A fix.\n')

    def test_markdown_is_documented_under_its_original_release(self):
        changelog = (release.ROOT / 'CHANGELOG.md').read_text(encoding='utf-8')
        notes = release.release_notes(changelog, '0.1.10')
        self.assertIn('Markdown files have Source, Split, and Preview modes', notes)
        self.assertIn('LaTeX', notes)
        self.assertIn('mermaid', notes)


if __name__ == '__main__':
    unittest.main()
