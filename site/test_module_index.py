"""scripts/module-index.py: one file saying what a release's modules are.

Run against the real module sources here, with release-shaped module.json
files written for three of them: a page-only module, one that runs on both
a machine and a page, and a test double.
"""
import json
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


class ModuleIndex(unittest.TestCase):
    def test_a_release_index_says_what_each_module_is(self):
        with tempfile.TemporaryDirectory(prefix='code-module-index-') as temp:
            dist = Path(temp)
            for name, platforms in [('telemetry', ['linux-x86_64', 'wasm32']), ('arrays', ['linux-x86_64', 'wasm32']), ('mongodb_mock', ['linux-x86_64'])]:
                (dist / f'{name}.json').write_text(json.dumps({
                    'name': name, 'version': '9.9.9', 'handlers': ['A', 'B'],
                    'platforms': {p: {'asset': f'{name}-{p}', 'sha256': '0'} for p in platforms},
                }))
            (dist / 'module-hashes.json').write_text('{"telemetry": "abc"}')
            made = subprocess.run(['python3', str(ROOT / 'scripts/module-index.py'), str(dist), str(ROOT)],
                                  check=True, capture_output=True, text=True)
            index = json.loads(made.stdout)
        self.assertEqual(index['version'], '9.9.9')
        by = {m['name']: m for m in index['modules']}
        self.assertEqual(sorted(by), ['arrays', 'mongodb_mock', 'telemetry'])
        self.assertEqual(by['telemetry']['runs'], 'browser')
        self.assertEqual(by['arrays']['runs'], 'both')
        self.assertEqual(by['mongodb_mock']['runs'], 'machine')
        self.assertTrue(by['mongodb_mock']['test_double'])
        self.assertEqual(by['telemetry']['description'], 'What went wrong on a page, kept and sent.')
        self.assertIn('Configure', by['telemetry']['readme'])
        self.assertEqual(by['telemetry']['platforms'], ['linux-x86_64', 'wasm32'])


if __name__ == '__main__':
    unittest.main()
