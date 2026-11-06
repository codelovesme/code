#!/usr/bin/env python3
"""Run the lesson's exact assembled scripts in an empty Linux x86_64 project.

Usage: python3 site/check_first_program.py /absolute/path/to/built-site
Downloads the actual public v2.23.2 CLI and console. Needs sh/curl/tar/cc/network.
No source-built CLI/modules or existing project/cache is used.
"""
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import tempfile


def main():
    if len(sys.argv) != 2 or platform.system() != 'Linux' or platform.machine() != 'x86_64':
        raise SystemExit(__doc__)
    site = Path(sys.argv[1]).resolve(strict=True)
    assets = site / 'lesson-examples/first-program'
    script = assets / 'start.sh'
    with tempfile.TemporaryDirectory(prefix='code-lesson-219 space-') as directory:
        root = Path(directory)
        env = dict(os.environ)
        for key in ['CODE_RUN', 'CODE_LINK', 'CODE_MODULE_PATH', 'CODE_MODULE_RELEASE', 'CODE_CACHE_DIR']:
            env.pop(key, None)
        env.update(CODE_CACHE='1', CODE_CACHE_DIR=str(root / 'cache'))
        result = subprocess.run(['sh', str(script)], cwd=root, env=env,
                                text=True, capture_output=True, check=True)
        if 'Code v2.23.2' not in result.stdout or result.stdout.splitlines().count('hello, world') != 3:
            raise AssertionError(result.stdout)
        bundle = root / 'code-first-program/code-v2.23.2-x86_64-linux'
        cli = bundle / 'code'
        assert cli.is_file() and (bundle / 'code-driver').is_file()
        project = root / 'code-first-program/hello'
        assert (project / 'main.code').read_bytes() == (assets / 'hello.code').read_bytes()
        console = json.loads((project / '.code/lock.json').read_text())['modules']['console']
        assert console['version'] == '2.23.2'
        assert console['source'] == 'https://github.com/codelovesme/code/releases/tag/v2.23.2'
        installed = project / '.code/modules/console' / console['version'] / console['asset']
        assert hashlib.sha256(installed.read_bytes()).hexdigest() == console['sha256']

        def run(arguments, expected=None):
            answer = subprocess.run([str(cli), *arguments], cwd=project, env=env,
                                    text=True, capture_output=True, check=True)
            if expected is not None:
                assert answer.stdout.strip() == expected, answer.stdout
            return answer.stdout

        assert json.loads(run(['check'])) == {'schema_version': 1, 'diagnostics': []}
        source = (project / 'main.code').read_text()
        (project / 'main.code').write_text(source.replace('name = "world"', 'name = "Ada"'))
        assert json.loads(run(['check'])) == {'schema_version': 1, 'diagnostics': []}
        run(['run', '--interpret'], 'hello, Ada')
        run(['run'], 'hello, Ada')
        run(['build', '--release'])
        answer = subprocess.run([str(project / 'build/hello')], cwd=project, env=env,
                                text=True, capture_output=True, check=True)
        assert answer.stdout.strip() == 'hello, Ada'
        for name in ['code', 'code-driver']:
            print(f'Public {name} SHA256: {hashlib.sha256((bundle / name).read_bytes()).hexdigest()}')
    print('Lesson 1: exact displayed/downloaded commands, fresh public CLI/console hash, '
          'checks, original/edited interpreter + cached/native runs passed')


if __name__ == '__main__':
    try:
        main()
    except subprocess.CalledProcessError as error:
        print(error.stdout or '', file=sys.stderr)
        print(error.stderr or '', file=sys.stderr)
        raise SystemExit(f'Command failed ({error.returncode}): {error.cmd}')
