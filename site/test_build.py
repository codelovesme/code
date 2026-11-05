"""Static artifact checks: direct routes, anchors, assets and example preservation."""
import csv
import json
from collections import defaultdict
from statistics import median
import os
import posixpath
import shutil
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
        if tag == 'nav' and 'site-nav' in attrs.get('class', '').split():
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
        cls.pages = {p.relative_to(cls.dist).as_posix(): Page(p.read_text()) for p in cls.dist.rglob('*.html')}

    @classmethod
    def tearDownClass(cls):
        cls.temp.cleanup()

    def test_direct_pages_share_working_navigation(self):
        self.assertEqual(set(self.pages), {'index.html', 'learn.html', 'playground.html', 'docs.html', 'compare.html', 'modules.html', 'execution.html', 'memory.html', 'learn/quick-start.html'})
        expected = ['index.html', 'learn.html', 'playground.html', 'docs.html', 'https://github.com/codelovesme/code']
        for name, page in self.pages.items():
            prefix = '../' if name.startswith('learn/') else ''
            self.assertEqual(page.navigation, [prefix + href if not href.startswith('https:') else href for href in expected], name)
            self.assertEqual(len(page.ids), len(set(page.ids)), name)
            self.assertIn('main', page.ids)
            current = 'learn.html' if name.startswith('learn/') else ('docs.html' if name in ('modules.html', 'execution.html', 'memory.html', 'compare.html') else name)
            self.assertEqual(page.current, [prefix + current], name)

    def test_local_links_and_assets_exist_at_github_pages_subpath(self):
        for name, page in self.pages.items():
            for link in page.links:
                url = urlsplit(link)
                if url.scheme or url.netloc:
                    continue
                self.assertFalse(url.path.startswith('/'), (name, link))
                target = posixpath.normpath(posixpath.join(posixpath.dirname(name), unquote(url.path))) if url.path else name
                self.assertTrue((self.dist / target).is_file(), (name, link))
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
        previews = json.loads((self.dist / 'demos.json').read_text())
        self.assertTrue(3 <= len(previews) <= 6)
        self.assertEqual(len({preview['name'] for preview in previews}), len(previews))
        for preview in previews:
            self.assertTrue(preview['title'])
            self.assertTrue(preview['description'])
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

    def test_performance_numbers_match_raw_samples(self):
        # Verify against raw launches, independently of the JSON summaries used
        # by the renderer. A scale/unit error or stale edited number must fail.
        class Numbers(HTMLParser):
            def __init__(self, source):
                super().__init__()
                self.key = None
                self.values = {}
                self.feed(source)

            def handle_starttag(self, tag, attrs):
                if tag == 'span':
                    self.key = dict(attrs).get('data-evidence')

            def handle_data(self, data):
                if self.key:
                    self.values[tuple(self.key.split('|'))] = data

            def handle_endtag(self, tag):
                if tag == 'span':
                    self.key = None

        groups = defaultdict(list)
        raw = ROOT / 'benchmarks/website-evidence/results/2026-11-04-v2.23.0/samples.csv'
        with raw.open() as source:
            for row in csv.DictReader(source):
                groups[row['group'], row['workload'], row['mode']].append(row)
        page = Numbers((self.dist / 'compare.html').read_text())
        self.assertGreater(len(page.values), 40)
        for (group, workload, mode, metric), displayed in page.values.items():
            samples = groups[group, workload, mode]
            field, scale = ('elapsed_seconds', 1000) if metric == 'time' else ('peak_rss_kib', 1 / 1024)
            expected = median(float(row[field]) for row in samples) * scale
            self.assertEqual(displayed, f'{expected:.3f}', (group, workload, mode, metric))
        self.assertIn(('prebuilt', 'snapshots', 'Rust Rc COW', 'rss'), page.values)
        self.assertIn(('compile', 'sum_squares', 'Code release', 'rss'), page.values)

    def test_only_ready_lessons_are_published_and_linked(self):
        plan = json.loads((ROOT / 'site/lessons.json').read_text())
        for lesson in plan:
            self.assertEqual((self.dist / lesson['route']).exists(), lesson['ready'])
            if not lesson['ready']:
                for page in self.pages.values():
                    self.assertFalse(any(urlsplit(href).path.endswith(lesson['route']) for href in page.links))
        self.assertIn('hello, world', (self.dist / 'learn.html').read_text())
        self.assertIn('hello, Ada', (self.dist / 'learn/quick-start.html').read_text())
        variants = json.loads((self.dist / 'lesson-examples.json').read_text())
        self.assertEqual({row['output'] for row in variants}, {'hello, world', 'hello, Ada'})

    def test_lesson_navigation_follows_readiness_and_withdraws_old_pages(self):
        # Simulate the next task's publication, without shipping a draft lesson.
        with tempfile.TemporaryDirectory(prefix='code-lessons-publish-') as directory:
            repo = Path(directory) / 'repo'
            shutil.copytree(ROOT / 'site', repo / 'site', ignore=shutil.ignore_patterns('__pycache__'))
            (repo / 'tests').mkdir()
            for fixture in (ROOT / 'tests').glob('*.code'):
                shutil.copyfile(fixture, repo / 'tests' / fixture.name)
            for filename in ['.github/workflows/pages.yml', 'benchmarks/website-evidence/results/2026-11-04-v2.23.0/report.json']:
                destination = repo / filename
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(ROOT / filename, destination)
            plan = json.loads((repo / 'site/lessons.json').read_text())
            plan[1]['ready'] = True
            (repo / 'site' / plan[1]['source']).write_text('<section><h2>A ready lesson fixture</h2></section>')
            manifest = repo / 'site/lessons.json'
            manifest.write_text(json.dumps(plan))
            dist = Path(directory) / 'dist'
            command = ['python3', str(repo / 'site/build.py'), str(repo), str(dist)]
            subprocess.run(command, env=self.environment, check=True, capture_output=True)
            first = (dist / 'learn/first-program.html').read_text()
            starter = (dist / 'learn/quick-start.html').read_text()
            self.assertIn('href="../learn/quick-start.html" rel="prev"', first)
            self.assertIn('href="../learn/first-program.html" rel="next"', starter)
            self.assertNotIn('href="../learn/values.html"', first)
            plan[1]['ready'] = False
            manifest.write_text(json.dumps(plan))
            subprocess.run(command, env=self.environment, check=True, capture_output=True)
            self.assertFalse((dist / 'learn/first-program.html').exists())
            self.assertNotIn('href="../learn/first-program.html"', (dist / 'learn/quick-start.html').read_text())

    def test_rebuilding_is_deterministic(self):
        before = {str(p.relative_to(self.dist)): p.read_bytes() for p in self.dist.rglob('*') if p.is_file()}
        subprocess.run(['python3', str(ROOT / 'site/build.py'), str(ROOT), str(self.dist)],
                       env=self.environment, check=True, capture_output=True)
        self.assertEqual(before, {str(p.relative_to(self.dist)): p.read_bytes() for p in self.dist.rglob('*') if p.is_file()})


if __name__ == '__main__':
    unittest.main()
