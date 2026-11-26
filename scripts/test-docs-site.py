#!/usr/bin/env python3
"""Exercise shared-document routing, complete rendered links, and the docs-only JS exception."""
import importlib.util
from contextlib import redirect_stdout
import io
import json
from pathlib import Path
import tempfile
import tomllib
import unittest
from unittest.mock import patch


def module(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    loaded = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(loaded)
    return loaded


build = module('docs_build', 'build-docs.py')
rendered = module('docs_rendered', 'check-docs-site.py')
policy = module('native_policy', 'check-native.py')


class Imports(unittest.TestCase):
    def test_deployment_url_updates_only_the_assembled_configuration(self):
        original = (build.SITE / 'config.toml').read_bytes()
        with tempfile.TemporaryDirectory() as directory, patch.object(build, 'OUTPUT', Path(directory) / 'site'):
            with redirect_stdout(io.StringIO()):
                output = build.prepare('https://example.com/manual')
            configuration = tomllib.loads((output / 'config.toml').read_text())
            self.assertEqual(configuration['base_url'], 'https://example.com/manual/')
            self.assertEqual((build.SITE / 'config.toml').read_bytes(), original)

    def test_fences_do_not_create_sections_and_duplicates_keep_source_anchors(self):
        text = '# Guide\n\n```md\n## Not a heading\n```\n\n## Repeat\ntext\n## Repeat\nother\n'
        self.assertEqual([r[3] for r in build.headings(text)], ['guide', 'repeat', 'repeat-1'])
        with self.assertRaises(ValueError):
            build.extract(text, {'source': 'guide.md', 'heading': 'Repeat'})

    def test_extracts_nested_section_and_maps_repeated_headings(self):
        text = '# Guide\n## First\n### Details\none\n## Second\n### Details\ntwo\n### Omit\nsecret\n## Third\nthree\n'
        body, line, anchors = build.extract(text, {'source': 'guide.md', 'heading': 'Second', 'exclude': ['Omit']})
        self.assertEqual(body, '## Details\ntwo\n')
        self.assertEqual(line, 5)
        self.assertEqual(anchors, [('second', ''), ('details-1', 'details')])
        with self.assertRaises(ValueError):
            build.extract(text, {'source': 'guide.md', 'heading': 'Missing'})

    def test_cross_page_fragments_assets_and_code_examples(self):
        routes = {'USER_GUIDE.md': 'guide/_index.md', 'docs/sdk.md': 'reference/sdk.md'}
        anchors = {('USER_GUIDE.md', 'settings'): ('guide/settings.md', ''),
                   ('docs/sdk.md', 'edits'): ('reference/sdk.md', 'edits')}
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'docs/images').mkdir(parents=True)
            (root / 'docs/images/a.png').touch()
            rewrite = lambda target: build.rewrite_target(target, 'USER_GUIDE.md', routes, anchors, '/manual', root)
            text = '[settings](#settings) [API](docs/sdk.md#edits)\n![view](docs/images/a.png)\n```md\n[example](#not-real)\n```\n'
            self.assertEqual(build.rewrite_links(text, rewrite, '/manual'),
                             '[settings](@/guide/settings.md) [API](@/reference/sdk.md#edits)\n{{< screenshot file="images/a.png" alt="view" >}}\n```md\n[example](#not-real)\n```\n')
            with self.assertRaises(ValueError):
                rewrite('docs/sdk.md#missing')
            self.assertEqual(rewrite('https://example.com/#anything'), 'https://example.com/#anything')


class RenderedLinks(unittest.TestCase):
    def test_metadata_parser_preserves_jsonld_and_reports_malformed_data(self):
        page = rendered.Page('<meta name="robots" content="noindex, follow">'
                             '<meta property="og:image" content="https://example.com/a.png">'
                             '<script type="application/ld+json">{"@type":"WebPage","name":"A & B"}</script>')
        self.assertEqual(page.meta['robots'], 'noindex, follow')
        self.assertEqual(page.structured, [{'@type': 'WebPage', 'name': 'A & B'}])
        self.assertIn('https://example.com/a.png', page.links)
        broken = rendered.Page('<script type="application/ld+json">{broken}</script>')
        self.assertEqual(len(broken.json_errors), 1)

    def test_checks_navigation_fragments_assets_prefixes_and_search(self):
        with tempfile.TemporaryDirectory() as directory:
            public = Path(directory).resolve()
            (public / 'guide').mkdir()
            (public / 'index.html').write_text('<h1 id="home">Home</h1><a href="/manual/guide/#part">Guide</a><script src="/manual/code.js"></script>')
            (public / 'guide/index.html').write_text('<h1 id="part">Guide</h1><a href="../#home">Home</a>')
            (public / 'code.js').touch()
            docs = {'/guide/': {'title': 'Guide', 'body': 'text'}}
            (public / 'search-index.js').write_text('window.searchIndex = ' + json.dumps({'documentStore': {'docs': docs}}) + ';')
            self.assertEqual(rendered.check(public, 'https://example.com/manual/'), ([], 2))
            (public / 'guide/index.html').write_text('<h1 id="part">Guide</h1><p id="part">duplicate</p><a href="/manual/#missing">Bad</a><img src="/outside.png"><a href="/manual/gone/">Gone</a>')
            errors, _ = rendered.check(public, 'https://example.com/manual/')
            self.assertEqual(len(errors), 4)
            self.assertTrue(any('duplicate' in e for e in errors))
            self.assertTrue(any('fragment' in e for e in errors))
            self.assertTrue(any('prefix' in e for e in errors))
            self.assertTrue(any('missing /manual/gone/' in e for e in errors))


class NativePolicy(unittest.TestCase):
    def test_browser_js_exception_does_not_allow_app_frontend_or_node(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ['website/static/js/docs.js', 'src/app.js', 'website/package.json',
                         'website/static/js/component.ts', 'website/content/extra.js',
                         '.worktrees/other/website/static/js/docs.js', 'src/.worktrees/hidden.js']:
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.touch()
            with patch.object(policy, 'root', root):
                self.assertEqual(set(map(str, policy.scan(root))), {'src/app.js', 'website/package.json',
                                 'website/static/js/component.ts', 'website/content/extra.js',
                                 'src/.worktrees/hidden.js'})


if __name__ == '__main__':
    unittest.main()
