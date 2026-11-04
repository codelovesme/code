"""Static artifact checks: direct routes, anchors, assets and example preservation."""
import json
import os
from html.parser import HTMLParser
from pathlib import Path
import subprocess
import tempfile
import unittest
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]


class Page(HTMLParser):
    def __init__(self, source):
        super().__init__()
        self.ids = []
        self.links = []
        self.navigation = []
        self.current = []
        self.in_navigation = False
        self.feed(source)

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if 'id' in attrs:
            self.ids.append(attrs['id'])
        if tag == 'nav':
            self.in_navigation = True
        for name in ('href', 'src'):
            if name in attrs:
                self.links.append(attrs[name])
        if self.in_navigation and tag == 'a':
            self.navigation.append(attrs.get('href'))
            if attrs.get('aria-current') == 'page':
                self.current.append(attrs.get('href'))

    def handle_endtag(self, tag):
        if tag == 'nav':
            self.in_navigation = False


class SiteBuild(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory(prefix='code-site-build-')
        cls.dist = Path(cls.temp.name)
        cls.environment = dict(os.environ)
        cls.environment.pop('PLAYGROUND_CODE_WASM_VERSION', None)
        subprocess.run(['python3', str(ROOT / 'site/build.py'), str(ROOT), str(cls.dist)],
                       env=cls.environment, check=True, capture_output=True)
        cls.pages = {p.name: Page(p.read_text()) for p in cls.dist.glob('*.html')}

    @classmethod
    def tearDownClass(cls):
        cls.temp.cleanup()

    def test_direct_pages_share_working_navigation(self):
        self.assertEqual(set(self.pages), {'index.html', 'learn.html', 'playground.html', 'docs.html', 'compare.html', 'modules.html'})
        expected = ['index.html', 'learn.html', 'playground.html', 'docs.html', 'https://github.com/codelovesme/code']
        for name, page in self.pages.items():
            self.assertEqual(page.navigation, expected, name)
            self.assertEqual(len(page.ids), len(set(page.ids)), name)
            self.assertIn('main', page.ids)
            if name != 'compare.html':
                self.assertEqual(page.current, ['docs.html' if name == 'modules.html' else name], name)

    def test_local_links_and_assets_exist_at_github_pages_subpath(self):
        for name, page in self.pages.items():
            for link in page.links:
                url = urlsplit(link)
                if url.scheme or url.netloc:
                    continue
                self.assertFalse(url.path.startswith('/'), (name, link))
                target = url.path or name
                self.assertTrue((self.dist / unquote(target)).is_file(), (name, link))
                if url.fragment and target in self.pages:
                    self.assertIn(unquote(url.fragment), self.pages[target].ids, (name, link))

    def test_reference_and_gallery_are_not_rendered_on_homepage(self):
        home = (self.dist / 'index.html').read_text()
        self.assertNotIn('id="examples-data"', home)
        self.assertNotIn('id="ex-grid"', home)
        self.assertNotIn('id="embedding"', home)
        for anchor in ['features', 'packages', 'cli', 'applications', 'embedding', 'code-modules', 'native-modules', 'publishing']:
            self.assertIn(anchor, self.pages['docs.html'].ids)
        for anchor in ['playground', 'examples', 'source', 'output', 'run']:
            self.assertIn(anchor, self.pages['playground.html'].ids)
        for anchor in ['performance', 'memory', 'fit']:
            self.assertIn(anchor, self.pages['compare.html'].ids)

    def test_previews_are_checked_real_examples(self):
        examples = json.loads((self.dist / 'examples.json').read_text())
        self.assertGreater(len(examples), 100)
        by_name = {example['name']: example for example in examples}
        for preview in json.loads((self.dist / 'demos.json').read_text()):
            self.assertEqual(preview['code'], by_name[preview['name']]['code'])
            self.assertIn('output', preview)
        self.assertIn('fail_undefined_variable', by_name)

    def test_recursion_compatibility_exception_is_explicit_and_pin_specific(self):
        compatibility = json.loads((self.dist / 'browser-compatibility.json').read_text())
        if compatibility['engine'] != '2.16.0':
            self.assertFalse(compatibility['omitted'])
            return
        self.assertEqual([item['name'] for item in compatibility['omitted']], ['handler_recursion'])
        with tempfile.TemporaryDirectory(prefix='code-site-future-pin-') as directory:
            subprocess.run(['python3', str(ROOT / 'site/build.py'), str(ROOT), directory],
                           env=dict(self.environment, PLAYGROUND_CODE_WASM_VERSION='future-test-version'),
                           check=True, capture_output=True)
            future = json.loads((Path(directory) / 'examples.json').read_text())
            self.assertIn('handler_recursion', {example['name'] for example in future})

    def test_rebuilding_is_deterministic(self):
        before = {str(p.relative_to(self.dist)): p.read_bytes() for p in self.dist.rglob('*') if p.is_file()}
        subprocess.run(['python3', str(ROOT / 'site/build.py'), str(ROOT), str(self.dist)],
                       env=self.environment, check=True, capture_output=True)
        self.assertEqual(before, {str(p.relative_to(self.dist)): p.read_bytes() for p in self.dist.rglob('*') if p.is_file()})


if __name__ == '__main__':
    unittest.main()
