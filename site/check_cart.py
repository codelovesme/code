#!/usr/bin/env python3
"""Verify the downloadable cart programs using public v2.24.1 in a fresh project."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

site = Path(__file__).resolve().parent
cli = Path(sys.argv[1]).resolve(strict=True)
built = Path(sys.argv[2]).resolve(strict=True)
files = built / 'lesson-examples/cart'
lesson = next(x for x in json.loads((site / 'lessons.json').read_text()) if x['id'] == 'small-program')
release = 'https://github.com/codelovesme/code/releases/tag/v2.24.1'
source = (site / 'examples/learn_cart_final.code').read_text()
assert (files / 'receipt.code').read_text() == source
with tempfile.TemporaryDirectory(prefix='code-cart space-') as directory:
    root = Path(directory)
    env = dict(os.environ)
    for key in ['CODE_RUN', 'CODE_LINK', 'CODE_MODULE_PATH', 'CODE_MODULE_RELEASE', 'CODE_CACHE_DIR']:
        env.pop(key, None)
    env.update(CODE_MODULE_RELEASE=release, CODE_CACHE_DIR=str(root / 'cache'), CODE_CACHE='1')

    def run(args, cwd):
        result = subprocess.run([str(cli), *args], cwd=cwd, env=env, text=True, capture_output=True)
        assert result.returncode == 0, result.stdout + result.stderr
        return result.stdout.strip()

    assert run(['--version'], root) == 'Code v2.24.1'
    run(['init', 'hello'], root)
    project = root / 'hello'
    run(['install', 'console'], project)
    console = json.loads((project / '.code/lock.json').read_text())['modules']['console']
    assert console['version'] == '2.24.1' and console['source'] == release
    asset = project / '.code/modules/console' / console['version'] / console['asset']
    assert hashlib.sha256(asset.read_bytes()).hexdigest() == console['sha256']

    def verify(program, expected):
        (project / 'main.code').write_text(program)
        assert run(['run', '--interpret'], project) == expected
        run(['build', '--release'], project)
        result = subprocess.run([str(project / 'build/hello')], cwd=project, env=env,
                                text=True, capture_output=True, check=True)
        assert result.stdout.strip() == expected, result.stdout

    for step in lesson['steps']:
        program = (site / 'examples' / (step['example'] + '.code')).read_text()
        verify(program, step['output'])
        for edit in step.get('edits', []):
            verify(program.replace(edit['find'], edit['replace']), edit['output'])
    for name in ['cart.code', 'main.code']:
        (project / name).write_bytes((files / name).read_bytes())
    result = subprocess.run(['sh', '-eu', str(files / 'run.sh')], cwd=project,
                            env=dict(env, code_cli=str(cli)), text=True, capture_output=True, check=True)
    final = next(x for x in lesson['steps'] if x['id'] == 'final')
    assert result.stdout.count(final['output']) == 2, result.stdout
    assert json.loads(run(['check'], project)) == {'schema_version': 1, 'diagnostics': []}
    caller = (files / 'main.code').read_text()
    for edit in final['edits']:
        verify(caller.replace(edit['find'], edit['replace']), edit['output'])
    invalid = next(x for x in lesson['steps'] if x['id'] == 'invalid')
    verify(caller.replace('count = 2, price = 3', 'count = -1, price = 3'), invalid['output'])
print('Public v2.24.1: seven standalone variants, exact local files/commands, four local edits, check schema and console provenance/hash passed interpreted/compiled')
